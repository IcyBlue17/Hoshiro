//! Linux context capture: the active X11 window (`_NET_ACTIVE_WINDOW`, `WM_CLASS`, `_NET_WM_PID`) with app
//! names from the desktop entries.
//!
//! There is no cheap accessibility query here (AT-SPI is an asynchronous D-Bus tree that many apps only
//! expose with a screen reader running), so editability and text around the caret stay unknown; the selection
//! comes from the copy round trip. Native Wayland windows are not visible through XWayland: their app is
//! unknown, but the copy round trip still works because the keys go through the kernel.

use super::AppContext;
use crate::platform::linux;

pub fn capture(want_selection: bool, allow_copy_fallback: bool) -> AppContext {
    let start = std::time::Instant::now();
    let mut ctx = AppContext::default();
    if let Some(w) = linux::active_window() {
        ctx.pid = w.pid.map(|p| p as i32);
        ctx.window_title = w.title.clone();
        let id = w.app_id();
        ctx.app_name = id
            .as_deref()
            .and_then(linux::desktop_entry)
            .map(|e| e.name)
            .or_else(|| w.class.clone());
        ctx.bundle_id = id;
    }
    if want_selection && allow_copy_fallback {
        ctx.selected_text = crate::platform::insert::copy_selection_via_ctrl_c();
    }
    log::debug!("context captured in {:?}", start.elapsed());
    ctx
}

pub fn focused_text_value() -> Option<String> {
    None
}
