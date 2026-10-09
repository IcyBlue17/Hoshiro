//! Manual / automated check of the floating bar panel (non-activating, click-through).
//!
//! Needs a page at the dev URL with a dark pill at the bottom-centre of a 560x460 transparent window. Serve one with
//!   mkdir -p /tmp/barpage && cp examples/bar_probe_page.html /tmp/barpage/index.html && (cd /tmp/barpage && python3 -m http.server 14999)
//! and build with
//!   TAURI_CONFIG='{"build":{"devUrl":"http://localhost:14999"}}' cargo run --example bar_probe
//!
//! It shows the panel, moves the mouse over / away from the pill and prints whether the window toggled click-through,
//! plus the frontmost app before/after (must not change).

use core_graphics::display::CGDisplay;
use core_graphics::geometry::CGPoint;
use hoshiro_lib::platform::bar;
use hoshiro_lib::types::HitRect;
use objc2::msg_send;
use objc2::runtime::AnyObject;
use std::time::Duration;
use tauri::Manager;

fn on_main<T: Send + 'static>(app: &tauri::AppHandle, f: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    let (tx, rx) = std::sync::mpsc::channel();
    app.run_on_main_thread(move || {
        let _ = tx.send(f());
    })
    .ok()?;
    rx.recv_timeout(Duration::from_secs(3)).ok()
}

fn frontmost() -> String {
    use objc2_app_kit::NSWorkspace;
    NSWorkspace::sharedWorkspace()
        .frontmostApplication()
        .map(|a| format!("{} (pid {})", a.localizedName().map(|s| s.to_string()).unwrap_or_default(), a.processIdentifier()))
        .unwrap_or_else(|| "none".into())
}

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            let handle = app.handle().clone();
            bar::setup(&handle)?;
            bar::show(&handle);
            // pill: 104x34 at the bottom centre of the 560x460 window (CSS px, top-left origin)
            bar::set_hit_rects(&handle, vec![HitRect { x: 228.0, y: 416.0, width: 104.0, height: 34.0 }]);
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(1500));
                let before = frontmost();
                let ptr = handle.get_webview_window("bar").unwrap().ns_window().unwrap() as usize;
                let info = on_main(&handle, move || unsafe {
                    let w = ptr as *mut AnyObject;
                    let cls: &objc2::runtime::AnyClass = (*w).class();
                    let level: isize = msg_send![w, level];
                    let mask: usize = msg_send![w, styleMask];
                    let behavior: usize = msg_send![w, collectionBehavior];
                    let ignores: bool = msg_send![w, ignoresMouseEvents];
                    let key: bool = msg_send![w, canBecomeKeyWindow];
                    let main: bool = msg_send![w, canBecomeMainWindow];
                    let visible: bool = msg_send![w, isVisible];
                    let frame: objc2_foundation::NSRect = msg_send![w, frame];
                    format!(
                        "class={} level={level} styleMask={mask:#x} (nonactivating={}) behavior={behavior:#x} ignoresMouse={ignores} canBecomeKey={key} canBecomeMain={main} visible={visible} frame={frame:?}",
                        cls.name().to_string_lossy(),
                        mask & 0x80 != 0
                    )
                });
                println!("{}", info.unwrap_or_default());
                let frame = on_main(&handle, move || unsafe {
                    let w = ptr as *mut AnyObject;
                    let f: objc2_foundation::NSRect = msg_send![w, frame];
                    f
                })
                .unwrap();
                let primary_h = CGDisplay::main().pixels_high() as f64;
                // pill centre in global top-left coords
                let px = frame.origin.x + 228.0 + 52.0;
                let py_appkit = frame.origin.y + (460.0 - 416.0 - 17.0);
                let py = primary_h - py_appkit;
                let ignores_at = |handle: &tauri::AppHandle| {
                    on_main(handle, move || unsafe {
                        let w = ptr as *mut AnyObject;
                        let i: bool = msg_send![w, ignoresMouseEvents];
                        i
                    })
                };
                CGDisplay::warp_mouse_cursor_position(CGPoint::new(px, py)).ok();
                std::thread::sleep(Duration::from_millis(400));
                println!("mouse over pill  -> ignoresMouseEvents = {:?} (expect Some(false))", ignores_at(&handle));
                CGDisplay::warp_mouse_cursor_position(CGPoint::new(px, py - 200.0)).ok();
                std::thread::sleep(Duration::from_millis(400));
                println!("mouse above pill -> ignoresMouseEvents = {:?} (expect Some(true))", ignores_at(&handle));
                let _ = std::process::Command::new("/usr/sbin/screencapture").args(["-x", "/tmp/bar_probe.png"]).status();
                println!("frontmost before: {before}\nfrontmost after:  {}", frontmost());
                bar::hide(&handle);
                std::thread::sleep(Duration::from_millis(300));
                println!("hidden -> visible = {:?}", on_main(&handle, move || unsafe {
                    let w = ptr as *mut AnyObject;
                    let v: bool = msg_send![w, isVisible];
                    v
                }));
                std::process::exit(0);
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("tauri");
}
