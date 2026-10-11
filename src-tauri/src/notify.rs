//! Helpers that talk to the windows: bar alerts, bar state, navigation.

use crate::state::AppState;
use crate::types::{events, AlertAction, BarAlert, BarPhase, BarState, Mode, Settings};
use std::collections::{HashMap, VecDeque};
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter, Manager, WebviewWindow, WebviewWindowBuilder};

/// What an alert's buttons refer to.
#[derive(Clone, Debug, Default)]
pub struct AlertCtx {
    pub session_id: Option<String>,
    pub text: Option<String>,
}

#[derive(Default)]
pub struct AlertStore {
    map: HashMap<String, AlertCtx>,
    order: VecDeque<String>,
    by_kind: HashMap<String, String>,
}

impl AlertStore {
    fn insert(&mut self, kind: &str, id: String, ctx: AlertCtx) -> Option<String> {
        self.map.insert(id.clone(), ctx);
        self.order.push_back(id.clone());
        while self.order.len() > 64 {
            if let Some(old) = self.order.pop_front() {
                self.map.remove(&old);
            }
        }
        self.by_kind.insert(kind.to_string(), id)
    }
    pub fn get(&self, id: &str) -> Option<AlertCtx> {
        self.map.get(id).cloned()
    }
}

pub struct AlertBuilder {
    kind: String,
    title: String,
    description: Option<String>,
    actions: Vec<AlertAction>,
    duration_ms: Option<u64>,
    tone: String,
    ctx: AlertCtx,
}

pub fn alert(kind: &str, title: impl Into<String>, tone: &str) -> AlertBuilder {
    AlertBuilder {
        kind: kind.to_string(),
        title: title.into(),
        description: None,
        actions: Vec::new(),
        duration_ms: Some(5000),
        tone: tone.to_string(),
        ctx: AlertCtx::default(),
    }
}

impl AlertBuilder {
    pub fn desc(mut self, d: impl Into<String>) -> Self {
        self.description = Some(d.into());
        self
    }
    pub fn action(mut self, id: &str, label: &str, style: &str) -> Self {
        self.actions.push(AlertAction { id: id.into(), label: label.into(), style: style.into() });
        self
    }
    pub fn duration(mut self, ms: Option<u64>) -> Self {
        self.duration_ms = ms;
        self
    }
    pub fn session(mut self, id: &str) -> Self {
        self.ctx.session_id = Some(id.to_string());
        self
    }
    pub fn text(mut self, t: impl Into<String>) -> Self {
        self.ctx.text = Some(t.into());
        self
    }
    /// Emit to the bar window; returns the alert id. A previous alert of the same kind is replaced.
    pub fn show(self, app: &AppHandle) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        let payload = BarAlert {
            id: id.clone(),
            kind: self.kind.clone(),
            title: self.title,
            description: self.description,
            actions: self.actions,
            duration_ms: self.duration_ms,
            tone: self.tone,
        };
        let replaced = app.state::<AppState>().alerts.lock().insert(&self.kind, id.clone(), self.ctx);
        if let Some(old) = replaced {
            let _ = app.emit_to("bar", events::BAR_ALERT_DISMISS, old);
        }
        if let Err(e) = app.emit_to("bar", events::BAR_ALERT, &payload) {
            log::warn!("cannot emit bar alert: {e}");
        }
        id
    }
}

pub fn dismiss_alert(app: &AppHandle, id: &str) {
    let _ = app.emit_to("bar", events::BAR_ALERT_DISMISS, id.to_string());
}

// ───────────────────────── bar state ─────────────────────────

pub fn first_binding_label(settings: &Settings) -> String {
    settings.shortcuts.dictation.first().map(|b| key_label(b)).unwrap_or_else(|| "fn".into())
}

/// Human label for a binding such as "Fn" -> "fn", "RightOption" -> "右 ⌥" (Windows: "Ctrl+Cmd" -> "Ctrl+Win").
pub fn key_label(binding: &str) -> String {
    if cfg!(windows) {
        return windows_key_label(binding);
    }
    binding
        .split('+')
        .map(|k| match k {
            "Fn" => "fn".to_string(),
            "Help" => "Help".to_string(),
            "Esc" => "esc".to_string(),
            "Space" => "space".to_string(),
            "LeftOption" => "左 ⌥".to_string(),
            "RightOption" => "右 ⌥".to_string(),
            "Option" => "⌥".to_string(),
            "LeftCmd" => "左 ⌘".to_string(),
            "RightCmd" => "右 ⌘".to_string(),
            "Cmd" => "⌘".to_string(),
            "LeftShift" => "左 ⇧".to_string(),
            "RightShift" => "右 ⇧".to_string(),
            "Shift" => "⇧".to_string(),
            "LeftCtrl" => "左 ⌃".to_string(),
            "RightCtrl" => "右 ⌃".to_string(),
            "Ctrl" => "⌃".to_string(),
            other => other.to_string(),
        })
        .collect::<Vec<_>>()
        .join("")
}

/// Windows keyboards print words, not symbols: Option is Alt and Cmd is the Windows key.
fn windows_key_label(binding: &str) -> String {
    binding
        .split('+')
        .map(|k| {
            let (side, base) = match k {
                k if k.starts_with("Left") && k.len() > 4 => ("左 ", &k[4..]),
                k if k.starts_with("Right") && k.len() > 5 => ("右 ", &k[5..]),
                k => ("", k),
            };
            let base = match base {
                "Option" => "Alt",
                "Cmd" => "Win",
                "Esc" => "Esc",
                "Help" => "Insert",
                "Space" => "Space",
                b => b,
            };
            if matches!(base, "Alt" | "Win" | "Ctrl" | "Shift") {
                format!("{side}{base}")
            } else {
                k.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("+")
}

pub fn idle_phase(settings: &Settings) -> BarPhase {
    if settings.show_bar_on_desktop {
        BarPhase::Idle
    } else {
        BarPhase::Hidden
    }
}

pub fn translation_label(settings: &Settings) -> Option<String> {
    settings.translation_targets.first().map(|c| crate::config::language_label(c))
}

/// Mutate the shared bar state and push it to the bar window.
pub fn update_bar(app: &AppHandle, f: impl FnOnce(&mut BarState)) -> BarState {
    let state = app.state::<AppState>();
    let settings = state.settings.read().clone();
    let snapshot = {
        let mut bar = state.bar.lock();
        f(&mut bar);
        bar.show_idle_on_desktop = settings.show_bar_on_desktop;
        bar.dictation_key_label = first_binding_label(&settings);
        if bar.mode == Mode::Translation {
            bar.translation_target_label = translation_label(&settings);
        } else {
            bar.translation_target_label = None;
        }
        bar.clone()
    };
    if let Err(e) = app.emit_to("bar", events::BAR_STATE, &snapshot) {
        log::warn!("cannot emit bar state: {e}");
    }
    snapshot
}

// ───────────────────────── windows ─────────────────────────

/// Route requested while the hub was still loading; the frontend pulls it once subscribed.
static PENDING_ROUTE: Mutex<Option<String>> = Mutex::new(None);
/// True once the current hub webview has subscribed to `NAVIGATE`.
static HUB_READY: AtomicBool = AtomicBool::new(false);

/// Create the hub window from its `tauri.conf.json` entry (it is destroyed on close to free the webview).
fn create_main(app: &AppHandle) -> Option<WebviewWindow> {
    let cfg = app.config().app.windows.iter().find(|w| w.label == "main")?;
    HUB_READY.store(false, Ordering::SeqCst);
    *PENDING_ROUTE.lock() = None;
    match WebviewWindowBuilder::from_config(app, cfg).and_then(|b| b.build()) {
        Ok(w) => Some(w),
        Err(e) => {
            log::error!("cannot create main window: {e}");
            None
        }
    }
}

/// Called when the hub window is destroyed.
pub fn main_closed() {
    HUB_READY.store(false, Ordering::SeqCst);
    *PENDING_ROUTE.lock() = None;
}

/// Frontend handshake: marks the hub ready and returns any route requested before it was.
pub fn take_pending_route() -> Option<String> {
    let mut pending = PENDING_ROUTE.lock();
    HUB_READY.store(true, Ordering::SeqCst);
    pending.take()
}

/// Show + focus the hub window, creating it if it was closed.
pub fn show_main(app: &AppHandle) {
    let Some(w) = app.get_webview_window("main").or_else(|| create_main(app)) else { return };
    let _ = w.show();
    let _ = w.unminimize();
    let _ = w.set_focus();
}

/// Show + focus the main window and ask the hub to switch page.
pub fn navigate(app: &AppHandle, page: &str) {
    show_main(app);
    let pending = PENDING_ROUTE.lock();
    if HUB_READY.load(Ordering::SeqCst) {
        drop(pending);
        let _ = app.emit_to("main", events::NAVIGATE, page.to_string());
    } else {
        let mut pending = pending;
        *pending = Some(page.to_string());
    }
}
