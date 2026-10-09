//! Xiaomi MiMo ASR (mimo-v2.5-asr) via OpenAI-compatible chat completions with an input_audio part.
//! OWNER: audio/providers agent. See SPEC.md §Providers.

use super::http::{network_failure, status_failure, trim_base, Failure, CLIENT};
use crate::types::{MimoAsrConfig, TestResult};
use anyhow::anyhow;
use base64::Engine;
use std::time::{Duration, Instant};

const PROVIDER: &str = "MiMo";
const TIMEOUT: Duration = Duration::from_secs(30);
const MAX_BASE64_BYTES: usize = 10 * 1024 * 1024;

/// MiMo `asr_options.language` for the user's dictation languages.
pub fn language_param(selected: &[String], auto: bool) -> &'static str {
    if auto || selected.is_empty() {
        return "auto";
    }
    let is = |prefix: &str| selected.iter().all(|l| l.to_lowercase().starts_with(prefix));
    if is("zh") {
        "zh"
    } else if is("en") {
        "en"
    } else {
        "auto"
    }
}

/// Transcribe one WAV (16 kHz mono) buffer. `language`: "zh" | "en" | "auto".
pub async fn transcribe(cfg: &MimoAsrConfig, wav: Vec<u8>, language: &str) -> anyhow::Result<String> {
    if cfg.api_key.trim().is_empty() {
        return Err(anyhow!("未配置 MiMo API Key"));
    }
    if wav.len() <= 44 {
        return Ok(String::new());
    }
    let url = format!("{}/chat/completions", trim_base(&cfg.base_url));
    let model = if cfg.model.trim().is_empty() { "mimo-v2.5-asr" } else { cfg.model.trim() };

    let audio_secs = (wav.len().saturating_sub(44)) as f64 / 32000.0;
    let encoded = tokio::task::spawn_blocking(move || base64::engine::general_purpose::STANDARD.encode(&wav))
        .await
        .map_err(|e| anyhow!("编码失败: {e}"))?;
    if encoded.len() >= MAX_BASE64_BYTES {
        return Err(anyhow!("音频过长（超过 10 MB），请缩短单次录音"));
    }
    let body = serde_json::to_vec(&serde_json::json!({
        "model": model,
        "messages": [{
            "role": "user",
            "content": [{
                "type": "input_audio",
                "input_audio": { "data": format!("data:audio/wav;base64,{encoded}") }
            }]
        }],
        "asr_options": { "language": language }
    }))?;
    drop(encoded);

    let mut attempt = 0;
    loop {
        match send_once(&url, &cfg.api_key, &body, model, audio_secs).await {
            Ok(text) => return Ok(clean_transcript(&text)),
            Err(f) if f.retryable && attempt == 0 => {
                attempt += 1;
                log::info!("MiMo retrying after: {}", f.message);
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            Err(f) => return Err(anyhow::Error::new(f)),
        }
    }
}

async fn send_once(url: &str, key: &str, body: &[u8], model: &str, audio_secs: f64) -> Result<String, Failure> {
    let resp = CLIENT
        .post(url)
        .timeout(TIMEOUT)
        .header("Authorization", format!("Bearer {key}"))
        .header("api-key", key)
        .header("Content-Type", "application/json")
        .body(body.to_vec())
        .send()
        .await
        .map_err(|e| network_failure(PROVIDER, &e))?;
    let status = resp.status();
    let text = resp.text().await.map_err(|e| network_failure(PROVIDER, &e))?;
    if !status.is_success() {
        return Err(status_failure(PROVIDER, status.as_u16(), &text));
    }
    let v: serde_json::Value = serde_json::from_str(&text)
        .map_err(|_| Failure { message: "MiMo 返回了无法解析的响应".into(), retryable: false, status: Some(status.as_u16()) })?;
    crate::usage::record("MiMo", model, &v, audio_secs);
    Ok(extract_content(&v))
}

fn extract_content(v: &serde_json::Value) -> String {
    match v.pointer("/choices/0/message/content") {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Array(parts)) => parts
            .iter()
            .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
            .collect::<Vec<_>>()
            .join(""),
        _ => String::new(),
    }
}

/// Trim and strip wrapping quotes / code fences / tags the model may add around the transcript.
pub fn clean_transcript(raw: &str) -> String {
    let mut s = raw.trim().to_string();
    if s.starts_with("```") {
        s = s.trim_start_matches('`').trim_start_matches(|c: char| c.is_ascii_alphabetic()).to_string();
        s = s.trim_end().trim_end_matches('`').trim().to_string();
    }
    // Leading/trailing XML-ish tags such as <transcript>…</transcript> or <|zh|>.
    loop {
        let before = s.len();
        if s.starts_with('<') {
            if let Some(end) = s.find('>') {
                let tag = &s[1..end];
                if end <= 24 && !tag.is_empty() && tag.chars().all(|c| c.is_ascii_alphanumeric() || "_/|- ".contains(c)) {
                    s = s[end + 1..].trim_start().to_string();
                }
            }
        }
        if s.ends_with('>') {
            if let Some(start) = s.rfind('<') {
                let tag = &s[start + 1..s.len() - 1];
                if s.len() - start <= 24 && !tag.is_empty() && tag.chars().all(|c| c.is_ascii_alphanumeric() || "_/|- ".contains(c)) {
                    s = s[..start].trim_end().to_string();
                }
            }
        }
        if s.len() == before {
            break;
        }
    }
    strip_wrapping_quotes(&s).trim().to_string()
}

pub(crate) fn strip_wrapping_quotes(s: &str) -> String {
    let t = s.trim();
    const PAIRS: [(char, char); 6] = [('"', '"'), ('“', '”'), ('「', '」'), ('『', '』'), ('\'', '\''), ('‘', '’')];
    let mut chars = t.chars();
    if let (Some(first), Some(last)) = (chars.next(), t.chars().last()) {
        for (open, close) in PAIRS {
            if first == open && last == close && t.chars().count() >= 2 {
                let inner = &t[first.len_utf8()..t.len() - last.len_utf8()];
                // Only strip when the quote pair is not used inside (so it really wraps the whole text).
                if !inner.contains(open) && !inner.contains(close) {
                    return inner.trim().to_string();
                }
            }
        }
    }
    t.to_string()
}

/// Send ~1 s of generated audio to verify key/endpoint.
pub async fn test_asr(cfg: &MimoAsrConfig) -> TestResult {
    let samples: Vec<i16> = (0..16_000)
        .map(|n| {
            let t = n as f32 / 16_000.0;
            let env = (std::f32::consts::PI * t).sin().powi(2);
            (env * 0.08 * (2.0 * std::f32::consts::PI * 220.0 * t).sin() * 32767.0) as i16
        })
        .collect();
    let wav = crate::audio::wav::encode_wav(&samples, 16_000);
    let start = Instant::now();
    let result = transcribe(cfg, wav, "auto").await;
    let latency_ms = start.elapsed().as_millis() as u64;
    match result {
        Ok(_) => TestResult { ok: true, message: format!("连接成功（{latency_ms} ms）"), latency_ms },
        Err(e) => TestResult { ok: false, message: e.to_string(), latency_ms },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn language_param_rules() {
        assert_eq!(language_param(&v(&["zh-CN"]), false), "zh");
        assert_eq!(language_param(&v(&["zh-CN", "zh-TW"]), false), "zh");
        assert_eq!(language_param(&v(&["en-US", "en-GB"]), false), "en");
        assert_eq!(language_param(&v(&["zh-CN", "en-US"]), false), "auto");
        assert_eq!(language_param(&v(&["ja-JP"]), false), "auto");
        assert_eq!(language_param(&v(&["zh-CN"]), true), "auto");
        assert_eq!(language_param(&[], false), "auto");
    }

    #[test]
    fn cleans_transcripts() {
        assert_eq!(clean_transcript("  你好世界 \n"), "你好世界");
        assert_eq!(clean_transcript("\"你好世界\""), "你好世界");
        assert_eq!(clean_transcript("“你好世界”"), "你好世界");
        assert_eq!(clean_transcript("<transcript>你好</transcript>"), "你好");
        assert_eq!(clean_transcript("<|zh|>你好"), "你好");
        assert_eq!(clean_transcript("```\n你好\n```"), "你好");
        assert_eq!(clean_transcript("他说“你好”然后走了"), "他说“你好”然后走了");
        assert_eq!(clean_transcript("\"a\" and \"b\""), "\"a\" and \"b\"");
        assert_eq!(clean_transcript("1 < 2 and 3 > 2"), "1 < 2 and 3 > 2");
        assert_eq!(clean_transcript(""), "");
    }

    #[test]
    fn extracts_content_variants() {
        let a = serde_json::json!({"choices":[{"message":{"content":"hi"}}]});
        assert_eq!(extract_content(&a), "hi");
        let b = serde_json::json!({"choices":[{"message":{"content":[{"type":"text","text":"a"},{"type":"text","text":"b"}]}}]});
        assert_eq!(extract_content(&b), "ab");
        assert_eq!(extract_content(&serde_json::json!({})), "");
    }

    #[tokio::test]
    async fn rejects_missing_key_and_empty_audio() {
        let cfg = MimoAsrConfig::default();
        assert!(transcribe(&cfg, vec![0; 100], "zh").await.is_err());
        let cfg = MimoAsrConfig { api_key: "k".into(), ..Default::default() };
        assert_eq!(transcribe(&cfg, vec![], "zh").await.unwrap(), "");
    }

    #[tokio::test]
    async fn retries_5xx_once_and_sends_expected_request() {
        use crate::providers::http::testserver::serve;
        let (base, seen) = serve(vec![
            (503, "{}"),
            (200, r#"{"choices":[{"message":{"content":" \"你好，世界\" "}}]}"#),
        ])
        .await;
        let cfg = MimoAsrConfig { api_key: "sk-test".into(), base_url: format!("{base}/v1/"), model: "mimo-v2.5-asr".into() };
        let wav = crate::audio::wav::encode_wav(&[1000i16; 1600], 16_000);
        let text = transcribe(&cfg, wav, "zh").await.unwrap();
        assert_eq!(text, "你好，世界");
        let reqs = seen.lock().unwrap();
        assert_eq!(reqs.len(), 2);
        let r = &reqs[1];
        assert!(r.starts_with("POST /v1/chat/completions"));
        let lower = r.to_lowercase();
        assert!(lower.contains("authorization: bearer sk-test"));
        assert!(lower.contains("api-key: sk-test"));
        assert!(r.contains("\"input_audio\"") && r.contains("data:audio/wav;base64,UklGR"));
        assert!(r.contains("\"asr_options\":{\"language\":\"zh\"}"));
    }

    #[tokio::test]
    async fn maps_401_without_retry() {
        use crate::providers::http::testserver::serve;
        let (base, seen) = serve(vec![(401, r#"{"error":{"message":"bad key"}}"#)]).await;
        let cfg = MimoAsrConfig { api_key: "bad".into(), base_url: base, model: String::new() };
        let r = test_asr(&cfg).await;
        assert!(!r.ok);
        assert_eq!(r.message, "MiMo API Key 无效");
        assert_eq!(seen.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn network_error_is_chinese() {
        let cfg = MimoAsrConfig { api_key: "k".into(), base_url: "http://127.0.0.1:1".into(), model: String::new() };
        let err = transcribe(&cfg, crate::audio::wav::encode_wav(&[5i16; 800], 16_000), "auto").await.unwrap_err();
        assert_eq!(err.to_string(), "网络连接失败");
    }
}
