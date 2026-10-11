//! Linux bar plumbing (GTK on the X11 backend, see `linux::prefer_x11_backend`): a keep-above notification
//! window that never takes keyboard focus, is on every workspace and stays out of the taskbar / Alt+Tab,
//! placed bottom-center of the work area of the monitor under the pointer.
//!
//! Click-through is an X input shape: only the hit rects reported by the bar UI receive the pointer, so no
//! hover poller is needed. Transparency needs a compositing window manager (every mainstream desktop has one);
//! without one the window is clipped to the same rects so it never covers other apps with black.

use crate::types::HitRect;
use gtk::gdk;
use gtk::prelude::*;
use parking_lot::Mutex;
use tauri::{AppHandle, Manager};

const LABEL: &str = "bar";
/// Grow each rect a little so a pointer resting on the edge of a pill still hits it.
const HIT_SLOP: f64 = 1.0;

struct BarState {
    rects: Vec<HitRect>,
    visible: bool,
}

static STATE: Mutex<BarState> = Mutex::new(BarState { rects: Vec::new(), visible: false });

fn run_on_main(app: &AppHandle, f: impl FnOnce() + Send + 'static) {
    if let Err(e) = app.run_on_main_thread(f) {
        log::warn!("run_on_main_thread failed: {e}");
    }
}

fn gtk_window(app: &AppHandle) -> Option<gtk::ApplicationWindow> {
    app.get_webview_window(LABEL)?.gtk_window().ok()
}

/// Pure: integer rectangles (x, y, w, h) covering `rects` grown by `slop`.
pub(crate) fn shape_rects(rects: &[HitRect], slop: f64) -> Vec<(i32, i32, i32, i32)> {
    rects
        .iter()
        .filter(|r| r.width > 0.0 && r.height > 0.0)
        .map(|r| {
            let x0 = (r.x - slop).floor();
            let y0 = (r.y - slop).floor();
            let x1 = (r.x + r.width + slop).ceil();
            let y1 = (r.y + r.height + slop).ceil();
            (x0 as i32, y0 as i32, (x1 - x0) as i32, (y1 - y0) as i32)
        })
        .collect()
}

/// Main thread only.
fn apply_shape(win: &gtk::ApplicationWindow) {
    let Some(gw) = win.window() else { return }; // not realized yet: `connect_map` applies it later
    let region = gtk::cairo::Region::create();
    for (x, y, w, h) in shape_rects(&STATE.lock().rects, HIT_SLOP) {
        let _ = region.union_rectangle(&gtk::cairo::RectangleInt::new(x, y, w, h));
    }
    gw.input_shape_combine_region(&region, 0, 0);
    // Without a compositor the transparent parts would be painted black: clip what is drawn to the UI too
    // (square corners, but nothing covers the apps below).
    if composited(win) {
        gw.shape_combine_region(None, 0, 0);
    } else {
        gw.shape_combine_region(Some(&region), 0, 0);
    }
}

fn composited(win: &gtk::ApplicationWindow) -> bool {
    gtk::prelude::GtkWindowExt::screen(win).is_some_and(|s| s.is_composited())
}

/// Main thread only: bottom-center of the work area of the monitor under the pointer.
fn place(win: &gtk::ApplicationWindow) {
    let Some(display) = gdk::Display::default() else { return };
    let pointer = display.default_seat().and_then(|s| s.pointer());
    let monitor = match pointer {
        Some(p) => {
            let (_, x, y) = p.position();
            display.monitor_at_point(x, y)
        }
        None => display.primary_monitor(),
    };
    let Some(monitor) = monitor.or_else(|| display.monitor(0)) else { return };
    let area = monitor.workarea();
    let (w, h) = win.size();
    win.move_(area.x() + (area.width() - w) / 2, area.y() + area.height() - h);
}

pub fn setup(app: &AppHandle) -> anyhow::Result<()> {
    let handle = app.clone();
    run_on_main(app, move || {
        let Some(win) = gtk_window(&handle) else {
            log::warn!("bar window missing");
            return;
        };
        // Type hint and focus hints must be set before the window is mapped.
        win.set_type_hint(gdk::WindowTypeHint::Notification);
        win.set_accept_focus(false);
        win.set_focus_on_map(false);
        win.set_skip_taskbar_hint(true);
        win.set_skip_pager_hint(true);
        win.set_keep_above(true);
        win.stick();
        win.connect_map(|w| {
            apply_shape(w);
            w.set_keep_above(true);
            w.stick();
        });
        // A compositor can start (or stop) after us: re-clip when that changes.
        if let Some(screen) = gtk::prelude::GtkWindowExt::screen(&win) {
            let w = win.clone();
            screen.connect_composited_changed(move |_| apply_shape(&w));
        }
        apply_shape(&win);
        if !composited(&win) {
            log::warn!("no compositing window manager: the bar is clipped to its visible parts");
        }
    });
    Ok(())
}

pub fn show(app: &AppHandle) {
    let handle = app.clone();
    run_on_main(app, move || {
        let Some(w) = handle.get_webview_window(LABEL) else { return };
        if let Ok(win) = w.gtk_window() {
            place(&win);
        }
        if !STATE.lock().visible {
            if let Err(e) = w.show() {
                log::warn!("cannot show the bar: {e}");
                return;
            }
            STATE.lock().visible = true;
        }
        if let Ok(win) = w.gtk_window() {
            win.set_keep_above(true);
            apply_shape(&win);
        }
    });
}

pub fn hide(app: &AppHandle) {
    let handle = app.clone();
    run_on_main(app, move || {
        STATE.lock().visible = false;
        if let Some(w) = handle.get_webview_window(LABEL) {
            let _ = w.hide();
        }
    });
}

pub fn set_hit_rects(app: &AppHandle, rects: Vec<HitRect>) {
    {
        let mut s = STATE.lock();
        if s.rects == rects {
            return;
        }
        s.rects = rects;
    }
    let handle = app.clone();
    run_on_main(app, move || {
        if let Some(win) = gtk_window(&handle) {
            apply_shape(&win);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shape_covers_rects_with_slop() {
        let r = HitRect { x: 10.4, y: 20.6, width: 100.0, height: 30.0 };
        assert_eq!(shape_rects(&[r], 1.0), vec![(9, 19, 103, 33)]);
        let empty = HitRect { x: 0.0, y: 0.0, width: 0.0, height: 10.0 };
        assert!(shape_rects(&[empty], 1.0).is_empty());
    }
}
