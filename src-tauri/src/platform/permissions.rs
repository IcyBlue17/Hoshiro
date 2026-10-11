//! Permission checks.

use crate::types::{PermissionKind, PermissionState, Permissions};

pub fn check() -> Permissions {
    #[cfg(target_os = "macos")]
    {
        Permissions {
            microphone: mac_impl::microphone(),
            accessibility: mac_impl::accessibility(),
            input_monitoring: mac_impl::input_monitoring(),
        }
    }
    #[cfg(windows)]
    {
        // Hooks and UI Automation need no grant on Windows; only the microphone privacy switch matters.
        Permissions {
            microphone: win_impl::microphone(),
            accessibility: PermissionState::Granted,
            input_monitoring: PermissionState::Granted,
        }
    }
    #[cfg(target_os = "linux")]
    {
        Permissions {
            microphone: PermissionState::Granted,
            accessibility: linux_impl::uinput(),
            input_monitoring: linux_impl::input_monitoring(),
        }
    }
    #[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
    {
        Permissions {
            microphone: PermissionState::Granted,
            accessibility: PermissionState::Granted,
            input_monitoring: PermissionState::Granted,
        }
    }
}

/// Trigger the system prompt for `kind` (mic: AVCaptureDevice request; accessibility: AXIsProcessTrustedWithOptions
/// with prompt; input monitoring: CGRequestListenEventAccess). Returns the updated state.
///
/// The microphone prompt is asynchronous: the returned state may still be `NotDetermined`; poll `check()`.
/// Windows has no consent prompt for desktop apps: a denied microphone opens the privacy settings page instead.
/// Linux: input access installs the udev rule through pkexec (one password prompt grants both).
pub fn request(kind: PermissionKind) -> Permissions {
    #[cfg(target_os = "macos")]
    {
        match kind {
            PermissionKind::Microphone => mac_impl::request_microphone(),
            PermissionKind::Accessibility => mac_impl::request_accessibility(),
            PermissionKind::InputMonitoring => mac_impl::request_input_monitoring(),
        }
    }
    #[cfg(windows)]
    {
        if kind == PermissionKind::Microphone && win_impl::microphone() == PermissionState::Denied {
            open_settings(kind);
        }
    }
    #[cfg(target_os = "linux")]
    {
        let now = check();
        let missing = match kind {
            PermissionKind::Microphone => false,
            PermissionKind::Accessibility => now.accessibility != PermissionState::Granted,
            PermissionKind::InputMonitoring => now.input_monitoring != PermissionState::Granted,
        };
        if missing {
            linux_impl::install_rule();
        }
    }
    #[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
    {
        let _ = kind;
    }
    check()
}

/// URL of the System Settings pane for a permission.
#[cfg(not(any(windows, target_os = "linux")))]
pub fn settings_url(kind: PermissionKind) -> &'static str {
    match kind {
        PermissionKind::Microphone => "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone",
        PermissionKind::Accessibility => "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility",
        PermissionKind::InputMonitoring => "x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent",
    }
}

/// URL of the Settings page for a permission.
#[cfg(windows)]
pub fn settings_url(kind: PermissionKind) -> &'static str {
    match kind {
        PermissionKind::Microphone => "ms-settings:privacy-microphone",
        PermissionKind::Accessibility | PermissionKind::InputMonitoring => "ms-settings:privacy",
    }
}

/// Linux has no settings page for input access: the guide explains the udev rule.
#[cfg(target_os = "linux")]
pub fn settings_url(kind: PermissionKind) -> &'static str {
    match kind {
        PermissionKind::Microphone => "",
        PermissionKind::Accessibility | PermissionKind::InputMonitoring => LINUX_GUIDE_URL,
    }
}

#[cfg(target_os = "linux")]
const LINUX_GUIDE_URL: &str = "https://github.com/IcyBlue17/hoshiro/blob/main/docs/guide.md#linux";

#[cfg(not(any(windows, target_os = "linux")))]
const KEYBOARD_SETTINGS_URL: &str = "x-apple.systempreferences:com.apple.Keyboard-Settings.extension";
#[cfg(windows)]
const KEYBOARD_SETTINGS_URL: &str = "ms-settings:typing";

/// Open the relevant System Settings pane.
pub fn open_settings(kind: PermissionKind) {
    #[cfg(target_os = "linux")]
    if kind == PermissionKind::Microphone {
        linux_open_panel(&[("gnome-control-center", &["sound"]), ("systemsettings", &["kcm_pulseaudio"]), ("pavucontrol", &[])]);
        return;
    }
    open_url(settings_url(kind));
}

/// Open System Settings > Keyboard (to set "Press 🌐 key to: Do Nothing").
pub fn open_keyboard_settings() {
    #[cfg(target_os = "linux")]
    {
        linux_open_panel(&[("gnome-control-center", &["keyboard"]), ("systemsettings", &["kcm_keys"])]);
    }
    #[cfg(not(target_os = "linux"))]
    open_url(KEYBOARD_SETTINGS_URL);
}

/// Run the first settings tool that exists on this desktop.
#[cfg(target_os = "linux")]
fn linux_open_panel(candidates: &[(&str, &[&str])]) {
    for (cmd, args) in candidates {
        match std::process::Command::new(cmd).args(*args).spawn() {
            Ok(_) => return,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                log::warn!("failed to run {cmd}: {e}");
                return;
            }
        }
    }
    log::info!("no settings tool found for {candidates:?}");
}

fn open_url(url: &str) {
    #[cfg(target_os = "macos")]
    {
        if let Err(e) = std::process::Command::new("/usr/bin/open").arg(url).spawn() {
            log::warn!("failed to open {url}: {e}");
        }
    }
    #[cfg(windows)]
    {
        win_impl::open_url(url);
    }
    #[cfg(target_os = "linux")]
    {
        if !url.is_empty() {
            crate::platform::linux::open_url(url);
        }
    }
    #[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
    {
        let _ = url;
    }
}

/// Is another app (e.g. Typeless) running? Returns pids for the bundle id (Windows: its executable).
pub fn running_pids(bundle_id: &str) -> Vec<i32> {
    #[cfg(target_os = "macos")]
    {
        mac_impl::running_pids(bundle_id)
    }
    #[cfg(windows)]
    {
        win_impl::running_pids(bundle_id)
    }
    #[cfg(target_os = "linux")]
    {
        linux_impl::running_pids(bundle_id)
    }
    #[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
    {
        let _ = bundle_id;
        vec![]
    }
}

/// Politely terminate apps with this bundle id (NSRunningApplication.terminate; Windows: WM_CLOSE, then end
/// whatever is left after a grace period).
pub fn terminate_app(bundle_id: &str) -> bool {
    #[cfg(target_os = "macos")]
    {
        mac_impl::terminate_app(bundle_id)
    }
    #[cfg(windows)]
    {
        win_impl::terminate_app(bundle_id)
    }
    #[cfg(target_os = "linux")]
    {
        linux_impl::terminate_app(bundle_id)
    }
    #[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
    {
        let _ = bundle_id;
        false
    }
}

#[cfg(windows)]
#[path = "permissions_win.rs"]
mod win_impl;

#[cfg(target_os = "linux")]
#[path = "permissions_linux.rs"]
pub(crate) mod linux_impl;

#[cfg(target_os = "macos")]
mod mac_impl {
    use super::PermissionState;
    use crate::platform::mac::*;
    use block2::RcBlock;
    use core_foundation::base::TCFType;
    use core_foundation::boolean::CFBoolean;
    use core_foundation::dictionary::CFDictionary;
    use core_foundation::string::CFString;
    use objc2::msg_send;
    use objc2::runtime::{AnyClass, Bool};
    use objc2_app_kit::NSRunningApplication;
    use objc2_foundation::NSString;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[link(name = "AVFoundation", kind = "framework")]
    extern "C" {}

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        static kAXTrustedCheckOptionPrompt: CFStringRef;
    }

    // Accessibility / Input Monitoring have no "not determined" API: remember whether we ever asked.
    static ASKED_AX: AtomicBool = AtomicBool::new(false);
    static ASKED_IM: AtomicBool = AtomicBool::new(false);

    fn capture_device_class() -> Option<&'static AnyClass> {
        AnyClass::get(c"AVCaptureDevice")
    }

    pub fn microphone() -> PermissionState {
        let Some(cls) = capture_device_class() else {
            return PermissionState::NotDetermined;
        };
        let media = NSString::from_str("soun"); // AVMediaTypeAudio
        let status: isize = unsafe { msg_send![cls, authorizationStatusForMediaType: &*media] };
        match status {
            3 => PermissionState::Granted,
            1 | 2 => PermissionState::Denied, // restricted / denied
            _ => PermissionState::NotDetermined,
        }
    }

    pub fn request_microphone() {
        let Some(cls) = capture_device_class() else {
            return;
        };
        let media = NSString::from_str("soun");
        let handler = RcBlock::new(|granted: Bool| {
            log::info!("microphone access {}", if granted.as_bool() { "granted" } else { "denied" });
        });
        unsafe {
            let _: () = msg_send![cls, requestAccessForMediaType: &*media, completionHandler: &*handler];
        }
    }

    pub fn accessibility() -> PermissionState {
        if unsafe { AXIsProcessTrusted() } {
            PermissionState::Granted
        } else if ASKED_AX.load(Ordering::Relaxed) {
            PermissionState::Denied
        } else {
            PermissionState::NotDetermined
        }
    }

    pub fn request_accessibility() {
        ASKED_AX.store(true, Ordering::Relaxed);
        unsafe {
            let key = CFString::wrap_under_get_rule(kAXTrustedCheckOptionPrompt as _);
            let opts = CFDictionary::from_CFType_pairs(&[(key, CFBoolean::true_value())]);
            AXIsProcessTrustedWithOptions(opts.as_concrete_TypeRef() as *const _);
        }
    }

    pub fn input_monitoring() -> PermissionState {
        if unsafe { CGPreflightListenEventAccess() } {
            PermissionState::Granted
        } else if ASKED_IM.load(Ordering::Relaxed) {
            PermissionState::Denied
        } else {
            PermissionState::NotDetermined
        }
    }

    pub fn request_input_monitoring() {
        ASKED_IM.store(true, Ordering::Relaxed);
        unsafe { CGRequestListenEventAccess() };
    }

    pub fn running_pids(bundle_id: &str) -> Vec<i32> {
        let id = NSString::from_str(bundle_id);
        NSRunningApplication::runningApplicationsWithBundleIdentifier(&id)
            .iter()
            .map(|a| a.processIdentifier())
            .collect()
    }

    pub fn terminate_app(bundle_id: &str) -> bool {
        let id = NSString::from_str(bundle_id);
        let apps = NSRunningApplication::runningApplicationsWithBundleIdentifier(&id);
        let mut any = false;
        for app in apps.iter() {
            any |= app.terminate();
        }
        any
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "macos")]
    #[test]
    fn settings_urls() {
        assert!(settings_url(PermissionKind::Microphone).ends_with("Privacy_Microphone"));
        assert!(settings_url(PermissionKind::Accessibility).ends_with("Privacy_Accessibility"));
        assert!(settings_url(PermissionKind::InputMonitoring).ends_with("Privacy_ListenEvent"));
    }

    #[cfg(windows)]
    #[test]
    fn windows_settings_urls() {
        assert_eq!(settings_url(PermissionKind::Microphone), "ms-settings:privacy-microphone");
        let p = check();
        assert_eq!(p.accessibility, PermissionState::Granted);
        assert_eq!(p.input_monitoring, PermissionState::Granted);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_settings_point_to_the_guide() {
        assert!(settings_url(PermissionKind::InputMonitoring).contains("guide.md#linux"));
        assert_eq!(check().microphone, PermissionState::Granted);
        assert!(running_pids("hoshiro-test-does-not-exist").is_empty());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn check_does_not_panic() {
        let _ = check();
        assert!(running_pids("com.example.does-not-exist").is_empty());
    }
}
