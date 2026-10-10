//! The processing half of a dictation session: ASR → rewrite → delivery → history.
//! Shared by the live controller, "undo"/"retry" alerts and the history "retry" command.

use crate::audio::sounds::{self, Sound};
use crate::audio::{wav, TARGET_SAMPLE_RATE};
use crate::db::HistoryRow;
use crate::notify;
use crate::platform::{context, insert, AppContext};
use crate::providers::{self, RewriteOutcome, RewriteRequest};
use crate::state::{emit_history_changed, AppState};
use crate::types::{events, AskAnswer, AsrConfig, Mode, Settings};
use crate::{autodict, config, stats};
use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::async_runtime::JoinHandle;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::Semaphore;

static ASR_SEMAPHORE: once_cell::sync::Lazy<Arc<Semaphore>> =
    once_cell::sync::Lazy::new(|| Arc::new(Semaphore::new(3)));

pub fn play(app: &AppHandle, sound: Sound) {
    if app.state::<AppState>().settings.read().interaction_sounds {
        sounds::play(sound);
    }
}

pub fn short_id(id: &str) -> &str {
    id.get(..8).unwrap_or(id)
}

pub fn err_text(e: &anyhow::Error) -> String {
    let s = format!("{e:#}");
    if s.chars().count() > 300 {
        let cut: String = s.chars().take(300).collect();
        format!("{cut}…")
    } else {
        s
    }
}

// ───────────────────────── ASR ─────────────────────────

pub fn spawn_asr(
    cfg: AsrConfig,
    language: &'static str,
    words: Vec<String>,
    samples: Vec<i16>,
    tag: String,
) -> JoinHandle<anyhow::Result<String>> {
    tauri::async_runtime::spawn(async move {
        let _permit = ASR_SEMAPHORE.acquire().await.map_err(|e| anyhow::anyhow!("{e}"))?;
        let secs = samples.len() as f64 / TARGET_SAMPLE_RATE as f64;
        let wav_bytes = tauri::async_runtime::spawn_blocking(move || wav::encode_wav(&samples, TARGET_SAMPLE_RATE))
            .await
            .map_err(|e| anyhow::anyhow!("音频编码失败：{e}"))?;
        let t = Instant::now();
        use crate::providers::asr::BatchRecognizer;
        let r = cfg.recognize(wav_bytes, language, &words).await;
        match &r {
            Ok(text) => log::debug!(
                "[{tag}] asr chunk {secs:.1}s -> {} chars in {} ms",
                text.chars().count(),
                t.elapsed().as_millis()
            ),
            Err(e) => log::warn!("[{tag}] asr chunk failed after {} ms: {e:#}", t.elapsed().as_millis()),
        }
        r
    })
}

fn is_cjk_char(c: char) -> bool {
    matches!(c as u32,
        0x3000..=0x303F | 0x3040..=0x30FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xAC00..=0xD7AF | 0xFF00..=0xFFEF)
}

/// Join chunk transcripts in order: no separator next to CJK, a space between Latin words.
pub fn join_transcripts(parts: &[String]) -> String {
    let mut out = String::new();
    for p in parts {
        let p = p.trim();
        if p.is_empty() {
            continue;
        }
        if let (Some(last), Some(first)) = (out.chars().last(), p.chars().next()) {
            if !(is_cjk_char(last) || is_cjk_char(first)) && !last.is_whitespace() {
                out.push(' ');
            }
        }
        out.push_str(p);
    }
    out
}

/// Split long audio into <= ~90 s pieces, cutting at the quietest 100 ms frame in the last 15 s.
pub fn split_samples(samples: &[i16]) -> Vec<Vec<i16>> {
    const SR: usize = TARGET_SAMPLE_RATE as usize;
    const MAX_SINGLE: usize = 120 * SR;
    const TARGET: usize = 90 * SR;
    const WINDOW: usize = 15 * SR;
    const FRAME: usize = SR / 10;
    if samples.len() <= MAX_SINGLE {
        return vec![samples.to_vec()];
    }
    let mut out = Vec::new();
    let mut pos = 0usize;
    while samples.len() - pos > MAX_SINGLE {
        let lo = pos + TARGET - WINDOW;
        let hi = pos + TARGET;
        let mut best = (u64::MAX, hi);
        let mut i = lo;
        while i + FRAME <= hi {
            let energy: u64 = samples[i..i + FRAME].iter().map(|s| (*s as i64).unsigned_abs()).sum();
            if energy < best.0 {
                best = (energy, i + FRAME / 2);
            }
            i += FRAME / 2;
        }
        out.push(samples[pos..best.1].to_vec());
        pos = best.1;
    }
    out.push(samples[pos..].to_vec());
    out
}

async fn transcribe_samples(app: &AppHandle, settings: &Settings, samples: &[i16], tag: &str) -> anyhow::Result<String> {
    let language = config::asr_language(settings);
    let words = app.state::<AppState>().db.words_for_prompt(100)?;
    if settings.asr.streaming() {
        return providers::streaming::transcribe_samples(settings.asr.clone(), settings.selected_languages.clone(), words, samples.to_vec()).await;
    }
    let handles: Vec<_> = split_samples(samples)
        .into_iter()
        .enumerate()
        .map(|(i, s)| spawn_asr(settings.asr.clone(), language, words.clone(), s, format!("{tag}#{i}")))
        .collect();
    let mut handles = TranscriptTasks(handles.into_iter().enumerate().collect());
    let mut parts = Vec::new();
    for (_, h) in &mut handles.0 {
        parts.push(h.await.map_err(|e| anyhow::anyhow!("识别任务异常：{e}"))??);
    }
    Ok(join_transcripts(&parts))
}

// ───────────────────────── audio files ─────────────────────────

/// Writes `<dir>/<id>.wav` atomically; returns the file name.
pub fn save_audio(dir: &Path, id: &str, samples: &[i16]) -> anyhow::Result<String> {
    std::fs::create_dir_all(dir)?;
    let name = format!("{id}.wav");
    let tmp = dir.join(format!("{id}.wav.tmp"));
    let bytes = wav::encode_wav(samples, TARGET_SAMPLE_RATE);
    {
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = opts.open(&tmp)?;
        f.write_all(&bytes)?;
    }
    std::fs::rename(&tmp, dir.join(&name))?;
    Ok(name)
}

pub fn load_audio(path: &Path) -> anyhow::Result<Vec<i16>> {
    let bytes = std::fs::read(path)?;
    wav::decode_wav_16k_mono(&bytes)
}

// ───────────────────────── delivery ─────────────────────────

pub enum Delivered {
    Inserted,
    NotEditable,
    Failed(String),
}

pub async fn deliver_text(start_ctx: &AppContext, text: &str, replace: bool) -> Delivered {
    let start = start_ctx.clone();
    let text = text.to_string();
    let joined = tauri::async_runtime::spawn_blocking(move || {
        let fresh = context::capture(false, false);
        if let (Some(a), Some(b)) = (start.pid, fresh.pid) {
            if a != b {
                log::info!("frontmost app changed during the session (pid {a} -> {b}); pasting into the new target");
            }
        }
        let blocked = match (start.focused_editable, fresh.focused_editable) {
            (Some(true), _) | (_, Some(true)) => false,
            (Some(false), _) | (_, Some(false)) => true,
            _ => false,
        };
        if blocked {
            return Delivered::NotEditable;
        }
        let r = if replace { insert::replace_selection(&text) } else { insert::insert_text(&text) };
        match r {
            Ok(()) => Delivered::Inserted,
            Err(e) => Delivered::Failed(err_text(&e)),
        }
    })
    .await;
    joined.unwrap_or_else(|e| Delivered::Failed(format!("{e}")))
}

// ───────────────────────── the pipeline ─────────────────────────

/// Dropping/canceling a pipeline also cancels its in-flight provider tasks.
struct TranscriptTasks(Vec<(usize, JoinHandle<anyhow::Result<String>>)>);
impl Drop for TranscriptTasks {
    fn drop(&mut self) {
        for (_, task) in &self.0 { task.abort(); }
    }
}

pub enum Transcripts {
    /// Chunk transcriptions already running (index, handle), tail included.
    Live(Vec<(usize, JoinHandle<anyhow::Result<String>>)>),
    Samples(Arc<Vec<i16>>),
}

pub struct ProcessInput {
    /// Pre-populated: id, mode, created_at, app info, selected text, duration, audio_path.
    pub row: HistoryRow,
    pub ctx: AppContext,
    pub settings: Settings,
    pub transcripts: Transcripts,
    /// Insert/answer + alerts + sounds (live sessions, undo, alert retry). False for the history "retry".
    pub deliver: bool,
    /// Write the history row (false when retention is "never").
    pub store: bool,
}

pub struct Report {
    pub row: HistoryRow,
}

enum Delivery {
    Insert,
    Replace,
    Answer,
}

struct Stage {
    text: String,
    delivery: Delivery,
    warning: Option<String>,
}

async fn persist(app: &AppHandle, row: &HistoryRow, store: bool) {
    if store {
        let db = app.state::<AppState>().db.clone();
        let r = row.clone();
        match tauri::async_runtime::spawn_blocking(move || db.upsert_history(&r)).await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => log::error!("cannot write history row {}: {e:#}", short_id(&row.id)),
            Err(e) => log::error!("history write task failed: {e}"),
        }
    }
    emit_history_changed(app);
}

async fn rewrite_stage(
    app: &AppHandle,
    settings: &Settings,
    mode: Mode,
    raw: &str,
    ctx: &AppContext,
    selected: Option<String>,
    duration_secs: f64,
) -> Result<Stage, String> {
    let ep = settings.llm.active_endpoint();
    let have_llm = settings.llm.enabled && !ep.api_key.trim().is_empty();
    if !have_llm {
        if mode == Mode::Ask {
            return Err("问任何问题需要先在设置中启用并配置改写模型".into());
        }
        let warning = (mode == Mode::Translation).then(|| "未配置改写模型，无法翻译".to_string());
        return Ok(Stage { text: raw.to_string(), delivery: Delivery::Insert, warning });
    }

    let db = app.state::<AppState>().db.clone();
    let dictionary = tauri::async_runtime::spawn_blocking(move || db.words_for_prompt(300))
        .await
        .ok()
        .and_then(|r| r.ok())
        .unwrap_or_default();
    let req = RewriteRequest {
        mode,
        transcript: raw.to_string(),
        selected_text: if mode == Mode::Ask { selected } else { None },
        context: ctx.clone(),
        dictionary,
        languages: if settings.auto_select_languages { vec![] } else { settings.selected_languages.clone() },
        translation_target: (mode == Mode::Translation).then(|| settings.translation_targets.first().cloned()).flatten(),
        custom_instructions: settings.style.custom_instructions.clone(),
        smart_formatting: settings.style.smart_formatting,
        long_lists: settings.style.long_form_style == "list",
        profile: if settings.style.ai_personalization { app.state::<AppState>().db.profile().ok() } else { None },
        duration_secs,
    };
    let limit = Duration::from_secs(settings.llm.timeout_secs as u64 + (duration_secs / 60.0 * 6.0).ceil() as u64 + 8);
    let result = match tokio::time::timeout(limit, providers::rewrite(&settings.llm, &req)).await {
        Ok(r) => r,
        Err(_) => Err(anyhow::anyhow!("改写超时")),
    };
    match result {
        Ok(outcome) => {
            let (text, delivery) = match outcome {
                RewriteOutcome::Insert(t) => (t, Delivery::Insert),
                RewriteOutcome::Replace(t) => (t, Delivery::Replace),
                RewriteOutcome::Answer(t) => (t, Delivery::Answer),
            };
            if text.trim().is_empty() {
                if mode == Mode::Ask {
                    return Err("改写模型返回了空内容".into());
                }
                return Ok(Stage {
                    text: raw.to_string(),
                    delivery: Delivery::Insert,
                    warning: Some("改写模型返回了空内容".into()),
                });
            }
            Ok(Stage { text, delivery, warning: None })
        }
        Err(e) => {
            let msg = err_text(&e);
            if mode == Mode::Ask {
                Err(msg)
            } else {
                Ok(Stage { text: raw.to_string(), delivery: Delivery::Insert, warning: Some(msg) })
            }
        }
    }
}

/// "Provider · model", identical to the label the usage table builds for the same request.
fn asr_label(s: &Settings) -> String {
    match s.asr.provider.as_str() {
        "cloudflare" => format!("Cloudflare · {}", s.asr.cloudflare.model),
        "azure" => "Azure Speech · streaming".into(),
        "google" => format!("Google Speech · {}", s.asr.google.model),
        _ => format!("MiMo · {}", s.asr.mimo.model),
    }
}

fn llm_label(s: &Settings) -> String {
    use crate::types::LlmProvider;
    let name = match s.llm.provider {
        LlmProvider::Gemini => "Gemini",
        LlmProvider::Anthropic => "Anthropic",
        LlmProvider::Openai => "OpenAI 兼容服务",
    };
    format!("{name} · {}", s.llm.active_endpoint().model)
}

async fn fail(app: &AppHandle, mut row: HistoryRow, msg: String, deliver: bool, store: bool) -> Report {
    log::warn!("[{}] session failed: {msg}", short_id(&row.id));
    row.status = "error".into();
    row.error_message = Some(msg.clone());
    persist(app, &row, store).await;
    if deliver {
        play(app, Sound::Error);
        notify::alert("transcription_error", "出现问题", "error")
            .desc(msg.clone())
            .action("retry", "重试", "primary")
            .duration(Some(12_000))
            .session(&row.id)
            .show(app);
    }
    Report { row }
}

pub async fn run(app: AppHandle, input: ProcessInput) -> Report {
    let ProcessInput { mut row, ctx, settings, transcripts, deliver, store } = input;
    let tag = short_id(&row.id).to_string();
    let mode = row.mode;
    let t0 = Instant::now();

    // 1. transcript
    let raw = match transcripts {
        Transcripts::Live(handles) => {
            let mut handles = TranscriptTasks(handles);
            handles.0.sort_by_key(|(i, _)| *i);
            let mut parts = Vec::new();
            for (i, h) in &mut handles.0 {
                match h.await {
                    Ok(Ok(t)) => parts.push(t),
                    Ok(Err(e)) => return fail(&app, row, err_text(&e), deliver, store).await,
                    Err(e) => return fail(&app, row, format!("识别任务异常：{e}"), deliver, store).await,
                }
                log::trace!("[{tag}] chunk {i} collected");
            }
            join_transcripts(&parts)
        }
        Transcripts::Samples(samples) => match transcribe_samples(&app, &settings, &samples, &tag).await {
            Ok(t) => t,
            Err(e) => return fail(&app, row, err_text(&e), deliver, store).await,
        },
    };
    let asr_ms = t0.elapsed().as_millis();

    if raw.trim().is_empty() {
        log::info!("[{tag}] empty transcript (asr wait {asr_ms} ms)");
        row.status = "empty".into();
        row.raw_text = None;
        row.error_message = None;
        persist(&app, &row, store).await;
        return Report { row };
    }
    row.raw_text = Some(raw.clone());

    // 2. rewrite
    let t1 = Instant::now();
    let stage = match rewrite_stage(&app, &settings, mode, &raw, &ctx, row.selected_text.clone(), row.duration_secs).await {
        Ok(s) => s,
        Err(msg) => return fail(&app, row, msg, deliver, store).await,
    };
    let llm_ms = t1.elapsed().as_millis();
    if let Some(w) = &stage.warning {
        log::warn!("[{tag}] rewrite fallback to raw text: {w}");
        if deliver {
            notify::alert("rewrite_failed", "改写失败，已插入原始转录", "warning").desc(w.clone()).show(&app);
        }
    }

    // 3. deliver
    let mut inserted = false;
    if deliver {
        match stage.delivery {
            Delivery::Insert | Delivery::Replace => {
                let replace = matches!(stage.delivery, Delivery::Replace);
                match deliver_text(&ctx, &stage.text, replace).await {
                    Delivered::Inserted => inserted = true,
                    Delivered::NotEditable => {
                        log::info!("[{tag}] no editable field focused; offering copy");
                        notify::alert("copy_last", "复制最后的转录", "info")
                            .desc("当前没有可输入的文本框")
                            .action("copy", "复制", "primary")
                            .duration(Some(10_000))
                            .text(stage.text.clone())
                            .show(&app);
                    }
                    Delivered::Failed(e) => {
                        log::error!("[{tag}] insert failed: {e}");
                        play(&app, Sound::Error);
                        notify::alert("insert_failed", "无法插入文本", "error")
                            .desc(e)
                            .action("copy", "复制", "primary")
                            .duration(Some(10_000))
                            .text(stage.text.clone())
                            .show(&app);
                    }
                }
            }
            Delivery::Answer => {
                let answer = AskAnswer { id: row.id.clone(), question: raw.clone(), answer: stage.text.clone() };
                if let Err(e) = app.emit_to("bar", events::BAR_ANSWER, &answer) {
                    log::warn!("cannot emit answer: {e}");
                }
            }
        }
    }

    // 4. history
    if deliver {
        // Live sessions only: a history retry would otherwise overwrite the original timings.
        row.asr_ms = Some(asr_ms.min(u32::MAX as u128) as u32);
        let used_llm = settings.llm.enabled && !settings.llm.active_endpoint().api_key.trim().is_empty();
        row.llm_ms = used_llm.then(|| llm_ms.min(u32::MAX as u128) as u32);
        row.total_ms = Some(t0.elapsed().as_millis().min(u32::MAX as u128) as u32);
        row.asr_provider = Some(asr_label(&settings));
        row.llm_model = used_llm.then(|| llm_label(&settings));
    }
    row.status = "completed".into();
    row.error_message = None;
    row.words = stats::count_words(&stage.text);
    row.refined_text = Some(stage.text.clone());
    persist(&app, &row, store).await;
    if deliver {
        *app.state::<AppState>().last_text.lock() = Some(stage.text.clone());
    }

    log::info!(
        "[{tag}] done mode={} audio={:.1}s asr_wait={asr_ms}ms llm={llm_ms}ms total={}ms chars={}",
        mode.as_str(),
        row.duration_secs,
        t0.elapsed().as_millis(),
        stage.text.chars().count()
    );

    // 5. auto-dictionary
    if inserted && mode == Mode::Dictation && (settings.style.auto_add_dictionary || settings.style.ai_personalization) {
        spawn_auto_dictionary(&app, stage.text.clone(), row.id.clone(), row.app_name.clone(), ctx.pid, store);
    }
    Report { row }
}

fn spawn_auto_dictionary(app: &AppHandle, inserted: String, id: String, app_name: Option<String>, pid: Option<i32>, store: bool) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut latest = None;
        for wait in [3, 7, 20] {
            tokio::time::sleep(Duration::from_secs(wait)).await;
            let field = tauri::async_runtime::spawn_blocking(move || {
                let current = context::capture(false, false);
                if pid.is_some() && current.pid != pid { return None; }
                context::focused_text_value()
            }).await.ok().flatten();
            let Some(field) = field else { continue };
            let Some(region) = autodict::edited_region(&inserted, &field) else { continue };
            latest = Some(region.clone());
            let state = app.state::<AppState>();
            if state.settings.read().style.auto_add_dictionary {
                if let Some(word) = autodict::detect_correction(&inserted, &region) {
                    if state.db.add_words(&[word.clone()], "auto").is_ok_and(|v| !v.is_empty()) {
                        emit_history_changed(&app);
                        notify::alert("dictionary_added", format!("“{word}” 已添加到您的词典"), "success")
                            .duration(Some(3500)).show(&app);
                    }
                }
            }
        }
        let state = app.state::<AppState>();
        let settings = state.settings();
        if store && settings.history_retention != "never" && settings.style.ai_personalization {
            if let Some(final_text) = latest.filter(|text| text != &inserted) {
                if let Err(e) = state.db.save_edit(&id, &inserted, &final_text, app_name.as_deref()) {
                    log::debug!("cannot save edit: {e:#}");
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joins_cjk_without_space_and_latin_with_space() {
        let parts = vec!["你好，".to_string(), "世界".to_string()];
        assert_eq!(join_transcripts(&parts), "你好，世界");
        let parts = vec!["hello world".to_string(), "how are you".to_string()];
        assert_eq!(join_transcripts(&parts), "hello world how are you");
        let parts = vec!["你好".to_string(), "".to_string(), "hello".to_string()];
        assert_eq!(join_transcripts(&parts), "你好hello");
    }

    #[test]
    fn short_audio_is_not_split() {
        let s = vec![1i16; 60 * 16_000];
        assert_eq!(split_samples(&s).len(), 1);
    }

    #[test]
    fn long_audio_is_split_at_quiet_spot() {
        let sr = 16_000usize;
        let mut s = vec![5000i16; 300 * sr];
        // quiet frame at ~80 s
        for x in &mut s[80 * sr..80 * sr + 3200] {
            *x = 0;
        }
        let parts = split_samples(&s);
        assert!(parts.len() >= 3);
        assert!(parts[0].len() >= 75 * sr && parts[0].len() <= 90 * sr);
        assert!((parts[0].len() as i64 - (80 * sr + 800) as i64).abs() < 2000);
        assert_eq!(parts.iter().map(|p| p.len()).sum::<usize>(), s.len());
        assert!(parts.iter().all(|p| p.len() <= 120 * sr));
    }
}
