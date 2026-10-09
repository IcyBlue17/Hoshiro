//! Small shared macOS helpers (FFI declarations, synthetic key posting, main-thread utilities).

use core_foundation::base::{CFRelease, CFTypeRef};
use core_foundation::string::CFString;
use core_graphics::event::{CGEvent, CGEventFlags, CGEventTapLocation};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use std::ffi::c_void;

/// Value written into `kCGEventSourceUserData` of every event we post, so our own tap ignores them.
pub const SYNTH_MAGIC: i64 = 0x4853_4852; // "HSHR"
pub const EVENT_SOURCE_USER_DATA_FIELD: u32 = 42;

pub type CFStringRef = *const c_void;

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    pub fn AXIsProcessTrusted() -> bool;
    pub fn AXIsProcessTrustedWithOptions(options: *const c_void) -> bool;
    pub fn AXUIElementCreateSystemWide() -> CFTypeRef;
    pub fn AXUIElementCreateApplication(pid: i32) -> CFTypeRef;
    pub fn AXUIElementCopyAttributeValue(element: CFTypeRef, attribute: CFStringRef, value: *mut CFTypeRef) -> i32;
    pub fn AXUIElementCopyParameterizedAttributeValue(
        element: CFTypeRef,
        attribute: CFStringRef,
        parameter: CFTypeRef,
        value: *mut CFTypeRef,
    ) -> i32;
    pub fn AXUIElementSetAttributeValue(element: CFTypeRef, attribute: CFStringRef, value: CFTypeRef) -> i32;
    pub fn AXUIElementIsAttributeSettable(element: CFTypeRef, attribute: CFStringRef, settable: *mut u8) -> i32;
    pub fn AXUIElementSetMessagingTimeout(element: CFTypeRef, timeout_secs: f32) -> i32;
    pub fn AXValueGetValue(value: CFTypeRef, the_type: u32, ptr: *mut c_void) -> bool;
    pub fn AXValueCreate(the_type: u32, ptr: *const c_void) -> CFTypeRef;
    pub fn AXValueGetType(value: CFTypeRef) -> u32;
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    pub fn CGPreflightListenEventAccess() -> bool;
    pub fn CGRequestListenEventAccess() -> bool;
    pub fn CGEventSourceFlagsState(state: i32) -> u64;
    pub fn CGEventSourceKeyState(state: i32, key: u16) -> bool;
    pub fn CGEventSourceButtonState(state: i32, button: u32) -> bool;
}

/// Owned CF object, released on drop.
pub struct CfOwned(pub CFTypeRef);

impl CfOwned {
    pub fn new(p: CFTypeRef) -> Option<Self> {
        if p.is_null() {
            None
        } else {
            Some(Self(p))
        }
    }
    pub fn as_ptr(&self) -> CFTypeRef {
        self.0
    }
}

impl Drop for CfOwned {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CFRelease(self.0) };
        }
    }
}

unsafe impl Send for CfOwned {}

pub fn cfstr(s: &str) -> CFString {
    CFString::new(s)
}

/// Post a key down+up with exactly `flags` as modifiers, from a private event source so physically held
/// keys (Fn, Shift ...) cannot leak into the shortcut. Must not be called from inside the event-tap callback.
pub fn post_key_press(keycode: u16, flags: CGEventFlags) -> anyhow::Result<()> {
    let source = CGEventSource::new(CGEventSourceStateID::Private)
        .map_err(|_| anyhow::anyhow!("CGEventSourceCreate failed"))?;
    for down in [true, false] {
        let ev = CGEvent::new_keyboard_event(source.clone(), keycode, down)
            .map_err(|_| anyhow::anyhow!("CGEventCreateKeyboardEvent failed"))?;
        ev.set_flags(flags);
        ev.set_integer_value_field(EVENT_SOURCE_USER_DATA_FIELD, SYNTH_MAGIC);
        ev.post(CGEventTapLocation::HID);
        if down {
            std::thread::sleep(std::time::Duration::from_millis(8));
        }
    }
    Ok(())
}

pub fn is_main_thread() -> bool {
    objc2_foundation::NSThread::isMainThread_class()
}

/// Ask the main dispatch queue for a value, giving up after `timeout`. Runs inline on the main thread.
pub fn on_main_queue<T: Send + 'static>(
    timeout: std::time::Duration,
    f: impl FnOnce() -> T + Send + 'static,
) -> Option<T> {
    if is_main_thread() {
        return Some(f());
    }
    let (tx, rx) = std::sync::mpsc::channel();
    dispatch2::DispatchQueue::main().exec_async(move || {
        let _ = tx.send(f());
    });
    rx.recv_timeout(timeout).ok()
}
