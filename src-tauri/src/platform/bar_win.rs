//! Windows bar plumbing: a topmost, non-activating tool window (never steals focus, not in Alt+Tab or the
//! taskbar, present on every virtual desktop), placed bottom-center of the work area of the monitor under the
//! cursor, click-through except over the hit rects reported by the bar UI.
//!
//! tao (Tauri's windowing layer) owns the window styles: it recomputes and rewrites the complete style
//! whenever one of its flags changes (visibility, click-through, ...). So visibility and click-through go
//! through Tauri's API, and the few styles tao cannot express (tool window, no app window) are enforced in
//! WM_STYLECHANGING, which every one of those rewrites passes through.

use super::point_in_rects;
use crate::types::HitRect;
use parking_lot::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Manager};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTOPRIMARY};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON, VK_MBUTTON, VK_RBUTTON};
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetForegroundWindow, GetWindowLongPtrW, GetWindowRect, IsWindow, SetForegroundWindow, SetWindowLongPtrW,
    SetWindowPos,
    GWL_EXSTYLE, HWND_TOPMOST, MA_NOACTIVATE, STYLESTRUCT, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    WA_INACTIVE, WM_ACTIVATE, WM_MOUSEACTIVATE, WM_STYLECHANGING, WS_EX_APPWINDOW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
};

const LABEL: &str = "bar";
const HIT_SLOP: f64 = 1.0;

struct BarState {
    rects: Vec<HitRect>,
    visible: bool,
    /// Last value applied via set_ignore_cursor_events (inverted): true = window currently receives clicks.
    interactive: bool,
    poller_running: bool,
}

static STATE: Mutex<BarState> = Mutex::new(BarState { rects: Vec::new(), visible: false, interactive: false, poller_running: false });

fn hwnd(app: &AppHandle) -> Option<HWND> {
    let w = app.get_webview_window(LABEL)?;
    w.hwnd().ok().filter(|h| !h.0.is_null()).map(|h| HWND(h.0 as *mut _))
}

fn run_on_main(app: &AppHandle, f: impl FnOnce() + Send + 'static) {
    if let Err(e) = app.run_on_main_thread(f) {
        log::warn!("run_on_main_thread failed: {e}");
    }
}

/// The last foreground window that was not the bar (recorded whenever the bar is shown).
static LAST_OTHER_FOREGROUND: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);

/// Pure: the extended style the bar must always have, whatever tao computed.
pub(crate) fn enforce_ex_style(ex: u32) -> u32 {
    (ex | WS_EX_NOACTIVATE.0 | WS_EX_TOOLWINDOW.0) & !WS_EX_APPWINDOW.0
}

/// The bar must never be the foreground window: the user's app has to keep keyboard focus (the paste goes
/// there). WS_EX_NOACTIVATE covers clicks, but WebView2 can still activate its host programmatically, so any
/// activation is handed straight back to the window it was taken from.
unsafe extern "system" fn bar_subclass(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    match msg {
        WM_MOUSEACTIVATE => return LRESULT(MA_NOACTIVATE as isize),
        WM_ACTIVATE if (wparam.0 & 0xFFFF) as u32 != WA_INACTIVE => {
            // lParam is the window losing activation, or NULL when it belongs to another thread's queue.
            let mut previous = HWND(lparam.0 as *mut _);
            if previous.0.is_null() || previous == hwnd {
                previous = HWND(LAST_OTHER_FOREGROUND.load(std::sync::atomic::Ordering::Relaxed) as *mut _);
            }
            if !previous.0.is_null() && previous != hwnd && IsWindow(Some(previous)).as_bool() {
                let _ = SetForegroundWindow(previous);
            }
        }
        WM_STYLECHANGING if wparam.0 as i32 == GWL_EXSTYLE.0 && lparam.0 != 0 => {
            let ss = &mut *(lparam.0 as *mut STYLESTRUCT);
            ss.styleNew = enforce_ex_style(ss.styleNew);
        }
        _ => {}
    }
    DefSubclassProc(hwnd, msg, wparam, lparam)
}

/// Must run on the thread that owns the window (subclassing is per thread): the main thread.
pub fn setup(app: &AppHandle) -> anyhow::Result<()> {
    let window = app.get_webview_window(LABEL).ok_or_else(|| anyhow::anyhow!("bar window not found"))?;
    let h = hwnd(app).ok_or_else(|| anyhow::anyhow!("bar HWND unavailable"))?;
    unsafe {
        if !SetWindowSubclass(h, Some(bar_subclass), 1, 0).as_bool() {
            log::warn!("bar: cannot subclass the window; it may take focus or show in Alt+Tab");
        }
        // Run the current style through WM_STYLECHANGING once.
        let ex = GetWindowLongPtrW(h, GWL_EXSTYLE);
        SetWindowLongPtrW(h, GWL_EXSTYLE, ex);
        let _ = SetWindowPos(h, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_FRAMECHANGED);
    }
    if let Err(e) = window.set_ignore_cursor_events(true) {
        log::warn!("bar: cannot enable click-through: {e}");
    }
    STATE.lock().interactive = false;
    log::info!("bar window configured as a non-activating topmost tool window");
    Ok(())
}

/// Bottom-center of the work area (taskbar excluded) of the monitor under the cursor. Physical pixels.
fn place(h: HWND) {
    unsafe {
        let mut cursor = POINT::default();
        let _ = GetCursorPos(&mut cursor);
        let monitor = MonitorFromPoint(cursor, MONITOR_DEFAULTTOPRIMARY);
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return;
        }
        let work = info.rcWork;
        // Two passes: moving to a monitor with another DPI makes the window rescale itself (WM_DPICHANGED).
        for _ in 0..2 {
            let mut r = RECT::default();
            if GetWindowRect(h, &mut r).is_err() {
                return;
            }
            let (w, ht) = (r.right - r.left, r.bottom - r.top);
            let (x, y) = bottom_center(&work, w, ht);
            if (x, y) == (r.left, r.top) {
                break;
            }
            let _ = SetWindowPos(h, Some(HWND_TOPMOST), x, y, 0, 0, SWP_NOSIZE | SWP_NOACTIVATE);
        }
    }
}

/// Pure: top-left of a `w` x `h` window at the bottom-center of `work`.
pub(crate) fn bottom_center(work: &RECT, w: i32, h: i32) -> (i32, i32) {
    (work.left + (work.right - work.left - w) / 2, work.bottom - h)
}

pub fn show(app: &AppHandle) {
    let app2 = app.clone();
    run_on_main(app, move || {
        let (Some(w), Some(h)) = (app2.get_webview_window(LABEL), hwnd(&app2)) else { return };
        place(h);
        // Through tao, so its VISIBLE flag stays right (it rewrites WS_VISIBLE on every style change). tao shows
        // with SW_SHOW, which activates even a WS_EX_NOACTIVATE window, so the previous foreground window gets
        // its activation back right away (we are allowed to: we just were the foreground process).
        let before = unsafe { GetForegroundWindow() };
        if !before.0.is_null() && before != h {
            LAST_OTHER_FOREGROUND.store(before.0 as isize, std::sync::atomic::Ordering::Relaxed);
        }
        if let Err(e) = w.show() {
            log::warn!("bar: show failed: {e}");
        }
        unsafe {
            if GetForegroundWindow() == h {
                if !before.0.is_null() && before != h && IsWindow(Some(before)).as_bool() {
                    let _ = SetForegroundWindow(before);
                } else {
                    // Nothing to give it back to (e.g. right after launch): drop activation to the desktop.
                    let _ = SetForegroundWindow(windows::Win32::UI::WindowsAndMessaging::GetDesktopWindow());
                }
            }
            // Re-assert topmost: other topmost windows (taskbar flyouts, overlays) may have come in front.
            let _ = SetWindowPos(h, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        }
        STATE.lock().visible = true;
        ensure_poller(&app2);
    });
}

pub fn hide(app: &AppHandle) {
    let app2 = app.clone();
    run_on_main(app, move || {
        if let Some(w) = app2.get_webview_window(LABEL) {
            let _ = w.set_ignore_cursor_events(true);
            let _ = w.hide();
        }
        let mut st = STATE.lock();
        st.visible = false;
        st.interactive = false;
    });
}

pub fn set_hit_rects(app: &AppHandle, rects: Vec<HitRect>) {
    let empty = rects.is_empty();
    STATE.lock().rects = rects;
    if empty {
        set_interactive(app, false);
    } else {
        ensure_poller(app);
    }
}

fn set_interactive(app: &AppHandle, interactive: bool) {
    {
        let mut st = STATE.lock();
        if st.interactive == interactive {
            return;
        }
        st.interactive = interactive;
    }
    if let Some(w) = app.get_webview_window(LABEL) {
        if let Err(e) = w.set_ignore_cursor_events(!interactive) {
            log::warn!("bar: set_ignore_cursor_events failed: {e}");
        }
    }
}

fn ensure_poller(app: &AppHandle) {
    {
        let mut st = STATE.lock();
        if st.poller_running || st.rects.is_empty() {
            return;
        }
        st.poller_running = true;
    }
    let app = app.clone();
    let spawned = std::thread::Builder::new().name("hoshiro-bar-hit-test".into()).spawn(move || poll_loop(app));
    if let Err(e) = spawned {
        log::error!("failed to start the bar hit-test poller: {e}");
        STATE.lock().poller_running = false;
    }
}

fn any_button_down() -> bool {
    [VK_LBUTTON, VK_RBUTTON, VK_MBUTTON].iter().any(|vk| unsafe { GetAsyncKeyState(vk.0 as i32) } as u16 & 0x8000 != 0)
}

/// Toggles click-through by comparing the cursor position with the reported hit rects.
fn poll_loop(app: AppHandle) {
    loop {
        let (rects, visible, interactive) = {
            let mut st = STATE.lock();
            if st.rects.is_empty() {
                st.poller_running = false;
                return;
            }
            (st.rects.clone(), st.visible, st.interactive)
        };
        let mut sleep_ms = 100;
        if let (Some(h), true) = (hwnd(&app), visible) {
            unsafe {
                let mut r = RECT::default();
                let mut c = POINT::default();
                if GetWindowRect(h, &mut r).is_ok() && GetCursorPos(&mut c).is_ok() {
                    let near = c.x >= r.left - 60 && c.x <= r.right + 60 && c.y >= r.top - 60 && c.y <= r.bottom + 60;
                    sleep_ms = if near { 16 } else { 66 };
                    let scale = match GetDpiForWindow(h) {
                        0 => 1.0,
                        dpi => dpi as f64 / 96.0,
                    };
                    // window-local CSS pixels, origin top-left
                    let lx = (c.x - r.left) as f64 / scale;
                    let ly = (c.y - r.top) as f64 / scale;
                    let mut inside = point_in_rects(lx, ly, &rects, HIT_SLOP);
                    if !inside && interactive && any_button_down() {
                        inside = true; // a drag/click that started on the UI must not lose the window mid-gesture
                    }
                    if inside != interactive {
                        set_interactive(&app, inside);
                    }
                }
            }
        }
        std::thread::sleep(Duration::from_millis(sleep_ms));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bottom_center_of_work_area() {
        // 2560x1440 with a 48 px taskbar at the bottom, window 840x690 (560x460 @150%)
        let work = RECT { left: 0, top: 0, right: 2560, bottom: 1392 };
        assert_eq!(bottom_center(&work, 840, 690), (860, 702));
        // secondary monitor left of the primary
        let work = RECT { left: -1920, top: 0, right: 0, bottom: 1040 };
        assert_eq!(bottom_center(&work, 560, 460), (-1920 + 680, 580));
    }

    #[test]
    fn ex_style_is_tool_noactivate_without_appwindow() {
        let tao_style = 0x0004_0118; // APPWINDOW | WINDOWEDGE | ACCEPTFILES | TOPMOST, as tao writes it
        let s = enforce_ex_style(tao_style);
        assert_eq!(s & WS_EX_APPWINDOW.0, 0);
        assert_ne!(s & WS_EX_NOACTIVATE.0, 0);
        assert_ne!(s & WS_EX_TOOLWINDOW.0, 0);
        assert_ne!(s & 0x8, 0, "topmost kept");
    }
}
