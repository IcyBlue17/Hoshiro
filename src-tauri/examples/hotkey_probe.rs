//! Manual test for the hotkey engine. Run in a terminal that has Accessibility + Input Monitoring permission:
//!
//!   cargo run --example hotkey_probe                 # default bindings (Fn / Fn+Space / Fn+LeftShift / Ctrl+Cmd+V)
//!   cargo run --example hotkey_probe -- "Fn,Help,RightOption"   # custom dictation bindings
//!   cargo run --example hotkey_probe -- --capture    # shortcut capture mode
//!
//! A tiny fake controller keeps the session phase in sync with the actions it receives, like the real one.

use hoshiro_lib::platform::hotkey::{HotkeyAction, HotkeyEngine, SessionPhase};
use hoshiro_lib::types::ShortcutBindings;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Instant;

const IDLE: u8 = 0;
const PTT: u8 = 1;
const LOCKED: u8 = 2;
const PROCESSING: u8 = 3;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let capture = args.iter().any(|a| a == "--capture");

    let mut bindings = ShortcutBindings::default();
    if let Some(custom) = args.iter().find(|a| !a.starts_with("--")) {
        bindings.dictation = custom.split(',').map(|s| s.trim().to_string()).collect();
    }
    println!("bindings: {bindings:#?}");

    let perms = hoshiro_lib::platform::permissions::check();
    println!("permissions: {perms:?}");

    let phase = Arc::new(AtomicU8::new(IDLE));
    let phase_reader = {
        let phase = phase.clone();
        Arc::new(move || match phase.load(Ordering::SeqCst) {
            PTT => SessionPhase::RecordingPtt,
            LOCKED => SessionPhase::RecordingLocked,
            PROCESSING => SessionPhase::Processing,
            _ => SessionPhase::Idle,
        })
    };
    let t0 = Instant::now();
    let on_action = {
        let phase = phase.clone();
        Arc::new(move |a: HotkeyAction| {
            println!("[{:>8.3}s] {a:?}", t0.elapsed().as_secs_f64());
            let next = match &a {
                HotkeyAction::Start { locked: false, .. } => Some(PTT),
                HotkeyAction::Start { locked: true, .. } | HotkeyAction::Lock | HotkeyAction::SwitchMode { .. } => Some(LOCKED),
                HotkeyAction::Finish => Some(PROCESSING),
                HotkeyAction::Cancel | HotkeyAction::CancelSilently | HotkeyAction::TapTooShort => Some(IDLE),
                HotkeyAction::PasteLast => None,
            };
            if let Some(n) = next {
                phase.store(n, Ordering::SeqCst);
                if n == PROCESSING {
                    // pretend ASR/LLM takes a second
                    let phase = phase.clone();
                    std::thread::spawn(move || {
                        std::thread::sleep(std::time::Duration::from_millis(1000));
                        phase.store(IDLE, Ordering::SeqCst);
                        println!("          (processing done -> Idle)");
                    });
                }
            }
        })
    };

    let engine = match HotkeyEngine::start(bindings, phase_reader, on_action) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("could not start the hotkey engine: {e:#}");
            eprintln!("Grant Accessibility / Input Monitoring to this terminal in System Settings > Privacy & Security.");
            std::process::exit(1);
        }
    };
    println!("engine active: {}", engine.is_active());

    if capture {
        println!("capture mode: press and release a key combination (Ctrl-C to quit)");
        engine.start_capture(Arc::new(|keys, done| println!("capture: {keys:<30} done={done}")));
    } else {
        println!("listening: hold Fn, double-tap Fn, Fn+Space, Fn+LeftShift, Esc while recording, Ctrl+Cmd+V ... (Ctrl-C to quit)");
    }
    loop {
        std::thread::sleep(std::time::Duration::from_secs(5));
        if !engine.is_active() {
            eprintln!("warning: event tap is not enabled");
        }
    }
}
