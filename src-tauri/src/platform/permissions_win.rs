//! Windows permissions, processes and settings pages.
//!
//! Desktop (non-packaged) apps need no Accessibility / Input Monitoring grant on Windows: global hooks and
//! UI Automation just work. Microphone access is governed by Settings > Privacy > Microphone, whose state
//! lives in the CapabilityAccessManager consent store; there is no consent prompt for desktop apps.

use super::PermissionState;
use crate::platform::win::{self, wide};
use std::time::{Duration, Instant};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, WPARAM};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Registry::{RegGetValueW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ};
use windows::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, PostMessageW, SW_SHOWNORMAL, WM_CLOSE};

const CONSENT: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\CapabilityAccessManager\\ConsentStore\\microphone";

fn reg_string(root: HKEY, sub: &str, name: &str) -> Option<String> {
    let sub = wide(sub);
    let name = wide(name);
    let mut buf = vec![0u16; 256];
    let mut len = (buf.len() * 2) as u32;
    let r = unsafe {
        RegGetValueW(root, PCWSTR(sub.as_ptr()), PCWSTR(name.as_ptr()), RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr() as *mut _), Some(&mut len))
    };
    r.is_ok().then(|| win::from_wide(&buf))
}

/// Pure: combine the three consent switches (device-wide, per-user, "let desktop apps access").
pub(crate) fn mic_state(device: Option<&str>, user: Option<&str>, desktop_apps: Option<&str>) -> PermissionState {
    let denied = |v: Option<&str>| v.is_some_and(|v| v.eq_ignore_ascii_case("Deny"));
    if denied(device) || denied(user) || denied(desktop_apps) {
        PermissionState::Denied
    } else {
        PermissionState::Granted
    }
}

pub fn microphone() -> PermissionState {
    let device = reg_string(HKEY_LOCAL_MACHINE, CONSENT, "Value");
    let user = reg_string(HKEY_CURRENT_USER, CONSENT, "Value");
    let desktop = reg_string(HKEY_CURRENT_USER, &format!("{CONSENT}\\NonPackaged"), "Value");
    mic_state(device.as_deref(), user.as_deref(), desktop.as_deref())
}

pub fn open_url(url: &str) {
    let op = wide("open");
    let target = wide(url);
    let r = unsafe { ShellExecuteW(None, PCWSTR(op.as_ptr()), PCWSTR(target.as_ptr()), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL) };
    if r.0 as isize <= 32 {
        log::warn!("failed to open {url} (ShellExecute {})", r.0 as isize);
    }
}

/// Map a macOS bundle id we care about to its Windows executable name.
pub(crate) fn exe_for(bundle_id: &str) -> String {
    match bundle_id {
        "now.typeless.desktop" => "Typeless.exe".into(),
        other => other.to_string(),
    }
}

pub fn running_pids(bundle_id: &str) -> Vec<i32> {
    let exe = exe_for(bundle_id);
    let mut pids = Vec::new();
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return pids;
        };
        let mut entry = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        if Process32FirstW(snap, &mut entry).is_ok() {
            loop {
                if win::from_wide(&entry.szExeFile).eq_ignore_ascii_case(&exe) {
                    pids.push(entry.th32ProcessID as i32);
                }
                if Process32NextW(snap, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
    }
    pids
}

/// Ask every top-level window of the app to close (what clicking its close button / "Quit" does), then
/// end processes that are still around after a grace period: tray apps often only hide on WM_CLOSE.
pub fn terminate_app(bundle_id: &str) -> bool {
    let pids = running_pids(bundle_id);
    if pids.is_empty() {
        return false;
    }
    struct Ctx<'a> {
        pids: &'a [i32],
    }
    unsafe extern "system" fn visit(hwnd: HWND, lp: LPARAM) -> windows::core::BOOL {
        let ctx = &*(lp.0 as *const Ctx);
        if ctx.pids.contains(&(win::window_pid(hwnd) as i32)) {
            let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
        true.into()
    }
    let ctx = Ctx { pids: &pids };
    unsafe {
        let _ = EnumWindows(Some(visit), LPARAM(&ctx as *const Ctx as isize));
    }
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(150));
        if running_pids(bundle_id).is_empty() {
            return true;
        }
    }
    for pid in running_pids(bundle_id) {
        unsafe {
            if let Ok(h) = OpenProcess(PROCESS_TERMINATE, false, pid as u32) {
                let _ = TerminateProcess(h, 0);
                let _ = CloseHandle(h);
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mic_consent_combination() {
        assert_eq!(mic_state(None, None, None), PermissionState::Granted);
        assert_eq!(mic_state(Some("Allow"), Some("Allow"), Some("Allow")), PermissionState::Granted);
        assert_eq!(mic_state(Some("Allow"), Some("Deny"), None), PermissionState::Denied);
        assert_eq!(mic_state(Some("Deny"), None, None), PermissionState::Denied);
        assert_eq!(mic_state(None, None, Some("Deny")), PermissionState::Denied);
    }

    #[test]
    fn process_lookup_finds_ourselves() {
        let me = std::env::current_exe().unwrap();
        let exe = me.file_name().unwrap().to_string_lossy().to_string();
        assert!(running_pids(&exe).contains(&(std::process::id() as i32)));
        assert_eq!(exe_for("now.typeless.desktop"), "Typeless.exe");
        let _ = microphone();
    }
}
