//! Global hotkey engine (macOS: CGEventTap; Windows: low-level keyboard/mouse hooks, see `hotkey_win.rs`;
//! Linux: evdev, see `hotkey_linux.rs`).
//!
//! Turns raw key events into high-level `HotkeyAction`s using Typeless semantics.
//! Runs its own thread with a CFRunLoop. Must be cheap: never block inside the tap callback.
//!
//! Layout: `gesture.rs` holds the pure, unit-tested state machine; this file wires it to the OS.
//! tap thread (callback: normalize event -> machine -> swallow?) --channel--> worker thread (timers + `on_action`).

use crate::types::{Mode, ShortcutBindings};
use std::sync::Arc;

pub use super::gesture::{
    canonical_combo, normalize_binding, parse_binding, CaptureMachine, GestureMachine, KeyEvent, Outcome,
    DOUBLE_PRESS_WINDOW_MS, HOLD_THRESHOLD_MS, OTHER_KEY_WINDOW_MS,
};

/// Phase of the dictation session, written by the controller, read by the engine to interpret keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionPhase {
    Idle,
    /// Recording while the dictation key is held (push-to-talk).
    RecordingPtt,
    /// Recording in hands-free / toggle mode (translation & ask are always locked).
    RecordingLocked,
    /// Audio captured, waiting for ASR/LLM.
    Processing,
}

#[derive(Clone, Debug, PartialEq)]
pub enum HotkeyAction {
    /// Begin recording. `locked=false` => push-to-talk (release finishes).
    Start { mode: Mode, locked: bool },
    /// Active recording switches to another mode and becomes locked (e.g. Fn held, then Space => Ask).
    SwitchMode { mode: Mode },
    /// Double-press detected: current push-to-talk recording becomes hands-free.
    Lock,
    /// Stop recording and process.
    Finish,
    /// Discard without trace (dictation key used as a modifier for another key, e.g. Fn+Arrow).
    CancelSilently,
    /// Single short tap (< hold threshold, no second tap): discard and show a hint.
    TapTooShort,
    /// Esc while recording/processing: cancel with "undo" alert.
    Cancel,
    /// Paste last transcript shortcut.
    PasteLast,
}

pub type ActionCallback = Arc<dyn Fn(HotkeyAction) + Send + Sync>;
pub type PhaseReader = Arc<dyn Fn() -> SessionPhase + Send + Sync>;
/// (keys, done) — e.g. ("Fn+LeftShift", false) while held, then (final, true) on release.
pub type CaptureCallback = Arc<dyn Fn(String, bool) + Send + Sync>;

pub struct HotkeyEngine {
    #[cfg(any(target_os = "macos", windows, target_os = "linux"))]
    inner: driver::Driver,
    #[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
    inner: (),
}

#[cfg(any(target_os = "macos", windows, target_os = "linux"))]
impl HotkeyEngine {
    /// Install the global event tap and start delivering actions to `on_action`.
    /// Returns Err if the tap could not be created (missing Accessibility / Input Monitoring permission);
    /// the caller retries later (e.g. after permissions are granted).
    ///
    /// `phase` is called on the tap thread for every key event, so it must be a cheap non-blocking read.
    /// `on_action` is invoked on a dedicated worker thread (never inside the tap callback), in order.
    pub fn start(bindings: ShortcutBindings, phase: PhaseReader, on_action: ActionCallback) -> anyhow::Result<Self> {
        Ok(Self { inner: driver::Driver::start(bindings, phase, on_action)? })
    }

    /// Replace the active bindings (after settings change).
    pub fn update_bindings(&self, bindings: ShortcutBindings) {
        self.inner.update_bindings(bindings);
    }

    /// Enter shortcut-capture mode: hotkeys are suspended, every key combination is reported to `cb`
    /// (and swallowed) until `stop_capture` is called.
    pub fn start_capture(&self, cb: CaptureCallback) {
        self.inner.start_capture(cb);
    }

    pub fn stop_capture(&self) {
        self.inner.stop_capture();
    }

    /// True if the tap is installed and enabled.
    pub fn is_active(&self) -> bool {
        self.inner.is_active()
    }
}

#[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
impl HotkeyEngine {
    pub fn start(bindings: ShortcutBindings, phase: PhaseReader, on_action: ActionCallback) -> anyhow::Result<Self> {
        let _ = (bindings, phase, on_action);
        anyhow::bail!("global hotkeys are not implemented on this OS")
    }
    pub fn update_bindings(&self, bindings: ShortcutBindings) {
        let _ = bindings;
    }
    pub fn start_capture(&self, cb: CaptureCallback) {
        let _ = cb;
    }
    pub fn stop_capture(&self) {}
    pub fn is_active(&self) -> bool {
        false
    }
}

#[cfg(windows)]
#[path = "hotkey_win.rs"]
mod driver;

#[cfg(target_os = "linux")]
#[path = "hotkey_linux.rs"]
mod driver;

#[cfg(target_os = "macos")]
mod driver {
    use super::*;
    use crate::platform::gesture::{
        family_flag, is_modifier, key_for_code, modifier_family, modifier_transition, mouse_button_name,
        mouse_button_number, code_for_key, unknown_key_name, KeyName,
    };
    use crate::platform::mac::{
        CGEventSourceButtonState, CGEventSourceFlagsState, CGEventSourceKeyState, EVENT_SOURCE_USER_DATA_FIELD,
        SYNTH_MAGIC,
    };
    use core_foundation::base::TCFType;
    use core_foundation::mach_port::CFMachPort;
    use core_foundation::runloop::{kCFRunLoopCommonModes, CFRunLoop};
    use parking_lot::Mutex;
    use std::ffi::c_void;
    use std::panic::AssertUnwindSafe;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
    use std::time::{Duration, Instant};

    // CGEventType
    const KEY_DOWN: u32 = 10;
    const KEY_UP: u32 = 11;
    const FLAGS_CHANGED: u32 = 12;
    const OTHER_MOUSE_DOWN: u32 = 25;
    const OTHER_MOUSE_UP: u32 = 26;
    const TAP_DISABLED_BY_TIMEOUT: u32 = 0xFFFF_FFFE;
    const TAP_DISABLED_BY_USER_INPUT: u32 = 0xFFFF_FFFF;
    // CGEventField
    const FIELD_KEYCODE: u32 = 9;
    const FIELD_MOUSE_BUTTON: u32 = 3;

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventTapCreate(
            tap: u32,
            place: u32,
            options: u32,
            events_of_interest: u64,
            callback: unsafe extern "C" fn(*mut c_void, u32, *mut c_void, *mut c_void) -> *mut c_void,
            user_info: *mut c_void,
        ) -> *mut c_void;
        fn CGEventTapEnable(tap: *mut c_void, enable: bool);
        fn CGEventTapIsEnabled(tap: *mut c_void) -> bool;
        fn CGEventGetIntegerValueField(event: *mut c_void, field: u32) -> i64;
        fn CGEventGetFlags(event: *mut c_void) -> u64;
    }

    enum Msg {
        Actions(Vec<HotkeyAction>),
        Capture(String, bool),
        /// Deadline may have changed; recompute the wait.
        Wake,
        Stop,
    }

    struct Shared {
        machine: Mutex<GestureMachine>,
        capture: Mutex<CaptureMachine>,
        capture_cb: Mutex<Option<CaptureCallback>>,
        capturing: AtomicBool,
        phase: PhaseReader,
        tx: Mutex<Sender<Msg>>,
        epoch: Instant,
        port: AtomicUsize,
        active: AtomicBool,
    }

    impl Shared {
        fn now_ms(&self) -> u64 {
            self.epoch.elapsed().as_millis() as u64
        }

        fn send(&self, m: Msg) {
            let _ = self.tx.lock().send(m);
        }

        fn is_enabled(&self) -> bool {
            let p = self.port.load(Ordering::Acquire);
            self.active.load(Ordering::Acquire) && p != 0 && unsafe { CGEventTapIsEnabled(p as *mut c_void) }
        }

        /// Feed one normalized event. Returns true if the original event should be swallowed.
        fn process(&self, key: KeyName, down: bool, flags: Option<u64>, is_flags_changed: bool) -> bool {
            let regular = !is_flags_changed && !is_modifier(key);

            if self.capturing.load(Ordering::Acquire) {
                let report = self.capture.lock().on_key(key, down);
                if let Some((keys, done)) = report {
                    self.send(Msg::Capture(keys, done));
                }
                return regular;
            }

            let phase = (self.phase)(); // before taking the lock: the controller may call into us under its own lock
            let now = self.now_ms();
            let mut actions = Vec::new();
            let swallow;
            let deadline_before;
            let deadline_after;
            {
                let mut m = self.machine.lock();
                deadline_before = m.next_deadline();
                if is_flags_changed {
                    // Modifier state is authoritative in the flags: repair anything we missed.
                    if let Some(f) = flags {
                        for held in m.held_modifiers() {
                            let fam = modifier_family(held);
                            let stale = held != key
                                && fam.is_some_and(|fam| {
                                    fam != crate::platform::gesture::Family::Fn && f & family_flag(fam) == 0
                                });
                            let missed_up = down && held == key;
                            if stale || missed_up {
                                let o = m.handle(KeyEvent { key: held, down: false, t_ms: now }, phase);
                                actions.extend(o.actions);
                            }
                        }
                    }
                }
                let o = m.handle(KeyEvent { key, down, t_ms: now }, phase);
                actions.extend(o.actions);
                swallow = o.swallow && regular;
                deadline_after = m.next_deadline();
            }
            if !actions.is_empty() {
                self.send(Msg::Actions(actions));
            } else if deadline_before != deadline_after {
                self.send(Msg::Wake);
            }
            swallow
        }

        /// After the tap was disabled we cannot trust our held-key state: compare with the physical state.
        fn resync_after_disable(&self) {
            if self.capturing.load(Ordering::Acquire) {
                self.capture.lock().reset();
                return;
            }
            let phase = (self.phase)();
            let now = self.now_ms();
            let flags = unsafe { CGEventSourceFlagsState(1) };
            let mut actions = Vec::new();
            {
                let mut m = self.machine.lock();
                for held in m.held_keys().to_vec() {
                    if !physically_down(held, flags) {
                        let o = m.handle(KeyEvent { key: held, down: false, t_ms: now }, phase);
                        actions.extend(o.actions);
                    }
                }
            }
            if !actions.is_empty() {
                self.send(Msg::Actions(actions));
            } else {
                self.send(Msg::Wake);
            }
        }

        fn handle_event(&self, etype: u32, event: *mut c_void) -> bool {
            match etype {
                TAP_DISABLED_BY_TIMEOUT | TAP_DISABLED_BY_USER_INPUT => {
                    log::warn!("event tap disabled by system ({etype:#x}); re-enabling");
                    let p = self.port.load(Ordering::Acquire);
                    if p != 0 {
                        unsafe { CGEventTapEnable(p as *mut c_void, true) };
                    }
                    self.resync_after_disable();
                    return false;
                }
                KEY_DOWN | KEY_UP | FLAGS_CHANGED | OTHER_MOUSE_DOWN | OTHER_MOUSE_UP => {}
                _ => return false,
            }
            if event.is_null() {
                return false;
            }
            unsafe {
                if CGEventGetIntegerValueField(event, EVENT_SOURCE_USER_DATA_FIELD) == SYNTH_MAGIC {
                    return false; // our own CGEvent (paste / copy)
                }
                match etype {
                    KEY_DOWN | KEY_UP => {
                        let code = CGEventGetIntegerValueField(event, FIELD_KEYCODE) as u16;
                        let key = key_for_code(code).unwrap_or_else(|| unknown_key_name("Key", code as i64));
                        self.process(key, etype == KEY_DOWN, None, false)
                    }
                    FLAGS_CHANGED => {
                        let code = CGEventGetIntegerValueField(event, FIELD_KEYCODE) as u16;
                        let flags = CGEventGetFlags(event);
                        match modifier_transition(code, flags) {
                            Some((key, down)) => self.process(key, down, Some(flags), true),
                            None => false,
                        }
                    }
                    _ => {
                        let button = CGEventGetIntegerValueField(event, FIELD_MOUSE_BUTTON);
                        let key = mouse_button_name(button)
                            .unwrap_or_else(|| unknown_key_name("Mouse", button + 1));
                        self.process(key, etype == OTHER_MOUSE_DOWN, None, false)
                    }
                }
            }
        }
    }

    fn physically_down(key: KeyName, hid_flags: u64) -> bool {
        if let Some(fam) = modifier_family(key) {
            return hid_flags & family_flag(fam) != 0;
        }
        if let Some(code) = code_for_key(key) {
            return unsafe { CGEventSourceKeyState(1, code) };
        }
        if let Some(b) = mouse_button_number(key) {
            return unsafe { CGEventSourceButtonState(1, b as u32) };
        }
        false
    }

    unsafe extern "C" fn tap_callback(
        _proxy: *mut c_void,
        etype: u32,
        event: *mut c_void,
        user_info: *mut c_void,
    ) -> *mut c_void {
        if user_info.is_null() {
            return event;
        }
        let shared = &*(user_info as *const Shared);
        match std::panic::catch_unwind(AssertUnwindSafe(|| shared.handle_event(etype, event))) {
            Ok(true) => std::ptr::null_mut(),
            Ok(false) => event,
            Err(_) => {
                log::error!("panic in hotkey tap callback");
                event
            }
        }
    }

    pub struct Driver {
        shared: Arc<Shared>,
        run_loop: CFRunLoop,
        tap_thread: Option<std::thread::JoinHandle<()>>,
    }

    impl Driver {
        pub fn start(bindings: ShortcutBindings, phase: PhaseReader, on_action: ActionCallback) -> anyhow::Result<Self> {
            let (tx, rx) = mpsc::channel::<Msg>();
            let shared = Arc::new(Shared {
                machine: Mutex::new(GestureMachine::new(&bindings)),
                capture: Mutex::new(CaptureMachine::default()),
                capture_cb: Mutex::new(None),
                capturing: AtomicBool::new(false),
                phase,
                tx: Mutex::new(tx),
                epoch: Instant::now(),
                port: AtomicUsize::new(0),
                active: AtomicBool::new(false),
            });

            let (ready_tx, ready_rx) = mpsc::channel::<Result<CFRunLoop, String>>();
            let thread_shared = shared.clone();
            let tap_thread = std::thread::Builder::new()
                .name("hoshiro-hotkey-tap".into())
                .spawn(move || tap_thread_main(thread_shared, ready_tx))
                .map_err(|e| anyhow::anyhow!("failed to spawn hotkey thread: {e}"))?;

            let run_loop = match ready_rx.recv_timeout(Duration::from_secs(5)) {
                Ok(Ok(rl)) => rl,
                Ok(Err(e)) => {
                    let _ = tap_thread.join();
                    anyhow::bail!(e);
                }
                Err(_) => anyhow::bail!("timed out creating the event tap"),
            };

            let worker_shared = shared.clone();
            std::thread::Builder::new()
                .name("hoshiro-hotkey-worker".into())
                .spawn(move || worker_main(worker_shared, rx, on_action))
                .map_err(|e| anyhow::anyhow!("failed to spawn hotkey worker: {e}"))?;

            log::info!("hotkey engine started");
            Ok(Self { shared, run_loop, tap_thread: Some(tap_thread) })
        }

        pub fn update_bindings(&self, bindings: ShortcutBindings) {
            self.shared.machine.lock().set_bindings(&bindings);
            self.shared.send(Msg::Wake);
        }

        pub fn start_capture(&self, cb: CaptureCallback) {
            self.shared.machine.lock().reset();
            self.shared.capture.lock().reset();
            *self.shared.capture_cb.lock() = Some(cb);
            self.shared.capturing.store(true, Ordering::Release);
            self.shared.send(Msg::Wake);
        }

        pub fn stop_capture(&self) {
            self.shared.capturing.store(false, Ordering::Release);
            *self.shared.capture_cb.lock() = None;
            self.shared.capture.lock().reset();
            // Keys still physically down at this point would otherwise look like fresh state.
            self.shared.machine.lock().reset();
        }

        pub fn is_active(&self) -> bool {
            self.shared.is_enabled()
        }
    }

    impl Drop for Driver {
        fn drop(&mut self) {
            self.shared.send(Msg::Stop);
            self.run_loop.stop();
            if let Some(t) = self.tap_thread.take() {
                let _ = t.join();
            }
        }
    }

    fn tap_thread_main(shared: Arc<Shared>, ready: Sender<Result<CFRunLoop, String>>) {
        let mask: u64 = (1 << KEY_DOWN) | (1 << KEY_UP) | (1 << FLAGS_CHANGED) | (1 << OTHER_MOUSE_DOWN) | (1 << OTHER_MOUSE_UP);
        // kCGSessionEventTap = 1, kCGHeadInsertEventTap = 0, kCGEventTapOptionDefault = 0 (active)
        let user_info = Arc::as_ptr(&shared) as *mut c_void;
        let port = unsafe { CGEventTapCreate(1, 0, 0, mask, tap_callback, user_info) };
        if port.is_null() {
            let _ = ready.send(Err(
                "could not create the keyboard event tap (grant Accessibility / Input Monitoring permission)".into(),
            ));
            return;
        }
        let mach_port = unsafe { CFMachPort::wrap_under_create_rule(port as _) };
        let source = match mach_port.create_runloop_source(0) {
            Ok(s) => s,
            Err(_) => {
                let _ = ready.send(Err("could not create the event tap run loop source".into()));
                return;
            }
        };
        let run_loop = CFRunLoop::get_current();
        run_loop.add_source(&source, unsafe { kCFRunLoopCommonModes });
        unsafe { CGEventTapEnable(port, true) };
        shared.port.store(port as usize, Ordering::Release);
        shared.active.store(true, Ordering::Release);
        let _ = ready.send(Ok(run_loop));

        CFRunLoop::run_current();

        shared.active.store(false, Ordering::Release);
        shared.port.store(0, Ordering::Release);
        unsafe { CGEventTapEnable(port, false) };
        drop(source);
        drop(mach_port);
        log::info!("hotkey tap thread exited");
    }

    fn worker_main(shared: Arc<Shared>, rx: Receiver<Msg>, on_action: ActionCallback) {
        let dispatch = |actions: Vec<HotkeyAction>| {
            for a in actions {
                log::debug!("hotkey action: {a:?}");
                if std::panic::catch_unwind(AssertUnwindSafe(|| on_action(a))).is_err() {
                    log::error!("panic in hotkey action handler");
                }
            }
        };
        loop {
            let wait = match shared.machine.lock().next_deadline() {
                Some(d) => Duration::from_millis(d.saturating_sub(shared.now_ms())),
                None => Duration::from_secs(3600),
            };
            match rx.recv_timeout(wait) {
                Ok(Msg::Actions(v)) => dispatch(v),
                Ok(Msg::Capture(keys, done)) => {
                    let cb = shared.capture_cb.lock().clone();
                    if let Some(cb) = cb {
                        if std::panic::catch_unwind(AssertUnwindSafe(|| cb(keys, done))).is_err() {
                            log::error!("panic in shortcut capture callback");
                        }
                    }
                }
                Ok(Msg::Wake) => {}
                Ok(Msg::Stop) | Err(RecvTimeoutError::Disconnected) => break,
                Err(RecvTimeoutError::Timeout) => {
                    let phase = (shared.phase)();
                    let now = shared.now_ms();
                    let actions = shared.machine.lock().on_timer(now, phase);
                    dispatch(actions);
                }
            }
        }
    }
}
