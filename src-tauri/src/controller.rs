//! Dictation session state machine (SPEC §3). Every hotkey / bar action funnels into one channel that a
//! single task consumes, so transitions are strictly serialised. Slow work (ASR, LLM, paste) runs in a
//! separate processing task that can be aborted by Cancel.

use crate::audio::sounds::Sound;
use crate::audio::{AudioChunk, Recorder, RecorderSink};
use crate::db::HistoryRow;
use crate::notify::{self, alert, update_bar};
use crate::pipeline::{self, short_id, ProcessInput, Transcripts};
use crate::platform::hotkey::{HotkeyAction, SessionPhase};
use crate::platform::media::{self, MuteGuard};
use crate::platform::{bar, context, insert, permissions, AppContext};
use crate::state::{emit_history_changed, save_and_apply, AppState, Shared};
use crate::types::{events, BarPhase, Mode, PermissionKind, PermissionState};
use crate::{config, importer};
use futures_util::FutureExt;
use parking_lot::Mutex;
use std::collections::VecDeque;
use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::async_runtime::JoinHandle;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::mpsc::{self, UnboundedSender};

const MAX_SESSION_SECS: u64 = 20 * 60;
const WARN_SESSION_SECS: u64 = 19 * 60;
const MIN_CHUNK_SAMPLES: usize = 1_600; // 100 ms

#[derive(Debug)]
pub enum Cmd {
    Hotkey(HotkeyAction),
    BarAction(String),
    AlertAction { alert_id: String, action_id: String },
    Tick { session_id: String },
    ProcessingFinished { id: String },
    SettingsChanged,
    RecorderError { session_id: String, message: String },
}

type ChunkList = Arc<Mutex<Vec<(usize, JoinHandle<anyhow::Result<String>>)>>>;

struct MuteSlot {
    ended: bool,
    guard: Option<Shared<MuteGuard>>,
}

struct Session {
    stream: Arc<Mutex<Option<crate::providers::streaming::PcmSender>>>,
    asr: crate::types::AsrConfig,
    words: Vec<String>,
    id: String,
    mode: Mode,
    locked: bool,
    started: Instant,
    created_at: String,
    ctx_task: Option<JoinHandle<AppContext>>,
    chunks: ChunkList,
    mute: Arc<Mutex<MuteSlot>>,
    tick: JoinHandle<()>,
    warned_limit: bool,
}

#[derive(Clone)]
struct Stored {
    id: String,
    samples: Arc<Vec<i16>>,
    ctx: AppContext,
    mode: Mode,
    duration_secs: f64,
    created_at: String,
}

struct Processing {
    id: String,
    handle: JoinHandle<()>,
}

struct Controller {
    app: AppHandle,
    tx: UnboundedSender<Cmd>,
    recorder: Arc<Shared<Recorder>>,
    session: Option<Session>,
    processing: Option<Processing>,
    stash: VecDeque<Stored>,
    last_key_alert: Option<Instant>,
}

pub fn spawn(app: &AppHandle) -> UnboundedSender<Cmd> {
    let (tx, mut rx) = mpsc::unbounded_channel::<Cmd>();
    let mut ctl = Controller {
        app: app.clone(),
        tx: tx.clone(),
        recorder: Arc::new(Shared(Recorder::new())),
        session: None,
        processing: None,
        stash: VecDeque::new(),
        last_key_alert: None,
    };
    tauri::async_runtime::spawn(async move {
        while let Some(cmd) = rx.recv().await {
            ctl.handle(cmd).await;
        }
        log::warn!("controller loop ended");
    });
    tx
}

// ───────────────────────── recorder sink ─────────────────────────

struct Sink {
    app: AppHandle,
    tx: UnboundedSender<Cmd>,
    session_id: String,
    tag: String,
    chunks: ChunkList,
    asr: crate::types::AsrConfig,
    words: Vec<String>,
    stream: Arc<Mutex<Option<crate::providers::streaming::PcmSender>>>,
    language: &'static str,
    epoch: Instant,
    last_level_ms: AtomicU64,
}

impl RecorderSink for Sink {
    fn on_level(&self, level: f32) {
        let now = self.epoch.elapsed().as_millis() as u64;
        let last = self.last_level_ms.load(Ordering::Relaxed);
        if now.saturating_sub(last) < 28 {
            return;
        }
        self.last_level_ms.store(now, Ordering::Relaxed);
        let _ = self.app.emit_to("bar", events::BAR_LEVEL, level.clamp(0.0, 1.0));
    }

    fn on_error(&self, message: &str) {
        let _ = self.tx.send(Cmd::RecorderError { session_id: self.session_id.clone(), message: message.to_string() });
    }

    fn on_pcm(&self, samples: &[i16]) {
        if let Some(tx) = self.stream.lock().as_ref() { let _ = tx.send(samples.to_vec()); }
    }

    fn on_chunk(&self, chunk: AudioChunk) {
        if self.asr.streaming() { return; }
        if chunk.samples.len() < MIN_CHUNK_SAMPLES {
            return;
        }
        log::debug!("[{}] chunk {} ready ({:.1}s)", self.tag, chunk.index, chunk.samples.len() as f64 / 16_000.0);
        let h = pipeline::spawn_asr(
            self.asr.clone(),
            self.language,
            self.words.clone(),
            chunk.samples,
            format!("{}#{}", self.tag, chunk.index),
        );
        self.chunks.lock().push((chunk.index, h));
    }
}

// ───────────────────────── weak network guard ─────────────────────────

struct WeakNetGuard {
    app: AppHandle,
    timer: JoinHandle<()>,
    alert_id: Arc<Mutex<Option<String>>>,
}

impl WeakNetGuard {
    fn start(app: AppHandle) -> Self {
        let slot: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let (a, s) = (app.clone(), slot.clone());
        let timer = tauri::async_runtime::spawn(async move {
            tokio::time::sleep(Duration::from_secs(7)).await;
            let id = alert("weak_network", "连接比平常慢…", "warning").duration(Some(60_000)).show(&a);
            *s.lock() = Some(id);
        });
        Self { app, timer, alert_id: slot }
    }
}

impl Drop for WeakNetGuard {
    fn drop(&mut self) {
        self.timer.abort();
        if let Some(id) = self.alert_id.lock().take() {
            notify::dismiss_alert(&self.app, &id);
        }
    }
}

// ───────────────────────── helpers ─────────────────────────

fn make_row(id: &str, mode: Mode, ctx: &AppContext, duration: f64, created_at: &str, status: &str) -> HistoryRow {
    let mut row = HistoryRow::new(id.to_string(), mode, status);
    row.created_at = created_at.to_string();
    row.duration_secs = duration;
    row.app_name = ctx.app_name.clone();
    row.bundle_id = ctx.bundle_id.clone();
    row.window_title = ctx.window_title.clone();
    if mode == Mode::Ask {
        row.selected_text = ctx.selected_text.clone();
    }
    row
}

fn ctx_from_row(row: &HistoryRow) -> AppContext {
    AppContext {
        app_name: row.app_name.clone(),
        bundle_id: row.bundle_id.clone(),
        window_title: row.window_title.clone(),
        selected_text: row.selected_text.clone(),
        ..Default::default()
    }
}

impl Controller {
    fn state(&self) -> tauri::State<'_, AppState> {
        self.app.state::<AppState>()
    }

    fn set_phase(&self, p: SessionPhase) {
        self.state().set_phase(p);
    }

    fn show_idle_bar(&self) {
        let settings = self.state().settings();
        update_bar(&self.app, |b| {
            b.phase = notify::idle_phase(&settings);
            b.mode = Mode::Dictation;
            b.locked = false;
            b.elapsed_secs = 0.0;
        });
    }

    fn store_enabled(&self) -> bool {
        self.state().settings.read().history_retention != "never"
    }

    async fn handle(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Hotkey(a) => self.on_hotkey(a).await,
            Cmd::BarAction(a) => self.on_bar_action(&a).await,
            Cmd::AlertAction { alert_id, action_id } => self.on_alert_action(&alert_id, &action_id).await,
            Cmd::Tick { session_id } => self.on_tick(&session_id).await,
            Cmd::ProcessingFinished { id } => {
                if self.processing.as_ref().is_some_and(|p| p.id == id) {
                    self.processing = None;
                    if self.session.is_none() {
                        self.set_phase(SessionPhase::Idle);
                        self.show_idle_bar();
                    }
                }
            }
            Cmd::RecorderError { session_id, message } => {
                if self.session.as_ref().is_some_and(|s| s.id == session_id) {
                    log::error!("[{}] microphone failed mid-recording: {message}", short_id(&session_id));
                    pipeline::play(&self.app, Sound::Error);
                    alert("mic_unavailable", "麦克风不可用", "error").desc(message).duration(Some(6000)).show(&self.app);
                    self.finish().await;
                }
            }
            Cmd::SettingsChanged => {
                if self.session.is_none() && self.processing.is_none() {
                    self.show_idle_bar();
                } else {
                    update_bar(&self.app, |_| {});
                }
            }
        }
    }

    async fn on_hotkey(&mut self, action: HotkeyAction) {
        log::debug!("hotkey action: {action:?}");
        match action {
            HotkeyAction::Start { mode, locked } => self.start(mode, locked).await,
            HotkeyAction::SwitchMode { mode } => self.switch_mode(mode).await,
            HotkeyAction::Lock => self.lock(),
            HotkeyAction::Finish => self.finish().await,
            HotkeyAction::CancelSilently => self.discard(false).await,
            HotkeyAction::TapTooShort => self.discard(true).await,
            HotkeyAction::Cancel => self.cancel().await,
            HotkeyAction::PasteLast => self.paste_last().await,
        }
    }

    async fn on_bar_action(&mut self, action: &str) {
        match action {
            "cancel" => self.cancel().await,
            "finish" => self.finish().await,
            "start" => {
                if self.session.is_none() {
                    self.start(Mode::Dictation, true).await;
                }
            }
            "cycle_translation_target" => {
                let r = save_and_apply(&self.app, |s| {
                    if s.translation_targets.len() > 1 {
                        s.translation_targets.rotate_left(1);
                    }
                })
                .await;
                if let Err(e) = r {
                    log::warn!("cannot cycle translation target: {e}");
                }
                update_bar(&self.app, |_| {});
            }
            other => log::warn!("unknown bar action '{other}'"),
        }
    }

    // ───────────────────────── start ─────────────────────────

    fn busy_alert(&self) {
        alert("busy", "Hoshiro 仍在处理您的上一个转录", "warning")
            .action("cancel_processing", "取消", "secondary")
            .duration(Some(4000))
            .show(&self.app);
    }

    async fn start(&mut self, mode: Mode, locked: bool) {
        if self.processing.is_some() {
            self.busy_alert();
            return;
        }
        if self.session.is_some() {
            return;
        }
        let settings = self.state().settings();
        // Fire-and-forget: opens pooled connections to the ASR/LLM hosts while the mic starts.
        crate::providers::warm::warm_up(&settings);

        let perms = tauri::async_runtime::spawn_blocking(permissions::check).await.ok();
        if perms.is_some_and(|p| p.microphone == PermissionState::Denied) {
            self.refuse_start("microphone", || {
                alert("permission", "需要麦克风权限", "warning")
                    .desc("请在系统设置中允许 Hoshiro 使用麦克风")
                    .action("open_mic_settings", "打开设置", "primary")
                    .duration(Some(8000))
            });
            return;
        }
        if !settings.asr.ready() {
            self.refuse_start("asr_key", || {
                alert("missing_key", "尚未配置语音识别服务", "warning")
                    .desc("请先在设置中配置所选语音识别服务")
                    .action("open_settings", "打开设置", "primary")
                    .duration(Some(8000))
            });
            return;
        }

        let id = uuid::Uuid::new_v4().to_string();
        let tag = short_id(&id).to_string();
        let t0 = Instant::now();

        // State first: the engine must see the new phase before any slow work.
        self.set_phase(if locked { SessionPhase::RecordingLocked } else { SessionPhase::RecordingPtt });
        bar::show(&self.app);
        update_bar(&self.app, |b| {
            b.phase = BarPhase::Recording;
            b.mode = mode;
            b.locked = locked;
            b.elapsed_secs = 0.0;
        });

        let chunks: ChunkList = Arc::new(Mutex::new(Vec::new()));
        let words = self.app.state::<AppState>().db.words_for_prompt(100).unwrap_or_default();
        let stream = Arc::new(Mutex::new(None));
        if settings.asr.streaming() {
            let (tx, task) = crate::providers::streaming::start(settings.asr.clone(), settings.selected_languages.clone(), words.clone());
            *stream.lock() = Some(tx);
            chunks.lock().push((0, task));
        }
        let sink = Arc::new(Sink {
            app: self.app.clone(),
            tx: self.tx.clone(),
            session_id: id.clone(),
            tag: tag.clone(),
            chunks: chunks.clone(),
            asr: settings.asr.clone(),
            words: words.clone(),
            stream: stream.clone(),
            language: config::asr_language(&settings),
            epoch: Instant::now(),
            last_level_ms: AtomicU64::new(0),
        });
        // Audible feedback must be instant (Typeless plays it on key-down), so don't wait for the mic.
        pipeline::play(&self.app, Sound::Start);
        let rec = self.recorder.clone();
        let device = settings.microphone_device_id.clone();
        let started = tauri::async_runtime::spawn_blocking(move || rec.0.start(device, sink)).await;
        match started {
            Ok(Ok(())) => {}
            other => {
                let msg = match other {
                    Ok(Err(e)) => pipeline::err_text(&e),
                    Err(e) => e.to_string(),
                    Ok(Ok(())) => unreachable!(),
                };
                stream.lock().take();
                Self::abort_chunks(&chunks);
                log::error!("[{tag}] recorder failed to start: {msg}");
                self.set_phase(SessionPhase::Idle);
                self.show_idle_bar();
                pipeline::play(&self.app, Sound::Error);
                alert("record_failed", "无法开始录音", "error").desc(msg).duration(Some(6000)).show(&self.app);
                return;
            }
        }

        let want_sel = mode == Mode::Ask;
        let ctx_task = tauri::async_runtime::spawn_blocking(move || context::capture(want_sel, want_sel));

        let mute = Arc::new(Mutex::new(MuteSlot { ended: false, guard: None }));
        if settings.mute_while_dictating {
            let slot = mute.clone();
            let delay = if settings.interaction_sounds { Duration::from_millis(120) } else { Duration::ZERO };
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(delay).await;
                let guard = tauri::async_runtime::spawn_blocking(|| Shared(media::mute_system_output())).await.ok();
                let Some(Shared(Some(g))) = guard else { return };
                let mut s = slot.lock();
                if s.ended {
                    drop(s);
                    drop(g);
                } else {
                    s.guard = Some(Shared(g));
                }
            });
        }

        let tx = self.tx.clone();
        let sid = id.clone();
        let tick = tauri::async_runtime::spawn(async move {
            let mut iv = tokio::time::interval(Duration::from_secs(1));
            iv.tick().await;
            loop {
                iv.tick().await;
                if tx.send(Cmd::Tick { session_id: sid.clone() }).is_err() {
                    break;
                }
            }
        });

        log::info!("[{tag}] recording started mode={} locked={locked} (+{} ms)", mode.as_str(), t0.elapsed().as_millis());
        self.session = Some(Session {
            stream,
            asr: settings.asr.clone(),
            words,
            id,
            mode,
            locked,
            started: Instant::now(),
            created_at: crate::db::now_ts(),
            ctx_task: Some(ctx_task),
            chunks,
            mute,
            tick,
            warned_limit: false,
        });
    }

    fn refuse_start(&mut self, key: &str, build: impl FnOnce() -> notify::AlertBuilder) {
        log::info!("start refused: {key}");
        if self.last_key_alert.is_some_and(|t| t.elapsed() < Duration::from_secs(4)) {
            self.set_phase(SessionPhase::Idle);
            return;
        }
        self.last_key_alert = Some(Instant::now());
        self.set_phase(SessionPhase::Idle);
        build().show(&self.app);
    }

    // ───────────────────────── during recording ─────────────────────────

    fn lock(&mut self) {
        let Some(s) = self.session.as_mut() else { return };
        if s.locked {
            return;
        }
        s.locked = true;
        self.set_phase(SessionPhase::RecordingLocked);
        pipeline::play(&self.app, Sound::Lock);
        update_bar(&self.app, |b| b.locked = true);
    }

    async fn switch_mode(&mut self, mode: Mode) {
        let Some(s) = self.session.as_mut() else { return };
        s.mode = mode;
        s.locked = true;
        if mode == Mode::Ask {
            if let Some(old) = s.ctx_task.take() {
                old.abort();
            }
            s.ctx_task = Some(tauri::async_runtime::spawn_blocking(|| context::capture(true, true)));
        }
        self.set_phase(SessionPhase::RecordingLocked);
        pipeline::play(&self.app, Sound::Lock);
        update_bar(&self.app, |b| {
            b.mode = mode;
            b.locked = true;
        });
    }

    async fn on_tick(&mut self, session_id: &str) {
        let Some(s) = self.session.as_mut() else { return };
        if s.id != session_id {
            return;
        }
        let elapsed = s.started.elapsed().as_secs();
        update_bar(&self.app, |b| b.elapsed_secs = elapsed as f64);
        if elapsed >= WARN_SESSION_SECS && !s.warned_limit {
            s.warned_limit = true;
            alert("time_limit", "录音将在 1 分钟内结束", "warning").duration(Some(8000)).show(&self.app);
        }
        if elapsed >= MAX_SESSION_SECS {
            log::info!("[{}] max duration reached, finishing", short_id(&s.id));
            self.finish().await;
        }
    }

    /// Release mute + ticker of a finished session.
    async fn release_session(&self, s: &Session) {
        s.stream.lock().take();
        s.tick.abort();
        let guard = {
            let mut slot = s.mute.lock();
            slot.ended = true;
            slot.guard.take()
        };
        if let Some(g) = guard {
            let _ = tauri::async_runtime::spawn_blocking(move || drop(g)).await;
        }
    }

    async fn stop_recorder(&self) -> Result<crate::audio::StopResult, String> {
        let rec = self.recorder.clone();
        match tauri::async_runtime::spawn_blocking(move || rec.0.stop()).await {
            Ok(Ok(r)) => Ok(r),
            Ok(Err(e)) => Err(pipeline::err_text(&e)),
            Err(e) => Err(e.to_string()),
        }
    }

    fn abort_chunks(chunks: &ChunkList) {
        for (_, h) in chunks.lock().drain(..) {
            h.abort();
        }
    }

    async fn resolve_ctx(task: Option<JoinHandle<AppContext>>) -> AppContext {
        match task {
            Some(t) => match tokio::time::timeout(Duration::from_secs(2), t).await {
                Ok(Ok(c)) => c,
                _ => AppContext::default(),
            },
            None => AppContext::default(),
        }
    }

    // ───────────────────────── finish ─────────────────────────

    async fn finish(&mut self) {
        let Some(mut sess) = self.session.take() else { return };
        let t_finish = Instant::now();
        let tag = short_id(&sess.id).to_string();
        let settings = self.state().settings();

        self.set_phase(SessionPhase::Processing);
        update_bar(&self.app, |b| {
            b.phase = BarPhase::Processing;
            b.locked = false;
            b.elapsed_secs = 0.0;
        });

        let stop = self.stop_recorder().await;
        self.release_session(&sess).await;
        pipeline::play(&self.app, Sound::Stop);

        let stop = match stop {
            Ok(s) => s,
            Err(msg) => {
                log::error!("[{tag}] recorder stop failed: {msg}");
                Self::abort_chunks(&sess.chunks);
                self.set_phase(SessionPhase::Idle);
                self.show_idle_bar();
                pipeline::play(&self.app, Sound::Error);
                alert("record_failed", "录音出现问题", "error").desc(msg).duration(Some(6000)).show(&self.app);
                return;
            }
        };
        log::info!(
            "[{tag}] recording finished: {:.1}s silent={} samples={}",
            stop.duration_secs,
            stop.is_silent,
            stop.all_samples.len()
        );
        let ctx = Self::resolve_ctx(sess.ctx_task.take()).await;

        if stop.is_silent {
            Self::abort_chunks(&sess.chunks);
            let mut row = make_row(&sess.id, sess.mode, &ctx, stop.duration_secs, &sess.created_at, "empty");
            row.audio_path = None;
            self.write_row(row).await;
            self.set_phase(SessionPhase::Idle);
            self.show_idle_bar();
            return;
        }

        let mut handles: Vec<_> = sess.chunks.lock().drain(..).collect();
        if !sess.asr.streaming() && stop.tail.samples.len() >= MIN_CHUNK_SAMPLES {
            handles.push((
                stop.tail.index,
                pipeline::spawn_asr(
                    sess.asr.clone(),
                    config::asr_language(&settings),
                    sess.words.clone(),
                    stop.tail.samples.clone(),
                    format!("{tag}#{}", stop.tail.index),
                ),
            ));
        }

        let samples = Arc::new(stop.all_samples);
        let stored = Stored {
            id: sess.id.clone(),
            samples: samples.clone(),
            ctx: ctx.clone(),
            mode: sess.mode,
            duration_secs: stop.duration_secs,
            created_at: sess.created_at.clone(),
        };
        let mut row = make_row(&sess.id, sess.mode, &ctx, stop.duration_secs, &sess.created_at, "processing");
        let store = self.store_enabled();
        if store {
            row.audio_path = self.save_audio_and_row(&row, samples).await;
        }
        self.push_stash(stored);

        let input = ProcessInput {
            row,
            ctx,
            settings,
            transcripts: Transcripts::Live(handles),
            deliver: true,
            store,
        };
        log::debug!("[{tag}] finish handled in {} ms", t_finish.elapsed().as_millis());
        self.spawn_processing(input);
    }

    /// Writes the WAV and a "processing" row; returns the audio file name when saved.
    async fn save_audio_and_row(&self, row: &HistoryRow, samples: Arc<Vec<i16>>) -> Option<String> {
        let db = self.state().db.clone();
        let mut row = row.clone();
        let id = row.id.clone();
        let res = tauri::async_runtime::spawn_blocking(move || {
            let name = match pipeline::save_audio(db.recordings_dir(), &id, &samples) {
                Ok(n) => Some(n),
                Err(e) => {
                    log::error!("cannot save audio: {e:#}");
                    None
                }
            };
            row.audio_path = name.clone();
            if let Err(e) = db.upsert_history(&row) {
                log::error!("cannot write history row: {e:#}");
            }
            name
        })
        .await;
        emit_history_changed(&self.app);
        res.ok().flatten()
    }

    async fn write_row(&self, row: HistoryRow) {
        if !self.store_enabled() {
            return;
        }
        let db = self.state().db.clone();
        let _ = tauri::async_runtime::spawn_blocking(move || {
            if let Err(e) = db.upsert_history(&row) {
                log::error!("cannot write history row: {e:#}");
            }
        })
        .await;
        emit_history_changed(&self.app);
    }

    fn push_stash(&mut self, s: Stored) {
        self.stash.retain(|x| x.id != s.id);
        self.stash.push_back(s);
        while self.stash.len() > 3 {
            self.stash.pop_front();
        }
    }

    fn spawn_processing(&mut self, input: ProcessInput) {
        let id = input.row.id.clone();
        let (app, tx, id2) = (self.app.clone(), self.tx.clone(), id.clone());
        let handle = tauri::async_runtime::spawn(async move {
            let weak = WeakNetGuard::start(app.clone());
            let result = AssertUnwindSafe(pipeline::run(app.clone(), input)).catch_unwind().await;
            drop(weak);
            if result.is_err() {
                log::error!("[{}] processing task panicked", short_id(&id2));
                mark_error(&app, &id2, "处理时发生内部错误").await;
            }
            let _ = tx.send(Cmd::ProcessingFinished { id: id2 });
        });
        self.set_phase(SessionPhase::Processing);
        update_bar(&self.app, |b| {
            b.phase = BarPhase::Processing;
            b.locked = false;
        });
        self.processing = Some(Processing { id, handle });
    }

    // ───────────────────────── cancel / discard ─────────────────────────

    async fn cancel(&mut self) {
        if let Some(sess) = self.session.take() {
            let tag = short_id(&sess.id).to_string();
            self.set_phase(SessionPhase::Idle);
            self.show_idle_bar();
            let stop = self.stop_recorder().await;
            self.release_session(&sess).await;
            Self::abort_chunks(&sess.chunks);
            pipeline::play(&self.app, Sound::Cancel);
            let Ok(stop) = stop else { return };
            log::info!("[{tag}] cancelled after {:.1}s", stop.duration_secs);
            if stop.is_silent || stop.all_samples.len() < MIN_CHUNK_SAMPLES {
                return;
            }
            let ctx = Self::resolve_ctx(sess.ctx_task).await;
            let samples = Arc::new(stop.all_samples);
            let row = make_row(&sess.id, sess.mode, &ctx, stop.duration_secs, &sess.created_at, "dismissed");
            if self.store_enabled() {
                let mut row = row;
                row.audio_path = self.save_audio_and_row(&row, samples.clone()).await;
            }
            self.push_stash(Stored {
                id: sess.id.clone(),
                samples,
                ctx,
                mode: sess.mode,
                duration_secs: stop.duration_secs,
                created_at: sess.created_at,
            });
            self.cancelled_alert(&sess.id);
        } else if let Some(p) = self.processing.take() {
            log::info!("[{}] processing cancelled", short_id(&p.id));
            p.handle.abort();
            let _ = p.handle.await;
            self.set_phase(SessionPhase::Idle);
            self.show_idle_bar();
            pipeline::play(&self.app, Sound::Cancel);
            self.mark_dismissed(&p.id).await;
            self.cancelled_alert(&p.id);
        }
    }

    async fn mark_dismissed(&self, id: &str) {
        let db = self.state().db.clone();
        let id = id.to_string();
        let stored = self.stash.iter().find(|s| s.id == id).cloned();
        let store = self.store_enabled();
        let _ = tauri::async_runtime::spawn_blocking(move || {
            if !store {
                return;
            }
            let row = match db.get_history(&id) {
                Ok(Some(r)) => Some(r),
                _ => stored.map(|s| make_row(&s.id, s.mode, &s.ctx, s.duration_secs, &s.created_at, "dismissed")),
            };
            if let Some(mut r) = row {
                r.status = "dismissed".into();
                r.error_message = None;
                let _ = db.upsert_history(&r);
            }
        })
        .await;
        emit_history_changed(&self.app);
    }

    fn cancelled_alert(&self, id: &str) {
        alert("transcript_cancelled", "转录已取消", "info")
            .action("undo", "撤销", "primary")
            .duration(Some(5000))
            .session(id)
            .show(&self.app);
    }

    /// CancelSilently / TapTooShort.
    async fn discard(&mut self, hint: bool) {
        let Some(sess) = self.session.take() else { return };
        self.set_phase(SessionPhase::Idle);
        self.show_idle_bar();
        let rec = self.recorder.clone();
        let _ = tauri::async_runtime::spawn_blocking(move || rec.0.cancel()).await;
        self.release_session(&sess).await;
        Self::abort_chunks(&sess.chunks);
        if let Some(t) = sess.ctx_task {
            t.abort();
        }
        log::info!("[{}] discarded (hint={hint})", short_id(&sess.id));
        if hint {
            let label = notify::first_binding_label(&self.state().settings());
            alert("hint", format!("按住 [[{label}]] 说话，双击 [[{label}]] 进入免提模式"), "info")
                .duration(Some(3500))
                .show(&self.app);
        }
    }

    // ───────────────────────── paste last ─────────────────────────

    async fn paste_last(&mut self) {
        if self.session.is_some() {
            return;
        }
        let mut text = self.state().last_text.lock().clone();
        if text.is_none() {
            let db = self.state().db.clone();
            text = tauri::async_runtime::spawn_blocking(move || db.last_completed_text()).await.ok().and_then(|r| r.ok()).flatten();
        }
        let Some(text) = text else {
            alert("no_last", "没有可粘贴的转录", "info").duration(Some(2500)).show(&self.app);
            return;
        };
        match pipeline::deliver_text(&AppContext::default(), &text, false).await {
            pipeline::Delivered::Inserted => {}
            pipeline::Delivered::NotEditable => {
                alert("copy_last", "复制最后的转录", "info")
                    .desc("当前没有可输入的文本框")
                    .action("copy", "复制", "primary")
                    .duration(Some(10_000))
                    .text(text)
                    .show(&self.app);
            }
            pipeline::Delivered::Failed(e) => {
                alert("insert_failed", "无法插入文本", "error")
                    .desc(e)
                    .action("copy", "复制", "primary")
                    .duration(Some(10_000))
                    .text(text)
                    .show(&self.app);
            }
        }
    }

    // ───────────────────────── alert actions ─────────────────────────

    async fn on_alert_action(&mut self, alert_id: &str, action_id: &str) {
        let ctx = self.state().alerts.lock().get(alert_id).unwrap_or_default();
        notify::dismiss_alert(&self.app, alert_id);
        match action_id {
            "retry" | "undo" => {
                if let Some(sid) = ctx.session_id {
                    self.reprocess(&sid).await;
                }
            }
            "copy" => {
                if let Some(text) = ctx.text {
                    let r = tauri::async_runtime::spawn_blocking(move || insert::copy_to_clipboard(&text)).await;
                    match r {
                        Ok(Ok(())) => {
                            alert("copied", "已复制", "success").duration(Some(1500)).show(&self.app);
                        }
                        _ => {
                            alert("copied", "复制失败", "error").duration(Some(3000)).show(&self.app);
                        }
                    }
                }
            }
            "open_settings" => notify::navigate(&self.app, "settings"),
            "open_history" => notify::navigate(&self.app, "history"),
            "open_mic_settings" => {
                let _ = tauri::async_runtime::spawn_blocking(|| permissions::open_settings(PermissionKind::Microphone)).await;
            }
            "quit_typeless" => {
                let _ = tauri::async_runtime::spawn_blocking(|| permissions::terminate_app(importer::TYPELESS_BUNDLE_ID)).await;
            }
            "cancel_processing" => self.cancel().await,
            "dismiss" => {}
            other => log::warn!("unknown alert action '{other}'"),
        }
    }

    /// Process saved audio like a fresh Finish (undo / retry) and insert at the current focus.
    async fn reprocess(&mut self, id: &str) {
        if self.processing.is_some() {
            self.busy_alert();
            return;
        }
        if self.session.is_some() {
            return;
        }
        let stored = match self.stash.iter().find(|s| s.id == id).cloned() {
            Some(s) => Some(s),
            None => self.load_stored(id).await,
        };
        let Some(stored) = stored else {
            alert("no_audio", "找不到录音文件，无法重试", "error").duration(Some(5000)).show(&self.app);
            return;
        };
        let tag = short_id(id).to_string();
        log::info!("[{tag}] reprocessing saved audio ({:.1}s)", stored.duration_secs);

        let fresh = tauri::async_runtime::spawn_blocking(|| context::capture(false, false)).await.unwrap_or_default();
        let ctx = AppContext { selected_text: stored.ctx.selected_text.clone(), ..fresh };

        let settings = self.state().settings();
        let store = self.store_enabled();
        let db = self.state().db.clone();
        let sid = stored.id.clone();
        let mut row = tauri::async_runtime::spawn_blocking(move || db.get_history(&sid).ok().flatten())
            .await
            .ok()
            .flatten()
            .unwrap_or_else(|| make_row(&stored.id, stored.mode, &stored.ctx, stored.duration_secs, &stored.created_at, "processing"));
        row.status = "processing".into();
        row.error_message = None;
        if store {
            self.write_row(row.clone()).await;
        }
        self.push_stash(stored.clone());
        self.spawn_processing(ProcessInput {
            row,
            ctx,
            settings,
            transcripts: Transcripts::Samples(stored.samples),
            deliver: true,
            store,
        });
    }

    async fn load_stored(&self, id: &str) -> Option<Stored> {
        let db = self.state().db.clone();
        let id = id.to_string();
        tauri::async_runtime::spawn_blocking(move || {
            let row = db.get_history(&id).ok()??;
            let path = db.audio_file(&row)?;
            let samples = pipeline::load_audio(&path).ok()?;
            Some(Stored {
                id: row.id.clone(),
                samples: Arc::new(samples),
                ctx: ctx_from_row(&row),
                mode: row.mode,
                duration_secs: row.duration_secs,
                created_at: row.created_at.clone(),
            })
        })
        .await
        .ok()
        .flatten()
    }
}

async fn mark_error(app: &AppHandle, id: &str, msg: &str) {
    let db = app.state::<AppState>().db.clone();
    let (id, msg) = (id.to_string(), msg.to_string());
    let _ = tauri::async_runtime::spawn_blocking(move || {
        if let Ok(Some(mut r)) = db.get_history(&id) {
            r.status = "error".into();
            r.error_message = Some(msg);
            let _ = db.upsert_history(&r);
        }
    })
    .await;
    emit_history_changed(app);
}
