//! In-app updates (official Tauri updater). Checks run in the background (30 s after launch, then every 24 h),
//! from the tray, and from the About page; the result is cached so every surface agrees.

use crate::types::{events, UpdateChecked, UpdateInfo, UpdateProgress};
use crate::{notify, tray};
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use tauri_plugin_updater::{Update, UpdaterExt};

const FIRST_CHECK_DELAY: Duration = Duration::from_secs(30);
const CHECK_INTERVAL: Duration = Duration::from_secs(24 * 3600);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const CHECK_FAILED: &str = "无法检查更新，请稍后再试";
const INSTALL_FAILED: &str = "更新失败，请稍后再试";

#[derive(Default)]
struct Cache {
    info: Option<UpdateInfo>,
    pending: Option<Update>,
}

static CACHE: once_cell::sync::Lazy<Mutex<Cache>> = once_cell::sync::Lazy::new(|| Mutex::new(Cache::default()));
static CHECK_GATE: once_cell::sync::Lazy<tokio::sync::Mutex<()>> = once_cell::sync::Lazy::new(|| tokio::sync::Mutex::new(()));
static INSTALLING: AtomicBool = AtomicBool::new(false);

/// Version of the update we know about (for the tray label).
pub fn known_version() -> Option<String> {
    CACHE.lock().info.as_ref().filter(|i| i.available).and_then(|i| i.version.clone())
}

pub fn cached() -> Option<UpdateInfo> {
    CACHE.lock().info.clone()
}

fn clean_notes(body: Option<String>) -> Option<String> {
    body.map(|b| b.trim().to_string()).filter(|b| !b.is_empty())
}

fn info_from(current: String, update: Option<&Update>) -> UpdateInfo {
    match update {
        Some(u) => UpdateInfo {
            available: true,
            current_version: current,
            version: Some(u.version.clone()),
            notes: clean_notes(u.body.clone()),
            date: u
                .date
                .and_then(|d| chrono::DateTime::from_timestamp(d.unix_timestamp(), 0))
                .map(|d| d.to_rfc3339()),
        },
        None => UpdateInfo { available: false, current_version: current, version: None, notes: None, date: None },
    }
}

/// Asks the update endpoint. Always emits `update-checked` and refreshes the tray; `update-available`
/// is emitted only when a version we had not announced before shows up.
pub async fn check(app: &AppHandle) -> Result<UpdateInfo, String> {
    let _gate = CHECK_GATE.lock().await;
    let current = app.package_info().version.to_string();
    let result = async {
        let updater = app.updater_builder().timeout(REQUEST_TIMEOUT).build()?;
        updater.check().await
    }
    .await;
    match result {
        Ok(found) => {
            let info = info_from(current, found.as_ref());
            let announce = info.available && cached().is_none_or(|c| c.version != info.version);
            {
                let mut c = CACHE.lock();
                c.info = Some(info.clone());
                c.pending = found;
            }
            tray::refresh_now(app);
            let _ = app.emit(events::UPDATE_CHECKED, UpdateChecked { info: Some(info.clone()), error: None });
            if announce {
                let _ = app.emit(events::UPDATE_AVAILABLE, &info);
            }
            Ok(info)
        }
        Err(e) => {
            log::warn!("update check failed: {e}");
            let _ = app.emit(events::UPDATE_CHECKED, UpdateChecked { info: None, error: Some(CHECK_FAILED.into()) });
            Err(CHECK_FAILED.into())
        }
    }
}

/// Downloads and installs the known update, reporting progress, then relaunches.
pub async fn install(app: &AppHandle) -> Result<(), String> {
    if INSTALLING.swap(true, Ordering::SeqCst) {
        return Err("正在更新".into());
    }
    let result = install_inner(app).await;
    if let Err(e) = &result {
        log::warn!("update install failed: {e}");
        INSTALLING.store(false, Ordering::SeqCst);
        return Err(INSTALL_FAILED.into());
    }
    app.restart()
}

async fn install_inner(app: &AppHandle) -> Result<(), String> {
    let mut pending = CACHE.lock().pending.clone();
    if pending.is_none() {
        // The cached handle is gone (e.g. never checked in this run): look again.
        check(app).await?;
        pending = CACHE.lock().pending.clone();
    }
    let update = pending.ok_or_else(|| "没有可用的更新".to_string())?;
    let mut downloaded = 0u64;
    let handle = app.clone();
    update
        .download_and_install(
            move |chunk, total| {
                downloaded += chunk as u64;
                let _ = handle.emit(events::UPDATE_PROGRESS, UpdateProgress { downloaded, total });
            },
            || {},
        )
        .await
        .map_err(|e| e.to_string())
}

/// Tray entry: with a known update install it (showing progress in the hub), otherwise check now.
pub fn tray_action(app: &AppHandle) {
    notify::navigate(app, "settings:about");
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if known_version().is_some() {
            let _ = install(&app).await;
        } else {
            let _ = check(&app).await;
        }
    });
}

pub fn spawn(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(FIRST_CHECK_DELAY).await;
        loop {
            let _ = check(&app).await; // silent: failures are only logged
            tokio::time::sleep(CHECK_INTERVAL).await;
        }
    });
}
