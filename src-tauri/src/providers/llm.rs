//! LLM rewrite clients: Gemini (official), Anthropic (official), OpenAI-compatible.

use super::http::{network_failure, status_failure, trim_base, Failure, CLIENT};
use super::mimo::strip_wrapping_quotes;
use super::prompts;
use crate::platform::AppContext;
use crate::types::{LlmConfig, LlmEndpointConfig, LlmProvider, Mode, TestResult};
use anyhow::anyhow;
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub struct RewriteRequest {
    pub mode: Mode,
    /// Raw ASR transcript (for Ask mode: the spoken instruction/question).
    pub transcript: String,
    /// Ask mode: the text that was selected when the session started.
    pub selected_text: Option<String>,
    pub context: AppContext,
    pub dictionary: Vec<String>,
    /// Dictation languages, e.g. ["zh-CN"].
    pub languages: Vec<String>,
    /// Translation mode target, e.g. "en-US".
    pub translation_target: Option<String>,
    pub custom_instructions: String,
    pub smart_formatting: bool,
    /// Organise long dictation as numbered lists instead of paragraphs.
    pub long_lists: bool,
    pub profile: Option<crate::types::PersonalProfile>,
    pub duration_secs: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum RewriteOutcome {
    /// Insert at caret (dictation, translation, or ask-mode generated text when nothing was selected).
    Insert(String),
    /// Replace the selected text (ask mode with selection).
    Replace(String),
    /// Show in the answer card (ask mode question).
    Answer(String),
}

const GEMINI_DEFAULT_BASE: &str = "https://generativelanguage.googleapis.com";
const ANTHROPIC_DEFAULT_BASE: &str = "https://api.anthropic.com";
const OPENAI_DEFAULT_BASE: &str = "https://api.openai.com/v1";

fn provider_label(p: LlmProvider) -> &'static str {
    match p {
        LlmProvider::Gemini => "Gemini",
        LlmProvider::Anthropic => "Anthropic",
        LlmProvider::Openai => "OpenAI 兼容服务",
    }
}

/// Unified reasoning effort; mapped per provider in the `*_complete` functions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Effort {
    #[default]
    Off,
    Low,
    Medium,
    High,
}

impl Effort {
    /// Parse the settings string; unknown values mean Off.
    pub fn parse(s: &str) -> Self {
        match s.trim().to_lowercase().as_str() {
            "low" => Effort::Low,
            "medium" => Effort::Medium,
            "high" => Effort::High,
            _ => Effort::Off,
        }
    }

    fn level(self) -> &'static str {
        match self {
            Effort::Off | Effort::Low => "low",
            Effort::Medium => "medium",
            Effort::High => "high",
        }
    }

    fn anthropic_budget(self) -> u32 {
        match self {
            Effort::Off | Effort::Low => 1024,
            Effort::Medium => 4096,
            Effort::High => 16000,
        }
    }

    fn gemini_budget(self) -> u32 {
        match self {
            Effort::Off => 0,
            Effort::Low => 1024,
            Effort::Medium => 8192,
            Effort::High => 24576,
        }
    }

    fn gemini_level(self) -> &'static str {
        match self {
            Effort::Off => "minimal",
            other => other.level(),
        }
    }
}

/// Optional request parameters the server rejected for a given provider+model; not sent again.
static REJECTED_PARAMS: Lazy<Mutex<HashSet<String>>> = Lazy::new(|| Mutex::new(HashSet::new()));

fn param_key(p: LlmProvider, model: &str, param: &str) -> String {
    format!("{p:?}|{model}|{param}")
}

/// Run the rewrite for the active provider in `cfg` (honours cfg.timeout_secs).
pub async fn rewrite(cfg: &LlmConfig, req: &RewriteRequest) -> anyhow::Result<RewriteOutcome> {
    let secs = cfg.timeout_secs.clamp(1, 120) as u64 + (req.duration_secs / 60.0 * 6.0).ceil() as u64;
    match tokio::time::timeout(Duration::from_secs(secs), rewrite_with(cfg.provider, cfg.active_endpoint(), req, secs, Effort::parse(&cfg.reasoning_effort))).await {
        Ok(r) => r,
        Err(_) => Err(anyhow!("改写超时（{secs} 秒）")),
    }
}

async fn rewrite_with(
    provider: LlmProvider,
    ep: &LlmEndpointConfig,
    req: &RewriteRequest,
    timeout_secs: u64,
    effort: Effort,
) -> anyhow::Result<RewriteOutcome> {
    let system = prompts::system_prompt_for(req.mode, req.smart_formatting, req.long_lists);
    let user = prompts::build_user_message(req);
    let raw = complete(provider, ep, &system, &user, req.mode == Mode::Ask, timeout_secs, effort).await?;
    finalize(req, &raw)
}

/// Turn the model's raw reply into an outcome (post-processing + guards).
pub(crate) fn finalize(req: &RewriteRequest, raw: &str) -> anyhow::Result<RewriteOutcome> {
    match req.mode {
        Mode::Dictation => {
            let out = clean_output(raw);
            let transcript = req.transcript.trim();
            if out.is_empty() || looks_like_assistant_reply(transcript, &out) {
                log::warn!("dictation rewrite rejected (empty or assistant-style); using transcript");
                return Ok(RewriteOutcome::Insert(transcript.to_string()));
            }
            Ok(RewriteOutcome::Insert(out))
        }
        Mode::Translation => {
            let out = clean_output(raw);
            if out.is_empty() {
                return Err(anyhow!("翻译结果为空"));
            }
            Ok(RewriteOutcome::Insert(out))
        }
        Mode::Ask => {
            let has_selection = req.selected_text.as_deref().map(|s| !s.trim().is_empty()).unwrap_or(false);
            parse_ask(raw, has_selection)
        }
    }
}

// ───────────────────────── Output post-processing ─────────────────────────

fn strip_code_fence(s: &str) -> String {
    let t = s.trim();
    if let Some(rest) = t.strip_prefix("```") {
        let rest = match rest.find('\n') {
            Some(i) if rest[..i].chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') => &rest[i + 1..],
            _ => rest,
        };
        let rest = rest.trim_end();
        return rest.strip_suffix("```").unwrap_or(rest).trim().to_string();
    }
    t.to_string()
}

fn is_preface_line(line: &str) -> bool {
    let l = line.trim();
    if l.chars().count() > 60 || !(l.ends_with('：') || l.ends_with(':')) {
        return false;
    }
    let lower = l.to_lowercase();
    ["以下是", "下面是", "这是", "好的", "当然", "整理后", "润色后", "翻译如下", "译文", "here is", "here's", "here are", "sure", "certainly", "of course", "the following", "cleaned", "translation"]
        .iter()
        .any(|p| lower.starts_with(p))
}

/// Trim, drop wrapping quotes / code fences / tags / "以下是…：" prefaces.
pub fn clean_output(raw: &str) -> String {
    let mut s = strip_code_fence(raw);
    if s.starts_with("<analysis>") {
        s = s.split_once("</analysis>").map(|(_, text)| text.trim().to_string()).unwrap_or_default();
    }
    for tag in ["transcript", "output", "result", "text"] {
        let open = format!("<{tag}>");
        let close = format!("</{tag}>");
        if let Some(rest) = s.strip_prefix(&open) {
            s = rest.strip_suffix(&close).unwrap_or(rest).trim().to_string();
        }
    }
    if let Some((first, rest)) = s.split_once('\n') {
        if is_preface_line(first) {
            s = rest.trim().to_string();
        }
    }
    s = strip_wrapping_quotes(&s);
    // A model sometimes wraps the answer in labels like "Output:".
    for label in ["Output:", "输出：", "输出:", "Result:", "结果："] {
        if let Some(rest) = s.strip_prefix(label) {
            s = rest.trim().to_string();
            break;
        }
    }
    s.trim().to_string()
}

const ASSISTANT_OPENERS: &[&str] = &[
    "好的", "好的，", "当然", "没问题", "抱歉", "对不起", "很抱歉", "我无法", "我不能", "我没办法", "作为一个", "作为ai", "作为 ai", "以下是", "下面是", "为您", "为你",
    "sure", "certainly", "of course", "absolutely", "i'm sorry", "i am sorry", "sorry,", "i cannot", "i can't", "i'm unable", "as an ai", "here is", "here's", "here are",
    "great question", "happy to help",
];

/// Heuristic guard: did the model answer/chat instead of cleaning up the dictation?
pub(crate) fn looks_like_assistant_reply(transcript: &str, output: &str) -> bool {
    let t = transcript.trim().to_lowercase();
    let o = output.trim().to_lowercase();
    if t.is_empty() {
        return false;
    }
    let t_len = t.chars().count();
    let o_len = o.chars().count();
    if o_len as f64 > t_len as f64 * 2.5 + 40.0 {
        return true;
    }
    let head: String = t.chars().take(40).collect();
    ASSISTANT_OPENERS.iter().any(|op| o.starts_with(op) && !head.contains(op.trim_end_matches('，')))
}

fn json_candidates(raw: &str) -> Vec<String> {
    let stripped = strip_code_fence(raw);
    let mut out = vec![stripped.clone()];
    if let (Some(a), Some(b)) = (stripped.find('{'), stripped.rfind('}')) {
        if a < b {
            out.push(stripped[a..=b].to_string());
        }
    }
    out
}

fn lenient_text_field(s: &str) -> Option<String> {
    let key = s.find("\"text\"")?;
    let after = &s[key + 6..];
    let colon = after.find(':')?;
    let after = after[colon + 1..].trim_start();
    let body = after.strip_prefix('"')?;
    let end = body.rfind('"')?;
    let raw = &body[..end];
    let mut out = String::new();
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some('/') => out.push('/'),
                Some(o) => {
                    out.push('\\');
                    out.push(o);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    Some(out)
}

/// Parse the Ask-mode reply: strict JSON `{"action","text"}`, tolerant of fences and surrounding prose.
pub fn parse_ask(raw: &str, has_selection: bool) -> anyhow::Result<RewriteOutcome> {
    let mut action: Option<String> = None;
    let mut text: Option<String> = None;
    for cand in json_candidates(raw) {
        if let Ok(Value::Object(map)) = serde_json::from_str::<Value>(&cand) {
            action = map.get("action").and_then(|v| v.as_str()).map(|s| s.trim().to_lowercase());
            text = map.get("text").or_else(|| map.get("answer")).and_then(|v| v.as_str()).map(str::to_string);
            if text.is_some() {
                break;
            }
        }
    }
    if text.is_none() {
        let stripped = strip_code_fence(raw);
        if let Some(t) = lenient_text_field(&stripped) {
            action = ["replace", "insert", "answer"]
                .iter()
                .find(|a| stripped.contains(&format!("\"{a}\"")))
                .map(|a| a.to_string());
            text = Some(t);
        }
    }
    let Some(text) = text else {
        let answer = raw.trim();
        if answer.is_empty() {
            return Err(anyhow!("模型没有返回内容"));
        }
        return Ok(RewriteOutcome::Answer(answer.to_string()));
    };
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err(anyhow!("模型没有返回内容"));
    }
    Ok(match action.as_deref() {
        Some("replace") if has_selection => RewriteOutcome::Replace(text),
        Some("replace") | Some("insert") => RewriteOutcome::Insert(text),
        _ => RewriteOutcome::Answer(text),
    })
}

// ───────────────────────── Provider calls ─────────────────────────

pub(crate) async fn complete(
    provider: LlmProvider,
    ep: &LlmEndpointConfig,
    system: &str,
    user: &str,
    json_mode: bool,
    timeout_secs: u64,
    effort: Effort,
) -> anyhow::Result<String> {
    if ep.api_key.trim().is_empty() {
        return Err(anyhow!("未配置{}的 API Key", provider_label(provider)));
    }
    if ep.model.trim().is_empty() {
        return Err(anyhow!("未配置{}的模型名称", provider_label(provider)));
    }
    let timeout = Duration::from_secs(timeout_secs.max(1));
    let r = match provider {
        LlmProvider::Gemini => gemini_complete(ep, system, user, json_mode, timeout, effort).await,
        LlmProvider::Anthropic => anthropic_complete(ep, system, user, timeout, effort).await,
        LlmProvider::Openai => openai_complete(ep, system, user, timeout, effort).await,
    };
    r.map_err(anyhow::Error::new)
}

async fn post_json(
    label: &str,
    url: &str,
    headers: &[(&str, String)],
    body: &Value,
    timeout: Duration,
) -> Result<Value, Failure> {
    let mut rb = CLIENT.post(url).timeout(timeout).header("Content-Type", "application/json");
    for (k, v) in headers {
        rb = rb.header(*k, v.as_str());
    }
    let resp = rb.json(body).send().await.map_err(|e| network_failure(label, &e))?;
    let status = resp.status();
    let text = resp.text().await.map_err(|e| network_failure(label, &e))?;
    if !status.is_success() {
        return Err(status_failure(label, status.as_u16(), &text));
    }
    let v: Value = serde_json::from_str(&text).map_err(|_| Failure { message: format!("{label} 返回了无法解析的响应"), retryable: false, status: None })?;
    let model = body.get("model").and_then(Value::as_str).unwrap_or_else(|| url.split("/models/").nth(1).unwrap_or("").split(':').next().unwrap_or(""));
    crate::usage::record(label, model, &v, 0.0);
    Ok(v)
}

fn mentions(failure: &Failure, body_hint: &str) -> bool {
    failure.status == Some(400) && failure.message.to_lowercase().contains(body_hint)
}

// -- Gemini --

fn gemini_base(ep: &LlmEndpointConfig) -> String {
    let base = trim_base(&ep.base_url);
    let base = if base.is_empty() { GEMINI_DEFAULT_BASE } else { base };
    if base.ends_with("/v1beta") || base.ends_with("/v1") {
        base.to_string()
    } else {
        format!("{base}/v1beta")
    }
}

/// gemini-3 takes `thinkingLevel`; 2.5 and earlier take `thinkingBudget`.
fn gemini_thinking(model: &str, effort: Effort) -> Value {
    if model.to_lowercase().contains("gemini-3") {
        json!({ "thinkingLevel": effort.gemini_level() })
    } else {
        json!({ "thinkingBudget": effort.gemini_budget() })
    }
}

fn gemini_model(ep: &LlmEndpointConfig) -> String {
    ep.model.trim().trim_start_matches("models/").to_string()
}

async fn gemini_complete(
    ep: &LlmEndpointConfig,
    system: &str,
    user: &str,
    json_mode: bool,
    timeout: Duration,
    effort: Effort,
) -> Result<String, Failure> {
    let model = gemini_model(ep);
    let url = format!("{}/models/{}:generateContent", gemini_base(ep), model);
    let headers = [("x-goog-api-key", ep.api_key.trim().to_string())];
    let thinking_key = param_key(LlmProvider::Gemini, &model, "thinkingConfig");
    let mut with_thinking = !REJECTED_PARAMS.lock().contains(&thinking_key);
    let mut with_mime = json_mode;

    for _ in 0..3 {
        let mut gen = json!({ "temperature": 0.2 });
        if with_thinking {
            gen["thinkingConfig"] = gemini_thinking(&model, effort);
        }
        if with_mime {
            gen["responseMimeType"] = json!("application/json");
        }
        let body = json!({
            "systemInstruction": { "parts": [{ "text": system }] },
            "contents": [{ "role": "user", "parts": [{ "text": user }] }],
            "generationConfig": gen,
            "safetySettings": [
                { "category": "HARM_CATEGORY_HARASSMENT", "threshold": "BLOCK_NONE" },
                { "category": "HARM_CATEGORY_HATE_SPEECH", "threshold": "BLOCK_NONE" },
                { "category": "HARM_CATEGORY_SEXUALLY_EXPLICIT", "threshold": "BLOCK_NONE" },
                { "category": "HARM_CATEGORY_DANGEROUS_CONTENT", "threshold": "BLOCK_NONE" }
            ]
        });
        match post_json("Gemini", &url, &headers, &body, timeout).await {
            Ok(v) => return extract_gemini(&v),
            Err(f) if with_thinking && mentions(&f, "thinking") => {
                log::info!("Gemini model {model} rejects thinkingConfig; retrying without it");
                REJECTED_PARAMS.lock().insert(thinking_key.clone());
                with_thinking = false;
            }
            Err(f) if with_mime && mentions(&f, "mime") => with_mime = false,
            Err(f) => return Err(f),
        }
    }
    Err(Failure { message: "Gemini 请求失败".into(), retryable: false, status: None })
}

fn extract_gemini(v: &Value) -> Result<String, Failure> {
    let fail = |m: &str| Failure { message: m.to_string(), retryable: false, status: None };
    let parts = v.pointer("/candidates/0/content/parts").and_then(|p| p.as_array());
    let text: String = parts
        .map(|ps| {
            ps.iter()
                .filter(|p| p.get("thought").and_then(|t| t.as_bool()) != Some(true))
                .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
                .collect::<Vec<_>>()
                .join("")
        })
        .unwrap_or_default();
    if !text.trim().is_empty() {
        return Ok(text);
    }
    if let Some(reason) = v.pointer("/promptFeedback/blockReason").and_then(|r| r.as_str()) {
        return Err(fail(&format!("Gemini 拒绝处理该内容（{reason}）")));
    }
    match v.pointer("/candidates/0/finishReason").and_then(|r| r.as_str()) {
        Some("SAFETY") | Some("PROHIBITED_CONTENT") | Some("BLOCKLIST") => Err(fail("Gemini 因安全策略拦截了该内容")),
        Some("MAX_TOKENS") => Err(fail("Gemini 输出被截断")),
        _ => Err(fail("Gemini 没有返回内容")),
    }
}

// -- Anthropic --

fn anthropic_base(ep: &LlmEndpointConfig) -> String {
    let base = trim_base(&ep.base_url);
    let base = if base.is_empty() { ANTHROPIC_DEFAULT_BASE } else { base };
    base.strip_suffix("/v1").unwrap_or(base).to_string()
}

async fn anthropic_complete(ep: &LlmEndpointConfig, system: &str, user: &str, timeout: Duration, effort: Effort) -> Result<String, Failure> {
    let model = ep.model.trim().to_string();
    let url = format!("{}/v1/messages", anthropic_base(ep));
    let headers = [("x-api-key", ep.api_key.trim().to_string()), ("anthropic-version", "2023-06-01".to_string())];
    // Current Claude models think adaptively by default, which multiplies rewrite latency.
    // Off: thinking disabled -> (models that refuse it) effort low -> plain.
    // Low/Medium/High: output_config.effort -> (older models) legacy thinking budget -> plain.
    // Rejections are remembered per model. Sampling params are never sent: current models reject them.
    let key = |p: &str| param_key(LlmProvider::Anthropic, &model, p);
    for _ in 0..5 {
        let (mode, cache) = {
            let rej = REJECTED_PARAMS.lock();
            let mode = if effort == Effort::Off {
                if !rej.contains(&key("thinking")) {
                    AnthropicMode::Disabled
                } else if !rej.contains(&key("effort")) {
                    AnthropicMode::Effort(Effort::Low)
                } else {
                    AnthropicMode::Plain
                }
            } else if !rej.contains(&key("effort")) {
                AnthropicMode::Effort(effort)
            } else if !rej.contains(&key("budget")) {
                AnthropicMode::Budget(effort.anthropic_budget())
            } else {
                AnthropicMode::Plain
            };
            (mode, !rej.contains(&key("cache_control")))
        };
        let mut body = json!({
            "model": model,
            "max_tokens": 4096,
            "system": if cache {
                json!([{ "type": "text", "text": system, "cache_control": { "type": "ephemeral" } }])
            } else {
                json!(system)
            },
            "messages": [{ "role": "user", "content": user }]
        });
        match mode {
            AnthropicMode::Disabled => body["thinking"] = json!({ "type": "disabled" }),
            AnthropicMode::Effort(e) => body["output_config"] = json!({ "effort": e.level() }),
            AnthropicMode::Budget(b) => {
                body["thinking"] = json!({ "type": "enabled", "budget_tokens": b });
                body["max_tokens"] = json!(b + 4096);
            }
            AnthropicMode::Plain => {}
        }
        match post_json("Anthropic", &url, &headers, &body, timeout).await {
            Ok(v) => return extract_anthropic(&v),
            Err(f) if cache && mentions(&f, "cache_control") => {
                REJECTED_PARAMS.lock().insert(key("cache_control"));
            }
            Err(f) if mode == AnthropicMode::Disabled && mentions(&f, "thinking") => {
                REJECTED_PARAMS.lock().insert(key("thinking"));
            }
            Err(f) if matches!(mode, AnthropicMode::Effort(_)) && mentions(&f, "effort") => {
                REJECTED_PARAMS.lock().insert(key("effort"));
            }
            Err(f) if matches!(mode, AnthropicMode::Budget(_)) && (mentions(&f, "thinking") || mentions(&f, "budget")) => {
                REJECTED_PARAMS.lock().insert(key("budget"));
            }
            Err(f) => return Err(f),
        }
    }
    Err(Failure { message: "Anthropic 请求失败".into(), retryable: false, status: None })
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum AnthropicMode {
    Disabled,
    Effort(Effort),
    Budget(u32),
    Plain,
}

fn extract_anthropic(v: &Value) -> Result<String, Failure> {
    let text: String = v
        .get("content")
        .and_then(|c| c.as_array())
        .map(|blocks| {
            blocks
                .iter()
                .filter(|b| b.get("type").and_then(|t| t.as_str()) == Some("text"))
                .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
                .collect::<Vec<_>>()
                .join("")
        })
        .unwrap_or_default();
    if text.trim().is_empty() {
        return Err(Failure { message: "Anthropic 没有返回内容".into(), retryable: false, status: None });
    }
    Ok(text)
}

// -- OpenAI-compatible --

fn openai_base(ep: &LlmEndpointConfig) -> String {
    let base = trim_base(&ep.base_url);
    if base.is_empty() { OPENAI_DEFAULT_BASE.to_string() } else { base.to_string() }
}

async fn openai_complete(ep: &LlmEndpointConfig, system: &str, user: &str, timeout: Duration, effort: Effort) -> Result<String, Failure> {
    let model = ep.model.trim().to_string();
    let url = format!("{}/chat/completions", openai_base(ep));
    let headers = [("Authorization", format!("Bearer {}", ep.api_key.trim()))];
    let temp_key = param_key(LlmProvider::Openai, &model, "temperature");
    let reasoning_key = param_key(LlmProvider::Openai, &model, "reasoning_effort");
    let mut with_temp = !REJECTED_PARAMS.lock().contains(&temp_key);
    let mut with_reasoning = effort != Effort::Off && !REJECTED_PARAMS.lock().contains(&reasoning_key);
    for _ in 0..3 {
        let mut body = json!({
            "model": model,
            "messages": [
                { "role": "system", "content": system },
                { "role": "user", "content": user }
            ]
        });
        if with_temp {
            body["temperature"] = json!(0.2);
        }
        if with_reasoning {
            body["reasoning_effort"] = json!(effort.level());
        }
        match post_json("OpenAI 兼容服务", &url, &headers, &body, timeout).await {
            Ok(v) => return extract_openai(&v),
            Err(f) if with_temp && mentions(&f, "temperature") => {
                REJECTED_PARAMS.lock().insert(temp_key.clone());
                with_temp = false;
            }
            Err(f) if with_reasoning && (mentions(&f, "reasoning") || mentions(&f, "effort")) => {
                REJECTED_PARAMS.lock().insert(reasoning_key.clone());
                with_reasoning = false;
            }
            Err(f) => return Err(f),
        }
    }
    Err(Failure { message: "OpenAI 兼容服务请求失败".into(), retryable: false, status: None })
}

fn extract_openai(v: &Value) -> Result<String, Failure> {
    let text = match v.pointer("/choices/0/message/content") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(parts)) => parts.iter().filter_map(|p| p.get("text").and_then(|t| t.as_str())).collect::<Vec<_>>().join(""),
        _ => String::new(),
    };
    if text.trim().is_empty() {
        return Err(Failure { message: "OpenAI 兼容服务没有返回内容".into(), retryable: false, status: None });
    }
    Ok(text)
}

// ───────────────────────── Models + test ─────────────────────────

async fn get_json(label: &str, url: &str, headers: &[(&str, String)]) -> Result<Value, Failure> {
    let mut rb = CLIENT.get(url).timeout(Duration::from_secs(15));
    for (k, v) in headers {
        rb = rb.header(*k, v.as_str());
    }
    let resp = rb.send().await.map_err(|e| network_failure(label, &e))?;
    let status = resp.status();
    let text = resp.text().await.map_err(|e| network_failure(label, &e))?;
    if !status.is_success() {
        return Err(status_failure(label, status.as_u16(), &text));
    }
    serde_json::from_str(&text).map_err(|_| Failure { message: format!("{label} 返回了无法解析的响应"), retryable: false, status: None })
}

pub async fn list_models(provider: LlmProvider, cfg: &LlmEndpointConfig) -> anyhow::Result<Vec<String>> {
    if cfg.api_key.trim().is_empty() {
        return Err(anyhow!("请先填写 API Key"));
    }
    let key = cfg.api_key.trim().to_string();
    let mut models: Vec<String> = Vec::new();
    match provider {
        LlmProvider::Gemini => {
            let headers = [("x-goog-api-key", key)];
            let mut page_token: Option<String> = None;
            for _ in 0..6 {
                let mut url = format!("{}/models?pageSize=200", gemini_base(cfg));
                if let Some(t) = &page_token {
                    url.push_str(&format!("&pageToken={t}"));
                }
                let v = get_json("Gemini", &url, &headers).await?;
                if let Some(arr) = v.get("models").and_then(|m| m.as_array()) {
                    for m in arr {
                        let supports = m
                            .get("supportedGenerationMethods")
                            .and_then(|s| s.as_array())
                            .map(|a| a.iter().any(|x| x.as_str() == Some("generateContent")))
                            .unwrap_or(false);
                        if let (true, Some(name)) = (supports, m.get("name").and_then(|n| n.as_str())) {
                            models.push(name.trim_start_matches("models/").to_string());
                        }
                    }
                }
                page_token = v.get("nextPageToken").and_then(|t| t.as_str()).filter(|t| !t.is_empty()).map(str::to_string);
                if page_token.is_none() {
                    break;
                }
            }
        }
        LlmProvider::Anthropic => {
            let headers = [("x-api-key", key), ("anthropic-version", "2023-06-01".to_string())];
            let url = format!("{}/v1/models?limit=1000", anthropic_base(cfg));
            let v = get_json("Anthropic", &url, &headers).await?;
            collect_ids(&v, &mut models);
        }
        LlmProvider::Openai => {
            let headers = [("Authorization", format!("Bearer {key}"))];
            let url = format!("{}/models", openai_base(cfg));
            let v = get_json("OpenAI 兼容服务", &url, &headers).await?;
            collect_ids(&v, &mut models);
        }
    }
    models.sort();
    models.dedup();
    Ok(models)
}

fn collect_ids(v: &Value, out: &mut Vec<String>) {
    if let Some(arr) = v.get("data").and_then(|d| d.as_array()) {
        out.extend(arr.iter().filter_map(|m| m.get("id").and_then(|i| i.as_str()).map(str::to_string)));
    }
}

pub async fn test_llm(provider: LlmProvider, cfg: &LlmEndpointConfig) -> TestResult {
    test_llm_with(provider, cfg, Effort::Off).await
}

/// Like `test_llm`, using the interactive reasoning effort (`LlmConfig.reasoning_effort`).
pub async fn test_llm_with(provider: LlmProvider, cfg: &LlmEndpointConfig, effort: Effort) -> TestResult {
    let req = RewriteRequest {
        mode: Mode::Dictation,
        transcript: "嗯 你好 世界".into(),
        selected_text: None,
        context: AppContext::default(),
        dictionary: vec![],
        languages: vec!["zh-CN".into()],
        translation_target: None,
        custom_instructions: String::new(),
        smart_formatting: true,
        long_lists: false,
        profile: None,
        duration_secs: 0.0,
    };
    let start = Instant::now();
    let r = tokio::time::timeout(Duration::from_secs(20), rewrite_with(provider, cfg, &req, 20, effort)).await;
    let latency_ms = start.elapsed().as_millis() as u64;
    match r {
        Ok(Ok(RewriteOutcome::Insert(t))) | Ok(Ok(RewriteOutcome::Replace(t))) | Ok(Ok(RewriteOutcome::Answer(t))) => {
            TestResult { ok: true, message: format!("连接成功（{latency_ms} ms）：{t}"), latency_ms }
        }
        Ok(Err(e)) => TestResult { ok: false, message: e.to_string(), latency_ms },
        Err(_) => TestResult { ok: false, message: "请求超时".into(), latency_ms },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analysis_draft_never_leaks_into_inserted_text() {
        assert_eq!(clean_output("<analysis>outline</analysis>\n正文"), "正文");
        assert_eq!(clean_output("<analysis>incomplete"), "");
    }

    fn req(mode: Mode, transcript: &str) -> RewriteRequest {
        RewriteRequest {
            mode,
            transcript: transcript.into(),
            selected_text: None,
            context: AppContext::default(),
            dictionary: vec![],
            languages: vec![],
            translation_target: None,
            custom_instructions: String::new(),
            smart_formatting: true,
            long_lists: false,
            profile: None,
            duration_secs: 0.0,
        }
    }

    #[test]
    fn clean_output_strips_wrappers() {
        assert_eq!(clean_output("  你好世界。 \n"), "你好世界。");
        assert_eq!(clean_output("\"Hello there\""), "Hello there");
        assert_eq!(clean_output("“你好”"), "你好");
        assert_eq!(clean_output("```\n你好\n```"), "你好");
        assert_eq!(clean_output("```text\nline1\nline2\n```"), "line1\nline2");
        assert_eq!(clean_output("以下是整理后的文本：\n明天开会。"), "明天开会。");
        assert_eq!(clean_output("Here is the cleaned text:\nSee you tomorrow."), "See you tomorrow.");
        assert_eq!(clean_output("<transcript>你好</transcript>"), "你好");
        assert_eq!(clean_output("Output: ok"), "ok");
        // Legit content is untouched.
        assert_eq!(clean_output("他说“好的”，然后走了。"), "他说“好的”，然后走了。");
        assert_eq!(clean_output("1. 牛奶\n2. 鸡蛋"), "1. 牛奶\n2. 鸡蛋");
        assert_eq!(clean_output("提醒：明天开会"), "提醒：明天开会");
    }

    #[test]
    fn assistant_guard() {
        assert!(looks_like_assistant_reply("今天北京天气怎么样", "好的，今天北京天气晴朗，气温 20 度。"));
        assert!(looks_like_assistant_reply("write me a poem", "Sure! Here's a poem about the sea:"));
        assert!(looks_like_assistant_reply("帮我写个邮件", &"很长的内容".repeat(30)));
        assert!(!looks_like_assistant_reply("好的我马上到", "好的，我马上到。"));
        assert!(!looks_like_assistant_reply("嗯 好的 我来了", "好的，我来了。"));
        assert!(!looks_like_assistant_reply("sure let's do it", "Sure, let's do it."));
        assert!(!looks_like_assistant_reply("今天开会", "今天开会。"));
    }

    #[test]
    fn dictation_finalize_falls_back_to_transcript() {
        let r = req(Mode::Dictation, "今天天气怎么样");
        assert_eq!(finalize(&r, "今天天气怎么样？").unwrap(), RewriteOutcome::Insert("今天天气怎么样？".into()));
        assert_eq!(
            finalize(&r, "好的，今天天气很好。").unwrap(),
            RewriteOutcome::Insert("今天天气怎么样".into())
        );
        assert_eq!(finalize(&r, "   ").unwrap(), RewriteOutcome::Insert("今天天气怎么样".into()));
    }

    #[test]
    fn translation_finalize() {
        let r = req(Mode::Translation, "你好");
        assert_eq!(finalize(&r, "\"Hello\"").unwrap(), RewriteOutcome::Insert("Hello".into()));
        assert!(finalize(&r, "").is_err());
    }

    #[test]
    fn ask_parsing() {
        let json = r#"{"action":"replace","text":"Hello"}"#;
        assert_eq!(parse_ask(json, true).unwrap(), RewriteOutcome::Replace("Hello".into()));
        assert_eq!(parse_ask(json, false).unwrap(), RewriteOutcome::Insert("Hello".into()));
        assert_eq!(
            parse_ask("```json\n{\"action\":\"answer\",\"text\":\"42\"}\n```", false).unwrap(),
            RewriteOutcome::Answer("42".into())
        );
        assert_eq!(
            parse_ask("好的，结果如下：\n{\"action\": \"insert\", \"text\": \"第一行\\n第二行\"} 希望有帮助", false).unwrap(),
            RewriteOutcome::Insert("第一行\n第二行".into())
        );
        assert_eq!(parse_ask("{\"action\":\"weird\",\"text\":\"x\"}", true).unwrap(), RewriteOutcome::Answer("x".into()));
    }

    #[test]
    fn ask_parsing_invalid_falls_back_to_answer() {
        assert_eq!(parse_ask("光速约为 3 亿米每秒。", false).unwrap(), RewriteOutcome::Answer("光速约为 3 亿米每秒。".into()));
        // Broken JSON (raw newline inside the string) is recovered leniently.
        let broken = "{\"action\":\"insert\",\"text\":\"line1\nline2\"}";
        assert_eq!(parse_ask(broken, false).unwrap(), RewriteOutcome::Insert("line1\nline2".into()));
        assert!(parse_ask("   ", false).is_err());
        assert!(parse_ask("{\"action\":\"answer\",\"text\":\"\"}", false).is_err());
    }

    #[test]
    fn urls_are_built_from_defaults() {
        let mut ep = LlmEndpointConfig::default();
        assert_eq!(gemini_base(&ep), "https://generativelanguage.googleapis.com/v1beta");
        assert_eq!(anthropic_base(&ep), "https://api.anthropic.com");
        assert_eq!(openai_base(&ep), "https://api.openai.com/v1");
        ep.base_url = "https://proxy.example.com/v1/".into();
        assert_eq!(anthropic_base(&ep), "https://proxy.example.com");
        assert_eq!(openai_base(&ep), "https://proxy.example.com/v1");
        ep.model = "models/gemini-2.5-flash".into();
        assert_eq!(gemini_model(&ep), "gemini-2.5-flash");
    }

    #[test]
    fn extracts_provider_responses() {
        let g = json!({"candidates":[{"content":{"parts":[{"text":"think","thought":true},{"text":"你好"}]}}]});
        assert_eq!(extract_gemini(&g).unwrap(), "你好");
        let blocked = json!({"promptFeedback":{"blockReason":"SAFETY"}});
        assert!(extract_gemini(&blocked).unwrap_err().message.contains("SAFETY"));
        let a = json!({"content":[{"type":"text","text":"hi "},{"type":"text","text":"there"}]});
        assert_eq!(extract_anthropic(&a).unwrap(), "hi there");
        let o = json!({"choices":[{"message":{"content":"ok"}}]});
        assert_eq!(extract_openai(&o).unwrap(), "ok");
        assert!(extract_openai(&json!({"choices":[]})).is_err());
    }

    #[tokio::test]
    async fn missing_key_is_a_clear_error() {
        let cfg = LlmConfig::default();
        let err = rewrite(&cfg, &req(Mode::Dictation, "你好")).await.unwrap_err().to_string();
        assert!(err.contains("API Key"), "{err}");
    }

    #[tokio::test]
    async fn gemini_retries_without_thinking_config_and_remembers() {
        use crate::providers::http::testserver::serve;
        let (base, seen) = serve(vec![
            (400, r#"{"error":{"message":"Budget 0 is invalid. This model only works in thinking mode."}}"#),
            (200, r#"{"candidates":[{"content":{"parts":[{"text":"你好世界"}]}}]}"#),
            (200, r#"{"candidates":[{"content":{"parts":[{"text":"再见"}]}}]}"#),
        ])
        .await;
        let ep = LlmEndpointConfig { api_key: "gk".into(), base_url: base, model: "gemini-test-thinking-only".into() };
        let r = req(Mode::Dictation, "嗯 你好 世界");
        let out = rewrite_with(LlmProvider::Gemini, &ep, &r, 10, Effort::Off).await.unwrap();
        assert_eq!(out, RewriteOutcome::Insert("你好世界".into()));
        let out = rewrite_with(LlmProvider::Gemini, &ep, &r, 10, Effort::Off).await.unwrap();
        assert_eq!(out, RewriteOutcome::Insert("再见".into()));
        let reqs = seen.lock().unwrap();
        assert_eq!(reqs.len(), 3);
        assert!(reqs[0].starts_with("POST /v1beta/models/gemini-test-thinking-only:generateContent"));
        assert!(reqs[0].to_lowercase().contains("x-goog-api-key: gk"));
        assert!(reqs[0].contains("thinkingBudget") && reqs[0].contains("systemInstruction"));
        assert!(!reqs[1].contains("thinkingBudget"));
        assert!(!reqs[2].contains("thinkingBudget"));
    }

    #[tokio::test]
    async fn openai_and_anthropic_requests() {
        use crate::providers::http::testserver::serve;
        let (base, seen) = serve(vec![(200, r#"{"choices":[{"message":{"content":"Hello"}}]}"#)]).await;
        let ep = LlmEndpointConfig { api_key: "ok".into(), base_url: format!("{base}/v1"), model: "m".into() };
        let out = rewrite_with(LlmProvider::Openai, &ep, &req(Mode::Translation, "你好"), 10, Effort::Off).await.unwrap();
        assert_eq!(out, RewriteOutcome::Insert("Hello".into()));
        {
            let reqs = seen.lock().unwrap();
            assert!(reqs[0].starts_with("POST /v1/chat/completions"));
            assert!(reqs[0].to_lowercase().contains("authorization: bearer ok"));
        }

        let (base, seen) = serve(vec![(200, r#"{"content":[{"type":"text","text":"{\"action\":\"answer\",\"text\":\"42\"}"}]}"#)]).await;
        let ep = LlmEndpointConfig { api_key: "ak".into(), base_url: base, model: "claude-x".into() };
        let out = rewrite_with(LlmProvider::Anthropic, &ep, &req(Mode::Ask, "六乘七"), 10, Effort::Off).await.unwrap();
        assert_eq!(out, RewriteOutcome::Answer("42".into()));
        let reqs = seen.lock().unwrap();
        assert!(reqs[0].starts_with("POST /v1/messages"));
        let lower = reqs[0].to_lowercase();
        assert!(lower.contains("x-api-key: ak") && lower.contains("anthropic-version: 2023-06-01"));
        assert!(reqs[0].contains("\"max_tokens\":4096"));
    }

    #[tokio::test]
    async fn anthropic_turns_thinking_off_then_falls_back_and_remembers() {
        use crate::providers::http::testserver::serve;
        let ok = r#"{"content":[{"type":"text","text":"你好世界"}]}"#;
        let (base, seen) = serve(vec![
            (400, r#"{"type":"error","error":{"type":"invalid_request_error","message":"thinking.type disabled is not supported for this model"}}"#),
            (400, r#"{"type":"error","error":{"type":"invalid_request_error","message":"effort is not supported for this model"}}"#),
            (200, ok),
            (200, ok),
        ])
        .await;
        let ep = LlmEndpointConfig { api_key: "ak".into(), base_url: base, model: "claude-test-fallback".into() };
        let r = req(Mode::Dictation, "嗯 你好 世界");
        assert_eq!(rewrite_with(LlmProvider::Anthropic, &ep, &r, 10, Effort::Off).await.unwrap(), RewriteOutcome::Insert("你好世界".into()));
        assert_eq!(rewrite_with(LlmProvider::Anthropic, &ep, &r, 10, Effort::Off).await.unwrap(), RewriteOutcome::Insert("你好世界".into()));
        let reqs = seen.lock().unwrap();
        assert_eq!(reqs.len(), 4);
        assert!(reqs[0].contains(r#""thinking":{"type":"disabled"}"#) && !reqs[0].contains("temperature"));
        assert!(reqs[1].contains(r#""effort":"low""#) && !reqs[1].contains("thinking"));
        assert!(!reqs[2].contains("effort") && !reqs[2].contains("thinking"));
        assert!(!reqs[3].contains("effort") && !reqs[3].contains("thinking"));
    }

    fn body_of(req: &str) -> Value {
        serde_json::from_str(&req[req.find("\r\n\r\n").unwrap() + 4..]).unwrap()
    }

    #[test]
    fn effort_parsing_and_gemini_mapping() {
        assert_eq!(Effort::parse("High"), Effort::High);
        assert_eq!(Effort::parse("bogus"), Effort::Off);
        assert_eq!(gemini_thinking("gemini-2.5-flash", Effort::Off), json!({"thinkingBudget": 0}));
        assert_eq!(gemini_thinking("gemini-2.5-flash", Effort::Medium), json!({"thinkingBudget": 8192}));
        assert_eq!(gemini_thinking("gemini-2.5-flash", Effort::High), json!({"thinkingBudget": 24576}));
        assert_eq!(gemini_thinking("gemini-3-flash-preview", Effort::Off), json!({"thinkingLevel": "minimal"}));
        assert_eq!(gemini_thinking("gemini-3-pro", Effort::Low), json!({"thinkingLevel": "low"}));
        assert_eq!(gemini_thinking("gemini-3-pro", Effort::High), json!({"thinkingLevel": "high"}));
    }

    #[tokio::test]
    async fn gemini_effort_request_shapes_and_fallback() {
        use crate::providers::http::testserver::serve;
        let ok = r#"{"candidates":[{"content":{"parts":[{"text":"好"}]}}]}"#;
        let (base, seen) = serve(vec![(200, ok), (200, ok)]).await;
        let r = req(Mode::Dictation, "好");
        let ep = LlmEndpointConfig { api_key: "k".into(), base_url: base.clone(), model: "gemini-2.5-test-a".into() };
        rewrite_with(LlmProvider::Gemini, &ep, &r, 10, Effort::Medium).await.unwrap();
        let ep3 = LlmEndpointConfig { model: "gemini-3-test-a".into(), ..ep };
        rewrite_with(LlmProvider::Gemini, &ep3, &r, 10, Effort::Off).await.unwrap();
        let reqs = seen.lock().unwrap();
        assert_eq!(body_of(&reqs[0])["generationConfig"]["thinkingConfig"], json!({"thinkingBudget": 8192}));
        assert_eq!(body_of(&reqs[1])["generationConfig"]["thinkingConfig"], json!({"thinkingLevel": "minimal"}));
        drop(reqs);

        let (base, seen) = serve(vec![
            (400, r#"{"error":{"message":"Thinking level minimal is not supported"}}"#),
            (200, ok),
            (200, ok),
        ])
        .await;
        let ep = LlmEndpointConfig { api_key: "k".into(), base_url: base, model: "gemini-3-test-b".into() };
        rewrite_with(LlmProvider::Gemini, &ep, &r, 10, Effort::Off).await.unwrap();
        rewrite_with(LlmProvider::Gemini, &ep, &r, 10, Effort::Off).await.unwrap();
        let reqs = seen.lock().unwrap();
        assert!(reqs[0].contains("thinkingLevel"));
        assert!(!reqs[1].contains("thinkingConfig") && !reqs[2].contains("thinkingConfig"));
    }

    #[tokio::test]
    async fn openai_reasoning_effort_and_fallback() {
        use crate::providers::http::testserver::serve;
        let ok = r#"{"choices":[{"message":{"content":"Hi"}}]}"#;
        let (base, seen) = serve(vec![(200, ok), (200, ok)]).await;
        let r = req(Mode::Translation, "你好");
        let ep = LlmEndpointConfig { api_key: "k".into(), base_url: base, model: "o-test-a".into() };
        rewrite_with(LlmProvider::Openai, &ep, &r, 10, Effort::Off).await.unwrap();
        rewrite_with(LlmProvider::Openai, &ep, &r, 10, Effort::High).await.unwrap();
        {
            let reqs = seen.lock().unwrap();
            assert!(!reqs[0].contains("reasoning_effort"));
            assert_eq!(body_of(&reqs[1])["reasoning_effort"], "high");
        }
        let (base, seen) = serve(vec![
            (400, r#"{"error":{"message":"Unrecognized request argument: reasoning_effort"}}"#),
            (200, ok),
            (200, ok),
        ])
        .await;
        let ep = LlmEndpointConfig { api_key: "k".into(), base_url: base, model: "o-test-b".into() };
        rewrite_with(LlmProvider::Openai, &ep, &r, 10, Effort::Low).await.unwrap();
        rewrite_with(LlmProvider::Openai, &ep, &r, 10, Effort::Low).await.unwrap();
        let reqs = seen.lock().unwrap();
        assert!(reqs[0].contains("reasoning_effort") && !reqs[1].contains("reasoning_effort") && !reqs[2].contains("reasoning_effort"));
    }

    #[tokio::test]
    async fn anthropic_effort_shapes_and_budget_fallback() {
        use crate::providers::http::testserver::serve;
        let ok = r#"{"content":[{"type":"text","text":"好"}]}"#;
        let r = req(Mode::Dictation, "好");
        let (base, seen) = serve(vec![(200, ok), (200, ok)]).await;
        let ep = LlmEndpointConfig { api_key: "k".into(), base_url: base, model: "claude-test-shape".into() };
        rewrite_with(LlmProvider::Anthropic, &ep, &r, 10, Effort::Off).await.unwrap();
        rewrite_with(LlmProvider::Anthropic, &ep, &r, 10, Effort::High).await.unwrap();
        {
            let reqs = seen.lock().unwrap();
            let (off, high) = (body_of(&reqs[0]), body_of(&reqs[1]));
            assert_eq!(off["thinking"], json!({"type": "disabled"}));
            assert!(off.get("output_config").is_none() && off.get("temperature").is_none());
            assert_eq!(high["output_config"], json!({"effort": "high"}));
            assert!(high.get("thinking").is_none() && high.get("temperature").is_none());
            // Prompt caching on the static system prompt.
            assert_eq!(off["system"][0]["cache_control"], json!({"type": "ephemeral"}));
        }
        let (base, seen) = serve(vec![
            (400, r#"{"error":{"message":"effort is not supported for this model"}}"#),
            (200, ok),
            (200, ok),
        ])
        .await;
        let ep = LlmEndpointConfig { api_key: "k".into(), base_url: base, model: "claude-test-budget".into() };
        rewrite_with(LlmProvider::Anthropic, &ep, &r, 10, Effort::Medium).await.unwrap();
        rewrite_with(LlmProvider::Anthropic, &ep, &r, 10, Effort::High).await.unwrap();
        let reqs = seen.lock().unwrap();
        let (b1, b2) = (body_of(&reqs[1]), body_of(&reqs[2]));
        assert_eq!(b1["thinking"], json!({"type": "enabled", "budget_tokens": 4096}));
        assert_eq!(b1["max_tokens"], 8192);
        assert!(b1.get("output_config").is_none());
        assert_eq!(b2["thinking"]["budget_tokens"], 16000);
        assert_eq!(b2["max_tokens"], 20096);
    }

    #[tokio::test]
    async fn anthropic_budget_rejection_falls_back_to_plain() {
        use crate::providers::http::testserver::serve;
        let ok = r#"{"content":[{"type":"text","text":"好"}]}"#;
        let (base, seen) = serve(vec![
            (400, r#"{"error":{"message":"output_config.effort: extra inputs are not permitted"}}"#),
            (400, r#"{"error":{"message":"thinking is not supported"}}"#),
            (200, ok),
        ])
        .await;
        let ep = LlmEndpointConfig { api_key: "k".into(), base_url: base, model: "claude-test-plain".into() };
        rewrite_with(LlmProvider::Anthropic, &ep, &req(Mode::Dictation, "好"), 10, Effort::Low).await.unwrap();
        let reqs = seen.lock().unwrap();
        let last = body_of(&reqs[2]);
        assert!(last.get("thinking").is_none() && last.get("output_config").is_none());
    }

    #[tokio::test]
    async fn anthropic_cache_control_fallback_is_remembered() {
        use crate::providers::http::testserver::serve;
        let ok = r#"{"content":[{"type":"text","text":"好"}]}"#;
        let (base, seen) = serve(vec![
            (400, r#"{"error":{"message":"system.0.cache_control: Extra inputs are not permitted"}}"#),
            (200, ok),
            (200, ok),
        ])
        .await;
        let ep = LlmEndpointConfig { api_key: "k".into(), base_url: base, model: "claude-test-cache".into() };
        let r = req(Mode::Dictation, "好");
        rewrite_with(LlmProvider::Anthropic, &ep, &r, 10, Effort::Off).await.unwrap();
        rewrite_with(LlmProvider::Anthropic, &ep, &r, 10, Effort::Off).await.unwrap();
        let reqs = seen.lock().unwrap();
        assert!(reqs[0].contains("cache_control"));
        for q in &reqs[1..] {
            let b = body_of(q);
            assert!(b["system"].is_string(), "{b}");
            assert_eq!(b["thinking"], json!({"type": "disabled"}));
        }
    }

    #[tokio::test]
    async fn list_models_gemini_filters_and_sorts() {
        use crate::providers::http::testserver::serve;
        let body = r#"{"models":[
            {"name":"models/gemini-2.5-pro","supportedGenerationMethods":["generateContent"]},
            {"name":"models/embedding-001","supportedGenerationMethods":["embedContent"]},
            {"name":"models/gemini-2.5-flash","supportedGenerationMethods":["generateContent","countTokens"]}]}"#;
        let (base, _) = serve(vec![(200, body)]).await;
        let ep = LlmEndpointConfig { api_key: "k".into(), base_url: base, model: String::new() };
        let models = list_models(LlmProvider::Gemini, &ep).await.unwrap();
        assert_eq!(models, vec!["gemini-2.5-flash", "gemini-2.5-pro"]);
    }

    #[tokio::test]
    async fn rewrite_times_out() {
        use tokio::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _held = listener.accept().await;
            tokio::time::sleep(Duration::from_secs(5)).await;
        });
        let mut cfg = LlmConfig::default();
        cfg.timeout_secs = 1;
        cfg.provider = LlmProvider::Openai;
        cfg.openai = LlmEndpointConfig { api_key: "k".into(), base_url: format!("http://{addr}"), model: "m".into() };
        let t = Instant::now();
        let err = rewrite(&cfg, &req(Mode::Dictation, "你好")).await.unwrap_err().to_string();
        assert!(t.elapsed() < Duration::from_secs(3));
        assert!(err.contains("超时"), "{err}");
    }
}
