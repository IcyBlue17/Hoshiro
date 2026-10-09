//! Floating bar window plumbing. OWNER: platform agent.
//! The "bar" webview window (label "bar", 560x460, transparent) is created from tauri.conf.json.
//! This module turns it into a non-activating, all-spaces, above-everything panel, positions it at the
//! bottom-center of the screen that contains the mouse / key window, and implements click-through for
//! everything outside the hit rects reported by the bar UI.
//!
//! Lifecycle expected from the controller: `setup` once at startup, then `show` once (the panel is fully
//! transparent and click-through when idle, so it can stay ordered-front forever), `set_hit_rects` whenever the
//! interactive parts of the UI move. `hide` is `orderOut`.

use crate::types::HitRect;
use tauri::AppHandle;

/// Called once at startup. Converts the bar window to a non-activating panel (NSPanel with
/// NSWindowStyleMaskNonactivatingPanel, level above status bar, canJoinAllSpaces + fullScreenAuxiliary),
/// makes it ignore mouse events by default and starts the hit-test poller.
///
/// Safe to call from any thread; the AppKit work happens on the main thread (inline when already there).
pub fn setup(app: &AppHandle) -> anyhow::Result<()> {
    #[cfg(target_os = "macos")]
    {
        mac_impl::setup(app)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
        Ok(())
    }
}

/// Show (without activating / stealing focus) and re-position on the active screen.
pub fn show(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    mac_impl::show(app);
    #[cfg(not(target_os = "macos"))]
    let _ = app;
}

/// Hide the panel.
pub fn hide(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    mac_impl::hide(app);
    #[cfg(not(target_os = "macos"))]
    let _ = app;
}

/// Update interactive regions (CSS px relative to the bar window's top-left).
pub fn set_hit_rects(app: &AppHandle, rects: Vec<HitRect>) {
    #[cfg(target_os = "macos")]
    mac_impl::set_hit_rects(app, rects);
    #[cfg(not(target_os = "macos"))]
    let _ = (app, rects);
}

/// Pure hit test: is the point (window coordinates, origin top-left) inside any rect (with a small slop)?
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn point_in_rects(x: f64, y: f64, rects: &[HitRect], slop: f64) -> bool {
    rects
        .iter()
        .any(|r| x >= r.x - slop && x <= r.x + r.width + slop && y >= r.y - slop && y <= r.y + r.height + slop)
}

/// Pure placement math in AppKit coordinates (origin bottom-left):
/// bottom-center of `visible` (x, y, w, h) for a window of `win_w` x `win_h`.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn bottom_center_origin(visible: (f64, f64, f64, f64), win_w: f64, gap: f64) -> (f64, f64) {
    let (vx, vy, vw, _vh) = visible;
    (vx + (vw - win_w) / 2.0, vy + gap)
}

#[cfg(target_os = "macos")]
mod mac_impl {
    use super::{bottom_center_origin, point_in_rects};
    use crate::platform::mac::is_main_thread;
    use crate::types::HitRect;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject, Bool, ClassBuilder, Sel};
    use objc2::{sel, ClassType, MainThreadMarker};
    use objc2_app_kit::{
        NSColor, NSEvent, NSPanel, NSScreen, NSWindow, NSWindowCollectionBehavior, NSWindowStyleMask,
    };
    use objc2_foundation::NSPoint;
    use parking_lot::Mutex;
    use std::sync::OnceLock;
    use std::time::Duration;
    use tauri::{AppHandle, Manager};

    const LABEL: &str = "bar";
    /// Same as NSPopUpMenuWindowLevel: above the menu bar, Dock and full-screen apps' chrome.
    const PANEL_LEVEL: isize = 101;
    const BOTTOM_GAP: f64 = 0.0;
    const HIT_SLOP: f64 = 1.0;

    struct BarState {
        rects: Vec<HitRect>,
        /// Window frame in AppKit global coordinates (x, y, w, h), refreshed whenever we move it.
        frame: Option<(f64, f64, f64, f64)>,
        visible: bool,
        /// Last value applied via setIgnoresMouseEvents (inverted): true = window currently receives clicks.
        interactive: bool,
        poller_running: bool,
    }

    static STATE: Mutex<BarState> =
        Mutex::new(BarState { rects: Vec::new(), frame: None, visible: false, interactive: false, poller_running: false });

    fn run_on_main(app: &AppHandle, f: impl FnOnce() + Send + 'static) {
        if is_main_thread() {
            f();
        } else if let Err(e) = app.run_on_main_thread(f) {
            log::warn!("run_on_main_thread failed: {e}");
        }
    }

    extern "C" fn no(_this: &AnyObject, _sel: Sel) -> Bool {
        Bool::NO
    }

    /// NSPanel subclass that never becomes key/main, so clicking the bar never activates our app or steals
    /// focus from the app the user is typing in.
    fn panel_class() -> Option<&'static AnyClass> {
        static CLASS: OnceLock<Option<&'static AnyClass>> = OnceLock::new();
        *CLASS.get_or_init(|| {
            let mut b = ClassBuilder::new(c"HoshiroBarPanel", NSPanel::class())?;
            // Keep the ivar tao's window class declares so any stray ivar access stays valid after the class swap.
            b.add_ivar::<Bool>(c"focusable");
            unsafe {
                b.add_method(sel!(canBecomeKeyWindow), no as extern "C" fn(_, _) -> _);
                b.add_method(sel!(canBecomeMainWindow), no as extern "C" fn(_, _) -> _);
            }
            Some(b.register())
        })
    }

    #[cfg(test)]
    pub(super) fn panel_class_for_test() -> Option<&'static AnyClass> {
        panel_class()
    }

    fn ns_window_ptr(app: &AppHandle) -> Option<*mut NSWindow> {
        let w = app.get_webview_window(LABEL)?;
        match w.ns_window() {
            Ok(p) if !p.is_null() => Some(p as *mut NSWindow),
            _ => None,
        }
    }

    pub fn setup(app: &AppHandle) -> anyhow::Result<()> {
        let Some(window) = app.get_webview_window(LABEL) else {
            anyhow::bail!("bar window not found");
        };
        let class = panel_class().ok_or_else(|| anyhow::anyhow!("could not register the panel class"))?;
        let (tx, rx) = std::sync::mpsc::channel::<anyhow::Result<()>>();
        let app2 = app.clone();
        let task = move || {
            let result = (|| -> anyhow::Result<()> {
                let ptr = window.ns_window().map_err(|e| anyhow::anyhow!("ns_window: {e}"))?;
                if ptr.is_null() {
                    anyhow::bail!("bar NSWindow is null");
                }
                unsafe { convert_to_panel(ptr as *mut NSWindow, class) };
                let _ = &app2;
                Ok(())
            })();
            let _ = tx.send(result);
        };
        if is_main_thread() {
            task();
        } else {
            app.run_on_main_thread(task).map_err(|e| anyhow::anyhow!("run_on_main_thread: {e}"))?;
        }
        match rx.recv_timeout(Duration::from_secs(5)) {
            Ok(r) => r,
            Err(_) => anyhow::bail!("timed out configuring the bar panel"),
        }
    }

    /// Main thread only.
    unsafe fn convert_to_panel(win: *mut NSWindow, class: &'static AnyClass) {
        let object = win as *mut AnyObject;
        let current = (*object).class();
        if !std::ptr::eq(current, class) {
            log::debug!(
                "bar window class {} (size {}) -> {} (size {})",
                current.name().to_string_lossy(),
                current.instance_size(),
                class.name().to_string_lossy(),
                class.instance_size()
            );
            objc2::ffi::object_setClass(object as *mut _, class as *const AnyClass as *const _);
        }
        let panel: &NSPanel = &*(win as *const NSPanel);
        let window: &NSWindow = panel;

        let mask = window.styleMask() | NSWindowStyleMask::NonactivatingPanel;
        window.setStyleMask(mask);
        panel.setFloatingPanel(true);
        panel.setBecomesKeyOnlyIfNeeded(true);
        panel.setWorksWhenModal(true);
        window.setLevel(PANEL_LEVEL);
        window.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::FullScreenAuxiliary
                | NSWindowCollectionBehavior::Stationary
                | NSWindowCollectionBehavior::IgnoresCycle,
        );
        window.setHasShadow(false);
        window.setOpaque(false);
        window.setBackgroundColor(Some(&NSColor::clearColor()));
        window.setHidesOnDeactivate(false);
        window.setMovable(false);
        window.setAcceptsMouseMovedEvents(true);
        window.setIgnoresMouseEvents(true);

        let f = window.frame();
        let mut st = STATE.lock();
        st.frame = Some((f.origin.x, f.origin.y, f.size.width, f.size.height));
        st.interactive = false;
        log::info!("bar window converted to a non-activating panel");
    }

    /// Main thread only. Moves the window to the bottom-center of the screen under the mouse.
    unsafe fn position(win: *mut NSWindow) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let window: &NSWindow = &*win;
        let mouse = NSEvent::mouseLocation();
        let screens = NSScreen::screens(mtm);
        let screen: Option<Retained<NSScreen>> = screens
            .iter()
            .find(|s| {
                let f = s.frame();
                mouse.x >= f.origin.x
                    && mouse.x < f.origin.x + f.size.width
                    && mouse.y >= f.origin.y
                    && mouse.y < f.origin.y + f.size.height
            })
            .or_else(|| NSScreen::mainScreen(mtm));
        let Some(screen) = screen else {
            return;
        };
        let vf = screen.visibleFrame();
        let size = window.frame().size;
        let (x, y) = bottom_center_origin((vf.origin.x, vf.origin.y, vf.size.width, vf.size.height), size.width, BOTTOM_GAP);
        window.setFrameOrigin(NSPoint::new(x, y));
        STATE.lock().frame = Some((x, y, size.width, size.height));
    }

    pub fn show(app: &AppHandle) {
        let app2 = app.clone();
        run_on_main(app, move || {
            let Some(ptr) = ns_window_ptr(&app2) else {
                return;
            };
            unsafe {
                position(ptr);
                (*ptr).orderFrontRegardless();
            }
            STATE.lock().visible = true;
            ensure_poller(&app2);
        });
    }

    pub fn hide(app: &AppHandle) {
        let app2 = app.clone();
        run_on_main(app, move || {
            let Some(ptr) = ns_window_ptr(&app2) else {
                return;
            };
            unsafe {
                (*ptr).setIgnoresMouseEvents(true);
                (*ptr).orderOut(None);
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
        let app2 = app.clone();
        run_on_main(app, move || {
            if let Some(ptr) = ns_window_ptr(&app2) {
                unsafe { (*ptr).setIgnoresMouseEvents(!interactive) };
            }
        });
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

    /// Toggles click-through by comparing the global mouse position with the reported hit rects.
    fn poll_loop(app: AppHandle) {
        loop {
            let (rects, frame, visible, interactive) = {
                let mut st = STATE.lock();
                if st.rects.is_empty() {
                    st.poller_running = false;
                    return;
                }
                (st.rects.clone(), st.frame, st.visible, st.interactive)
            };
            let mut sleep_ms = 100;
            if let (Some((fx, fy, fw, fh)), true) = (frame, visible) {
                let mouse = NSEvent::mouseLocation();
                let near = mouse.x >= fx - 60.0 && mouse.x <= fx + fw + 60.0 && mouse.y >= fy - 60.0 && mouse.y <= fy + fh + 60.0;
                sleep_ms = if near { 16 } else { 66 };
                // window-local, origin top-left
                let lx = mouse.x - fx;
                let ly = (fy + fh) - mouse.y;
                let mut inside = point_in_rects(lx, ly, &rects, HIT_SLOP);
                if !inside && interactive && NSEvent::pressedMouseButtons() != 0 {
                    inside = true; // a drag/click that started on the UI must not lose the window mid-gesture
                }
                if inside != interactive {
                    set_interactive(&app, inside);
                }
            }
            std::thread::sleep(Duration::from_millis(sleep_ms));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x: f64, y: f64, w: f64, h: f64) -> HitRect {
        HitRect { x, y, width: w, height: h }
    }

    #[test]
    fn hit_test() {
        let rects = [r(100.0, 400.0, 120.0, 30.0), r(10.0, 10.0, 50.0, 50.0)];
        assert!(point_in_rects(150.0, 410.0, &rects, 0.0));
        assert!(point_in_rects(30.0, 30.0, &rects, 0.0));
        assert!(!point_in_rects(150.0, 300.0, &rects, 0.0));
        assert!(!point_in_rects(99.5, 410.0, &rects, 0.0));
        assert!(point_in_rects(99.5, 410.0, &rects, 1.0));
        assert!(!point_in_rects(1.0, 1.0, &[], 1.0));
    }

    #[test]
    fn placement() {
        // 1440x900 screen with 25 px menu bar on top and 70 px Dock at the bottom
        let (x, y) = bottom_center_origin((0.0, 70.0, 1440.0, 805.0), 560.0, 0.0);
        assert_eq!((x, y), (440.0, 70.0));
        // secondary display to the left with negative origin
        let (x, y) = bottom_center_origin((-1920.0, 0.0, 1920.0, 1055.0), 560.0, 2.0);
        assert_eq!((x, y), (-1920.0 + 680.0, 2.0));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn panel_has_same_layout_as_window() {
        use objc2::ClassType;
        // The class swap (object_setClass) is only sound if NSPanel adds no instance storage.
        let window = objc2_app_kit::NSWindow::class().instance_size();
        let panel = objc2_app_kit::NSPanel::class().instance_size();
        assert_eq!(window, panel, "NSPanel instance size differs from NSWindow");
        assert!(super::mac_impl::panel_class_for_test().is_some());
    }
}
