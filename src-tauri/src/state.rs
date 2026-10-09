//! Shared application state and the "settings changed" side effects.

use crate::controller::Cmd;
use crate::db::Db;
use crate::notify::{self, AlertStore};
use crate::platform::hotkey::{HotkeyEngine, SessionPhase};
use crate::types::{events, BarState, Settings};
use crate::{config, tray};
use anyhow::Context;
use parking_lot::{Mutex, RwLock};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, OnceLock};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::mpsc::UnboundedSender;

/// Wrapper for OS-bound handles that the owning module documents as callable from any thread but
/// whose auto traits we cannot verify here. Access is serialised by a surrounding `Mutex`.
pub struct Shared<T>(pub T);
unsafe impl<T> Send for Shared<T> {}
unsafe impl<T> Sync for Shared<T> {}

#[derive(Clone, Debug)]
pub struct Paths {
    pub data_dir: PathBuf,
    pub db_path: PathBuf,
    pub recordings_dir: PathBuf,
}

pub struct AppState {
    pub paths: Paths,
    pub settings: RwLock<Settings>,
    settings_write: Mutex<()>,
    pub db: Arc<Db>,
    pub phase: Arc<AtomicU8>,
    pub engine: Mutex<Option<Shared<HotkeyEngine>>>,
    pub ctl: OnceLock<UnboundedSender<Cmd>>,
    pub mic_test: Mutex<Option<Arc<Shared<crate::audio::Recorder>>>>,
    pub bar: Mutex<BarState>,
    pub alerts: Mutex<AlertStore>,
    /// Last text we delivered or produced (for "paste last transcript" when history is not stored).
    pub last_text: Mutex<Option<String>>,
}

pub fn phase_to_u8(p: SessionPhase) -> u8 {
    match p {
        SessionPhase::Idle => 0,
        SessionPhase::RecordingPtt => 1,
        SessionPhase::RecordingLocked => 2,
        SessionPhase::Processing => 3,
    }
}

pub fn phase_from_u8(v: u8) -> SessionPhase {
    match v {
        1 => SessionPhase::RecordingPtt,
        2 => SessionPhase::RecordingLocked,
        3 => SessionPhase::Processing,
        _ => SessionPhase::Idle,
    }
}

impl AppState {
    pub fn new(paths: Paths, settings: Settings, db: Db) -> Self {
        let bar = BarState {
            phase: notify::idle_phase(&settings),
            show_idle_on_desktop: settings.show_bar_on_desktop,
            dictation_key_label: notify::first_binding_label(&settings),
            ..Default::default()
        };
        Self {
            paths,
            settings: RwLock::new(settings),
            settings_write: Mutex::new(()),
            db: Arc::new(db),
            phase: Arc::new(AtomicU8::new(0)),
            engine: Mutex::new(None),
            ctl: OnceLock::new(),
            mic_test: Mutex::new(None),
            bar: Mutex::new(bar),
            alerts: Mutex::new(AlertStore::default()),
            last_text: Mutex::new(None),
        }
    }

    pub fn settings(&self) -> Settings {
        self.settings.read().clone()
    }

    pub fn set_phase(&self, p: SessionPhase) {
        self.phase.store(phase_to_u8(p), Ordering::SeqCst);
    }

    pub fn send(&self, cmd: Cmd) {
        match self.ctl.get() {
            Some(tx) => {
                if tx.send(cmd).is_err() {
                    log::error!("controller channel closed");
                }
            }
            None => log::warn!("controller not started yet; command dropped"),
        }
    }

    /// Mutate + persist settings (blocking file IO). Returns (old, new).
    pub fn update_settings(&self, f: impl FnOnce(&mut Settings)) -> anyhow::Result<(Settings, Settings)> {
        let _guard = self.settings_write.lock();
        let old = self.settings.read().clone();
        let mut new = old.clone();
        f(&mut new);
        config::sanitize(&mut new);
        new.onboarding_completed |= old.onboarding_completed;
        config::save(&self.paths.data_dir, &new).context("保存设置失败")?;
        *self.settings.write() = new.clone();
        Ok((old, new))
    }
}

/// Persist `f(settings)` and apply every side effect. Returns the saved settings.
pub async fn save_and_apply(
    app: &AppHandle,
    f: impl FnOnce(&mut Settings) + Send + 'static,
) -> Result<Settings, String> {
    let handle = app.clone();
    let (old, new) = tauri::async_runtime::spawn_blocking(move || handle.state::<AppState>().update_settings(f))
        .await
        .map_err(|e| format!("保存设置失败：{e}"))?
        .map_err(|e| format!("{e:#}"))?;
    apply_settings(app, &old, &new);
    Ok(new)
}

pub fn apply_settings(app: &AppHandle, old: &Settings, new: &Settings) {
    let state = app.state::<AppState>();

    if old.shortcuts != new.shortcuts {
        if let Some(engine) = state.engine.lock().as_ref() {
            engine.0.update_bindings(new.shortcuts.clone());
        }
    }
    if old.show_in_dock != new.show_in_dock {
        set_dock_visibility(app, new.show_in_dock);
    }
    if old.launch_at_login != new.launch_at_login {
        apply_autostart(app, new.launch_at_login);
    }
    if old.history_retention != new.history_retention {
        spawn_purge(app);
    }
    if old.microphone_device_id != new.microphone_device_id
        || old.translation_targets != new.translation_targets
        || old.show_bar_on_desktop != new.show_bar_on_desktop
        || old.shortcuts.dictation != new.shortcuts.dictation
    {
        state.send(Cmd::SettingsChanged);
        tray::refresh(app);
    }

    if let Err(e) = app.emit(events::SETTINGS_UPDATED, new) {
        log::warn!("cannot emit settings-updated: {e}");
    }
}

pub fn set_dock_visibility(app: &AppHandle, show: bool) {
    #[cfg(target_os = "macos")]
    {
        let policy = if show { tauri::ActivationPolicy::Regular } else { tauri::ActivationPolicy::Accessory };
        if let Err(e) = app.set_activation_policy(policy) {
            log::warn!("cannot change activation policy: {e}");
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (app, show);
}

pub fn apply_autostart(app: &AppHandle, enable: bool) {
    use tauri_plugin_autostart::ManagerExt;
    if cfg!(debug_assertions) {
        log::info!("launchAtLogin={enable}: skipped in debug builds (would register the dev binary)");
        return;
    }
    let launcher = app.autolaunch();
    match launcher.is_enabled() {
        Ok(current) if current == enable => {}
        _ => {
            let r = if enable { launcher.enable() } else { launcher.disable() };
            if let Err(e) = r {
                log::warn!("cannot update login item: {e}");
            }
        }
    }
}

/// Delete expired history (and audio). Runs on a blocking thread.
pub fn spawn_purge(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let retention = state.settings.read().history_retention.clone();
        let Some(secs) = config::retention_secs(&retention) else { return };
        let cutoff = chrono::Utc::now() - chrono::Duration::seconds(secs);
        match state.db.purge_older_than(cutoff) {
            Ok(0) => {}
            Ok(n) => {
                log::info!("purged {n} history rows (retention {retention})");
                let _ = app.emit(events::HISTORY_UPDATED, ());
                let _ = app.emit(events::STATS_UPDATED, ());
            }
            Err(e) => log::warn!("history purge failed: {e:#}"),
        }
    });
}

pub fn emit_history_changed(app: &AppHandle) {
    let _ = app.emit(events::HISTORY_UPDATED, ());
    let _ = app.emit(events::STATS_UPDATED, ());
}
