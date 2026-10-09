//! Self-contained end-to-end check of the event tap: starts the engine with harmless bindings
//! (dictation = RightOption / Help) and posts synthetic modifier / key events to itself.
//! Needs Accessibility permission for the terminal. The synthetic Help key presses are swallowed by the tap.
//!
//!   cargo run --example hotkey_selftest

#[cfg(target_os = "macos")]
use core_graphics::event::{CGEvent, CGEventFlags, CGEventTapLocation, CGEventType};
#[cfg(target_os = "macos")]
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
#[cfg(target_os = "macos")]
use hoshiro_lib::platform::hotkey::{HotkeyAction, HotkeyEngine, SessionPhase};
#[cfg(target_os = "macos")]
use hoshiro_lib::types::{Mode, ShortcutBindings};
#[cfg(target_os = "macos")]
use parking_lot::Mutex;
#[cfg(target_os = "macos")]
use std::sync::atomic::{AtomicU8, Ordering};
#[cfg(target_os = "macos")]
use std::sync::Arc;
#[cfg(target_os = "macos")]
use std::time::Duration;

#[cfg(target_os = "macos")]
fn post_flags(keycode: u16, flags: CGEventFlags) {
    let src = CGEventSource::new(CGEventSourceStateID::HIDSystemState).unwrap();
    let ev = CGEvent::new_keyboard_event(src, keycode, true).unwrap();
    ev.set_type(CGEventType::FlagsChanged);
    ev.set_flags(flags);
    ev.post(CGEventTapLocation::HID);
}

#[cfg(target_os = "macos")]
fn post_key(keycode: u16, down: bool) {
    let src = CGEventSource::new(CGEventSourceStateID::HIDSystemState).unwrap();
    let ev = CGEvent::new_keyboard_event(src, keycode, down).unwrap();
    ev.post(CGEventTapLocation::HID);
}

#[cfg(target_os = "macos")]
fn sleep(ms: u64) {
    std::thread::sleep(Duration::from_millis(ms));
}

#[cfg(target_os = "macos")]
fn main() {
    let bindings = ShortcutBindings {
        dictation: vec!["RightOption".into(), "Help".into()],
        ask_anything: vec!["RightOption+F13".into()],
        translation: vec![],
        paste_last_transcript: vec![],
    };
    let phase = Arc::new(AtomicU8::new(0));
    let log: Arc<Mutex<Vec<HotkeyAction>>> = Arc::new(Mutex::new(vec![]));
    let reader = {
        let phase = phase.clone();
        Arc::new(move || match phase.load(Ordering::SeqCst) {
            1 => SessionPhase::RecordingPtt,
            2 => SessionPhase::RecordingLocked,
            3 => SessionPhase::Processing,
            _ => SessionPhase::Idle,
        })
    };
    let on_action = {
        let phase = phase.clone();
        let log = log.clone();
        Arc::new(move |a: HotkeyAction| {
            let n = match &a {
                HotkeyAction::Start { locked: false, .. } => 1,
                HotkeyAction::Start { locked: true, .. } | HotkeyAction::Lock | HotkeyAction::SwitchMode { .. } => 2,
                HotkeyAction::Finish => 3,
                _ => 0,
            };
            phase.store(n, Ordering::SeqCst);
            log.lock().push(a);
        })
    };
    let engine = HotkeyEngine::start(bindings, reader, on_action).expect("tap (grant Accessibility)");
    assert!(engine.is_active());
    sleep(200);

    let ropt = CGEventFlags::CGEventFlagAlternate.bits() | 0x40;
    let mut failures = 0;
    let mut check = |name: &str, want: Vec<HotkeyAction>| {
        sleep(150);
        let got: Vec<HotkeyAction> = std::mem::take(&mut *log.lock());
        let ok = got == want;
        println!("{} {name}: {got:?}", if ok { "PASS" } else { "FAIL" });
        if !ok {
            println!("     expected {want:?}");
            failures += 1;
        }
        phase.store(0, Ordering::SeqCst);
        sleep(450);
    };

    // 1. push-to-talk with RightOption
    post_flags(61, CGEventFlags::from_bits_truncate(ropt));
    sleep(600);
    post_flags(61, CGEventFlags::empty());
    check("PTT hold/release", vec![HotkeyAction::Start { mode: Mode::Dictation, locked: false }, HotkeyAction::Finish]);

    // 2. short tap
    post_flags(61, CGEventFlags::from_bits_truncate(ropt));
    sleep(80);
    post_flags(61, CGEventFlags::empty());
    sleep(450);
    check("short tap", vec![HotkeyAction::Start { mode: Mode::Dictation, locked: false }, HotkeyAction::TapTooShort]);

    // 3. double press -> lock, then press again -> finish
    post_flags(61, CGEventFlags::from_bits_truncate(ropt));
    sleep(60);
    post_flags(61, CGEventFlags::empty());
    sleep(100);
    post_flags(61, CGEventFlags::from_bits_truncate(ropt));
    sleep(60);
    post_flags(61, CGEventFlags::empty());
    sleep(300);
    post_flags(61, CGEventFlags::from_bits_truncate(ropt));
    sleep(60);
    post_flags(61, CGEventFlags::empty());
    check(
        "double press + finish",
        vec![HotkeyAction::Start { mode: Mode::Dictation, locked: false }, HotkeyAction::Lock, HotkeyAction::Finish],
    );

    // 4. RightOption + F13 -> Ask
    post_flags(61, CGEventFlags::from_bits_truncate(ropt));
    sleep(100);
    post_key(105, true);
    post_key(105, false);
    sleep(100);
    post_flags(61, CGEventFlags::empty());
    check(
        "RightOption+F13 -> ask",
        vec![HotkeyAction::Start { mode: Mode::Dictation, locked: false }, HotkeyAction::SwitchMode { mode: Mode::Ask }],
    );

    // 5. Help key (non-modifier dictation key, swallowed)
    post_key(114, true);
    sleep(600);
    post_key(114, false);
    check("Help PTT", vec![HotkeyAction::Start { mode: Mode::Dictation, locked: false }, HotkeyAction::Finish]);

    // 6. RightOption + other key right away -> silent cancel
    post_flags(61, CGEventFlags::from_bits_truncate(ropt));
    sleep(100);
    post_key(111, true); // F12
    post_key(111, false);
    sleep(100);
    post_flags(61, CGEventFlags::empty());
    check(
        "RightOption+F12 -> cancel silently",
        vec![HotkeyAction::Start { mode: Mode::Dictation, locked: false }, HotkeyAction::CancelSilently],
    );

    // 7. capture mode
    let captured: Arc<Mutex<Vec<(String, bool)>>> = Arc::new(Mutex::new(vec![]));
    {
        let captured = captured.clone();
        engine.start_capture(Arc::new(move |k, d| captured.lock().push((k, d))));
    }
    post_flags(61, CGEventFlags::from_bits_truncate(ropt));
    sleep(80);
    post_key(105, true);
    sleep(40);
    post_key(105, false);
    post_flags(61, CGEventFlags::empty());
    sleep(300);
    engine.stop_capture();
    let c = captured.lock().clone();
    let ok = c.last() == Some(&("RightOption+F13".to_string(), true));
    println!("{} capture: {c:?}", if ok { "PASS" } else { "FAIL" });
    if !ok {
        failures += 1;
    }

    println!("{}", if failures == 0 { "ALL PASSED" } else { "FAILURES" });
    std::process::exit(failures);
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("此诊断工具需要在 macOS 上运行。");
}
