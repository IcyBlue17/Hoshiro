//! Menu-bar / notification-area tray icon + macOS application menu.

use crate::audio::Recorder;
use crate::controller::Cmd;
use crate::notify;
use crate::platform::hotkey::HotkeyAction;
use crate::platform::permissions;
use crate::state::{save_and_apply, AppState};
use crate::types::{MicDevice, PermissionState};
use parking_lot::Mutex;
use std::time::{Duration, Instant};
use tauri::image::Image;
use tauri::menu::{
    CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu, SubmenuBuilder,
};
use tauri::tray::{TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

const TRAY_ID: &str = "main";

static DEVICES: once_cell::sync::Lazy<Mutex<Vec<MicDevice>>> = once_cell::sync::Lazy::new(|| Mutex::new(Vec::new()));
static LAST_REFRESH: once_cell::sync::Lazy<Mutex<Option<Instant>>> = once_cell::sync::Lazy::new(|| Mutex::new(None));

fn build_menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let current = app.state::<AppState>().settings.read().microphone_device_id.clone();
    let devices = DEVICES.lock().clone();

    let mic_menu = {
        let mut b = SubmenuBuilder::new(app, "选择麦克风");
        let auto = CheckMenuItem::with_id(app, "tray:mic:auto", "自动检测", true, current.is_none(), None::<&str>)?;
        b = b.item(&auto);
        if !devices.is_empty() {
            b = b.separator();
        }
        for d in &devices {
            let checked = current.as_deref() == Some(d.id.as_str());
            let item = CheckMenuItem::with_id(app, format!("tray:mic:id:{}", d.id), &d.name, true, checked, None::<&str>)?;
            b = b.item(&item);
        }
        b.build()?
    };

    let version = app.package_info().version.to_string();
    let home = MenuItem::with_id(app, "tray:home", "打开 Hoshiro 主页", true, None::<&str>)?;
    let history = MenuItem::with_id(app, "tray:history", "显示历史记录", true, None::<&str>)?;
    let dict = MenuItem::with_id(app, "tray:dictionary", "将词汇添加到词典", true, None::<&str>)?;
    let paste = MenuItem::with_id(app, "tray:paste_last", "粘贴上一次转录", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "tray:settings", "设置…", true, None::<&str>)?;
    let upd_label = match crate::update::known_version() {
        Some(v) => format!("安装新版本 {v}"),
        None => "检查更新…".to_string(),
    };
    let upd = MenuItem::with_id(app, "tray:update", upd_label, true, None::<&str>)?;
    let ver = MenuItem::with_id(app, "tray:version", format!("版本 {version}"), false, None::<&str>)?;
    let quit = MenuItem::with_id(app, "tray:quit", "退出 Hoshiro", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    Menu::with_items(app, &[&home, &history, &dict, &mic_menu, &paste, &settings, &sep, &ver, &upd, &quit])
}

/// The tray glyph is black on transparent: a template image on macOS (the menu bar recolors it). Windows and
/// Linux show it as is, so dark taskbars / panels (the default) get a white copy.
fn tray_icon() -> anyhow::Result<Image<'static>> {
    let icon = Image::from_bytes(include_bytes!("../icons/tray.png"))?.to_owned();
    if cfg!(any(windows, target_os = "linux")) && !light_taskbar() {
        let mut rgba = icon.rgba().to_vec();
        for px in rgba.chunks_exact_mut(4) {
            px[0] = 255 - px[0];
            px[1] = 255 - px[1];
            px[2] = 255 - px[2];
        }
        return Ok(Image::new_owned(rgba, icon.width(), icon.height()));
    }
    Ok(icon)
}

/// Windows: is the taskbar light (Settings > Personalization > Colors > "Choose your default Windows mode")?
#[cfg(windows)]
fn light_taskbar() -> bool {
    use windows::core::w;
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
    let mut value = 0u32;
    let mut len = std::mem::size_of::<u32>() as u32;
    let r = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            w!("SystemUsesLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut value as *mut u32 as *mut _),
            Some(&mut len),
        )
    };
    r.is_ok() && value == 1
}

/// Linux: GNOME's top bar (and most panels) are dark whatever the theme; KDE's follows the color scheme.
#[cfg(target_os = "linux")]
fn light_taskbar() -> bool {
    let desktop = crate::platform::linux::desktop_name().to_ascii_uppercase();
    if !desktop.contains("KDE") {
        return false;
    }
    let scheme = dirs::config_dir()
        .and_then(|d| std::fs::read_to_string(d.join("kdeglobals")).ok())
        .and_then(|s| s.lines().find_map(|l| l.strip_prefix("ColorScheme=").map(str::to_string)))
        .unwrap_or_default();
    !scheme.to_ascii_lowercase().contains("dark")
}

#[cfg(not(any(windows, target_os = "linux")))]
fn light_taskbar() -> bool {
    true
}

pub fn setup(app: &AppHandle) -> anyhow::Result<()> {
    let menu = build_menu(app)?;
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(tray_icon()?)
        .icon_as_template(true)
        .tooltip("Hoshiro")
        .menu(&menu)
        // macOS: the menu opens on click. Windows convention: click opens the app, right-click the menu.
        .show_menu_on_left_click(!cfg!(windows))
        .on_tray_icon_event(|tray, event| match event {
            TrayIconEvent::Enter { .. } => refresh(tray.app_handle()),
            #[cfg(windows)]
            TrayIconEvent::Click { button: tauri::tray::MouseButton::Left, button_state: tauri::tray::MouseButtonState::Up, .. } => {
                notify::show_main(tray.app_handle());
            }
            _ => {}
        })
        .build(app)?;
    refresh(app);
    Ok(())
}

/// Rebuild the tray menu right away (used when the update state changes).
pub fn refresh_now(app: &AppHandle) {
    *LAST_REFRESH.lock() = None;
    refresh(app);
}

/// Re-enumerate microphones (when permitted) and rebuild the tray menu. Cheap to call often.
pub fn refresh(app: &AppHandle) {
    {
        let mut last = LAST_REFRESH.lock();
        if last.is_some_and(|t| t.elapsed() < Duration::from_millis(1500)) {
            return;
        }
        *last = Some(Instant::now());
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let devices = tauri::async_runtime::spawn_blocking(|| {
            if permissions::check().microphone == PermissionState::Granted {
                Recorder::list_devices()
            } else {
                Vec::new()
            }
        })
        .await
        .unwrap_or_default();
        *DEVICES.lock() = devices;
        match build_menu(&app) {
            Ok(menu) => {
                if let Some(tray) = app.tray_by_id(TRAY_ID) {
                    if let Err(e) = tray.set_menu(Some(menu)) {
                        log::warn!("cannot update tray menu: {e}");
                    }
                    if cfg!(any(windows, target_os = "linux")) {
                        // Follow taskbar theme switches.
                        if let Ok(icon) = tray_icon() {
                            let _ = tray.set_icon(Some(icon));
                        }
                    }
                }
            }
            Err(e) => log::warn!("cannot build tray menu: {e}"),
        }
    });
}

/// Handles clicks of both the tray menu and the application menu.
pub fn on_menu_event(app: &AppHandle, event: MenuEvent) {
    let id = event.id().as_ref();
    match id {
        "tray:home" => notify::navigate(app, "home"),
        "tray:history" => notify::navigate(app, "history"),
        "tray:dictionary" => notify::navigate(app, "dictionary"),
        "tray:settings" | "app:settings" => notify::navigate(app, "settings"),
        "tray:paste_last" => app.state::<AppState>().send(Cmd::Hotkey(HotkeyAction::PasteLast)),
        "app:about" => notify::navigate(app, "settings:about"),
        "tray:update" | "app:update" => crate::update::tray_action(app),
        "tray:quit" | "app:quit" => app.exit(0),
        "tray:mic:auto" => select_mic(app, None),
        _ => {
            if let Some(dev) = id.strip_prefix("tray:mic:id:") {
                select_mic(app, Some(dev.to_string()));
            }
        }
    }
}

fn select_mic(app: &AppHandle, device: Option<String>) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = save_and_apply(&app, move |s| s.microphone_device_id = device).await {
            log::warn!("cannot save microphone choice: {e}");
        }
        *LAST_REFRESH.lock() = None;
        refresh(&app);
    });
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn build_app_menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    // In-app About (Settings › 关于) rather than the stock macOS panel.
    let about = MenuItem::with_id(app, "app:about", "关于 Hoshiro", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "app:settings", "设置…", true, Some("CmdOrCtrl+,"))?;
    let upd = MenuItem::with_id(app, "app:update", "检查更新…", true, None::<&str>)?;
    // Not `PredefinedMenuItem::quit`: its exit carries no code, which the run loop ignores (tray app).
    let quit = MenuItem::with_id(app, "app:quit", "退出 Hoshiro", true, Some("CmdOrCtrl+Q"))?;
    let app_menu: Submenu<_> = SubmenuBuilder::new(app, "Hoshiro")
        .item(&about)
        .item(&upd)
        .separator()
        .item(&settings)
        .separator()
        .item(&PredefinedMenuItem::hide(app, Some("隐藏 Hoshiro"))?)
        .item(&PredefinedMenuItem::hide_others(app, Some("隐藏其他"))?)
        .item(&PredefinedMenuItem::show_all(app, Some("全部显示"))?)
        .separator()
        .item(&quit)
        .build()?;
    let edit_menu = SubmenuBuilder::new(app, "编辑")
        .item(&PredefinedMenuItem::undo(app, Some("撤销"))?)
        .item(&PredefinedMenuItem::redo(app, Some("重做"))?)
        .separator()
        .item(&PredefinedMenuItem::cut(app, Some("剪切"))?)
        .item(&PredefinedMenuItem::copy(app, Some("拷贝"))?)
        .item(&PredefinedMenuItem::paste(app, Some("粘贴"))?)
        .item(&PredefinedMenuItem::select_all(app, Some("全选"))?)
        .build()?;
    let window_menu = SubmenuBuilder::new(app, "窗口")
        .item(&PredefinedMenuItem::minimize(app, Some("最小化"))?)
        .item(&PredefinedMenuItem::maximize(app, Some("缩放"))?)
        .separator()
        .item(&PredefinedMenuItem::close_window(app, Some("关闭窗口"))?)
        .build()?;
    Menu::with_items(app, &[&app_menu, &edit_menu, &window_menu])
}
