//! Tauri commands (names/args mirror `src/lib/ipc.ts`). Async so they never run on the main thread;
//! errors are user-readable Chinese strings.

use crate::audio::{Recorder, RecorderSink};
use crate::controller::Cmd;
use crate::pipeline::{self, ProcessInput, Transcripts};
use crate::platform::{self, insert, permissions};
use crate::state::{emit_history_changed, save_and_apply, AppState, Shared};
use crate::types::*;
use crate::{importer, stats};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, State};

type CmdResult<T> = Result<T, String>;

fn blocking_err(e: tauri::Error) -> String {
    format!("内部任务失败：{e}")
}

fn any_err(e: anyhow::Error) -> String {
    pipeline::err_text(&e)
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> CmdResult<T> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(blocking_err)
}

// ───────────────────────── app / settings ─────────────────────────

#[tauri::command]
pub async fn check_update(app: AppHandle) -> CmdResult<UpdateInfo> {
    crate::update::check(&app).await
}

/// Downloads, installs and relaunches (only returns on failure).
#[tauri::command]
pub async fn install_update(app: AppHandle) -> CmdResult<()> {
    crate::update::install(&app).await
}

#[tauri::command]
pub async fn get_update_state() -> CmdResult<Option<UpdateInfo>> {
    Ok(crate::update::cached())
}

#[tauri::command]
pub async fn app_info(app: AppHandle) -> CmdResult<AppInfo> {
    Ok(AppInfo { name: "Hoshiro".into(), version: app.package_info().version.to_string(), platform: std::env::consts::OS.into() })
}

/// Route requested before the hub finished loading (see `notify::navigate`).
#[tauri::command]
pub fn take_pending_route() -> Option<String> {
    crate::notify::take_pending_route()
}

#[tauri::command]
pub async fn get_settings(state: State<'_, AppState>) -> CmdResult<Settings> {
    Ok(state.settings())
}

#[tauri::command]
pub async fn save_settings(app: AppHandle, settings: Settings) -> CmdResult<Settings> {
    save_and_apply(&app, move |s| *s = settings).await
}

#[tauri::command]
pub async fn complete_onboarding(app: AppHandle) -> CmdResult<()> {
    save_and_apply(&app, |s| s.onboarding_completed = true).await.map(|_| ())
}

// ───────────────────────── permissions ─────────────────────────

#[tauri::command]
pub async fn get_permissions() -> CmdResult<Permissions> {
    blocking(permissions::check).await
}

#[tauri::command]
pub async fn request_permission(kind: PermissionKind) -> CmdResult<Permissions> {
    blocking(move || permissions::request(kind)).await
}

#[tauri::command]
pub async fn open_permission_settings(kind: PermissionKind) -> CmdResult<()> {
    blocking(move || permissions::open_settings(kind)).await
}

#[tauri::command]
pub async fn open_keyboard_settings() -> CmdResult<()> {
    blocking(permissions::open_keyboard_settings).await
}

// ───────────────────────── audio ─────────────────────────

#[tauri::command]
pub async fn list_microphones() -> CmdResult<Vec<MicDevice>> {
    // Enumeration blocks on a pending permission prompt, so only do it once access is granted.
    blocking(|| {
        if permissions::check().microphone == PermissionState::Granted {
            Recorder::list_devices()
        } else {
            Vec::new()
        }
    })
    .await
}

struct MicTestSink {
    app: AppHandle,
    epoch: Instant,
    last_ms: AtomicU64,
}

impl RecorderSink for MicTestSink {
    fn on_level(&self, level: f32) {
        let now = self.epoch.elapsed().as_millis() as u64;
        if now.saturating_sub(self.last_ms.load(Ordering::Relaxed)) < 28 {
            return;
        }
        self.last_ms.store(now, Ordering::Relaxed);
        let _ = self.app.emit_to("main", events::MIC_TEST_LEVEL, level.clamp(0.0, 1.0));
    }
    fn on_chunk(&self, _chunk: crate::audio::AudioChunk) {}
}

async fn stop_mic_test_inner(state: &AppState) {
    let rec = state.mic_test.lock().take();
    if let Some(rec) = rec {
        let _ = tauri::async_runtime::spawn_blocking(move || rec.0.cancel()).await;
    }
}

#[tauri::command]
pub async fn start_mic_test(app: AppHandle, state: State<'_, AppState>, device_id: Option<String>) -> CmdResult<()> {
    stop_mic_test_inner(&state).await;
    let device = device_id.filter(|d| !d.is_empty() && d != "default" && d != "auto");
    let rec = Arc::new(Shared(Recorder::new()));
    let sink = Arc::new(MicTestSink { app: app.clone(), epoch: Instant::now(), last_ms: AtomicU64::new(0) });
    let r = rec.clone();
    blocking(move || r.0.start(device, sink)).await?.map_err(|e| format!("无法打开麦克风：{}", any_err(e)))?;
    *state.mic_test.lock() = Some(rec.clone());

    // Safety net: never leave the mic open.
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(120)).await;
        let st = handle.state::<AppState>();
        let same = st.mic_test.lock().as_ref().is_some_and(|cur| Arc::ptr_eq(cur, &rec));
        if same {
            stop_mic_test_inner(&st).await;
        }
    });
    Ok(())
}

#[tauri::command]
pub async fn stop_mic_test(state: State<'_, AppState>) -> CmdResult<()> {
    stop_mic_test_inner(&state).await;
    Ok(())
}

// ───────────────────────── shortcut capture ─────────────────────────

#[tauri::command]
pub async fn start_shortcut_capture(app: AppHandle, state: State<'_, AppState>) -> CmdResult<()> {
    let engine = state.engine.lock();
    let Some(engine) = engine.as_ref() else {
        return Err("快捷键引擎尚未启动，请先授予「辅助功能」和「输入监控」权限".into());
    };
    let handle = app.clone();
    engine.0.start_capture(Arc::new(move |keys, done| {
        let _ = handle.emit_to("main", events::SHORTCUT_CAPTURE, ShortcutCaptureEvent { keys, done });
    }));
    Ok(())
}

#[tauri::command]
pub async fn stop_shortcut_capture(state: State<'_, AppState>) -> CmdResult<()> {
    if let Some(engine) = state.engine.lock().as_ref() {
        engine.0.stop_capture();
    }
    Ok(())
}

// ───────────────────────── history ─────────────────────────

#[tauri::command]
pub async fn list_history(state: State<'_, AppState>, query: HistoryQuery) -> CmdResult<Vec<HistoryItem>> {
    let db = state.db.clone();
    blocking(move || db.list_history(&query).map(|rows| rows.iter().map(|r| db.to_item(r)).collect::<Vec<_>>()))
        .await?
        .map_err(|e| format!("读取历史记录失败：{}", any_err(e)))
}

#[tauri::command]
pub async fn delete_history(app: AppHandle, state: State<'_, AppState>, id: String) -> CmdResult<()> {
    let db = state.db.clone();
    blocking(move || db.delete_history(&id)).await?.map_err(any_err)?;
    emit_history_changed(&app);
    Ok(())
}

#[tauri::command]
pub async fn delete_all_history(app: AppHandle, state: State<'_, AppState>) -> CmdResult<()> {
    let db = state.db.clone();
    blocking(move || db.delete_all_history()).await?.map_err(any_err)?;
    emit_history_changed(&app);
    Ok(())
}

#[tauri::command]
pub async fn retry_history(app: AppHandle, state: State<'_, AppState>, id: String) -> CmdResult<HistoryItem> {
    let db = state.db.clone();
    let id2 = id.clone();
    let db2 = db.clone();
    let loaded = blocking(move || {
        let row = db2.get_history(&id2)?.ok_or_else(|| anyhow::anyhow!("找不到这条记录"))?;
        let path = db2.audio_file(&row).ok_or_else(|| anyhow::anyhow!("没有可用的录音文件，无法重试"))?;
        let samples = pipeline::load_audio(&path)?;
        Ok::<_, anyhow::Error>((row, samples))
    })
    .await?
    .map_err(any_err)?;
    let (row, samples) = loaded;
    let ctx = platform::AppContext {
        app_name: row.app_name.clone(),
        bundle_id: row.bundle_id.clone(),
        window_title: row.window_title.clone(),
        selected_text: row.selected_text.clone(),
        ..Default::default()
    };
    let input = ProcessInput {
        row,
        ctx,
        settings: state.settings(),
        transcripts: Transcripts::Samples(Arc::new(samples)),
        deliver: false,
        store: true,
    };
    let report = pipeline::run(app, input).await;
    Ok(db.to_item(&report.row))
}

#[tauri::command]
pub async fn export_history_audio(state: State<'_, AppState>, id: String, path: String) -> CmdResult<()> {
    let db = state.db.clone();
    blocking(move || {
        let row = db.get_history(&id)?.ok_or_else(|| anyhow::anyhow!("找不到这条记录"))?;
        let src = db.audio_file(&row).ok_or_else(|| anyhow::anyhow!("这条记录没有保存录音"))?;
        std::fs::copy(src, &path).map_err(|e| anyhow::anyhow!("导出失败：{e}"))?;
        Ok::<_, anyhow::Error>(())
    })
    .await?
    .map_err(any_err)
}

// ───────────────────────── dictionary ─────────────────────────

/// Trim; reject empty, multi-line or > 64 chars.
pub fn normalize_word(s: &str) -> Option<String> {
    let t = s.trim();
    if t.is_empty() || t.chars().count() > 64 || t.chars().any(|c| c.is_control()) {
        return None;
    }
    Some(t.to_string())
}

fn dedupe_words<'a>(words: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for w in words {
        if let Some(n) = normalize_word(w) {
            if seen.insert(n.to_lowercase()) {
                out.push(n);
            }
        }
    }
    out
}

#[tauri::command]
pub async fn list_dictionary(state: State<'_, AppState>) -> CmdResult<Vec<DictionaryWord>> {
    let db = state.db.clone();
    blocking(move || db.list_words()).await?.map_err(any_err)
}

#[tauri::command]
pub async fn add_dictionary_words(
    state: State<'_, AppState>,
    words: Vec<String>,
    source: Option<String>,
) -> CmdResult<Vec<DictionaryWord>> {
    let source = match source.as_deref() {
        Some("auto") => "auto",
        _ => "manual",
    };
    let words = dedupe_words(words.iter().map(String::as_str));
    if words.is_empty() {
        return Err("词条不能为空，且长度不能超过 64 个字符".into());
    }
    let db = state.db.clone();
    blocking(move || db.add_words(&words, source)).await?.map_err(any_err)
}

#[tauri::command]
pub async fn update_dictionary_word(state: State<'_, AppState>, id: String, word: String) -> CmdResult<DictionaryWord> {
    let word = normalize_word(&word).ok_or("词条不能为空，且长度不能超过 64 个字符")?;
    let db = state.db.clone();
    blocking(move || db.update_word(&id, &word))
        .await?
        .map_err(|e| {
            if format!("{e:#}").contains("UNIQUE") {
                "词典中已有这个词".to_string()
            } else {
                any_err(e)
            }
        })?
        .ok_or_else(|| "找不到这个词条".to_string())
}

#[tauri::command]
pub async fn delete_dictionary_words(state: State<'_, AppState>, ids: Vec<String>) -> CmdResult<()> {
    let db = state.db.clone();
    blocking(move || db.delete_words(&ids)).await?.map(|_| ()).map_err(any_err)
}

/// First column of every CSV record; an optional header row is dropped; duplicates removed.
pub fn parse_csv_words(text: &str) -> Vec<String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut records: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut first_done = false;
    let mut in_quotes = false;
    let mut chars = text.chars().peekable();
    let mut push_record = |field: &mut String, first_done: &mut bool| {
        records.push(std::mem::take(field));
        *first_done = false;
    };
    while let Some(c) = chars.next() {
        if in_quotes {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    if !first_done {
                        field.push('"');
                    }
                } else {
                    in_quotes = false;
                }
            } else if !first_done {
                field.push(c);
            }
            continue;
        }
        match c {
            '"' if field.is_empty() && !first_done => in_quotes = true,
            ',' | ';' | '\t' => first_done = true,
            '\r' => {}
            '\n' => push_record(&mut field, &mut first_done),
            _ => {
                if !first_done {
                    field.push(c);
                }
            }
        }
    }
    if !field.is_empty() || first_done {
        push_record(&mut field, &mut first_done);
    }
    let mut iter = records.into_iter().peekable();
    if let Some(first) = iter.peek() {
        let l = first.trim().to_lowercase();
        if ["word", "words", "term", "terms", "vocabulary", "hotword", "词", "词汇", "单词", "词语", "词条", "词典"]
            .contains(&l.as_str())
        {
            iter.next();
        }
    }
    let all: Vec<String> = iter.collect();
    dedupe_words(all.iter().map(String::as_str))
}

#[tauri::command]
pub async fn preview_dictionary_csv(path: String) -> CmdResult<Vec<String>> {
    blocking(move || {
        let meta = std::fs::metadata(&path).map_err(|e| format!("无法读取文件：{e}"))?;
        if meta.len() > 5 * 1024 * 1024 {
            return Err("文件超过 5 MB，无法导入".to_string());
        }
        let bytes = std::fs::read(&path).map_err(|e| format!("无法读取文件：{e}"))?;
        let text = String::from_utf8(bytes).map_err(|_| "文件不是 UTF-8 编码".to_string())?;
        let words = parse_csv_words(&text);
        if words.is_empty() {
            return Err("没有找到可导入的词条".to_string());
        }
        Ok(words)
    })
    .await?
}

// ───────────────────────── stats ─────────────────────────

#[tauri::command]
pub async fn get_stats(state: State<'_, AppState>) -> CmdResult<Stats> {
    let db = state.db.clone();
    blocking(move || {
        db.stat_rows().map(|rows| stats::compute(&rows, chrono::Local::now().date_naive()))
    })
    .await?
    .map_err(|e| format!("统计失败：{}", any_err(e)))
}

// ───────────────────────── providers ─────────────────────────

#[tauri::command]
pub async fn test_asr(config: serde_json::Value, state: State<'_, AppState>) -> CmdResult<TestResult> {
    // Continue accepting the original MiMo-only command payload.
    let config: AsrConfig = if config.get("provider").is_some() { serde_json::from_value(config).map_err(|e| e.to_string())? }
        else { AsrConfig { mimo: serde_json::from_value(config).map_err(|e| e.to_string())?, ..Default::default() } };
    let settings = state.settings();
    let words = state.db.words_for_prompt(100).map_err(any_err)?;
    Ok(crate::providers::asr::test(&config, &settings.selected_languages, &words).await)
}

#[tauri::command]
pub async fn test_llm(state: State<'_, AppState>, provider: LlmProvider, config: LlmEndpointConfig) -> CmdResult<TestResult> {
    let settings = state.settings();
    let effort = crate::providers::llm::Effort::parse(&settings.llm.reasoning_effort);
    // Cloudflare is configured by account id, not base URL: use the saved settings (with the speech-recognition fallback).
    let config = if provider == LlmProvider::Cloudflare { settings.llm.resolved(&settings.asr).endpoint_for(provider) } else { config };
    Ok(crate::providers::llm::test_llm_with(provider, &config, effort).await)
}

#[tauri::command]
pub async fn list_models(provider: LlmProvider, config: LlmEndpointConfig) -> CmdResult<Vec<String>> {
    crate::providers::list_models(provider, &config).await.map_err(|e| format!("获取模型列表失败：{}", any_err(e)))
}

// ───────────────────────── migration ─────────────────────────

#[tauri::command]
pub async fn detect_typeless() -> CmdResult<TypelessDetection> {
    blocking(importer::detect).await
}

#[tauri::command]
pub async fn import_typeless(
    app: AppHandle,
    state: State<'_, AppState>,
    options: TypelessImportOptions,
) -> CmdResult<TypelessImportResult> {
    let dir = importer::typeless_dir().ok_or("无法定位 Typeless 数据目录")?;
    if !dir.is_dir() {
        return Err("未找到 Typeless 的数据目录".into());
    }
    let mut result = TypelessImportResult::default();

    if options.settings {
        let d = dir.clone();
        match blocking(move || importer::read_settings_json(&d)).await? {
            Ok(v) => {
                let (_, msgs) = importer::map_settings(&v, &state.settings());
                result.messages.extend(msgs);
                save_and_apply(&app, move |s| {
                    let (merged, _) = importer::map_settings(&v, s);
                    *s = merged;
                })
                .await?;
                result.settings_imported = true;
            }
            Err(e) => result.messages.push(format!("设置导入失败：{}", any_err(e))),
        }
    }

    if options.history {
        let db = state.db.clone();
        let d = dir.clone();
        match blocking(move || importer::import_history(&d, &db)).await? {
            Ok((imported, skipped)) => {
                result.history_imported = imported;
                result.history_skipped = skipped;
                emit_history_changed(&app);
            }
            Err(e) => result.messages.push(format!("历史记录导入失败：{}", any_err(e))),
        }
    }

    importer::fill_messages(&mut result, &options);
    log::info!(
        "typeless import: settings={} history imported={} skipped={}",
        result.settings_imported,
        result.history_imported,
        result.history_skipped
    );
    Ok(result)
}

#[tauri::command]
pub async fn quit_typeless() -> CmdResult<()> {
    let ok = blocking(|| {
        let asked = permissions::terminate_app(importer::TYPELESS_BUNDLE_ID);
        asked || permissions::running_pids(importer::TYPELESS_BUNDLE_ID).is_empty()
    })
    .await?;
    if ok {
        Ok(())
    } else {
        Err("无法退出 Typeless，请手动退出".into())
    }
}

// ───────────────────────── misc ─────────────────────────

#[tauri::command]
pub async fn copy_text(text: String) -> CmdResult<()> {
    blocking(move || insert::copy_to_clipboard(&text)).await?.map_err(|e| format!("复制失败：{}", any_err(e)))
}

#[tauri::command]
pub async fn open_url(app: AppHandle, url: String) -> CmdResult<()> {
    use tauri_plugin_opener::OpenerExt;
    let allowed = ["https://", "http://", "x-apple.systempreferences:", "mailto:"];
    if !allowed.iter().any(|p| url.starts_with(p)) {
        return Err("不支持打开该链接".into());
    }
    app.opener().open_url(url, None::<&str>).map_err(|e| format!("无法打开链接：{e}"))
}

// ───────────────────────── floating bar ─────────────────────────

#[tauri::command]
pub async fn bar_action(state: State<'_, AppState>, action: String) -> CmdResult<()> {
    state.send(Cmd::BarAction(action));
    Ok(())
}

#[tauri::command]
pub async fn bar_alert_action(state: State<'_, AppState>, alert_id: String, action_id: String) -> CmdResult<()> {
    state.send(Cmd::AlertAction { alert_id, action_id });
    Ok(())
}

#[tauri::command]
pub async fn bar_set_hit_rects(app: AppHandle, rects: Vec<HitRect>) -> CmdResult<()> {
    platform::bar::set_hit_rects(&app, rects);
    Ok(())
}

#[tauri::command]
pub async fn bar_get_state(state: State<'_, AppState>) -> CmdResult<BarState> {
    Ok(state.bar.lock().clone())
}

#[tauri::command]
pub async fn bar_close_answer(_id: String) -> CmdResult<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_normalisation() {
        assert_eq!(normalize_word("  Typeless \n").as_deref(), Some("Typeless"));
        assert_eq!(normalize_word(""), None);
        assert_eq!(normalize_word(&"a".repeat(65)), None);
        assert!(normalize_word(&"字".repeat(64)).is_some());
        assert_eq!(normalize_word("a\nb"), None);
    }

    #[test]
    fn csv_parsing() {
        assert_eq!(parse_csv_words("word\nTauri\nMiMo\ntauri\n"), vec!["Tauri", "MiMo"]);
        assert_eq!(parse_csv_words("\u{feff}Hoshiro,ignored\r\n\"Hello, World\",x\r\n"), vec!["Hoshiro", "Hello, World"]);
        assert_eq!(parse_csv_words("词汇\n星辰\n\n  \n"), vec!["星辰"]);
        assert_eq!(parse_csv_words("\"say \"\"hi\"\"\"\nplain"), vec!["say \"hi\"", "plain"]);
        assert!(parse_csv_words("").is_empty());
        let long = "x".repeat(70);
        assert_eq!(parse_csv_words(&format!("{long}\nok")), vec!["ok"]);
    }
}

#[tauri::command]
pub async fn get_profile(app: AppHandle) -> CmdResult<PersonalProfile> {
    crate::personalize::snapshot(&app).map_err(any_err)
}

#[tauri::command]
pub async fn run_personalization(app: AppHandle) -> CmdResult<PersonalProfile> {
    crate::personalize::run(&app, true).await.map_err(any_err)
}

#[tauri::command]
pub async fn update_profile(app: AppHandle, mut profile: PersonalProfile) -> CmdResult<PersonalProfile> {
    let state = app.state::<AppState>();
    let _gate = state.profile_gate.try_lock().map_err(|_| "请等待当前分析完成")?;
    let old = state.db.profile().map_err(any_err)?;
    profile.addendum_edited_by_user |= profile.prompt_addendum != old.prompt_addendum;
    profile.updated_at = old.updated_at; profile.analyzed_sessions = old.analyzed_sessions;
    profile.next_run_at = old.next_run_at; profile.last_error = old.last_error;
    profile.running = false;
    crate::personalize::sanitize(&mut profile);
    state.db.save_profile(&profile, &[]).map_err(any_err)?;
    let profile = crate::personalize::snapshot(&app).map_err(any_err)?;
    let _ = app.emit(events::PROFILE_UPDATED, &profile);
    Ok(profile)
}

#[tauri::command]
pub async fn reset_profile(app: AppHandle) -> CmdResult<PersonalProfile> {
    let state = app.state::<AppState>();
    let _gate = state.profile_gate.try_lock().map_err(|_| "请等待当前分析完成")?;
    state.db.reset_profile().map_err(any_err)?;
    let profile = crate::personalize::snapshot(&app).map_err(any_err)?;
    let _ = app.emit(events::PROFILE_UPDATED, &profile);
    Ok(profile)
}

#[tauri::command]
pub async fn get_usage(state: State<'_, AppState>, days: u32) -> CmdResult<UsageStats> {
    let db = state.db.clone();
    blocking(move || crate::usage::summary(&db, days)).await?.map_err(any_err)
}

/// App icons as PNG data URLs, keyed by bundle id (missing apps are omitted). Cached for the process lifetime.
#[tauri::command]
pub async fn app_icons(bundle_ids: Vec<String>) -> CmdResult<std::collections::HashMap<String, String>> {
    use base64::Engine;
    static CACHE: once_cell::sync::Lazy<parking_lot::Mutex<std::collections::HashMap<String, Option<String>>>> =
        once_cell::sync::Lazy::new(Default::default);
    let out = tauri::async_runtime::spawn_blocking(move || {
        let mut out = std::collections::HashMap::new();
        for id in bundle_ids.into_iter().take(32) {
            let cached = CACHE.lock().get(&id).cloned();
            let url = match cached {
                Some(v) => v,
                None => {
                    let v = crate::platform::icons::app_icon_png(&id, 64)
                        .map(|png| format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(png)));
                    CACHE.lock().insert(id.clone(), v.clone());
                    v
                }
            };
            if let Some(url) = url {
                out.insert(id, url);
            }
        }
        out
    })
    .await
    .map_err(|e| e.to_string())?;
    Ok(out)
}
