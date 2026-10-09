//! Manual test for AX context capture. Run in a terminal with Accessibility permission, then switch to another
//! app and focus a text field / select some text within 2 seconds:
//!
//!   cargo run --example context_probe

#[cfg(target_os = "macos")]
use std::time::{Duration, Instant};

#[cfg(target_os = "macos")]
#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    static kCFRunLoopDefaultMode: *const std::ffi::c_void;
    fn CFRunLoopRunInMode(mode: *const std::ffi::c_void, seconds: f64, return_after_source_handled: bool) -> i32;
}

#[cfg(target_os = "macos")]
fn main() {
    let perms = hoshiro_lib::platform::permissions::check();
    println!("permissions: {perms:?}");
    if perms.accessibility != hoshiro_lib::types::PermissionState::Granted {
        println!("Accessibility is not granted to this terminal; only the app name will be available.");
        hoshiro_lib::platform::permissions::request(hoshiro_lib::types::PermissionKind::Accessibility);
    }
    loop {
        let t = Instant::now();
        let ctx = hoshiro_lib::platform::context::capture(true, false);
        println!("--- capture(true,false) took {:?}", t.elapsed());
        println!("{}", serde_json::to_string_pretty(&ctx).unwrap_or_default());
        println!("focused_text_value: {:?}", hoshiro_lib::platform::context::focused_text_value().map(|s| s.chars().take(80).collect::<String>()));
        // NSWorkspace needs the main run loop to learn about app switches.
        unsafe { CFRunLoopRunInMode(kCFRunLoopDefaultMode, Duration::from_secs(2).as_secs_f64(), false) };
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("此诊断工具需要在 macOS 上运行。");
}
