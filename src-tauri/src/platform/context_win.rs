//! Windows context capture: foreground process + UI Automation (the Windows counterpart of the AX API).

use super::{window_around, AppContext};
use crate::platform::win::{self, ensure_com};
use std::cell::RefCell;
use std::time::{Duration, Instant};
use windows::core::{Interface, BSTR};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
use windows::Win32::UI::Accessibility::*;

/// Per-call timeouts for cross-process UIA requests (a hung app must not stall the dictation).
const UIA_TIMEOUT_MS: u32 = 150;
const UIA_BUDGET: Duration = Duration::from_millis(60);
const SURROUNDING_MAX: usize = 1500;
const FOCUSED_VALUE_MAX: i32 = 100_000;

/// Apps whose UIA tree is known to be unreliable (custom-drawn UI): never claim to know editability.
const BAD_UIA: &[&str] = &["weixin.exe", "wechat.exe", "wechatappex.exe", "qq.exe", "dingtalk.exe", "sublime_text.exe"];

thread_local! {
    static UIA: RefCell<Option<IUIAutomation>> = const { RefCell::new(None) };
}

fn automation() -> Option<IUIAutomation> {
    ensure_com();
    UIA.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.is_none() {
            let created: windows::core::Result<IUIAutomation> =
                unsafe { CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER) }
                    .or_else(|_| unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) });
            match created {
                Ok(a) => {
                    if let Ok(a2) = a.cast::<IUIAutomation2>() {
                        unsafe {
                            let _ = a2.SetConnectionTimeout(UIA_TIMEOUT_MS);
                            let _ = a2.SetTransactionTimeout(UIA_TIMEOUT_MS);
                        }
                    }
                    *slot = Some(a);
                }
                Err(e) => log::warn!("UI Automation unavailable: {e}"),
            }
        }
        slot.clone()
    })
}

const NON_TEXT_TYPES: &[UIA_CONTROLTYPE_ID] = &[
    UIA_ButtonControlTypeId, UIA_CheckBoxControlTypeId, UIA_RadioButtonControlTypeId, UIA_MenuItemControlTypeId,
    UIA_MenuControlTypeId, UIA_MenuBarControlTypeId, UIA_ImageControlTypeId, UIA_TextControlTypeId,
    UIA_SliderControlTypeId, UIA_TabControlTypeId, UIA_TabItemControlTypeId, UIA_ToolBarControlTypeId,
    UIA_ListControlTypeId, UIA_ListItemControlTypeId, UIA_TreeControlTypeId, UIA_TreeItemControlTypeId,
    UIA_TableControlTypeId, UIA_DataGridControlTypeId, UIA_DataItemControlTypeId, UIA_HyperlinkControlTypeId,
    UIA_ProgressBarControlTypeId, UIA_ScrollBarControlTypeId, UIA_SplitButtonControlTypeId, UIA_TitleBarControlTypeId,
    UIA_HeaderControlTypeId, UIA_HeaderItemControlTypeId, UIA_SpinnerControlTypeId,
];

struct Focus {
    el: IUIAutomationElement,
    control: UIA_CONTROLTYPE_ID,
    secure: bool,
    chromium: bool,
    text: Option<IUIAutomationTextPattern>,
    value: Option<IUIAutomationValuePattern>,
}

impl Focus {
    fn read(uia: &IUIAutomation) -> Option<Self> {
        unsafe {
            let el = uia.GetFocusedElement().ok()?;
            let control = el.CurrentControlType().unwrap_or_default();
            let secure = el.CurrentIsPassword().map(|b| b.as_bool()).unwrap_or(false);
            let framework = el.CurrentFrameworkId().map(|s| s.to_string()).unwrap_or_default();
            let text = el.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId).ok();
            let value = el.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId).ok();
            Some(Self { el, control, secure, chromium: framework == "Chrome", text, value })
        }
    }

    fn value_writable(&self) -> bool {
        self.value.as_ref().is_some_and(|v| unsafe { v.CurrentIsReadOnly() }.is_ok_and(|ro| !ro.as_bool()))
    }

    fn editable(&self) -> Option<bool> {
        let c = self.control;
        if self.value_writable() || (c == UIA_EditControlTypeId && self.text.is_some()) {
            return Some(true);
        }
        if c == UIA_EditControlTypeId || c == UIA_ComboBoxControlTypeId {
            // An edit without a writable value: read-only field, or a rich editor that only exposes TextPattern.
            return if self.text.is_some() { Some(true) } else { None };
        }
        if NON_TEXT_TYPES.contains(&c) {
            return Some(false);
        }
        None
    }

    /// Native edit controls report an empty selection reliably; Chromium does not always.
    fn trustworthy_empty_selection(&self) -> bool {
        !self.chromium && self.control == UIA_EditControlTypeId
    }

    fn selection(&self) -> Option<IUIAutomationTextRange> {
        let tp = self.text.as_ref()?;
        unsafe {
            let ranges = tp.GetSelection().ok()?;
            if ranges.Length().ok()? < 1 {
                return None;
            }
            ranges.GetElement(0).ok()
        }
    }

    fn selected_text(&self) -> Option<String> {
        let r = self.selection()?;
        unsafe { r.GetText(-1).ok().map(|b| b.to_string()) }
    }

    /// Up to SURROUNDING_MAX characters around the caret (two thirds before it), straight from UIA.
    fn surrounding_text(&self) -> Option<String> {
        if let Some(sel) = self.selection() {
            unsafe {
                if let Ok(r) = sel.Clone() {
                    let before = (SURROUNDING_MAX * 2 / 3) as i32;
                    let after = SURROUNDING_MAX as i32 - before;
                    let _ = r.MoveEndpointByUnit(TextPatternRangeEndpoint_Start, TextUnit_Character, -before);
                    let _ = r.MoveEndpointByUnit(TextPatternRangeEndpoint_End, TextUnit_Character, after);
                    if let Ok(t) = r.GetText((SURROUNDING_MAX * 2) as i32) {
                        let t = t.to_string();
                        if !t.is_empty() {
                            return Some(window_around(&t, t.encode_utf16().count() * 2 / 3, SURROUNDING_MAX));
                        }
                    }
                }
            }
        }
        let v = self.value_string()?;
        Some(window_around(&v, usize::MAX, SURROUNDING_MAX)).filter(|s| !s.is_empty())
    }

    fn value_string(&self) -> Option<String> {
        let v = self.value.as_ref()?;
        unsafe { v.CurrentValue().ok().map(|b: BSTR| b.to_string()).filter(|s| !s.is_empty()) }
    }

    fn full_text(&self) -> Option<String> {
        if let Some(tp) = &self.text {
            unsafe {
                if let Ok(t) = tp.DocumentRange().and_then(|r| r.GetText(FOCUSED_VALUE_MAX)) {
                    return Some(t.to_string());
                }
            }
        }
        self.value_string()
    }
}

/// Browsers expose the page URL as the Value of their top Document element: walk up from the focus.
fn browser_url(uia: &IUIAutomation, el: &IUIAutomationElement, start: Instant) -> Option<String> {
    unsafe {
        let walker = uia.ControlViewWalker().ok()?;
        let mut cur = Some(el.clone());
        let mut found = None;
        for _ in 0..30 {
            let Some(e) = cur else { break };
            if start.elapsed() > UIA_BUDGET * 2 {
                break;
            }
            if e.CurrentControlType().ok() == Some(UIA_DocumentControlTypeId) {
                if let Ok(v) = e.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId) {
                    if let Ok(s) = v.CurrentValue() {
                        let s = s.to_string();
                        if s.starts_with("http://") || s.starts_with("https://") {
                            found = Some(s); // keep walking: the outermost document is the page itself
                        }
                    }
                }
            }
            cur = walker.GetParentElement(&e).ok();
        }
        found
    }
}

const BROWSERS: &[&str] = &[
    "chrome.exe", "msedge.exe", "brave.exe", "firefox.exe", "opera.exe", "vivaldi.exe", "arc.exe", "chromium.exe",
    "zen.exe", "librewolf.exe", "waterfox.exe", "thorium.exe", "360chrome.exe", "360se.exe", "qqbrowser.exe",
    "sogouexplorer.exe", "2345explorer.exe", "yandex.exe",
];

pub fn capture(want_selection: bool, allow_copy_fallback: bool) -> AppContext {
    let start = Instant::now();
    let mut ctx = AppContext::default();
    let Some(fg) = win::foreground() else {
        return ctx;
    };
    ctx.pid = Some(fg.pid as i32);
    ctx.window_title = win::window_title(fg.hwnd);
    let exe = fg.path.as_deref().and_then(win::app_id_for_path);
    if let (Some(path), Some(id)) = (fg.path.as_deref(), exe.as_deref()) {
        ctx.app_name = Some(win::app_display_name(path));
        win::remember_path(id, path);
    }
    ctx.bundle_id = exe.clone();
    let bad_uia = exe.as_deref().is_some_and(|e| BAD_UIA.contains(&e));

    let Some(uia) = automation() else {
        return ctx;
    };
    let Some(focus) = Focus::read(&uia) else {
        return ctx;
    };
    // The focused element must belong to the foreground app (or to a child process of a browser/UWP host).
    let focus_pid = unsafe { focus.el.CurrentProcessId() }.unwrap_or(0) as u32;
    let same_app = focus_pid == fg.pid
        || focus_pid == win::window_pid(fg.hwnd)
        || win::process_path(focus_pid).as_deref().and_then(win::app_id_for_path) == exe
        || win::is_same_or_descendant(focus_pid, fg.pid);
    if !same_app {
        log::debug!("UIA focus (pid {focus_pid}) is outside the foreground app (pid {})", fg.pid);
        return ctx;
    }

    ctx.focused_editable = if bad_uia { None } else { focus.editable() };

    let mut known_empty = false;
    if want_selection && !focus.secure && start.elapsed() < UIA_BUDGET {
        match focus.selected_text() {
            Some(s) if !s.trim().is_empty() => ctx.selected_text = Some(s),
            Some(_) => known_empty = focus.trustworthy_empty_selection(),
            None => {}
        }
    }
    if !focus.secure && start.elapsed() < UIA_BUDGET {
        ctx.surrounding_text = focus.surrounding_text();
    }
    if exe.as_deref().is_some_and(|e| BROWSERS.contains(&e)) && start.elapsed() < UIA_BUDGET {
        ctx.url = browser_url(&uia, &focus.el, start);
    }

    if want_selection && ctx.selected_text.is_none() && allow_copy_fallback && !known_empty && !focus.secure {
        ctx.selected_text = crate::platform::insert::copy_selection_via_ctrl_c();
    }
    log::debug!("context captured in {:?}", start.elapsed());
    ctx
}

pub fn focused_text_value() -> Option<String> {
    let uia = automation()?;
    let focus = Focus::read(&uia)?;
    if focus.secure || focus.editable() != Some(true) {
        return None; // not a text field: its value would be meaningless
    }
    let mut v = focus.full_text()?;
    if v.len() > FOCUSED_VALUE_MAX as usize {
        let mut cut = FOCUSED_VALUE_MAX as usize;
        while !v.is_char_boundary(cut) {
            cut -= 1;
        }
        v.truncate(cut);
    }
    Some(v)
}
