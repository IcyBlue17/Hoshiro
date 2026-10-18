//! Linux driver for the hotkey engine: reads key events straight from the kernel (`/dev/input/event*`).
//!
//! reader thread per device (normalize event -> machine) --channel--> worker thread (timers + `on_action`).
//!
//! evdev works the same under X11 and every Wayland compositor (there is no global key hook on Wayland), but
//! it needs read access to the input devices (see `permissions_linux.rs`) and it only observes: events
//! cannot be swallowed, so a regular key that is part of a shortcut (Space in Ctrl+Super+Space) also
//! reaches the focused app. The default shortcuts are therefore modifier-only chords.
//!
//! Linux specifics handled here:
//! - Releasing Super or Alt after they took part in a shortcut would open the GNOME overview / the menu bar
//!   of the focused app. While they are held, an unassigned key (F24) is tapped on our virtual keyboard so
//!   the release no longer counts as a lone tap.
//! - Devices come and go (Bluetooth keyboards, docks): the device list is rescanned periodically and keys
//!   held on a device that disappears are released.
//! - Our own uinput keyboard (pastes) is never read.

use super::*;
use crate::platform::gesture::{
    code_for_key, is_modifier, key_for_code, mouse_button_name, mouse_button_number, unknown_key_name, KeyName,
};
use crate::platform::linux::{self, is_alt_or_super, modifier_bit, LISTENING, PHYSICAL_MODS, VIRTUAL_KEYBOARD_NAME};
use parking_lot::Mutex;
use std::collections::HashSet;
use std::os::fd::AsRawFd;
use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

const RESCAN_EVERY: Duration = Duration::from_secs(2);
const POLL_MS: i32 = 400;

const EV_SYN: u16 = 0;
const EV_KEY: u16 = 1;
const SYN_DROPPED: u16 = 3;
const KEY_A: u16 = 30;
const KEY_SPACE: u16 = 57;
const BTN_MIDDLE: u16 = 0x112;
const BTN_SIDE: u16 = 0x113;
const BTN_EXTRA: u16 = 0x114;
const BTN_FORWARD: u16 = 0x115;
const BTN_BACK: u16 = 0x116;

enum Msg {
    Actions(Vec<HotkeyAction>),
    Capture(String, bool),
    /// Tap the mask key (Super / Alt is held and took part in a shortcut).
    Mask,
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
    stop: AtomicBool,
    /// Device nodes with a running reader.
    devices: Mutex<HashSet<PathBuf>>,
    readers: AtomicUsize,
    /// Super / Alt keys (modifier bits) whose release is already masked for the current hold.
    masked: AtomicU32,
}

/// evdev mouse button -> 0-based button number used by the shared tables (2 = middle, 3/4 = side buttons).
fn mouse_button(code: u16) -> Option<i64> {
    Some(match code {
        BTN_MIDDLE => 2,
        BTN_SIDE | BTN_BACK => 3,
        BTN_EXTRA | BTN_FORWARD => 4,
        _ => return None,
    })
}

impl Shared {
    fn now_ms(&self) -> u64 {
        self.epoch.elapsed().as_millis() as u64
    }

    fn send(&self, m: Msg) {
        let _ = self.tx.lock().send(m);
    }

    /// Ask for a mask-key tap if Super / Alt is held and not masked yet.
    fn mask_held_alt_super(&self) {
        let held = PHYSICAL_MODS.load(Ordering::Acquire);
        let alt_super = held
            & [linux::KEY_LEFTALT, linux::KEY_RIGHTALT, linux::KEY_LEFTMETA, linux::KEY_RIGHTMETA]
                .iter()
                .filter_map(|c| modifier_bit(*c))
                .fold(0, |a, b| a | b);
        let fresh = alt_super & !self.masked.load(Ordering::Acquire);
        if fresh != 0 {
            self.masked.fetch_or(fresh, Ordering::AcqRel);
            self.send(Msg::Mask);
        }
    }

    fn process(&self, key: KeyName, down: bool) {
        if self.capturing.load(Ordering::Acquire) {
            if code_for_key(key).is_none() && mouse_button_number(key).is_none() && !is_modifier(key) {
                // "KeyNN"/"MouseNN" names can't round-trip the binding parser; a combo built from
                // them could never fire.
                return;
            }
            let report = self.capture.lock().on_key(key, down);
            if down && !is_modifier(key) {
                self.mask_held_alt_super();
            }
            if let Some((keys, done)) = report {
                self.send(Msg::Capture(keys, done));
            }
            return;
        }

        let phase = (self.phase)(); // before taking the lock: the controller may call into us under its own lock
        let now = self.now_ms();
        let (actions, matched, deadline_before, deadline_after) = {
            let mut m = self.machine.lock();
            let before = m.next_deadline();
            let o = m.handle(KeyEvent { key, down, t_ms: now }, phase);
            let matched = !o.actions.is_empty() || o.swallow;
            (o.actions, matched, before, m.next_deadline())
        };
        if matched {
            self.mask_held_alt_super();
        }
        if !actions.is_empty() {
            self.send(Msg::Actions(actions));
        } else if deadline_before != deadline_after {
            self.send(Msg::Wake);
        }
    }

    /// One key transition from a physical device.
    fn on_key(&self, code: u16, down: bool) {
        if let Some(bit) = modifier_bit(code) {
            if down {
                PHYSICAL_MODS.fetch_or(bit, Ordering::AcqRel);
            } else {
                PHYSICAL_MODS.fetch_and(!bit, Ordering::AcqRel);
                if is_alt_or_super(code) {
                    self.masked.fetch_and(!bit, Ordering::AcqRel);
                }
            }
        }
        let key = match mouse_button(code) {
            Some(b) => mouse_button_name(b).unwrap_or_else(|| unknown_key_name("Mouse", b + 1)),
            None => key_for_code(code).unwrap_or_else(|| unknown_key_name("Key", code as i64)),
        };
        self.process(key, down);
    }
}

// ───────────────────────── devices ─────────────────────────

enum Opened {
    Device(Box<evdev::Device>),
    /// Not a keyboard / mouse with extra buttons, or our own virtual keyboard.
    Ignored,
    Denied,
    Failed,
}

fn open_device(path: &Path) -> Opened {
    let dev = match evdev::Device::open(path) {
        Ok(d) => d,
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => return Opened::Denied,
        Err(_) => return Opened::Failed,
    };
    if dev.name() == Some(VIRTUAL_KEYBOARD_NAME) {
        return Opened::Ignored;
    }
    let Some(keys) = dev.supported_keys() else { return Opened::Ignored };
    let has = |c: u16| keys.contains(evdev::KeyCode::new(c));
    let keyboard = has(KEY_A) && has(KEY_SPACE);
    let mouse = [BTN_MIDDLE, BTN_SIDE, BTN_EXTRA, BTN_FORWARD, BTN_BACK].into_iter().any(has);
    if keyboard || mouse {
        Opened::Device(Box::new(dev))
    } else {
        Opened::Ignored
    }
}

fn event_nodes() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir("/dev/input")
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("event")))
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

/// Open every new keyboard / mouse and start a reader for it. Returns (started, denied).
fn scan(shared: &Arc<Shared>) -> (usize, usize) {
    let mut started = 0;
    let mut denied = 0;
    for path in event_nodes() {
        if shared.devices.lock().contains(&path) {
            continue;
        }
        match open_device(&path) {
            Opened::Device(dev) => {
                let name = dev.name().unwrap_or("?").to_string();
                shared.devices.lock().insert(path.clone());
                shared.readers.fetch_add(1, Ordering::AcqRel);
                let s = shared.clone();
                let p = path.clone();
                let spawned = std::thread::Builder::new()
                    .name("hoshiro-evdev".into())
                    .spawn(move || reader_main(s, p, *dev));
                match spawned {
                    Ok(_) => {
                        log::info!("listening to {} ({name})", path.display());
                        started += 1;
                    }
                    Err(e) => {
                        log::warn!("failed to spawn evdev reader: {e}");
                        shared.devices.lock().remove(&path);
                        shared.readers.fetch_sub(1, Ordering::AcqRel);
                    }
                }
            }
            Opened::Denied => denied += 1,
            Opened::Ignored | Opened::Failed => {}
        }
    }
    (started, denied)
}

fn reader_main(shared: Arc<Shared>, path: PathBuf, mut dev: evdev::Device) {
    let fd = dev.as_raw_fd();
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL);
        libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
    }
    let mut held: Vec<u16> = Vec::new();
    'outer: while !shared.stop.load(Ordering::Acquire) {
        let mut pfd = libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
        let r = unsafe { libc::poll(&mut pfd, 1, POLL_MS) };
        if r < 0 {
            if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            break;
        }
        if r == 0 {
            continue;
        }
        if pfd.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
            break;
        }
        let events: Vec<(u16, u16, i32)> = match dev.fetch_events() {
            Ok(evs) => evs.map(|e| (e.event_type().0, e.code(), e.value())).collect(),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => continue,
            Err(e) => {
                log::info!("{} went away: {e}", path.display());
                break;
            }
        };
        for (ty, code, value) in events {
            if shared.stop.load(Ordering::Acquire) {
                break 'outer;
            }
            match (ty, value) {
                (EV_KEY, 0) => {
                    held.retain(|c| *c != code);
                    shared.on_key(code, false);
                }
                (EV_KEY, 1) => {
                    if !held.contains(&code) {
                        held.push(code);
                    }
                    shared.on_key(code, true);
                }
                (EV_SYN, _) if code == SYN_DROPPED => {
                    // The kernel buffer overflowed: compare our view with the device's key state.
                    if let Ok(state) = dev.get_key_state() {
                        let gone: Vec<u16> =
                            held.iter().copied().filter(|c| !state.contains(evdev::KeyCode::new(*c))).collect();
                        for c in gone {
                            held.retain(|h| *h != c);
                            shared.on_key(c, false);
                        }
                    }
                }
                _ => {} // autorepeat (value 2), SYN_REPORT, ...
            }
        }
    }
    for c in held {
        shared.on_key(c, false);
    }
    shared.devices.lock().remove(&path);
    shared.readers.fetch_sub(1, Ordering::AcqRel);
}

// ───────────────────────── driver ─────────────────────────

pub struct Driver {
    shared: Arc<Shared>,
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
            stop: AtomicBool::new(false),
            devices: Mutex::new(HashSet::new()),
            readers: AtomicUsize::new(0),
            masked: AtomicU32::new(0),
        });

        PHYSICAL_MODS.store(0, Ordering::Release);
        let (started, denied) = scan(&shared);
        if started == 0 {
            shared.stop.store(true, Ordering::Release);
            if denied > 0 {
                anyhow::bail!("no permission to read the keyboards in /dev/input (grant input access in Hoshiro)");
            }
            anyhow::bail!("no keyboard found in /dev/input");
        }
        LISTENING.store(true, Ordering::Release);
        linux::warm_up();

        let scan_shared = shared.clone();
        std::thread::Builder::new()
            .name("hoshiro-evdev-scan".into())
            .spawn(move || {
                while !scan_shared.stop.load(Ordering::Acquire) {
                    std::thread::sleep(RESCAN_EVERY);
                    if !scan_shared.stop.load(Ordering::Acquire) {
                        scan(&scan_shared);
                    }
                }
            })
            .map_err(|e| anyhow::anyhow!("failed to spawn device scanner: {e}"))?;

        let worker_shared = shared.clone();
        std::thread::Builder::new()
            .name("hoshiro-hotkey-worker".into())
            .spawn(move || worker_main(worker_shared, rx, on_action))
            .map_err(|e| anyhow::anyhow!("failed to spawn hotkey worker: {e}"))?;

        log::info!("hotkey engine started (evdev, {started} devices)");
        Ok(Self { shared })
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
        // Only an active capture may reset the machine: a gratuitous call (the hub window closing
        // mid-gesture, frontend cleanup after a failed start) must not wipe in-flight state.
        if !self.shared.capturing.swap(false, Ordering::AcqRel) {
            return;
        }
        *self.shared.capture_cb.lock() = None;
        self.shared.capture.lock().reset();
        self.shared.machine.lock().reset();
    }

    pub fn is_active(&self) -> bool {
        !self.shared.stop.load(Ordering::Acquire) && self.shared.readers.load(Ordering::Acquire) > 0
    }
}

impl Drop for Driver {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
        self.shared.send(Msg::Stop);
        LISTENING.store(false, Ordering::Release);
        PHYSICAL_MODS.store(0, Ordering::Release);
    }
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
            Ok(Msg::Mask) => {
                if let Err(e) = linux::send_keys(&[(linux::KEY_MASK, true), (linux::KEY_MASK, false)]) {
                    log::debug!("mask key tap failed: {e:#}");
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mouse_buttons_map_to_shared_numbers() {
        assert_eq!(mouse_button(BTN_MIDDLE).and_then(mouse_button_name), Some("Mouse3"));
        assert_eq!(mouse_button(BTN_SIDE).and_then(mouse_button_name), Some("Mouse4"));
        assert_eq!(mouse_button(BTN_BACK).and_then(mouse_button_name), Some("Mouse4"));
        assert_eq!(mouse_button(BTN_EXTRA).and_then(mouse_button_name), Some("Mouse5"));
        assert_eq!(mouse_button(KEY_A), None);
    }
}

#[cfg(test)]
mod live_tests {
    use super::*;
    use evdev::{AttributeSet, EventType, InputEvent, KeyCode};

    /// A second virtual keyboard standing in for the user's (ours is skipped by the listener).
    fn fake_keyboard() -> evdev::uinput::VirtualDevice {
        let mut keys = AttributeSet::<KeyCode>::new();
        for c in [KEY_A, KEY_SPACE, linux::KEY_LEFTCTRL, linux::KEY_LEFTMETA] {
            keys.insert(KeyCode::new(c));
        }
        let d = evdev::uinput::VirtualDevice::builder()
            .unwrap()
            .name("Hoshiro Test Keyboard")
            .with_keys(&keys)
            .unwrap()
            .build()
            .unwrap();
        std::thread::sleep(Duration::from_millis(400));
        d
    }

    fn key(d: &mut evdev::uinput::VirtualDevice, code: u16, down: bool) {
        d.emit(&[InputEvent::new(EventType::KEY.0, code, down as i32)]).unwrap();
    }

    /// Needs /dev/uinput and read access to /dev/input: run with `--ignored`.
    #[test]
    #[ignore = "creates a virtual keyboard"]
    fn ctrl_super_hold_starts_and_finishes() {
        let mut kb = fake_keyboard();
        let (tx, rx) = std::sync::mpsc::channel();
        let tx = Mutex::new(tx);
        let on_action: ActionCallback = Arc::new(move |a| {
            let _ = tx.lock().send(a);
        });
        let phase: PhaseReader = Arc::new(|| SessionPhase::Idle);
        let d = Driver::start(ShortcutBindings::windows_defaults(), phase, on_action).unwrap();
        assert!(d.is_active());
        key(&mut kb, linux::KEY_LEFTCTRL, true);
        std::thread::sleep(Duration::from_millis(40));
        key(&mut kb, linux::KEY_LEFTMETA, true);
        let first = rx.recv_timeout(Duration::from_secs(2));
        std::thread::sleep(Duration::from_millis(500));
        assert_ne!(PHYSICAL_MODS.load(Ordering::Acquire), 0);
        key(&mut kb, linux::KEY_LEFTMETA, false);
        key(&mut kb, linux::KEY_LEFTCTRL, false);
        let second = rx.recv_timeout(Duration::from_secs(2));
        assert_eq!(first, Ok(HotkeyAction::Start { mode: crate::types::Mode::Dictation, locked: false }));
        assert_eq!(second, Ok(HotkeyAction::Finish));
        drop(d);
    }
}
