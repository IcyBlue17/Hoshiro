//! App entry. OWNER: core agent.

pub mod audio;
pub mod config;
pub mod secrets;
pub mod platform;
pub mod providers;
pub mod types;

mod autodict;
mod commands;
mod controller;
mod db;
mod importer;
mod notify;
mod pipeline;
mod personalize;
mod usage;
mod state;
mod stats;
mod tray;

use crate::controller::Cmd;
use crate::platform::hotkey::{ActionCallback, HotkeyEngine, PhaseReader};
use crate::platform::{bar, permissions};
use crate::state::{phase_from_u8, AppState, Paths, Shared};
use crate::types::{events, Permissions};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, RunEvent, WindowEvent};
use tauri_plugin_autostart::MacosLauncher;
use tauri_plugin_log::{RotationStrategy, Target, TargetKind};

const IDENTIFIER: &str = "dev.hoshiro.app";

fn open_db(paths: &Paths) -> db::Db {
    match db::Db::open(&paths.db_path, paths.recordings_dir.clone()) {
        Ok(d) => d,
        Err(first) => {
            eprintln!("cannot open database ({first:#}); moving it aside and starting fresh");
            let stamp = chrono::Utc::now().format("%Y%m%d%H%M%S");
            let _ = std::fs::rename(&paths.db_path, paths.db_path.with_extension(format!("db.corrupt-{stamp}")));
            let _ = std::fs::remove_file(paths.db_path.with_extension("db-wal"));
            let _ = std::fs::remove_file(paths.db_path.with_extension("db-shm"));
            match db::Db::open(&paths.db_path, paths.recordings_dir.clone()) {
                Ok(d) => d,
                Err(e) => {
                    eprintln!("fatal: cannot create database: {e:#}");
                    std::process::exit(1);
                }
            }
        }
    }
}

fn build_state() -> AppState {
    let data_dir = dirs::data_dir().unwrap_or_else(std::env::temp_dir).join(IDENTIFIER);
    if let Err(e) = std::fs::create_dir_all(&data_dir) {
        eprintln!("cannot create data dir {}: {e}", data_dir.display());
    }
    let paths = Paths {
        db_path: data_dir.join("hoshiro.db"),
        recordings_dir: data_dir.join("recordings"),
        data_dir,
    };
    let settings = config::load(&paths.data_dir);
    let db = open_db(&paths);
    AppState::new(paths, settings, db)
}

fn sounds_dir(app: &AppHandle) -> PathBuf {
    let bundled = app.path().resource_dir().ok().map(|d| d.join("sounds"));
    match bundled {
        Some(d) if d.is_dir() => d,
        _ => PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/sounds"),
    }
}

fn start_engine(app: &AppHandle) -> anyhow::Result<()> {
    let state = app.state::<AppState>();
    let bindings = state.settings.read().shortcuts.clone();
    let phase = state.phase.clone();
    let reader: PhaseReader = Arc::new(move || phase_from_u8(phase.load(Ordering::SeqCst)));
    let handle = app.clone();
    let on_action: ActionCallback = Arc::new(move |action| handle.state::<AppState>().send(Cmd::Hotkey(action)));
    *state.engine.lock() = None;
    let engine = HotkeyEngine::start(bindings, reader, on_action)?;
    *state.engine.lock() = Some(Shared(engine));
    Ok(())
}

fn same_permissions(a: &Permissions, b: &Permissions) -> bool {
    a.microphone == b.microphone && a.accessibility == b.accessibility && a.input_monitoring == b.input_monitoring
}

/// Keeps the hotkey engine alive and tells the UI when permissions change.
fn spawn_permission_watcher(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut last: Option<Permissions> = None;
        let mut inactive_checks = 0u32;
        let mut warned = false;
        loop {
            tokio::time::sleep(Duration::from_secs(2)).await;
            let perms = match tauri::async_runtime::spawn_blocking(permissions::check).await {
                Ok(p) => p,
                Err(_) => continue,
            };
            let mut changed = last.as_ref().is_none_or(|l| !same_permissions(l, &perms));

            let active = app.state::<AppState>().engine.lock().as_ref().map(|e| e.0.is_active());
            match active {
                Some(true) => inactive_checks = 0,
                other => {
                    if other == Some(false) {
                        inactive_checks += 1;
                    }
                    if other.is_none() || inactive_checks >= 2 {
                        let handle = app.clone();
                        let started = tauri::async_runtime::spawn_blocking(move || start_engine(&handle)).await;
                        match started {
                            Ok(Ok(())) => {
                                log::info!("hotkey engine started");
                                inactive_checks = 0;
                                warned = false;
                                changed = true;
                            }
                            Ok(Err(e)) => {
                                if !warned {
                                    log::warn!("hotkey engine unavailable (permissions?): {e:#}");
                                    warned = true;
                                }
                            }
                            Err(e) => log::error!("hotkey start task failed: {e}"),
                        }
                    }
                }
            }
            if changed {
                last = Some(perms.clone());
                let _ = app.emit(events::PERMISSIONS_CHANGED, &perms);
            }
        }
    });
}

fn spawn_typeless_notice(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        // Give the bar webview time to load and subscribe.
        tokio::time::sleep(Duration::from_secs(4)).await;
        let running = tauri::async_runtime::spawn_blocking(|| {
            !permissions::running_pids(importer::TYPELESS_BUNDLE_ID).is_empty()
        })
        .await
        .unwrap_or(false);
        if running {
            notify::alert("typeless_running", "检测到 Typeless 正在运行", "warning")
                .desc("Typeless 与 Hoshiro 会同时响应同一个按键，建议先退出 Typeless。")
                .action("quit_typeless", "退出 Typeless", "primary")
                .duration(Some(15_000))
                .show(&app);
        }
    });
}

fn spawn_hourly_purge(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            state::spawn_purge(&app);
            tokio::time::sleep(Duration::from_secs(3600)).await;
        }
    });
}

fn setup(app: &mut tauri::App, autostart_launch: bool) -> Result<(), Box<dyn std::error::Error>> {
    let handle = app.handle().clone();
    let state = handle.state::<AppState>();
    let settings = state.settings();
    log::info!("Hoshiro {} starting (data dir {})", handle.package_info().version, state.paths.data_dir.display());

    audio::sounds::init(sounds_dir(&handle));
    if let Err(e) = state.db.fail_stale_processing() {
        log::warn!("cannot reset stale rows: {e:#}");
    }

    state::set_dock_visibility(&handle, settings.show_in_dock);
    match tray::build_app_menu(&handle) {
        Ok(menu) => {
            if let Err(e) = app.set_menu(menu) {
                log::warn!("cannot set application menu: {e}");
            }
        }
        Err(e) => log::warn!("cannot build application menu: {e}"),
    }
    if let Err(e) = tray::setup(&handle) {
        log::error!("tray setup failed: {e:#}");
    }

    let tx = controller::spawn(&handle);
    let _ = state.ctl.set(tx);

    if let Err(e) = bar::setup(&handle) {
        log::error!("bar setup failed: {e:#}");
    }
    bar::show(&handle);

    if !autostart_launch || !settings.onboarding_completed {
        notify::show_main(&handle);
    }

    if let Err(e) = start_engine(&handle) {
        log::warn!("hotkey engine not started yet: {e:#}");
    }
    spawn_permission_watcher(&handle);
    spawn_hourly_purge(&handle);
    usage::init(&handle);
    personalize::spawn(&handle);
    spawn_typeless_notice(&handle);
    state::apply_autostart(&handle, settings.launch_at_login);
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let autostart_launch = std::env::args().any(|a| a == "--autostart");

    let mut logger = tauri_plugin_log::Builder::new()
        .level(log::LevelFilter::Info)
        .level_for("hoshiro_lib", log::LevelFilter::Debug)
        .level_for("hoshiro", log::LevelFilter::Debug)
        .rotation_strategy(RotationStrategy::KeepSome(5))
        .max_file_size(5_000_000)
        .clear_targets()
        .target(Target::new(TargetKind::Stdout));
    if let Some(dir) = dirs::data_dir().map(|d| d.join(IDENTIFIER).join("logs")) {
        logger = logger.target(Target::new(TargetKind::Folder { path: dir, file_name: Some("hoshiro".into()) }));
    }

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| notify::show_main(app)))
        .plugin(logger.build())
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, Some(vec!["--autostart"])))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(build_state())
        .on_menu_event(tray::on_menu_event)
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::get_settings,
            commands::save_settings,
            commands::complete_onboarding,
            commands::get_permissions,
            commands::request_permission,
            commands::open_permission_settings,
            commands::open_keyboard_settings,
            commands::list_microphones,
            commands::start_mic_test,
            commands::stop_mic_test,
            commands::start_shortcut_capture,
            commands::stop_shortcut_capture,
            commands::list_history,
            commands::delete_history,
            commands::delete_all_history,
            commands::retry_history,
            commands::export_history_audio,
            commands::list_dictionary,
            commands::add_dictionary_words,
            commands::update_dictionary_word,
            commands::delete_dictionary_words,
            commands::preview_dictionary_csv,
            commands::get_stats,
            commands::get_usage,
            commands::get_profile,
            commands::run_personalization,
            commands::update_profile,
            commands::reset_profile,
            commands::test_asr,
            commands::test_llm,
            commands::list_models,
            commands::detect_typeless,
            commands::import_typeless,
            commands::quit_typeless,
            commands::copy_text,
            commands::open_url,
            commands::bar_action,
            commands::bar_alert_action,
            commands::bar_set_hit_rects,
            commands::bar_get_state,
            commands::bar_close_answer,
        ])
        .setup(move |app| setup(app, autostart_launch))
        .build(tauri::generate_context!());

    match app {
        Ok(app) => app.run(|_app, _event| {
            #[cfg(target_os = "macos")]
            if let RunEvent::Reopen { .. } = _event {
                notify::show_main(_app);
            }
        }),
        Err(e) => {
            eprintln!("error while building tauri application: {e}");
            std::process::exit(1);
        }
    }
}
