//! Pure hotkey logic: key naming, binding parsing and the Typeless gesture state machine.
//! No OS calls in here — everything is driven by explicit events and timestamps so it is unit-testable.

use super::hotkey::{HotkeyAction, SessionPhase};
use crate::types::{Mode, ShortcutBindings, TriggerMode};

pub type KeyName = &'static str;

/// Held at least this long => push-to-talk release finishes the recording.
pub const HOLD_THRESHOLD_MS: u64 = 350;
/// After a short tap, wait this long for a second press (double-press => hands-free lock).
pub const DOUBLE_PRESS_WINDOW_MS: u64 = 300;
/// Pressing another key within this time after the dictation key went down cancels silently.
pub const OTHER_KEY_WINDOW_MS: u64 = 1500;
/// Our own view of the session phase wins over the controller's for this long after we emitted an action
/// (the controller updates its phase asynchronously).
const ASSUMED_PHASE_GRACE_MS: u64 = 400;

// ───────────────────────── key tables ─────────────────────────

/// macOS virtual keycode -> canonical key name.
const KEYS: &[(u16, KeyName)] = &[
    (0, "A"), (11, "B"), (8, "C"), (2, "D"), (14, "E"), (3, "F"), (5, "G"), (4, "H"), (34, "I"),
    (38, "J"), (40, "K"), (37, "L"), (46, "M"), (45, "N"), (31, "O"), (35, "P"), (12, "Q"),
    (15, "R"), (1, "S"), (17, "T"), (32, "U"), (9, "V"), (13, "W"), (7, "X"), (16, "Y"), (6, "Z"),
    (29, "0"), (18, "1"), (19, "2"), (20, "3"), (21, "4"), (23, "5"), (22, "6"), (26, "7"),
    (28, "8"), (25, "9"),
    (122, "F1"), (120, "F2"), (99, "F3"), (118, "F4"), (96, "F5"), (97, "F6"), (98, "F7"),
    (100, "F8"), (101, "F9"), (109, "F10"), (103, "F11"), (111, "F12"), (105, "F13"), (107, "F14"),
    (113, "F15"), (106, "F16"), (64, "F17"), (79, "F18"), (80, "F19"), (90, "F20"),
    (123, "Left"), (124, "Right"), (125, "Down"), (126, "Up"),
    (49, "Space"), (53, "Esc"), (36, "Enter"), (48, "Tab"), (51, "Backspace"), (117, "Delete"),
    (115, "Home"), (119, "End"), (116, "PageUp"), (121, "PageDown"), (114, "Help"),
    (27, "-"), (24, "="), (33, "["), (30, "]"), (42, "\\"), (41, ";"), (39, "'"), (43, ","),
    (47, "."), (44, "/"), (50, "`"),
    (82, "Num0"), (83, "Num1"), (84, "Num2"), (85, "Num3"), (86, "Num4"), (87, "Num5"),
    (88, "Num6"), (89, "Num7"), (91, "Num8"), (92, "Num9"), (76, "NumEnter"),
    // modifiers
    (63, "Fn"), (56, "LeftShift"), (60, "RightShift"), (59, "LeftCtrl"), (62, "RightCtrl"),
    (58, "LeftOption"), (61, "RightOption"), (55, "LeftCmd"), (54, "RightCmd"),
];

/// CGEvent mouse button number (0-based) -> name. Buttons 0/1 (left/right) are never tapped.
const MOUSE: &[(i64, KeyName)] = &[(2, "Mouse3"), (3, "Mouse4"), (4, "Mouse5")];

pub fn key_for_code(code: u16) -> Option<KeyName> {
    KEYS.iter().find(|(c, _)| *c == code).map(|(_, n)| *n)
}

pub fn code_for_key(name: &str) -> Option<u16> {
    KEYS.iter().find(|(_, n)| *n == name).map(|(c, _)| *c)
}

pub fn mouse_button_name(button: i64) -> Option<KeyName> {
    MOUSE.iter().find(|(b, _)| *b == button).map(|(_, n)| *n)
}

pub fn mouse_button_number(name: &str) -> Option<i64> {
    MOUSE.iter().find(|(_, n)| *n == name).map(|(b, _)| *b)
}

/// Name for keycodes/buttons that are not in the tables. They still need an identity so that
/// "any other key pressed" logic sees them. Interned (leaked) once per distinct code.
pub fn unknown_key_name(kind: &str, id: i64) -> KeyName {
    use once_cell::sync::Lazy;
    use parking_lot::Mutex;
    use std::collections::HashMap;
    static CACHE: Lazy<Mutex<HashMap<String, KeyName>>> = Lazy::new(|| Mutex::new(HashMap::new()));
    let label = format!("{kind}{id}");
    let mut cache = CACHE.lock();
    if let Some(n) = cache.get(&label) {
        return n;
    }
    let leaked: KeyName = Box::leak(label.clone().into_boxed_str());
    cache.insert(label, leaked);
    leaked
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    Fn,
    Ctrl,
    Option,
    Shift,
    Cmd,
}

pub fn modifier_family(name: &str) -> Option<Family> {
    Some(match name {
        "Fn" => Family::Fn,
        "LeftCtrl" | "RightCtrl" => Family::Ctrl,
        "LeftOption" | "RightOption" => Family::Option,
        "LeftShift" | "RightShift" => Family::Shift,
        "LeftCmd" | "RightCmd" => Family::Cmd,
        _ => return None,
    })
}

pub fn is_modifier(name: &str) -> bool {
    modifier_family(name).is_some()
}

// CGEventFlags device-independent masks.
pub const FLAG_SHIFT: u64 = 0x0002_0000;
pub const FLAG_CTRL: u64 = 0x0004_0000;
pub const FLAG_OPTION: u64 = 0x0008_0000;
pub const FLAG_CMD: u64 = 0x0010_0000;
pub const FLAG_FN: u64 = 0x0080_0000;
// Device-dependent (NX_DEVICE*) bits.
const DEV_LCTRL: u64 = 0x0001;
const DEV_LSHIFT: u64 = 0x0002;
const DEV_RSHIFT: u64 = 0x0004;
const DEV_LCMD: u64 = 0x0008;
const DEV_RCMD: u64 = 0x0010;
const DEV_LOPT: u64 = 0x0020;
const DEV_ROPT: u64 = 0x0040;
const DEV_RCTRL: u64 = 0x2000;
const DEV_ALL: u64 = DEV_LCTRL | DEV_LSHIFT | DEV_RSHIFT | DEV_LCMD | DEV_RCMD | DEV_LOPT | DEV_ROPT | DEV_RCTRL;

pub fn family_flag(f: Family) -> u64 {
    match f {
        Family::Fn => FLAG_FN,
        Family::Ctrl => FLAG_CTRL,
        Family::Option => FLAG_OPTION,
        Family::Shift => FLAG_SHIFT,
        Family::Cmd => FLAG_CMD,
    }
}

/// Interpret a `flagsChanged` event: which modifier key moved and whether it went down.
/// Returns None for keycodes that are not tracked modifiers (e.g. Caps Lock).
pub fn modifier_transition(keycode: u16, flags: u64) -> Option<(KeyName, bool)> {
    let (name, device, generic) = match keycode {
        63 => ("Fn", 0, FLAG_FN),
        56 => ("LeftShift", DEV_LSHIFT, FLAG_SHIFT),
        60 => ("RightShift", DEV_RSHIFT, FLAG_SHIFT),
        59 => ("LeftCtrl", DEV_LCTRL, FLAG_CTRL),
        62 => ("RightCtrl", DEV_RCTRL, FLAG_CTRL),
        58 => ("LeftOption", DEV_LOPT, FLAG_OPTION),
        61 => ("RightOption", DEV_ROPT, FLAG_OPTION),
        55 => ("LeftCmd", DEV_LCMD, FLAG_CMD),
        54 => ("RightCmd", DEV_RCMD, FLAG_CMD),
        _ => return None,
    };
    let down = if device == 0 || flags & DEV_ALL == 0 {
        // Fn has no per-side bit; synthetic events may lack device bits entirely.
        flags & generic != 0
    } else {
        flags & device != 0
    };
    Some((name, down))
}

// ───────────────────────── bindings ─────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeySpec {
    Key(KeyName),
    /// Generic modifier: matches either side.
    Any(Family),
}

impl KeySpec {
    pub fn accepts(&self, key: &str) -> bool {
        match self {
            KeySpec::Key(k) => *k == key,
            KeySpec::Any(f) => modifier_family(key) == Some(*f) && *f != Family::Fn,
        }
    }
}

pub type Binding = Vec<KeySpec>;

fn lookup_key_token(token: &str) -> Option<KeySpec> {
    let norm: String = token
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '_')
        .collect::<String>()
        .to_ascii_lowercase();
    let n = norm.as_str();
    let generic = match n {
        "fn" | "globe" => return Some(KeySpec::Key("Fn")),
        "ctrl" | "control" => Some(Family::Ctrl),
        "option" | "opt" | "alt" => Some(Family::Option),
        "shift" => Some(Family::Shift),
        "cmd" | "command" | "meta" | "super" => Some(Family::Cmd),
        _ => None,
    };
    if let Some(f) = generic {
        return Some(KeySpec::Any(f));
    }
    let alias = match n {
        "leftctrl" | "leftcontrol" | "lctrl" => "LeftCtrl",
        "rightctrl" | "rightcontrol" | "rctrl" => "RightCtrl",
        "leftoption" | "leftalt" | "loption" | "lalt" => "LeftOption",
        "rightoption" | "rightalt" | "roption" | "ralt" => "RightOption",
        "leftshift" | "lshift" => "LeftShift",
        "rightshift" | "rshift" => "RightShift",
        "leftcmd" | "leftcommand" | "lcmd" => "LeftCmd",
        "rightcmd" | "rightcommand" | "rcmd" => "RightCmd",
        "escape" | "esc" => "Esc",
        "return" | "enter" => "Enter",
        "backspace" => "Backspace",
        "delete" | "forwarddelete" | "del" => "Delete",
        "help" | "insert" | "ins" => "Help",
        "space" | "spacebar" => "Space",
        "tab" => "Tab",
        "home" => "Home",
        "end" => "End",
        "pageup" => "PageUp",
        "pagedown" => "PageDown",
        "up" | "arrowup" | "uparrow" => "Up",
        "down" | "arrowdown" | "downarrow" => "Down",
        "left" | "arrowleft" | "leftarrow" => "Left",
        "right" | "arrowright" | "rightarrow" => "Right",
        "plus" => "=",
        "minus" => "-",
        "mouse3" | "middlemouse" => "Mouse3",
        "mouse4" => "Mouse4",
        "mouse5" => "Mouse5",
        _ => "",
    };
    if !alias.is_empty() {
        return Some(KeySpec::Key(alias));
    }
    // Direct table lookup, case-insensitive (letters, digits, F-keys, punctuation, Num*).
    KEYS.iter()
        .find(|(_, name)| name.eq_ignore_ascii_case(n))
        .map(|(_, name)| KeySpec::Key(name))
}

/// Parse "Fn+LeftShift", "Ctrl+Cmd+V", "RightOption", ... Unknown tokens make the whole binding invalid.
pub fn parse_binding(s: &str) -> Option<Binding> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    // "Cmd++" style: a trailing '+' token is the plus key.
    let mut tokens: Vec<&str> = s.split('+').map(|t| t.trim()).collect();
    if tokens.len() >= 2 && tokens.last() == Some(&"") && tokens[tokens.len() - 2] == "" {
        tokens.truncate(tokens.len() - 2);
        tokens.push("plus");
    }
    let mut specs = Vec::with_capacity(tokens.len());
    for t in tokens {
        if t.is_empty() {
            return None;
        }
        let spec = lookup_key_token(t)?;
        if specs.contains(&spec) {
            return None;
        }
        specs.push(spec);
    }
    Some(specs)
}

fn spec_name(spec: &KeySpec) -> &'static str {
    match spec {
        KeySpec::Key(k) => k,
        KeySpec::Any(Family::Ctrl) => "Ctrl",
        KeySpec::Any(Family::Option) => "Option",
        KeySpec::Any(Family::Shift) => "Shift",
        KeySpec::Any(Family::Cmd) => "Cmd",
        KeySpec::Any(Family::Fn) => "Fn",
    }
}

fn modifier_rank(name: &str) -> u8 {
    match name {
        "Fn" => 0,
        "Ctrl" => 10,
        "LeftCtrl" => 11,
        "RightCtrl" => 12,
        "Option" => 20,
        "LeftOption" => 21,
        "RightOption" => 22,
        "Shift" => 30,
        "LeftShift" => 31,
        "RightShift" => 32,
        "Cmd" => 40,
        "LeftCmd" => 41,
        "RightCmd" => 42,
        _ => 100,
    }
}

/// Canonical textual form of a set of key names: Fn, Ctrl, Option, Shift, Cmd (left before right), then the
/// remaining keys in press order.
pub fn canonical_combo(keys: &[&str]) -> String {
    let mut sorted: Vec<&str> = keys.to_vec();
    sorted.sort_by_key(|k| modifier_rank(k)); // stable: non-modifiers keep press order
    sorted.join("+")
}

/// Normalize a user/imported binding string to the canonical textual form (None if invalid).
pub fn normalize_binding(s: &str) -> Option<String> {
    let b = parse_binding(s)?;
    let names: Vec<&str> = b.iter().map(spec_name).collect();
    Some(canonical_combo(&names))
}

/// Does `held` satisfy `specs` exactly? Returns indices into `held` used by the specs.
/// Rules: every spec needs a distinct held key; every held *modifier* must be used (no stray modifiers);
/// unrelated held regular keys / mouse buttons are ignored (fast typists often still have a key down).
pub fn match_binding(specs: &[KeySpec], held: &[KeyName]) -> Option<Vec<usize>> {
    let mut used = vec![false; held.len()];
    for s in specs {
        if let KeySpec::Key(k) = s {
            let i = held.iter().enumerate().position(|(i, h)| !used[i] && h == k)?;
            used[i] = true;
        }
    }
    for s in specs {
        if let KeySpec::Any(_) = s {
            let i = held.iter().enumerate().position(|(i, h)| !used[i] && s.accepts(h))?;
            used[i] = true;
        }
    }
    for (i, h) in held.iter().enumerate() {
        if !used[i] && is_modifier(h) {
            return None;
        }
    }
    Some(used.iter().enumerate().filter(|(_, u)| **u).map(|(i, _)| i).collect())
}

// ───────────────────────── gesture machine ─────────────────────────

#[derive(Clone, Copy, Debug)]
pub struct KeyEvent {
    pub key: KeyName,
    pub down: bool,
    /// Monotonic milliseconds.
    pub t_ms: u64,
}

#[derive(Debug, Default, PartialEq)]
pub struct Outcome {
    pub actions: Vec<HotkeyAction>,
    /// True if the original event must be removed from the system event stream.
    pub swallow: bool,
}

#[derive(Debug)]
struct Ptt {
    /// Held keys that make up the dictation binding that started this session.
    keys: Vec<KeyName>,
    down_at: u64,
}

enum Found {
    None,
    /// Matched, but one of the keys was already "consumed" by a previous action (e.g. the Fn that just finished a session).
    Blocked,
    Hit { used: Vec<KeyName> },
}

#[derive(Default)]
pub struct GestureMachine {
    dictation: Vec<Binding>,
    ask: Vec<Binding>,
    translation: Vec<Binding>,
    paste_last: Vec<Binding>,
    held: Vec<KeyName>,
    /// Keys whose down event we swallowed (so repeats and the matching up are swallowed too).
    swallowed: Vec<KeyName>,
    /// Keys that were held when a session-ending action fired; combos involving them are inert until released.
    consumed: Vec<KeyName>,
    ptt: Option<Ptt>,
    pending_tap: Option<u64>,
    assumed: Option<(SessionPhase, u64)>,
    mode: Mode,
    /// Toggle trigger: every start behaves as if the hands-free lock were already engaged.
    toggle: bool,
}

fn compile(list: &[String], what: &str) -> Vec<Binding> {
    list.iter()
        .filter_map(|s| match parse_binding(s) {
            Some(b) => Some(b),
            None => {
                log::warn!("ignoring invalid {what} shortcut {s:?}");
                None
            }
        })
        .collect()
}

impl GestureMachine {
    pub fn new(bindings: &ShortcutBindings) -> Self {
        let mut m = Self::default();
        m.set_bindings(bindings);
        m
    }

    pub fn set_bindings(&mut self, b: &ShortcutBindings) {
        self.dictation = compile(&b.dictation, "dictation");
        self.ask = compile(&b.ask_anything, "askAnything");
        self.translation = compile(&b.translation, "translation");
        self.paste_last = compile(&b.paste_last_transcript, "pasteLastTranscript");
        self.toggle = b.trigger == TriggerMode::Toggle;
        self.ptt = None;
        self.pending_tap = None;
    }

    /// Forget all input state (tap re-enabled, capture mode entered, ...). Bindings are kept.
    pub fn reset(&mut self) {
        self.held.clear();
        self.swallowed.clear();
        self.consumed.clear();
        self.ptt = None;
        self.pending_tap = None;
        self.assumed = None;
    }

    pub fn held_keys(&self) -> &[KeyName] {
        &self.held
    }

    pub fn held_modifiers(&self) -> Vec<KeyName> {
        self.held.iter().copied().filter(|k| is_modifier(k)).collect()
    }

    pub fn is_held(&self, key: &str) -> bool {
        self.held.contains(&key)
    }

    /// Absolute time (ms) at which `on_timer` has something to do.
    pub fn next_deadline(&self) -> Option<u64> {
        self.pending_tap
    }

    pub fn on_timer(&mut self, now_ms: u64, _reported: SessionPhase) -> Vec<HotkeyAction> {
        match self.pending_tap {
            Some(d) if now_ms >= d => {
                self.pending_tap = None;
                self.assume(SessionPhase::Idle, now_ms);
                vec![HotkeyAction::TapTooShort]
            }
            _ => vec![],
        }
    }

    pub fn handle(&mut self, ev: KeyEvent, reported: SessionPhase) -> Outcome {
        let phase = self.effective_phase(reported, ev.t_ms);
        if ev.down {
            self.on_down(ev, phase)
        } else {
            self.on_up(ev, phase)
        }
    }

    // ── helpers ──

    fn effective_phase(&mut self, reported: SessionPhase, now: u64) -> SessionPhase {
        match self.assumed {
            Some((p, t)) if now.saturating_sub(t) < ASSUMED_PHASE_GRACE_MS => {
                if p == reported {
                    self.assumed = None;
                }
                p
            }
            _ => {
                self.assumed = None;
                reported
            }
        }
    }

    fn assume(&mut self, p: SessionPhase, now: u64) {
        self.assumed = Some((p, now));
    }

    fn find(&self, list: &[Binding], key: KeyName) -> Found {
        let Some(key_idx) = self.held.iter().rposition(|k| *k == key) else {
            return Found::None;
        };
        for b in list.iter() {
            if let Some(used) = match_binding(b, &self.held) {
                if used.contains(&key_idx) {
                    let names: Vec<KeyName> = used.iter().map(|i| self.held[*i]).collect();
                    if names.iter().any(|k| self.consumed.contains(k)) {
                        return Found::Blocked;
                    }
                    return Found::Hit { used: names };
                }
            }
        }
        Found::None
    }

    fn is_dictation_key(&self, key: &str) -> bool {
        self.dictation.iter().any(|b| b.iter().any(|s| s.accepts(key)))
    }

    fn swallow_if_regular(&mut self, key: KeyName, out: &mut Outcome) {
        if !is_modifier(key) {
            out.swallow = true;
            if !self.swallowed.contains(&key) {
                self.swallowed.push(key);
            }
        }
    }

    fn mode_combo(&self, key: KeyName) -> Option<(Mode, Vec<KeyName>)> {
        for (list, mode) in [(&self.translation, Mode::Translation), (&self.ask, Mode::Ask)] {
            if let Found::Hit { used } = self.find(list, key) {
                return Some((mode, used));
            }
        }
        None
    }

    fn mode_combo_blocked(&self, key: KeyName) -> bool {
        [&self.translation, &self.ask, &self.dictation]
            .iter()
            .any(|l| matches!(self.find(l, key), Found::Blocked))
    }

    // ── key down ──

    fn on_down(&mut self, ev: KeyEvent, phase: SessionPhase) -> Outcome {
        let key = ev.key;
        let t = ev.t_ms;
        let mut out = Outcome::default();
        if self.held.contains(&key) {
            // Autorepeat / duplicate: keep swallowing what we swallowed.
            out.swallow = self.swallowed.contains(&key);
            return out;
        }
        self.held.push(key);

        // Esc cancels any active session and is swallowed; with nothing active it is the app's.
        if key == "Esc" && phase != SessionPhase::Idle {
            self.ptt = None;
            self.pending_tap = None;
            self.consumed = self.held.clone();
            self.assume(SessionPhase::Idle, t);
            out.actions.push(HotkeyAction::Cancel);
            self.swallow_if_regular(key, &mut out);
            return out;
        }

        // Paste-last works in every phase; the controller decides whether it is sensible.
        match self.find(&self.paste_last, key) {
            Found::Hit { .. } => {
                out.actions.push(HotkeyAction::PasteLast);
                self.swallow_if_regular(key, &mut out);
                return out;
            }
            Found::Blocked => {
                self.swallow_if_regular(key, &mut out);
                return out;
            }
            Found::None => {}
        }

        match phase {
            SessionPhase::Idle | SessionPhase::Processing => self.down_startable(key, t, &mut out),
            SessionPhase::RecordingPtt => self.down_ptt(key, t, &mut out),
            SessionPhase::RecordingLocked => self.down_locked(key, t, &mut out),
        }
        out
    }

    fn down_startable(&mut self, key: KeyName, t: u64, out: &mut Outcome) {
        match self.find(&self.dictation, key) {
            Found::Hit { used } => {
                // In toggle mode `ptt` stays set while the key is held, only as the other-key guard (see `down_locked`).
                self.ptt = Some(Ptt { keys: used, down_at: t });
                self.pending_tap = None;
                self.mode = Mode::Dictation;
                let locked = self.toggle;
                self.assume(if locked { SessionPhase::RecordingLocked } else { SessionPhase::RecordingPtt }, t);
                out.actions.push(HotkeyAction::Start { mode: Mode::Dictation, locked });
                self.swallow_if_regular(key, out);
                return;
            }
            Found::Blocked => {
                self.swallow_if_regular(key, out);
                return;
            }
            Found::None => {}
        }
        if let Some((mode, _)) = self.mode_combo(key) {
            self.ptt = None;
            self.mode = mode;
            self.assume(SessionPhase::RecordingLocked, t);
            out.actions.push(HotkeyAction::Start { mode, locked: true });
            self.swallow_if_regular(key, out);
        } else if self.mode_combo_blocked(key) {
            self.swallow_if_regular(key, out);
        }
    }

    fn down_ptt(&mut self, key: KeyName, t: u64, out: &mut Outcome) {
        // Second press inside the double-press window => hands-free.
        if self.pending_tap.is_some() {
            match self.find(&self.dictation, key) {
                Found::Hit { .. } => {
                    self.pending_tap = None;
                    self.ptt = None;
                    self.assume(SessionPhase::RecordingLocked, t);
                    out.actions.push(HotkeyAction::Lock);
                    self.swallow_if_regular(key, out);
                }
                Found::Blocked => self.swallow_if_regular(key, out),
                Found::None => {}
            }
            return;
        }
        let Some(ptt) = self.ptt.as_ref() else {
            return;
        };
        if ptt.keys.contains(&key) || self.is_dictation_key(key) {
            return;
        }
        let down_at = ptt.down_at;
        if let Some((mode, _)) = self.mode_combo(key) {
            self.ptt = None;
            self.mode = mode;
            self.assume(SessionPhase::RecordingLocked, t);
            out.actions.push(HotkeyAction::SwitchMode { mode });
            self.swallow_if_regular(key, out);
        } else if t.saturating_sub(down_at) < OTHER_KEY_WINDOW_MS {
            // The dictation key is being used as a modifier (Fn+Arrow, Fn+F5, RightOption+E ...).
            self.ptt = None;
            self.assume(SessionPhase::Idle, t);
            out.actions.push(HotkeyAction::CancelSilently);
        }
        // else: user is talking; unrelated key passes through, recording continues.
    }

    fn down_locked(&mut self, key: KeyName, t: u64, out: &mut Outcome) {
        match self.find(&self.dictation, key) {
            Found::Hit { used } => {
                self.finish_locked(used, t, out, key);
                return;
            }
            Found::Blocked => {
                self.swallow_if_regular(key, out);
                return;
            }
            Found::None => {}
        }
        if let Some((mode, used)) = self.mode_combo(key) {
            if mode == self.mode {
                self.finish_locked(used, t, out, key);
            } else {
                self.mode = mode;
                self.ptt = None;
                out.actions.push(HotkeyAction::SwitchMode { mode });
                self.swallow_if_regular(key, out);
            }
        } else if self.mode_combo_blocked(key) {
            self.swallow_if_regular(key, out);
        } else if let Some(ptt) = self.ptt.as_ref() {
            // Toggle mode, dictation key still held from the start press: same "used as a modifier" rule as push-to-talk,
            // so Fn+Arrow / Fn+F5 never leave a stuck recording. Once the key is released this no longer applies.
            if !ptt.keys.contains(&key) && !self.is_dictation_key(key) && t.saturating_sub(ptt.down_at) < OTHER_KEY_WINDOW_MS {
                self.ptt = None;
                self.assume(SessionPhase::Idle, t);
                out.actions.push(HotkeyAction::CancelSilently);
            }
        }
    }

    fn finish_locked(&mut self, used: Vec<KeyName>, t: u64, out: &mut Outcome, key: KeyName) {
        self.consumed.extend(used);
        self.ptt = None;
        self.assume(SessionPhase::Processing, t);
        out.actions.push(HotkeyAction::Finish);
        self.swallow_if_regular(key, out);
    }

    // ── key up ──

    fn on_up(&mut self, ev: KeyEvent, phase: SessionPhase) -> Outcome {
        let key = ev.key;
        let mut out = Outcome::default();
        if let Some(i) = self.swallowed.iter().position(|k| *k == key) {
            self.swallowed.remove(i);
            out.swallow = true;
        }
        self.consumed.retain(|k| *k != key);
        let was_held = self.held.contains(&key);
        self.held.retain(|k| *k != key);
        if !was_held {
            return out;
        }
        let is_part = self.ptt.as_ref().is_some_and(|p| p.keys.contains(&key));
        if !is_part {
            return out;
        }
        let ptt = self.ptt.take().expect("checked above");
        // Anything that is not "recording by push-to-talk" (or that the controller has not caught up with yet,
        // i.e. Idle) means this release has nothing to finish.
        if matches!(phase, SessionPhase::RecordingLocked | SessionPhase::Processing) {
            return out;
        }
        let held_ms = ev.t_ms.saturating_sub(ptt.down_at);
        if held_ms >= HOLD_THRESHOLD_MS {
            self.assume(SessionPhase::Processing, ev.t_ms);
            out.actions.push(HotkeyAction::Finish);
        } else {
            // Recording keeps running while we wait for a possible second press.
            self.pending_tap = Some(ev.t_ms + DOUBLE_PRESS_WINDOW_MS);
        }
        out
    }
}

// ───────────────────────── shortcut capture ─────────────────────────

/// Tracks the keys held while the settings UI records a new shortcut.
#[derive(Default)]
pub struct CaptureMachine {
    held: Vec<KeyName>,
    /// Keys simultaneously held at the moment the last key went down — the combo the user "formed".
    peak: Vec<KeyName>,
}

impl CaptureMachine {
    pub fn reset(&mut self) {
        self.held.clear();
        self.peak.clear();
    }

    pub fn held(&self) -> &[KeyName] {
        &self.held
    }

    /// Returns `(combo, done)` to report, if anything changed.
    pub fn on_key(&mut self, key: KeyName, down: bool) -> Option<(String, bool)> {
        if down {
            if self.held.contains(&key) {
                return None;
            }
            if self.held.is_empty() {
                self.peak.clear();
            }
            self.held.push(key);
            self.peak = self.held.clone();
            Some((canonical_combo(&self.held), false))
        } else {
            let pos = self.held.iter().position(|k| *k == key)?;
            self.held.remove(pos);
            if self.held.is_empty() {
                let combo = canonical_combo(&self.peak);
                self.peak.clear();
                Some((combo, true))
            } else {
                Some((canonical_combo(&self.held), false))
            }
        }
    }
}

// ───────────────────────── tests ─────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use SessionPhase::*;

    fn bindings(d: &[&str], a: &[&str], tr: &[&str], p: &[&str]) -> ShortcutBindings {
        let v = |x: &[&str]| x.iter().map(|s| s.to_string()).collect();
        ShortcutBindings {
            trigger: TriggerMode::Hold,
            dictation: v(d),
            ask_anything: v(a),
            translation: v(tr),
            paste_last_transcript: v(p),
        }
    }

    fn toggle_sim() -> Sim {
        Sim::new(&ShortcutBindings { trigger: TriggerMode::Toggle, ..ShortcutBindings::default() })
    }

    /// Simulates the controller: its phase follows the actions immediately.
    struct Sim {
        m: GestureMachine,
        phase: SessionPhase,
        t: u64,
    }

    impl Sim {
        fn new(b: &ShortcutBindings) -> Self {
            Self { m: GestureMachine::new(b), phase: Idle, t: 1000 }
        }
        fn default() -> Self {
            Self::new(&ShortcutBindings::default())
        }
        fn advance(&mut self, ms: u64) -> Vec<HotkeyAction> {
            self.t += ms;
            let acts = self.m.on_timer(self.t, self.phase);
            self.apply(&acts);
            acts
        }
        fn apply(&mut self, acts: &[HotkeyAction]) {
            for a in acts {
                self.phase = match a {
                    HotkeyAction::Start { locked: false, .. } => RecordingPtt,
                    HotkeyAction::Start { locked: true, .. } => RecordingLocked,
                    HotkeyAction::SwitchMode { .. } | HotkeyAction::Lock => RecordingLocked,
                    HotkeyAction::Finish => Processing,
                    HotkeyAction::CancelSilently | HotkeyAction::TapTooShort | HotkeyAction::Cancel => Idle,
                    HotkeyAction::PasteLast => self.phase,
                };
            }
        }
        fn ev(&mut self, key: KeyName, down: bool, dt: u64) -> Outcome {
            self.t += dt;
            let o = self.m.handle(KeyEvent { key, down, t_ms: self.t }, self.phase);
            self.apply(&o.actions);
            o
        }
        fn down(&mut self, key: KeyName, dt: u64) -> Outcome {
            self.ev(key, true, dt)
        }
        fn up(&mut self, key: KeyName, dt: u64) -> Outcome {
            self.ev(key, false, dt)
        }
    }

    fn start(mode: Mode, locked: bool) -> Vec<HotkeyAction> {
        vec![HotkeyAction::Start { mode, locked }]
    }

    #[test]
    fn toggle_press_release_keeps_recording_and_second_press_finishes() {
        let mut s = toggle_sim();
        assert_eq!(s.down("Fn", 0).actions, start(Mode::Dictation, true));
        assert_eq!(s.phase, RecordingLocked);
        assert!(s.up("Fn", 80).actions.is_empty());
        assert_eq!(s.m.next_deadline(), None);
        // A long hold-then-release also does nothing.
        assert!(s.advance(5000).is_empty());
        assert_eq!(s.phase, RecordingLocked);
        assert_eq!(s.down("Fn", 100).actions, vec![HotkeyAction::Finish]);
        assert!(s.up("Fn", 80).actions.is_empty());
        assert_eq!(s.phase, Processing);
    }

    #[test]
    fn toggle_long_hold_release_does_not_finish() {
        let mut s = toggle_sim();
        s.down("Fn", 0);
        assert!(s.up("Fn", 2000).actions.is_empty());
        assert_eq!(s.phase, RecordingLocked);
    }

    #[test]
    fn toggle_translation_and_ask_combos() {
        for (key, mode) in [("LeftShift", Mode::Translation), ("Space", Mode::Ask)] {
            let mut s = toggle_sim();
            s.down("Fn", 0);
            assert_eq!(s.down(key, 100).actions, vec![HotkeyAction::SwitchMode { mode }]);
            s.up(key, 50);
            assert!(s.up("Fn", 50).actions.is_empty());
            assert_eq!(s.phase, RecordingLocked);
            assert_eq!(s.down("Fn", 3000).actions, vec![HotkeyAction::Finish]);
            s.up("Fn", 50);
        }
    }

    #[test]
    fn toggle_mode_combo_pressed_again_finishes() {
        let mut s = toggle_sim();
        s.down("LeftShift", 0);
        assert_eq!(s.down("Fn", 20).actions, start(Mode::Translation, true));
        s.up("Fn", 50);
        s.up("LeftShift", 20);
        s.down("LeftShift", 1000);
        assert_eq!(s.down("Fn", 20).actions, vec![HotkeyAction::Finish]);
    }

    #[test]
    fn toggle_esc_cancels() {
        let mut s = toggle_sim();
        s.down("Fn", 0);
        s.up("Fn", 60);
        let o = s.down("Esc", 500);
        assert_eq!(o.actions, vec![HotkeyAction::Cancel]);
        assert_eq!(s.phase, Idle);
    }

    #[test]
    fn toggle_fn_as_modifier_cancels_silently() {
        let mut s = toggle_sim();
        s.down("Fn", 0);
        assert_eq!(s.down("Right", 200).actions, vec![HotkeyAction::CancelSilently]);
        assert_eq!(s.phase, Idle);
        s.up("Right", 50);
        assert!(s.up("Fn", 50).actions.is_empty());
        assert_eq!(s.phase, Idle);
    }

    #[test]
    fn toggle_other_key_after_release_or_window_is_ignored() {
        // Released: typing afterwards must not cancel the hands-free recording.
        let mut s = toggle_sim();
        s.down("Fn", 0);
        s.up("Fn", 60);
        assert!(s.down("A", 200).actions.is_empty());
        assert_eq!(s.phase, RecordingLocked);
        // Still held but past the window: user is talking.
        let mut s = toggle_sim();
        s.down("Fn", 0);
        assert!(s.down("A", OTHER_KEY_WINDOW_MS + 10).actions.is_empty());
        assert_eq!(s.phase, RecordingLocked);
    }

    #[test]
    fn toggle_does_not_wait_for_double_press() {
        let mut s = toggle_sim();
        s.down("Fn", 0);
        s.up("Fn", 50);
        assert_eq!(s.m.next_deadline(), None);
    }

    #[test]
    fn hold_mode_unchanged_by_default() {
        let mut s = Sim::default();
        assert_eq!(s.down("Fn", 0).actions, start(Mode::Dictation, false));
        assert_eq!(s.phase, RecordingPtt);
    }

    #[test]
    fn ptt_hold_and_release() {
        let mut s = Sim::default();
        assert_eq!(s.down("Fn", 0).actions, start(Mode::Dictation, false));
        assert_eq!(s.phase, RecordingPtt);
        let o = s.up("Fn", 600);
        assert_eq!(o.actions, vec![HotkeyAction::Finish]);
        assert!(!o.swallow);
        assert_eq!(s.m.next_deadline(), None);
    }

    #[test]
    fn hold_exactly_threshold_finishes() {
        let mut s = Sim::default();
        s.down("Fn", 0);
        assert_eq!(s.up("Fn", 350).actions, vec![HotkeyAction::Finish]);
    }

    #[test]
    fn short_tap_becomes_tap_too_short_after_window() {
        let mut s = Sim::default();
        s.down("Fn", 0);
        assert!(s.up("Fn", 100).actions.is_empty());
        let d = s.m.next_deadline().unwrap();
        assert_eq!(d, s.t + DOUBLE_PRESS_WINDOW_MS);
        assert!(s.advance(299).is_empty());
        assert_eq!(s.advance(1), vec![HotkeyAction::TapTooShort]);
        assert_eq!(s.phase, Idle);
        assert_eq!(s.m.next_deadline(), None);
        assert!(s.advance(1000).is_empty());
    }

    #[test]
    fn double_press_locks_and_second_release_is_ignored() {
        let mut s = Sim::default();
        s.down("Fn", 0);
        s.up("Fn", 80);
        let o = s.down("Fn", 120);
        assert_eq!(o.actions, vec![HotkeyAction::Lock]);
        assert_eq!(s.phase, RecordingLocked);
        assert!(s.up("Fn", 70).actions.is_empty());
        assert!(s.advance(1000).is_empty(), "pending timer must be gone");
        assert_eq!(s.phase, RecordingLocked);
    }

    #[test]
    fn locked_dictation_key_finishes() {
        let mut s = Sim::default();
        s.down("Fn", 0);
        s.up("Fn", 80);
        s.down("Fn", 100);
        s.up("Fn", 50);
        let o = s.down("Fn", 2000);
        assert_eq!(o.actions, vec![HotkeyAction::Finish]);
        assert!(s.up("Fn", 60).actions.is_empty());
        assert_eq!(s.phase, Processing);
    }

    #[test]
    fn locked_finish_with_swallowed_key_swallows_down_and_up() {
        let mut s = Sim::new(&bindings(&["Help"], &[], &[], &[]));
        let o = s.down("Help", 0);
        assert!(o.swallow, "non-modifier dictation key is swallowed");
        assert!(s.up("Help", 80).swallow);
        assert!(s.down("Help", 100).swallow);
        assert_eq!(s.phase, RecordingLocked);
        s.up("Help", 40);
        let o = s.down("Help", 1000);
        assert_eq!(o.actions, vec![HotkeyAction::Finish]);
        assert!(o.swallow);
        assert!(s.up("Help", 50).swallow);
    }

    #[test]
    fn fn_space_switches_to_ask_and_swallows_space() {
        let mut s = Sim::default();
        s.down("Fn", 0);
        let o = s.down("Space", 200);
        assert_eq!(o.actions, vec![HotkeyAction::SwitchMode { mode: Mode::Ask }]);
        assert!(o.swallow);
        assert!(s.down("Space", 30).swallow, "autorepeat swallowed");
        assert!(s.up("Space", 50).swallow);
        assert!(s.up("Fn", 50).actions.is_empty(), "locked now: releasing Fn does not finish");
        assert_eq!(s.phase, RecordingLocked);
        // Fn again finishes the locked ask session.
        assert_eq!(s.down("Fn", 800).actions, vec![HotkeyAction::Finish]);
    }

    #[test]
    fn fn_leftshift_switches_to_translation() {
        let mut s = Sim::default();
        s.down("Fn", 0);
        let o = s.down("LeftShift", 300);
        assert_eq!(o.actions, vec![HotkeyAction::SwitchMode { mode: Mode::Translation }]);
        assert!(!o.swallow, "modifier events are never swallowed");
        s.up("LeftShift", 100);
        s.up("Fn", 20);
        assert_eq!(s.phase, RecordingLocked);
    }

    #[test]
    fn fn_finish_then_space_does_not_start_ask() {
        let mut s = Sim::default();
        s.down("Fn", 0);
        s.down("Space", 100); // -> locked ask
        s.up("Space", 20);
        s.up("Fn", 20);
        // user presses Fn+Space again to finish
        assert_eq!(s.down("Fn", 1000).actions, vec![HotkeyAction::Finish]);
        let o = s.down("Space", 10);
        assert!(o.actions.is_empty());
        assert!(o.swallow, "the Space of the finishing combo must not type a space");
        assert!(s.up("Space", 10).swallow);
    }

    #[test]
    fn fn_arrow_within_window_cancels_silently_and_passes_through() {
        let mut s = Sim::default();
        s.down("Fn", 0);
        let o = s.down("Left", 200);
        assert_eq!(o.actions, vec![HotkeyAction::CancelSilently]);
        assert!(!o.swallow);
        assert!(!s.up("Left", 50).swallow);
        // releasing Fn afterwards does nothing
        assert!(s.up("Fn", 50).actions.is_empty());
        assert!(s.advance(2000).is_empty());
    }

    #[test]
    fn other_key_after_window_passes_through_and_keeps_recording() {
        let mut s = Sim::default();
        s.down("Fn", 0);
        let o = s.down("Left", 2000);
        assert!(o.actions.is_empty());
        assert!(!o.swallow);
        s.up("Left", 10);
        assert_eq!(s.phase, RecordingPtt);
        assert_eq!(s.up("Fn", 100).actions, vec![HotkeyAction::Finish]);
    }

    #[test]
    fn esc_only_swallowed_when_not_idle() {
        let mut s = Sim::default();
        let o = s.down("Esc", 0);
        assert!(o.actions.is_empty() && !o.swallow);
        assert!(!s.up("Esc", 10).swallow);

        s.down("Fn", 100);
        let o = s.down("Esc", 100);
        assert_eq!(o.actions, vec![HotkeyAction::Cancel]);
        assert!(o.swallow);
        assert!(s.up("Esc", 10).swallow);
        assert!(s.up("Fn", 10).actions.is_empty(), "release after cancel is inert");

        // Locked
        let mut s = Sim::default();
        s.down("Fn", 0);
        s.up("Fn", 50);
        s.down("Fn", 50);
        s.up("Fn", 50);
        assert_eq!(s.down("Esc", 500).actions, vec![HotkeyAction::Cancel]);

        // Processing
        let mut s = Sim::default();
        s.phase = Processing;
        let o = s.down("Esc", 0);
        assert_eq!(o.actions, vec![HotkeyAction::Cancel]);
        assert!(o.swallow);
    }

    #[test]
    fn esc_during_pending_tap_cancels() {
        let mut s = Sim::default();
        s.down("Fn", 0);
        s.up("Fn", 100);
        assert_eq!(s.down("Esc", 50).actions, vec![HotkeyAction::Cancel]);
        assert!(s.advance(1000).is_empty());
    }

    #[test]
    fn paste_last_combo() {
        let mut s = Sim::default();
        s.down("LeftCtrl", 0);
        s.down("RightCmd", 10);
        let o = s.down("V", 10);
        assert_eq!(o.actions, vec![HotkeyAction::PasteLast]);
        assert!(o.swallow);
        assert!(s.up("V", 10).swallow);
        s.up("RightCmd", 5);
        s.up("LeftCtrl", 5);
        assert_eq!(s.phase, Idle);

        // With a stray Shift it is not the paste-last shortcut.
        let mut s = Sim::default();
        s.down("LeftShift", 0);
        s.down("LeftCtrl", 10);
        s.down("LeftCmd", 10);
        let o = s.down("V", 10);
        assert!(o.actions.is_empty() && !o.swallow);
    }

    #[test]
    fn multiple_dictation_bindings() {
        let mut s = Sim::new(&bindings(&["Fn", "Help", "RightOption"], &["Fn+Space"], &["Fn+LeftShift"], &[]));
        // Fn
        assert_eq!(s.down("Fn", 0).actions, start(Mode::Dictation, false));
        assert_eq!(s.up("Fn", 700).actions, vec![HotkeyAction::Finish]);
        s.advance(5);
        s.phase = Idle;
        // Help (swallowed)
        let o = s.down("Help", 500);
        assert_eq!(o.actions, start(Mode::Dictation, false));
        assert!(o.swallow);
        let o = s.up("Help", 600);
        assert_eq!(o.actions, vec![HotkeyAction::Finish]);
        assert!(o.swallow);
        s.phase = Idle;
        // RightOption
        assert_eq!(s.down("RightOption", 500).actions, start(Mode::Dictation, false));
        assert_eq!(s.up("RightOption", 900).actions, vec![HotkeyAction::Finish]);
        s.phase = Idle;
        // Help does not trigger Fn+Space ask; Space after Help is "another key" => cancel
        s.down("Help", 500);
        assert_eq!(s.down("Space", 100).actions, vec![HotkeyAction::CancelSilently]);
    }

    #[test]
    fn second_dictation_key_while_recording_is_ignored() {
        let mut s = Sim::new(&bindings(&["Fn", "RightOption"], &[], &[], &[]));
        s.down("Fn", 0);
        let o = s.down("RightOption", 200);
        assert!(o.actions.is_empty());
        assert_eq!(s.phase, RecordingPtt);
        s.up("RightOption", 200);
        assert_eq!(s.up("Fn", 100).actions, vec![HotkeyAction::Finish]);
    }

    #[test]
    fn right_option_alone_vs_with_other_key() {
        let b = bindings(&["RightOption"], &[], &[], &[]);
        let mut s = Sim::new(&b);
        assert_eq!(s.down("RightOption", 0).actions, start(Mode::Dictation, false));
        assert_eq!(s.up("RightOption", 800).actions, vec![HotkeyAction::Finish]);

        // RightOption+E (accent) shortly after => silent cancel, key passes through
        let mut s = Sim::new(&b);
        s.down("RightOption", 0);
        let o = s.down("E", 150);
        assert_eq!(o.actions, vec![HotkeyAction::CancelSilently]);
        assert!(!o.swallow);
        s.up("E", 30);
        assert!(s.up("RightOption", 30).actions.is_empty());

        // LeftOption is not the dictation key
        let mut s = Sim::new(&b);
        assert!(s.down("LeftOption", 0).actions.is_empty());
    }

    #[test]
    fn processing_plus_dictation_key_starts() {
        let mut s = Sim::default();
        s.phase = Processing;
        assert_eq!(s.down("Fn", 0).actions, start(Mode::Dictation, false));
        assert_eq!(s.phase, RecordingPtt);
    }

    #[test]
    fn modifier_held_blocks_dictation_start() {
        let mut s = Sim::default();
        s.down("LeftCmd", 0);
        let o = s.down("Fn", 10);
        assert!(o.actions.is_empty());
    }

    #[test]
    fn regular_key_held_does_not_block_dictation_start() {
        let mut s = Sim::default();
        s.down("A", 0);
        assert_eq!(s.down("Fn", 10).actions, start(Mode::Dictation, false));
    }

    #[test]
    fn non_dictation_combo_starts_locked_and_swallows() {
        let mut s = Sim::new(&bindings(&["Fn"], &["RightCmd+A"], &[], &[]));
        assert!(s.down("RightCmd", 0).actions.is_empty());
        let o = s.down("A", 20);
        assert_eq!(o.actions, start(Mode::Ask, true));
        assert!(o.swallow);
        assert!(s.up("A", 10).swallow);
        s.up("RightCmd", 10);
        // same combo again finishes
        s.down("RightCmd", 1000);
        let o = s.down("A", 10);
        assert_eq!(o.actions, vec![HotkeyAction::Finish]);
        assert!(o.swallow);
    }

    #[test]
    fn non_dictation_modifier_combo_translation_without_dictation_prefix() {
        // Fn is not a dictation key here: Fn+LeftShift starts translation hands-free.
        let mut s = Sim::new(&bindings(&["RightOption"], &[], &["Fn+LeftShift"], &[]));
        assert!(s.down("Fn", 0).actions.is_empty());
        assert_eq!(s.down("LeftShift", 10).actions, start(Mode::Translation, true));
    }

    #[test]
    fn lagging_controller_phase_does_not_confuse_machine() {
        let mut m = GestureMachine::new(&ShortcutBindings::default());
        // Controller keeps reporting Idle (it has not processed Start yet).
        let o = m.handle(KeyEvent { key: "Fn", down: true, t_ms: 0 }, Idle);
        assert_eq!(o.actions, start(Mode::Dictation, false));
        let o = m.handle(KeyEvent { key: "Space", down: true, t_ms: 100 }, Idle);
        assert_eq!(o.actions, vec![HotkeyAction::SwitchMode { mode: Mode::Ask }]);
        let o = m.handle(KeyEvent { key: "Fn", down: false, t_ms: 200 }, Idle);
        assert!(o.actions.is_empty());

        let mut m = GestureMachine::new(&ShortcutBindings::default());
        m.handle(KeyEvent { key: "Fn", down: true, t_ms: 0 }, Idle);
        let o = m.handle(KeyEvent { key: "Fn", down: false, t_ms: 600 }, RecordingPtt);
        assert_eq!(o.actions, vec![HotkeyAction::Finish]);
    }

    #[test]
    fn rejected_start_still_gets_its_closing_action() {
        // Controller never left Idle (e.g. mic failure) and the grace period expired: release still finishes.
        let mut m = GestureMachine::new(&ShortcutBindings::default());
        m.handle(KeyEvent { key: "Fn", down: true, t_ms: 0 }, Idle);
        let o = m.handle(KeyEvent { key: "Fn", down: false, t_ms: 2000 }, Idle);
        assert_eq!(o.actions, vec![HotkeyAction::Finish]);
    }

    #[test]
    fn controller_finished_on_its_own_means_release_is_inert() {
        // 20 minute auto-finish: controller is Processing when the user finally releases.
        let mut m = GestureMachine::new(&ShortcutBindings::default());
        m.handle(KeyEvent { key: "Fn", down: true, t_ms: 0 }, Idle);
        let o = m.handle(KeyEvent { key: "Fn", down: false, t_ms: 5_000_000 }, Processing);
        assert!(o.actions.is_empty());
    }

    #[test]
    fn set_bindings_takes_effect() {
        let mut s = Sim::default();
        s.m.set_bindings(&bindings(&["RightOption"], &[], &[], &[]));
        assert!(s.down("Fn", 0).actions.is_empty());
        s.up("Fn", 10);
        assert_eq!(s.down("RightOption", 10).actions, start(Mode::Dictation, false));
    }

    #[test]
    fn invalid_bindings_are_ignored() {
        let mut s = Sim::new(&bindings(&["Bogus", "Fn"], &["Fn+"], &[], &[]));
        assert_eq!(s.down("Fn", 0).actions, start(Mode::Dictation, false));
    }

    // ── parsing / naming ──

    #[test]
    fn parse_bindings() {
        assert_eq!(parse_binding("Fn"), Some(vec![KeySpec::Key("Fn")]));
        assert_eq!(parse_binding("Fn+LeftShift"), Some(vec![KeySpec::Key("Fn"), KeySpec::Key("LeftShift")]));
        assert_eq!(
            parse_binding("Ctrl+Cmd+V"),
            Some(vec![KeySpec::Any(Family::Ctrl), KeySpec::Any(Family::Cmd), KeySpec::Key("V")])
        );
        assert_eq!(parse_binding("Left Shift"), Some(vec![KeySpec::Key("LeftShift")]));
        assert_eq!(parse_binding("right option"), Some(vec![KeySpec::Key("RightOption")]));
        assert_eq!(parse_binding("Help"), Some(vec![KeySpec::Key("Help")]));
        assert_eq!(parse_binding("fn+space"), Some(vec![KeySpec::Key("Fn"), KeySpec::Key("Space")]));
        assert_eq!(parse_binding("Cmd+ArrowUp"), Some(vec![KeySpec::Any(Family::Cmd), KeySpec::Key("Up")]));
        assert_eq!(parse_binding("Mouse4"), Some(vec![KeySpec::Key("Mouse4")]));
        assert_eq!(parse_binding("Cmd++"), Some(vec![KeySpec::Any(Family::Cmd), KeySpec::Key("=")]));
        assert_eq!(parse_binding("F13"), Some(vec![KeySpec::Key("F13")]));
        assert_eq!(parse_binding(""), None);
        assert_eq!(parse_binding("Fn+"), None);
        assert_eq!(parse_binding("Nope"), None);
        assert_eq!(parse_binding("Fn+Fn"), None);
    }

    #[test]
    fn generic_modifier_matches_either_side() {
        let b = parse_binding("Ctrl+Cmd+V").unwrap();
        assert!(match_binding(&b, &["LeftCtrl", "LeftCmd", "V"]).is_some());
        assert!(match_binding(&b, &["RightCtrl", "RightCmd", "V"]).is_some());
        assert!(match_binding(&b, &["LeftCtrl", "V"]).is_none());
        assert!(match_binding(&b, &["LeftCtrl", "LeftCmd", "LeftShift", "V"]).is_none());
        // Generic Cmd cannot consume both sides at once
        let b = parse_binding("Cmd+A").unwrap();
        assert!(match_binding(&b, &["LeftCmd", "RightCmd", "A"]).is_none());
        // Fn is never matched by a generic spec
        assert!(!KeySpec::Any(Family::Shift).accepts("Fn"));
    }

    #[test]
    fn keycodes_and_names() {
        assert_eq!(key_for_code(63), Some("Fn"));
        assert_eq!(key_for_code(114), Some("Help"));
        assert_eq!(key_for_code(53), Some("Esc"));
        assert_eq!(key_for_code(49), Some("Space"));
        assert_eq!(key_for_code(56), Some("LeftShift"));
        assert_eq!(key_for_code(60), Some("RightShift"));
        assert_eq!(key_for_code(59), Some("LeftCtrl"));
        assert_eq!(key_for_code(62), Some("RightCtrl"));
        assert_eq!(key_for_code(58), Some("LeftOption"));
        assert_eq!(key_for_code(61), Some("RightOption"));
        assert_eq!(key_for_code(55), Some("LeftCmd"));
        assert_eq!(key_for_code(54), Some("RightCmd"));
        assert_eq!(key_for_code(9), Some("V"));
        assert_eq!(key_for_code(96), Some("F5"));
        assert_eq!(key_for_code(126), Some("Up"));
        assert_eq!(code_for_key("V"), Some(9));
        assert_eq!(mouse_button_name(2), Some("Mouse3"));
        assert_eq!(mouse_button_name(3), Some("Mouse4"));
        assert_eq!(mouse_button_name(4), Some("Mouse5"));
        assert_eq!(mouse_button_name(0), None);
        // every key code and name is unique
        for (i, (c, n)) in KEYS.iter().enumerate() {
            for (c2, n2) in &KEYS[i + 1..] {
                assert!(c != c2 && n != n2, "duplicate {c} {n}");
            }
        }
        assert_eq!(unknown_key_name("Key", 200), unknown_key_name("Key", 200));
    }

    #[test]
    fn flags_changed_interpretation() {
        assert_eq!(modifier_transition(63, FLAG_FN), Some(("Fn", true)));
        assert_eq!(modifier_transition(63, 0), Some(("Fn", false)));
        assert_eq!(modifier_transition(56, FLAG_SHIFT | DEV_LSHIFT), Some(("LeftShift", true)));
        assert_eq!(modifier_transition(56, 0), Some(("LeftShift", false)));
        // right shift still held after left shift was released
        assert_eq!(modifier_transition(56, FLAG_SHIFT | DEV_RSHIFT), Some(("LeftShift", false)));
        assert_eq!(modifier_transition(60, FLAG_SHIFT | DEV_RSHIFT), Some(("RightShift", true)));
        assert_eq!(modifier_transition(61, FLAG_OPTION | DEV_ROPT), Some(("RightOption", true)));
        assert_eq!(modifier_transition(62, FLAG_CTRL | DEV_RCTRL), Some(("RightCtrl", true)));
        assert_eq!(modifier_transition(54, FLAG_CMD | DEV_RCMD | FLAG_FN), Some(("RightCmd", true)));
        // synthetic event without device bits
        assert_eq!(modifier_transition(58, FLAG_OPTION), Some(("LeftOption", true)));
        // caps lock is not tracked
        assert_eq!(modifier_transition(57, 0x10000), None);
    }

    #[test]
    fn canonical_order() {
        assert_eq!(canonical_combo(&["V", "LeftCmd", "LeftCtrl"]), "LeftCtrl+LeftCmd+V");
        assert_eq!(canonical_combo(&["Space", "Fn"]), "Fn+Space");
        assert_eq!(canonical_combo(&["LeftShift", "Fn"]), "Fn+LeftShift");
        assert_eq!(
            canonical_combo(&["RightCmd", "LeftShift", "RightOption", "LeftCtrl", "Fn"]),
            "Fn+LeftCtrl+RightOption+LeftShift+RightCmd"
        );
        assert_eq!(canonical_combo(&["LeftCmd", "RightCmd"]), "LeftCmd+RightCmd");
        assert_eq!(canonical_combo(&["B", "A", "Cmd"]), "Cmd+B+A");
        assert_eq!(normalize_binding("cmd+ctrl+v").as_deref(), Some("Ctrl+Cmd+V"));
        assert_eq!(normalize_binding("space+fn").as_deref(), Some("Fn+Space"));
        assert_eq!(normalize_binding("nonsense"), None);
    }

    // ── capture ──

    #[test]
    fn capture_reports_canonical_combo_and_done() {
        let mut c = CaptureMachine::default();
        assert_eq!(c.on_key("Space", true), Some(("Space".into(), false)));
        c.reset();
        assert_eq!(c.on_key("Fn", true), Some(("Fn".into(), false)));
        assert_eq!(c.on_key("Space", true), Some(("Fn+Space".into(), false)));
        assert_eq!(c.on_key("Space", true), None, "repeat");
        assert_eq!(c.on_key("Space", false), Some(("Fn".into(), false)));
        assert_eq!(c.on_key("Fn", false), Some(("Fn+Space".into(), true)));
        // next gesture starts fresh
        assert_eq!(c.on_key("RightOption", true), Some(("RightOption".into(), false)));
        assert_eq!(c.on_key("RightOption", false), Some(("RightOption".into(), true)));
    }

    #[test]
    fn capture_orders_modifiers_regardless_of_press_order() {
        let mut c = CaptureMachine::default();
        c.on_key("V", true);
        c.on_key("LeftCmd", true);
        let r = c.on_key("LeftCtrl", true).unwrap();
        assert_eq!(r.0, "LeftCtrl+LeftCmd+V");
        c.on_key("LeftCtrl", false);
        c.on_key("LeftCmd", false);
        assert_eq!(c.on_key("V", false), Some(("LeftCtrl+LeftCmd+V".into(), true)));
    }
}
