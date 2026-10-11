//! Linux permissions.
//!
//! - Microphone: no OS-level consent for regular desktop apps.
//! - "Input monitoring" = read access to the keyboards in `/dev/input` (hotkeys).
//! - "Accessibility" = write access to `/dev/uinput` (typing the paste shortcut).
//!
//! Both come from a udev rule that tags the devices `uaccess` (systemd-logind then grants them to the user of
//! the active session, no re-login needed). The deb / rpm packages install it; otherwise `request` installs it
//! through pkexec, which shows the desktop's administrator password prompt.

use crate::platform::linux;
use crate::types::PermissionState;
use std::sync::atomic::{AtomicBool, Ordering};

const RULE: &str = include_str!("../../linux/70-hoshiro.rules");
const RULE_PATH: &str = "/etc/udev/rules.d/70-hoshiro.rules";

static ASKED: AtomicBool = AtomicBool::new(false);

fn not_granted() -> PermissionState {
    if ASKED.load(Ordering::Acquire) {
        PermissionState::Denied
    } else {
        PermissionState::NotDetermined
    }
}

/// Can we read at least one keyboard?
pub fn input_monitoring() -> PermissionState {
    const KEY_A: u16 = 30;
    let Ok(rd) = std::fs::read_dir("/dev/input") else { return not_granted() };
    for e in rd.flatten() {
        let p = e.path();
        if !p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("event")) {
            continue;
        }
        if let Ok(dev) = evdev::Device::open(&p) {
            let keyboard = dev.supported_keys().is_some_and(|k| k.contains(evdev::KeyCode::new(KEY_A)));
            if keyboard && dev.name() != Some(linux::VIRTUAL_KEYBOARD_NAME) {
                return PermissionState::Granted;
            }
        }
    }
    not_granted()
}

pub fn uinput() -> PermissionState {
    if linux::uinput_writable() {
        PermissionState::Granted
    } else {
        not_granted()
    }
}

/// Install the udev rule (and load uinput) as root through pkexec. Blocks until the password prompt is answered.
pub fn install_rule() {
    ASKED.store(true, Ordering::Release);
    let script = format!(
        "set -e\n\
         cat > {RULE_PATH} <<'HOSHIRO_RULE'\n{RULE}HOSHIRO_RULE\n\
         echo uinput > /etc/modules-load.d/hoshiro-uinput.conf\n\
         modprobe uinput || true\n\
         udevadm control --reload-rules\n\
         udevadm trigger --subsystem-match=input --subsystem-match=misc --action=change\n\
         udevadm settle --timeout=5 || true\n"
    );
    match std::process::Command::new("pkexec").args(["/bin/sh", "-c", &script]).status() {
        Ok(s) if s.success() => {
            log::info!("installed {RULE_PATH}");
            linux::warm_up();
        }
        Ok(s) => log::warn!("installing the udev rule was cancelled or failed ({s})"),
        Err(e) => log::warn!("pkexec unavailable ({e}); install {RULE_PATH} manually"),
    }
}

pub fn running_pids(name: &str) -> Vec<i32> {
    linux::pids_named(name)
}

/// SIGTERM, then SIGKILL whatever is left after a grace period.
pub fn terminate_app(name: &str) -> bool {
    let pids = running_pids(name);
    if pids.is_empty() {
        return false;
    }
    for pid in &pids {
        unsafe { libc::kill(*pid, libc::SIGTERM) };
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while std::time::Instant::now() < deadline {
        if running_pids(name).is_empty() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    for pid in running_pids(name) {
        unsafe { libc::kill(pid, libc::SIGKILL) };
    }
    true
}

#[cfg(test)]
mod tests {
    #[test]
    fn rule_grants_keyboards_mice_and_uinput() {
        let rule = super::RULE;
        assert!(rule.contains("KERNEL==\"uinput\""));
        assert!(rule.contains("ID_INPUT_KEYBOARD"));
        assert!(rule.contains("ID_INPUT_MOUSE"));
        assert!(!rule.contains("HOSHIRO_RULE"), "would end the heredoc early");
    }
}
