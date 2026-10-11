//! Windows driver for the hotkey engine: WH_KEYBOARD_LL + WH_MOUSE_LL hooks on a dedicated thread.
//!
//! hook thread (callback: normalize event -> machine -> swallow?) --channel--> worker thread (timers + `on_action`).
//!
//! Windows specifics handled here:
//! - Releasing Alt or Win after they took part in a shortcut would open the Start menu / activate the menu
//!   bar of the focused app (which then eats the Ctrl+V of the paste). Such releases are swallowed and
//!   replayed after a tap of an unassigned key (`VK_MASK`), exactly like AutoHotkey does.
//! - AltGr layouts send a fake Left Ctrl together with Right Alt; it is ignored.
//! - Windows silently uninstalls low-level hooks that time out, so the hooks are re-installed periodically
//!   while idle, and held modifiers are re-checked against the async key state.

use super::*;
use crate::platform::gesture::{
    code_for_key, is_modifier, key_for_code, mouse_button_name, unknown_key_name, KeyName, VK_NUMPAD_ENTER,
};
use crate::platform::win::{
    is_alt_or_win, key_down_now, key_input, modifier_bit, send_inputs, HOOK_RUNNING, MASK_ON_RELEASE, PHYSICAL_MODS,
    SYNTH_MAGIC, VK_MASK,
};
use parking_lot::{Mutex, RwLock};
use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{VIRTUAL_KEY, VK_LCONTROL};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, KillTimer, PeekMessageW, PostThreadMessageW, SetTimer,
    SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx, HC_ACTION, HHOOK, KBDLLHOOKSTRUCT, LLKHF_EXTENDED,
    MSG, MSLLHOOKSTRUCT, PM_NOREMOVE, WH_KEYBOARD_LL, WH_MOUSE_LL, WM_APP,
    WM_KEYDOWN, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_QUIT, WM_SYSKEYDOWN, WM_TIMER, WM_XBUTTONDOWN, WM_XBUTTONUP,
};

/// Replay a swallowed Alt/Win release behind a mask key. wParam = virtual key.
const WM_MASKED_RELEASE: u32 = WM_APP + 1;
/// Re-install the hooks if nothing is going on (Windows drops hooks that once timed out, without telling).
const REINSTALL_EVERY: Duration = Duration::from_secs(30);
/// While modifiers are held, compare them with the async key state this often.
const RESYNC_EVERY: Duration = Duration::from_millis(250);

enum Msg {
    Actions(Vec<HotkeyAction>),
    Capture(String, bool),
    /// Diagnostics (`HOSHIRO_HOOK_TRACE=1`): logged by the worker, never from inside the hook.
    Trace(String),
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
    thread_id: AtomicU32,
    active: AtomicBool,
    last_event_ms: AtomicU64,
    /// Held modifiers that the async key state already reported as released once (confirmed on the next check).
    suspects: Mutex<Vec<KeyName>>,
}

static TRACE: once_cell::sync::Lazy<bool> =
    once_cell::sync::Lazy::new(|| std::env::var("HOSHIRO_HOOK_TRACE").is_ok_and(|v| v == "1"));

/// The engine the hook procedures talk to (hooks carry no user data).
static CURRENT: RwLock<Option<Arc<Shared>>> = RwLock::new(None);

fn vk_of(name: &str) -> Option<VIRTUAL_KEY> {
    code_for_key(name).map(VIRTUAL_KEY)
}

impl Shared {
    fn now_ms(&self) -> u64 {
        self.epoch.elapsed().as_millis() as u64
    }

    fn send(&self, m: Msg) {
        let _ = self.tx.lock().send(m);
    }

    /// Alt / Win keys currently held get their release masked: something happened while they were down.
    fn mark_held_alt_win(&self, held: &[KeyName]) {
        for k in held {
            if let Some(vk) = vk_of(k).filter(|vk| is_alt_or_win(*vk)) {
                if let Some(b) = modifier_bit(vk) {
                    MASK_ON_RELEASE.fetch_or(b, Ordering::AcqRel);
                }
            }
        }
    }

    /// Feed one normalized event. Returns true if the original event should be swallowed.
    fn process(&self, key: KeyName, down: bool) -> bool {
        self.last_event_ms.store(self.now_ms(), Ordering::Relaxed);
        let regular = !is_modifier(key);

        if self.capturing.load(Ordering::Acquire) {
            let report = {
                let mut c = self.capture.lock();
                let r = c.on_key(key, down);
                if regular {
                    let held = c.held().to_vec();
                    self.mark_held_alt_win(&held);
                }
                r
            };
            if let Some((keys, done)) = report {
                self.send(Msg::Capture(keys, done));
            }
            return regular;
        }

        let phase = (self.phase)(); // before taking the lock: the controller may call into us under its own lock
        let now = self.now_ms();
        let (actions, swallow, deadline_before, deadline_after) = {
            let mut m = self.machine.lock();
            let before = m.next_deadline();
            let o = m.handle(KeyEvent { key, down, t_ms: now }, phase);
            if !o.actions.is_empty() || o.swallow {
                let mut held = m.held_keys().to_vec();
                if !down {
                    held.push(key); // its own release is being processed right now
                }
                self.mark_held_alt_win(&held);
            }
            (o.actions, o.swallow && regular, before, m.next_deadline())
        };
        if !actions.is_empty() {
            self.send(Msg::Actions(actions));
        } else if deadline_before != deadline_after {
            self.send(Msg::Wake);
        }
        swallow
    }

    /// Keyboard hook body. Returns true to swallow the event.
    fn on_key(&self, kb: &KBDLLHOOKSTRUCT, down: bool) -> bool {
        if *TRACE {
            self.send(Msg::Trace(format!(
                "vk={:#04x} scan={:#x} flags={:#x} extra={:#x} down={down}",
                kb.vkCode, kb.scanCode, kb.flags.0, kb.dwExtraInfo
            )));
        }
        if kb.dwExtraInfo == SYNTH_MAGIC {
            return false; // our own SendInput (paste / copy / replayed release)
        }
        let mut vk = kb.vkCode as u16;
        if vk == VK_MASK.0 || vk == 0xFF {
            return false;
        }
        // AltGr = fake Left Ctrl (scan code 0x21D) + Right Alt: only Right Alt is real.
        if vk == VK_LCONTROL.0 && kb.scanCode & 0x200 != 0 {
            return false;
        }
        if vk == 0x0D && kb.flags.0 & LLKHF_EXTENDED.0 != 0 {
            vk = VK_NUMPAD_ENTER;
        }
        // Everything not tagged by us counts as the user's input, including events injected by remapping
        // tools (AutoHotkey, PowerToys) and remote-desktop clients.
        let vkey = VIRTUAL_KEY(vk);
        let bit = modifier_bit(vkey);
        if let Some(b) = bit {
            if down {
                PHYSICAL_MODS.fetch_or(b, Ordering::AcqRel);
            } else {
                PHYSICAL_MODS.fetch_and(!b, Ordering::AcqRel);
            }
        }

        let key = key_for_code(vk).unwrap_or_else(|| unknown_key_name("Key", vk as i64));
        let swallow = self.process(key, down);

        // A paste/copy is being injected: hold the user's modifier release back until it is done.
        if !down && bit.is_some() && crate::platform::win::defer_release(vk) {
            return true;
        }

        if !down && is_alt_or_win(vkey) {
            let b = bit.unwrap_or(0);
            if MASK_ON_RELEASE.fetch_and(!b, Ordering::AcqRel) & b != 0 {
                // Replay the release after a mask key once this callback returned.
                let tid = self.thread_id.load(Ordering::Acquire);
                if tid != 0 && unsafe { PostThreadMessageW(tid, WM_MASKED_RELEASE, WPARAM(vk as usize), LPARAM(0)) }.is_ok() {
                    return true;
                }
            }
        }
        swallow
    }

    fn on_mouse(&self, msg: u32, ms: &MSLLHOOKSTRUCT) -> bool {
        if ms.dwExtraInfo == SYNTH_MAGIC {
            return false;
        }
        let (button, down) = match msg {
            WM_MBUTTONDOWN => (2, true),
            WM_MBUTTONUP => (2, false),
            WM_XBUTTONDOWN | WM_XBUTTONUP => {
                let which = (ms.mouseData >> 16) & 0xFFFF; // XBUTTON1 = 1, XBUTTON2 = 2
                (2 + which as i64, msg == WM_XBUTTONDOWN)
            }
            _ => return false,
        };
        let key = mouse_button_name(button).unwrap_or_else(|| unknown_key_name("Mouse", button + 1));
        self.process(key, down)
    }

    /// Hooks can lose events (timeouts, secure desktop, UAC prompts): release held modifiers that the async
    /// key state has reported as up on two consecutive checks.
    fn resync(&self) {
        if self.capturing.load(Ordering::Acquire) {
            return;
        }
        let held = self.machine.lock().held_modifiers();
        let mut suspects = self.suspects.lock();
        let mut released = Vec::new();
        let mut next = Vec::new();
        for k in held {
            let Some(vk) = vk_of(k) else { continue };
            if key_down_now(vk) {
                continue;
            }
            if suspects.contains(&k) {
                released.push(k);
            } else {
                next.push(k);
            }
        }
        *suspects = next;
        drop(suspects);
        if released.is_empty() {
            return;
        }
        let phase = (self.phase)();
        let now = self.now_ms();
        let mut actions = Vec::new();
        {
            let mut m = self.machine.lock();
            for k in released {
                log::debug!("hotkey resync: {k} is no longer down");
                actions.extend(m.handle(KeyEvent { key: k, down: false, t_ms: now }, phase).actions);
            }
        }
        if !actions.is_empty() {
            self.send(Msg::Actions(actions));
        }
    }
}

fn current() -> Option<Arc<Shared>> {
    CURRENT.read().clone()
}

unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 && lparam.0 != 0 {
        if let Some(shared) = current() {
            let kb = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
            let msg = wparam.0 as u32;
            let down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
            match std::panic::catch_unwind(AssertUnwindSafe(|| shared.on_key(kb, down))) {
                Ok(true) => return LRESULT(1),
                Ok(false) => {}
                Err(_) => log::error!("panic in keyboard hook"),
            }
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let msg = wparam.0 as u32;
    if code == HC_ACTION as i32
        && lparam.0 != 0
        && matches!(msg, WM_MBUTTONDOWN | WM_MBUTTONUP | WM_XBUTTONDOWN | WM_XBUTTONUP)
    {
        if let Some(shared) = current() {
            let ms = &*(lparam.0 as *const MSLLHOOKSTRUCT);
            match std::panic::catch_unwind(AssertUnwindSafe(|| shared.on_mouse(msg, ms))) {
                Ok(true) => return LRESULT(1),
                Ok(false) => {}
                Err(_) => log::error!("panic in mouse hook"),
            }
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

struct Hooks {
    keyboard: HHOOK,
    mouse: Option<HHOOK>,
}

fn install_hooks() -> anyhow::Result<Hooks> {
    unsafe {
        let module = GetModuleHandleW(None).map_err(|e| anyhow::anyhow!("GetModuleHandle: {e}"))?;
        let keyboard = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), Some(module.into()), 0)
            .map_err(|e| anyhow::anyhow!("could not install the keyboard hook: {e}"))?;
        let mouse = match SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), Some(module.into()), 0) {
            Ok(h) => Some(h),
            Err(e) => {
                log::warn!("could not install the mouse hook (mouse-button shortcuts disabled): {e}");
                None
            }
        };
        Ok(Hooks { keyboard, mouse })
    }
}

fn remove_hooks(h: Hooks) {
    unsafe {
        let _ = UnhookWindowsHookEx(h.keyboard);
        if let Some(m) = h.mouse {
            let _ = UnhookWindowsHookEx(m);
        }
    }
}

pub struct Driver {
    shared: Arc<Shared>,
    hook_thread: Option<std::thread::JoinHandle<()>>,
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
            thread_id: AtomicU32::new(0),
            active: AtomicBool::new(false),
            last_event_ms: AtomicU64::new(0),
            suspects: Mutex::new(Vec::new()),
        });

        let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
        let thread_shared = shared.clone();
        let hook_thread = std::thread::Builder::new()
            .name("hoshiro-hotkey-hook".into())
            .spawn(move || hook_thread_main(thread_shared, ready_tx))
            .map_err(|e| anyhow::anyhow!("failed to spawn hotkey thread: {e}"))?;

        match ready_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                let _ = hook_thread.join();
                anyhow::bail!(e);
            }
            Err(_) => anyhow::bail!("timed out installing the keyboard hook"),
        }

        let worker_shared = shared.clone();
        std::thread::Builder::new()
            .name("hoshiro-hotkey-worker".into())
            .spawn(move || worker_main(worker_shared, rx, on_action))
            .map_err(|e| anyhow::anyhow!("failed to spawn hotkey worker: {e}"))?;

        log::info!("hotkey engine started (low-level hooks)");
        Ok(Self { shared, hook_thread: Some(hook_thread) })
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
        self.shared.machine.lock().reset();
    }

    pub fn is_active(&self) -> bool {
        self.shared.active.load(Ordering::Acquire) && self.hook_thread.as_ref().is_some_and(|t| !t.is_finished())
    }
}

impl Drop for Driver {
    fn drop(&mut self) {
        self.shared.send(Msg::Stop);
        let tid = self.shared.thread_id.load(Ordering::Acquire);
        if tid != 0 {
            unsafe {
                let _ = PostThreadMessageW(tid, WM_QUIT, WPARAM(0), LPARAM(0));
            }
        }
        if let Some(t) = self.hook_thread.take() {
            let _ = t.join();
        }
    }
}

fn hook_thread_main(shared: Arc<Shared>, ready: Sender<Result<(), String>>) {
    unsafe {
        // Make sure this thread has a message queue before anyone posts to it.
        let mut msg = MSG::default();
        let _ = PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE);
        shared.thread_id.store(GetCurrentThreadId(), Ordering::Release);
    }
    *CURRENT.write() = Some(shared.clone());
    let mut hooks = match install_hooks() {
        Ok(h) => h,
        Err(e) => {
            *CURRENT.write() = None;
            let _ = ready.send(Err(format!("{e:#}")));
            return;
        }
    };
    HOOK_RUNNING.store(true, Ordering::Release);
    PHYSICAL_MODS.store(0, Ordering::Release);
    shared.active.store(true, Ordering::Release);
    let _ = ready.send(Ok(()));

    let timer = unsafe { SetTimer(None, 0, REINSTALL_EVERY.as_millis() as u32, None) };
    let mut msg = MSG::default();
    loop {
        let r = unsafe { GetMessageW(&mut msg, None, 0, 0) };
        if r.0 <= 0 {
            break; // WM_QUIT or error
        }
        match msg.message {
            WM_MASKED_RELEASE => {
                let vk = VIRTUAL_KEY(msg.wParam.0 as u16);
                let seq = [key_input(VK_MASK, false), key_input(VK_MASK, true), key_input(vk, true)];
                if let Err(e) = send_inputs(&seq) {
                    log::warn!("replaying a masked release failed: {e:#}");
                }
            }
            WM_TIMER => {
                // Re-install only when idle and quiet for a while, so no key event can fall between the hooks.
                let idle = (shared.phase)() == SessionPhase::Idle
                    && shared.machine.lock().held_keys().is_empty()
                    && !shared.capturing.load(Ordering::Acquire)
                    && shared.now_ms().saturating_sub(shared.last_event_ms.load(Ordering::Relaxed)) > 5_000;
                if idle {
                    match install_hooks() {
                        Ok(fresh) => remove_hooks(std::mem::replace(&mut hooks, fresh)),
                        Err(e) => log::warn!("re-installing hooks failed: {e:#}"),
                    }
                }
            }
            _ => unsafe {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            },
        }
    }

    if timer != 0 {
        unsafe {
            let _ = KillTimer(None, timer);
        }
    }
    remove_hooks(hooks);
    shared.active.store(false, Ordering::Release);
    HOOK_RUNNING.store(false, Ordering::Release);
    {
        let mut cur = CURRENT.write();
        if cur.as_ref().is_some_and(|c| Arc::ptr_eq(c, &shared)) {
            *cur = None;
        }
    }
    log::info!("hotkey hook thread exited");
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
        let (deadline, holding) = {
            let m = shared.machine.lock();
            (m.next_deadline(), !m.held_modifiers().is_empty())
        };
        let mut wait = match deadline {
            Some(d) => Duration::from_millis(d.saturating_sub(shared.now_ms())),
            None => Duration::from_secs(3600),
        };
        if holding {
            wait = wait.min(RESYNC_EVERY);
        }
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
            Ok(Msg::Trace(t)) => log::debug!("hook: {t}"),
            Ok(Msg::Wake) => {}
            Ok(Msg::Stop) | Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => {
                let phase = (shared.phase)();
                let now = shared.now_ms();
                let actions = shared.machine.lock().on_timer(now, phase);
                dispatch(actions);
                if holding {
                    shared.resync();
                }
            }
        }
    }
}

#[cfg(test)]
mod live_tests {
    use super::*;
    use crate::platform::win::{key_input, send_inputs};
    use std::sync::mpsc::Receiver;
    use windows::Win32::UI::Input::KeyboardAndMouse::{INPUT, VK_LWIN};

    /// Like a user's key press: injected without our marker, so the hook treats it as real input.
    fn press(vk: u16, up: bool) {
        let mut i: INPUT = key_input(VIRTUAL_KEY(vk), up);
        i.Anonymous.ki.dwExtraInfo = 0;
        send_inputs(&[i]).unwrap();
    }

    fn engine() -> (Driver, Receiver<HotkeyAction>) {
        let (tx, rx) = std::sync::mpsc::channel();
        let tx = parking_lot::Mutex::new(tx);
        let on_action: ActionCallback = Arc::new(move |a| {
            let _ = tx.lock().send(a);
        });
        let phase: PhaseReader = Arc::new(|| SessionPhase::Idle);
        let d = Driver::start(ShortcutBindings::windows_defaults(), phase, on_action).unwrap();
        (d, rx)
    }

    /// Injects real key events: run with `--ignored` on an interactive desktop.
    #[test]
    #[ignore = "injects global key events"]
    fn ctrl_win_hold_starts_and_finishes() {
        let (d, rx) = engine();
        assert!(d.is_active());
        if let Some(wait) = std::env::var("HOSHIRO_TEST_WAIT_MS").ok().and_then(|v| v.parse().ok()) {
            std::thread::sleep(Duration::from_millis(wait)); // e.g. past a periodic hook re-install
        }
        press(VK_LCONTROL.0, false);
        std::thread::sleep(Duration::from_millis(40));
        press(VK_LWIN.0, false);
        let first = rx.recv_timeout(Duration::from_secs(2));
        std::thread::sleep(Duration::from_millis(500));
        press(VK_LWIN.0, true);
        press(VK_LCONTROL.0, true);
        let second = rx.recv_timeout(Duration::from_secs(2));
        assert_eq!(first, Ok(HotkeyAction::Start { mode: crate::types::Mode::Dictation, locked: false }));
        assert_eq!(second, Ok(HotkeyAction::Finish));
        drop(d);
    }
}
