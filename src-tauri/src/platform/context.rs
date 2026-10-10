//! Captures context about where the user is typing.

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct AppContext {
    pub app_name: Option<String>,
    pub bundle_id: Option<String>,
    pub window_title: Option<String>,
    /// Browser URL when cheaply available (optional).
    pub url: Option<String>,
    /// Currently selected text in the focused element (via Accessibility).
    pub selected_text: Option<String>,
    /// Some(true) if the focused element is an editable text input, Some(false) if we are confident
    /// nothing editable is focused, None if unknown (AX not supported by the app).
    pub focused_editable: Option<bool>,
    /// Up to ~1500 chars of text around the caret in the focused field (for style/context), if available.
    pub surrounding_text: Option<String>,
    /// Pid of the frontmost app (used later to verify focus didn't change before pasting).
    pub pid: Option<i32>,
}

/// Fast (< ~50 ms) snapshot of the frontmost app and focused element.
/// `want_selection`: also read selected text (via AX; falls back to a Cmd+C round-trip that restores the
/// clipboard, only when `allow_copy_fallback` is true — that fallback can add up to ~200 ms, so call this
/// from a worker thread, never from the hotkey callback).
pub fn capture(want_selection: bool, allow_copy_fallback: bool) -> AppContext {
    #[cfg(target_os = "macos")]
    {
        mac_impl::capture(want_selection, allow_copy_fallback)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (want_selection, allow_copy_fallback);
        AppContext::default()
    }
}

/// Read the current value of the focused text element (for auto-dictionary learning), if available.
pub fn focused_text_value() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        mac_impl::focused_text_value()
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

/// Pure helper: up to `max_chars` UTF-16 units around `caret` (a UTF-16 offset), with a bias towards the text before the caret.
/// Never splits surrogate pairs.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn window_around(text: &str, caret_utf16: usize, max_chars: usize) -> String {
    let units: Vec<u16> = text.encode_utf16().collect();
    if units.len() <= max_chars {
        return text.to_string();
    }
    let caret = caret_utf16.min(units.len());
    let mut start = caret.saturating_sub(max_chars * 2 / 3);
    let mut end = start + max_chars;
    if end > units.len() {
        end = units.len();
        start = end - max_chars;
    }
    // do not start on a low surrogate or end after a lone high surrogate
    if start < units.len() && (0xDC00..0xE000).contains(&units[start]) {
        start += 1;
    }
    if end > start && (0xD800..0xDC00).contains(&units[end - 1]) {
        end -= 1;
    }
    String::from_utf16_lossy(&units[start..end])
}

#[cfg(target_os = "macos")]
mod mac_impl {
    use super::{window_around, AppContext};
    use crate::platform::insert::{restore_snapshot, snapshot_pasteboard};
    use crate::platform::mac::*;
    use core_foundation::base::{CFGetTypeID, CFRange, CFTypeRef, TCFType};
    use core_foundation::boolean::CFBoolean;
    use core_foundation::number::CFNumber;
    use core_foundation::string::CFString;
    use core_graphics::event::CGEventFlags;
    use objc2_app_kit::{NSPasteboard, NSWorkspace};
    use objc2_foundation::NSString;
    use parking_lot::Mutex;
    use std::collections::HashMap;
    use std::ffi::c_void;
    use std::time::{Duration, Instant};

    const AX_TIMEOUT_SECS: f32 = 0.12;
    const AX_BUDGET: Duration = Duration::from_millis(50);
    const AX_ERR_CANNOT_COMPLETE: i32 = -25204;
    const AX_VALUE_CFRANGE: u32 = 4;
    const SURROUNDING_MAX: usize = 1500;
    const FOCUSED_VALUE_MAX: usize = 100_000;

    const WECHAT: &str = "com.tencent.xinWeChat";
    const SUBLIME: &str = "com.sublimetext.4";

    const TEXT_ROLES: &[&str] = &["AXTextField", "AXTextArea", "AXComboBox", "AXSearchField"];
    /// Roles that are clearly not text input. Anything else (AXGroup, AXWebArea, AXWindow ...) stays "unknown".
    const NON_TEXT_ROLES: &[&str] = &[
        "AXButton", "AXCheckBox", "AXRadioButton", "AXPopUpButton", "AXMenuButton", "AXMenuItem", "AXMenuBarItem",
        "AXMenu", "AXMenuBar", "AXImage", "AXStaticText", "AXSlider", "AXTabGroup", "AXToolbar", "AXList", "AXTable",
        "AXOutline", "AXRow", "AXColumn", "AXCell", "AXBrowser", "AXDisclosureTriangle", "AXScrollBar", "AXSplitter",
        "AXLink", "AXProgressIndicator", "AXIncrementor", "AXColorWell", "AXDockItem",
    ];

    /// pid -> whether the app accepted AXManualAccessibility (i.e. is Chromium/Electron).
    static MANUAL_AX: Mutex<Option<HashMap<i32, bool>>> = Mutex::new(None);

    struct Ax {
        start: Instant,
        hung: bool,
    }

    impl Ax {
        fn new() -> Self {
            Self { start: Instant::now(), hung: false }
        }

        fn over_budget(&self) -> bool {
            self.hung || self.start.elapsed() > AX_BUDGET
        }

        fn copy(&mut self, el: CFTypeRef, attr: &str) -> Option<CfOwned> {
            if self.hung {
                return None;
            }
            let key = cfstr(attr);
            let mut out: CFTypeRef = std::ptr::null();
            let t = Instant::now();
            let err = unsafe { AXUIElementCopyAttributeValue(el, key.as_concrete_TypeRef() as CFStringRef, &mut out) };
            if err == AX_ERR_CANNOT_COMPLETE && t.elapsed() > Duration::from_millis(80) {
                log::debug!("AX call {attr} timed out; skipping the remaining AX reads");
                self.hung = true;
            }
            if err != 0 {
                return None;
            }
            CfOwned::new(out)
        }

        fn string(&mut self, el: CFTypeRef, attr: &str) -> Option<String> {
            self.copy(el, attr).and_then(|v| cf_string(&v))
        }

        fn settable(&mut self, el: CFTypeRef, attr: &str) -> bool {
            if self.hung {
                return false;
            }
            let key = cfstr(attr);
            let mut s: u8 = 0;
            let err = unsafe { AXUIElementIsAttributeSettable(el, key.as_concrete_TypeRef() as CFStringRef, &mut s) };
            err == 0 && s != 0
        }
    }

    fn cf_string(v: &CfOwned) -> Option<String> {
        unsafe {
            if CFGetTypeID(v.as_ptr()) != CFString::type_id() {
                return None;
            }
            Some(CFString::wrap_under_get_rule(v.as_ptr() as _).to_string())
        }
    }

    fn cf_i64(v: &CfOwned) -> Option<i64> {
        unsafe {
            if CFGetTypeID(v.as_ptr()) != CFNumber::type_id() {
                return None;
            }
            CFNumber::wrap_under_get_rule(v.as_ptr() as _).to_i64()
        }
    }

    fn cf_range(v: &CfOwned) -> Option<CFRange> {
        unsafe {
            let mut r = CFRange { location: 0, length: 0 };
            if AXValueGetType(v.as_ptr()) != AX_VALUE_CFRANGE {
                return None;
            }
            if AXValueGetValue(v.as_ptr(), AX_VALUE_CFRANGE, &mut r as *mut CFRange as *mut c_void) {
                Some(r)
            } else {
                None
            }
        }
    }

    struct Frontmost {
        name: Option<String>,
        bundle_id: Option<String>,
        pid: i32,
    }

    fn frontmost() -> Option<Frontmost> {
        let app = NSWorkspace::sharedWorkspace().frontmostApplication()?;
        Some(Frontmost {
            name: app.localizedName().map(|s| s.to_string()),
            bundle_id: app.bundleIdentifier().map(|s| s.to_string()),
            pid: app.processIdentifier(),
        })
    }

    /// Set AXManualAccessibility once per pid so Chromium/Electron expose their web content to AX.
    fn ensure_manual_accessibility(app_el: CFTypeRef, pid: i32) -> bool {
        let mut guard = MANUAL_AX.lock();
        let map = guard.get_or_insert_with(HashMap::new);
        if let Some(v) = map.get(&pid) {
            return *v;
        }
        let key = cfstr("AXManualAccessibility");
        let err = unsafe {
            AXUIElementSetAttributeValue(
                app_el,
                key.as_concrete_TypeRef() as CFStringRef,
                CFBoolean::true_value().as_CFTypeRef(),
            )
        };
        let ok = err == 0;
        if ok {
            log::debug!("enabled AXManualAccessibility for pid {pid}");
        }
        if map.len() > 512 {
            map.clear();
        }
        map.insert(pid, ok);
        ok
    }

    struct Focus {
        app_el: CfOwned,
        element: Option<CfOwned>,
        chromium: bool,
    }

    fn resolve_focus(ax: &mut Ax, pid: i32) -> Option<Focus> {
        let app_el = CfOwned::new(unsafe { AXUIElementCreateApplication(pid) })?;
        unsafe { AXUIElementSetMessagingTimeout(app_el.as_ptr(), AX_TIMEOUT_SECS) };
        let chromium = ensure_manual_accessibility(app_el.as_ptr(), pid);
        let element = ax.copy(app_el.as_ptr(), "AXFocusedUIElement");
        if let Some(e) = &element {
            unsafe { AXUIElementSetMessagingTimeout(e.as_ptr(), AX_TIMEOUT_SECS) };
        }
        Some(Focus { app_el, element, chromium })
    }

    pub fn capture(want_selection: bool, allow_copy_fallback: bool) -> AppContext {
        let mut ctx = AppContext::default();
        let Some(front) = frontmost() else {
            return ctx;
        };
        ctx.app_name = front.name;
        ctx.bundle_id = front.bundle_id.clone();
        ctx.pid = Some(front.pid);

        if !unsafe { AXIsProcessTrusted() } {
            return ctx;
        }
        let bad_ax = matches!(front.bundle_id.as_deref(), Some(WECHAT) | Some(SUBLIME));

        let mut ax = Ax::new();
        unsafe {
            let sys = AXUIElementCreateSystemWide();
            if !sys.is_null() {
                AXUIElementSetMessagingTimeout(sys, AX_TIMEOUT_SECS);
                core_foundation::base::CFRelease(sys);
            }
        }
        let Some(focus) = resolve_focus(&mut ax, front.pid) else {
            return ctx;
        };

        // Window title / URL.
        if !ax.over_budget() {
            let window = ax
                .copy(focus.app_el.as_ptr(), "AXFocusedWindow")
                .or_else(|| ax.copy(focus.app_el.as_ptr(), "AXMainWindow"));
            if let Some(w) = window {
                ctx.window_title = ax.string(w.as_ptr(), "AXTitle").filter(|t| !t.is_empty());
                if !ax.over_budget() {
                    ctx.url = ax
                        .string(w.as_ptr(), "AXDocument")
                        .filter(|u| u.starts_with("http://") || u.starts_with("https://"));
                }
            }
        }

        let mut ax_selection_known_empty = false;
        if let Some(el) = &focus.element {
            let p = el.as_ptr();
            let role = ax.string(p, "AXRole").unwrap_or_default();
            let subrole = ax.string(p, "AXSubrole").unwrap_or_default();
            let secure = subrole == "AXSecureTextField";
            let editable = TEXT_ROLES.contains(&role.as_str())
                || ax.settable(p, "AXValue")
                || (!matches!(role.as_str(), "AXStaticText" | "AXWebArea" | "AXScrollArea")
                    && ax.copy(p, "AXSelectedTextRange").is_some());
            ctx.focused_editable = if bad_ax {
                None
            } else if editable {
                Some(true)
            } else if NON_TEXT_ROLES.contains(&role.as_str()) {
                Some(false)
            } else {
                None
            };

            if want_selection && !secure && !ax.over_budget() {
                match ax.string(p, "AXSelectedText") {
                    Some(s) if !s.is_empty() => ctx.selected_text = Some(s),
                    Some(_) => {
                        // A definitive "nothing selected" from a native text control is trustworthy;
                        // Chromium/Electron fields often report empty regardless, so those still get the fallback.
                        ax_selection_known_empty = !focus.chromium && TEXT_ROLES.contains(&role.as_str());
                    }
                    None => {}
                }
            }
            if !secure && !ax.over_budget() {
                ctx.surrounding_text = surrounding_text(&mut ax, p);
            }
        }

        if want_selection && ctx.selected_text.is_none() && allow_copy_fallback && !ax_selection_known_empty {
            ctx.selected_text = copy_selection_via_cmd_c();
        }
        log::debug!("context captured in {:?}", ax.start.elapsed());
        ctx
    }

    fn surrounding_text(ax: &mut Ax, el: CFTypeRef) -> Option<String> {
        let range = ax.copy(el, "AXSelectedTextRange").and_then(|v| cf_range(&v));
        let total = ax.copy(el, "AXNumberOfCharacters").and_then(|v| cf_i64(&v));
        if let (Some(total), Some(r)) = (total, range) {
            if total as usize > SURROUNDING_MAX && !ax.over_budget() {
                // Ask only for the window we need: AXValue of a huge document can take very long.
                let before = (SURROUNDING_MAX * 2 / 3) as isize;
                let start = (r.location - before).max(0);
                let end = (start + SURROUNDING_MAX as isize).min(total as isize).max(start);
                let want = CFRange { location: start, length: end - start };
                unsafe {
                    let param = AXValueCreate(AX_VALUE_CFRANGE, &want as *const CFRange as *const c_void);
                    if let Some(param) = CfOwned::new(param) {
                        let key = cfstr("AXStringForRange");
                        let mut out: CFTypeRef = std::ptr::null();
                        let err = AXUIElementCopyParameterizedAttributeValue(
                            el,
                            key.as_concrete_TypeRef() as CFStringRef,
                            param.as_ptr(),
                            &mut out,
                        );
                        if err == 0 {
                            if let Some(s) = CfOwned::new(out).and_then(|v| cf_string(&v)) {
                                return Some(s).filter(|s| !s.is_empty());
                            }
                        }
                    }
                }
            }
        }
        let value = ax.string(el, "AXValue")?;
        if value.is_empty() {
            return None;
        }
        let caret = range.map(|r| (r.location + r.length).max(0) as usize).unwrap_or(usize::MAX);
        Some(window_around(&value, caret, SURROUNDING_MAX))
    }

    pub fn focused_text_value() -> Option<String> {
        if !unsafe { AXIsProcessTrusted() } {
            return None;
        }
        let front = frontmost()?;
        let mut ax = Ax::new();
        let focus = resolve_focus(&mut ax, front.pid)?;
        let el = focus.element?;
        if ax.string(el.as_ptr(), "AXSubrole").as_deref() == Some("AXSecureTextField") {
            return None;
        }
        let role = ax.string(el.as_ptr(), "AXRole").unwrap_or_default();
        if !TEXT_ROLES.contains(&role.as_str()) && !ax.settable(el.as_ptr(), "AXValue") {
            return None; // not a text field: AXValue would be meaningless (e.g. an empty web area)
        }
        let mut v = ax.string(el.as_ptr(), "AXValue")?;
        if v.len() > FOCUSED_VALUE_MAX {
            let mut cut = FOCUSED_VALUE_MAX;
            while !v.is_char_boundary(cut) {
                cut -= 1;
            }
            v.truncate(cut);
        }
        Some(v)
    }

    /// Cmd+C round trip: snapshot the pasteboard, copy, read, restore. None if the app copied nothing.
    fn copy_selection_via_cmd_c() -> Option<String> {
        let pb = NSPasteboard::generalPasteboard();
        let snapshot = snapshot_pasteboard(&pb);
        let before = pb.changeCount();
        let c = crate::platform::insert::keycode_for_char('c');
        if let Err(e) = post_key_press(c, CGEventFlags::CGEventFlagCommand) {
            log::warn!("Cmd+C fallback failed: {e}");
            return None;
        }
        let deadline = Instant::now() + Duration::from_millis(180);
        let mut changed = false;
        while Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(8));
            if pb.changeCount() != before {
                changed = true;
                break;
            }
        }
        if !changed {
            return None;
        }
        // Give the app a moment to finish writing all flavors.
        std::thread::sleep(Duration::from_millis(6));
        let text = pb
            .stringForType(&NSString::from_str("public.utf8-plain-text"))
            .map(|s| s.to_string())
            .filter(|s| !s.is_empty());
        restore_snapshot(&pb, &snapshot);
        text
    }
}

#[cfg(test)]
mod tests {
    use super::window_around;

    #[test]
    fn short_text_is_untouched() {
        assert_eq!(window_around("hello", 2, 1500), "hello");
    }

    #[test]
    fn window_is_bounded_and_contains_caret_context() {
        let text: String = (0..5000).map(|i| char::from(b'a' + (i % 26) as u8)).collect();
        let w = window_around(&text, 2500, 1500);
        assert_eq!(w.chars().count(), 1500);
        let start = 2500 - 1000;
        assert!(w.starts_with(&text[start..start + 10]));
    }

    #[test]
    fn window_at_edges() {
        let text = "x".repeat(4000);
        assert_eq!(window_around(&text, 0, 1500).len(), 1500);
        assert_eq!(window_around(&text, 4000, 1500).len(), 1500);
        assert_eq!(window_around(&text, usize::MAX, 1500).len(), 1500);
    }

    #[test]
    fn window_never_splits_surrogates() {
        let text = "😀".repeat(2000); // 4000 UTF-16 units
        for caret in [0usize, 1, 999, 1000, 1001, 3999] {
            let w = window_around(&text, caret, 1501);
            assert!(!w.contains('\u{FFFD}'), "caret {caret}");
            assert!(w.chars().all(|c| c == '😀'));
        }
    }
}
