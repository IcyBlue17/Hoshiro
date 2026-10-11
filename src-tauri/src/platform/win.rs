//! Small shared Windows helpers: synthetic input, physical modifier tracking, foreground app info, COM.

use parking_lot::Mutex;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Duration;
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM};
use windows::Win32::Storage::FileSystem::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, MapVirtualKeyW, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
    KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, MAPVK_VK_TO_VSC, VIRTUAL_KEY, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN,
    VK_RCONTROL, VK_RMENU, VK_RSHIFT, VK_RWIN,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumChildWindows, GetForegroundWindow, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId,
};

/// Written into `dwExtraInfo` of every input event we inject, so our own hook ignores them.
pub const SYNTH_MAGIC: usize = 0x4853_4852; // "HSHR"

/// An unassigned virtual key. Tapping it while Alt / Win is down stops their release from opening the
/// menu bar / Start menu (the same trick AutoHotkey uses).
pub const VK_MASK: VIRTUAL_KEY = VIRTUAL_KEY(0xE8);

/// The eight modifier keys, in the bit order used by [`PHYSICAL_MODS`] and [`MASK_ON_RELEASE`].
pub const MODIFIERS: [VIRTUAL_KEY; 8] =
    [VK_LSHIFT, VK_RSHIFT, VK_LCONTROL, VK_RCONTROL, VK_LMENU, VK_RMENU, VK_LWIN, VK_RWIN];

/// Modifiers held right now by the user, as seen by the keyboard hook (our own injected events excluded).
pub static PHYSICAL_MODS: AtomicU32 = AtomicU32::new(0);
/// True while the keyboard hook runs, i.e. while [`PHYSICAL_MODS`] is trustworthy.
pub static HOOK_RUNNING: AtomicBool = AtomicBool::new(false);
/// Alt / Win keys whose next release must be masked (they took part in a Hoshiro shortcut, or we re-pressed
/// them after a paste), so the release does not pop up the Start menu or activate a menu bar.
pub static MASK_ON_RELEASE: AtomicU32 = AtomicU32::new(0);

/// True while [`post_shortcut`] injects a key sequence: the keyboard hook then defers physical modifier
/// releases (see [`defer_release`]) so a user letting go of Ctrl mid-sequence cannot turn Ctrl+V into "v".
pub static INJECTING: AtomicBool = AtomicBool::new(false);
static DEFERRED_RELEASES: Mutex<Vec<u16>> = Mutex::new(Vec::new());

/// Called by the keyboard hook for a physical modifier release while [`INJECTING`]: the release is swallowed
/// and replayed when the injection ends. Returns false if nothing is being injected (handle normally).
pub fn defer_release(vk: u16) -> bool {
    if !INJECTING.load(Ordering::Acquire) {
        return false;
    }
    let mut d = DEFERRED_RELEASES.lock();
    if !d.contains(&vk) {
        d.push(vk);
    }
    true
}

/// Ends an injection window and replays the releases that were held back during it.
fn end_injection() {
    INJECTING.store(false, Ordering::Release);
    let deferred: Vec<u16> = std::mem::take(&mut *DEFERRED_RELEASES.lock());
    if deferred.is_empty() {
        return;
    }
    let mut seq = Vec::new();
    for vk in deferred {
        let vk = VIRTUAL_KEY(vk);
        if is_alt_or_win(vk) {
            seq.push(key_input(VK_MASK, false));
            seq.push(key_input(VK_MASK, true));
            if let Some(b) = modifier_bit(vk) {
                MASK_ON_RELEASE.fetch_and(!b, Ordering::AcqRel);
            }
        }
        seq.push(key_input(vk, true));
    }
    if let Err(e) = send_inputs(&seq) {
        log::warn!("replaying deferred modifier releases failed: {e:#}");
    }
}

pub fn modifier_bit(vk: VIRTUAL_KEY) -> Option<u32> {
    MODIFIERS.iter().position(|m| *m == vk).map(|i| 1 << i)
}

pub fn is_alt_or_win(vk: VIRTUAL_KEY) -> bool {
    matches!(vk, VK_LMENU | VK_RMENU | VK_LWIN | VK_RWIN)
}

/// Logical "is down" (includes injected events).
pub fn key_down_now(vk: VIRTUAL_KEY) -> bool {
    unsafe { GetAsyncKeyState(vk.0 as i32) as u16 & 0x8000 != 0 }
}

fn physically_down(vk: VIRTUAL_KEY) -> bool {
    if HOOK_RUNNING.load(Ordering::Acquire) {
        modifier_bit(vk).is_some_and(|b| PHYSICAL_MODS.load(Ordering::Acquire) & b != 0)
    } else {
        key_down_now(vk)
    }
}

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn from_wide(buf: &[u16]) -> String {
    let end = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

fn is_extended(vk: VIRTUAL_KEY) -> bool {
    // Right-hand modifiers, Win keys, navigation cluster, arrows.
    matches!(vk.0, 0xA3 | 0xA5 | 0x5B | 0x5C | 0x5D | 0x21..=0x28 | 0x2D | 0x2E | 0x6F | 0x90)
}

pub fn key_input(vk: VIRTUAL_KEY, up: bool) -> INPUT {
    let mut flags = KEYBD_EVENT_FLAGS(0);
    if up {
        flags |= KEYEVENTF_KEYUP;
    }
    if is_extended(vk) {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    let scan = unsafe { MapVirtualKeyW(vk.0 as u32, MAPVK_VK_TO_VSC) } as u16;
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT { wVk: vk, wScan: scan, dwFlags: flags, time: 0, dwExtraInfo: SYNTH_MAGIC },
        },
    }
}

pub fn send_inputs(inputs: &[INPUT]) -> anyhow::Result<()> {
    if inputs.is_empty() {
        return Ok(());
    }
    let sent = unsafe { SendInput(inputs, std::mem::size_of::<INPUT>() as i32) };
    if sent as usize != inputs.len() {
        anyhow::bail!(
            "SendInput injected {sent}/{} events (blocked by UIPI? the target app may run as administrator)",
            inputs.len()
        );
    }
    Ok(())
}

/// Press `key` with exactly `mods` held (e.g. Ctrl+V), no matter which modifiers the user is holding:
/// other held modifiers are lifted for the duration and pressed again afterwards if they are still
/// physically down. Must not be called from inside the keyboard hook.
pub fn post_shortcut(mods: &[VIRTUAL_KEY], key: VIRTUAL_KEY) -> anyhow::Result<()> {
    static SERIAL: Mutex<()> = Mutex::new(());
    let _one_at_a_time = SERIAL.lock();
    INJECTING.store(true, Ordering::Release);
    let r = post_shortcut_inner(mods, key);
    end_injection();
    r
}

fn post_shortcut_inner(mods: &[VIRTUAL_KEY], key: VIRTUAL_KEY) -> anyhow::Result<()> {
    let lifted: Vec<VIRTUAL_KEY> = MODIFIERS.iter().copied().filter(|m| !mods.contains(m) && key_down_now(*m)).collect();
    let mut first = Vec::with_capacity(16);
    if lifted.iter().any(|m| is_alt_or_win(*m)) {
        first.push(key_input(VK_MASK, false));
        first.push(key_input(VK_MASK, true));
    }
    for m in &lifted {
        first.push(key_input(*m, true));
    }
    let mut pressed = Vec::new();
    for m in mods {
        if !key_down_now(*m) {
            first.push(key_input(*m, false));
            pressed.push(*m);
        }
    }
    first.push(key_input(key, false));
    send_inputs(&first)?;
    std::thread::sleep(Duration::from_millis(8));

    let mut second = vec![key_input(key, true)];
    for m in pressed.iter().rev() {
        second.push(key_input(*m, true));
    }
    for m in &lifted {
        let released_meanwhile = DEFERRED_RELEASES.lock().contains(&m.0);
        if physically_down(*m) && !released_meanwhile {
            second.push(key_input(*m, false));
            if is_alt_or_win(*m) {
                if let Some(b) = modifier_bit(*m) {
                    MASK_ON_RELEASE.fetch_or(b, Ordering::AcqRel);
                }
            }
        }
    }
    send_inputs(&second)
}

/// Initialize COM (multithreaded apartment) on the current thread. Cheap after the first call.
pub fn ensure_com() {
    thread_local!(static DONE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) });
    DONE.with(|d| {
        if !d.get() {
            // S_FALSE (already initialized) and RPC_E_CHANGED_MODE (an STA thread) both leave COM usable.
            let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
            d.set(true);
        }
    });
}

// ───────────────────────── processes & windows ─────────────────────────

pub fn process_path(pid: u32) -> Option<PathBuf> {
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = vec![0u16; 1024];
        let mut len = buf.len() as u32;
        let r = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len);
        let _ = CloseHandle(h);
        r.ok()?;
        Some(PathBuf::from(String::from_utf16_lossy(&buf[..len as usize])))
    }
}

/// Parent process id (Toolhelp snapshot), if the process still exists.
pub fn parent_pid(pid: u32) -> Option<u32> {
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
    };
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).ok()?;
        let mut e = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut found = None;
        if Process32FirstW(snap, &mut e).is_ok() {
            loop {
                if e.th32ProcessID == pid {
                    found = Some(e.th32ParentProcessID).filter(|p| *p != 0 && *p != pid);
                    break;
                }
                if Process32NextW(snap, &mut e).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
        found
    }
}

/// Is `pid` the process `ancestor` or one of its (grand)children? WebView2 / Chromium / Electron hosts keep
/// their focused content in child processes.
pub fn is_same_or_descendant(pid: u32, ancestor: u32) -> bool {
    let mut cur = pid;
    for _ in 0..4 {
        if cur == ancestor {
            return true;
        }
        match parent_pid(cur) {
            Some(p) => cur = p,
            None => return false,
        }
    }
    false
}

pub fn window_pid(hwnd: HWND) -> u32 {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    pid
}

pub fn window_title(hwnd: HWND) -> Option<String> {
    unsafe {
        let len = GetWindowTextLengthW(hwnd);
        if len <= 0 {
            return None;
        }
        let mut buf = vec![0u16; len as usize + 1];
        let n = GetWindowTextW(hwnd, &mut buf);
        Some(String::from_utf16_lossy(&buf[..n.max(0) as usize])).filter(|s| !s.trim().is_empty())
    }
}

/// Stable per-app identifier stored in history / usage rows (the role of the macOS bundle id): the
/// lowercased executable file name, e.g. `code.exe`. Version-specific install folders (Squirrel apps such
/// as Slack and Discord live in `app-1.2.3\`) therefore do not split an app's statistics.
pub fn app_id_for_path(path: &Path) -> Option<String> {
    path.file_name().map(|n| n.to_string_lossy().to_lowercase())
}

/// Last seen full path for each app id (to find the icon of an app that is not running), persisted in the
/// data dir so the usage page keeps its icons across restarts.
static KNOWN_PATHS: Mutex<Option<HashMap<String, PathBuf>>> = Mutex::new(None);

fn known_paths_file() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("cn.icybit.hoshiro").join("app-paths.json"))
}

fn with_known_paths<T>(f: impl FnOnce(&mut HashMap<String, PathBuf>) -> T) -> T {
    let mut guard = KNOWN_PATHS.lock();
    let map = guard.get_or_insert_with(|| {
        known_paths_file()
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    });
    f(map)
}

pub fn remember_path(id: &str, path: &Path) {
    let changed = with_known_paths(|m| {
        if m.get(id).map(PathBuf::as_path) == Some(path) {
            return None;
        }
        m.insert(id.to_string(), path.to_path_buf());
        serde_json::to_vec(m).ok()
    });
    if let (Some(json), Some(file)) = (changed, known_paths_file()) {
        std::thread::spawn(move || {
            if let Err(e) = std::fs::write(&file, json) {
                log::debug!("cannot save {}: {e}", file.display());
            }
        });
    }
}

pub fn known_path(id: &str) -> Option<PathBuf> {
    with_known_paths(|m| m.get(id).cloned()).filter(|p| p.exists())
}

pub struct Foreground {
    pub hwnd: HWND,
    pub pid: u32,
    pub path: Option<PathBuf>,
}

/// The foreground window and the process that really owns its content. UWP / packaged apps are hosted by
/// ApplicationFrameHost.exe, whose frame window contains a CoreWindow owned by the actual app.
pub fn foreground() -> Option<Foreground> {
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.0.is_null() {
        return None;
    }
    let pid = window_pid(hwnd);
    let path = process_path(pid);
    let is_frame_host = path
        .as_deref()
        .and_then(|p| p.file_name())
        .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case("ApplicationFrameHost.exe"));
    if is_frame_host {
        if let Some(child_pid) = hosted_app_pid(hwnd, pid) {
            return Some(Foreground { hwnd, pid: child_pid, path: process_path(child_pid) });
        }
    }
    Some(Foreground { hwnd, pid, path })
}

fn hosted_app_pid(frame: HWND, frame_pid: u32) -> Option<u32> {
    struct Search {
        frame_pid: u32,
        found: u32,
    }
    unsafe extern "system" fn visit(child: HWND, lp: LPARAM) -> windows::core::BOOL {
        let s = &mut *(lp.0 as *mut Search);
        let pid = window_pid(child);
        if pid != 0 && pid != s.frame_pid {
            s.found = pid;
            return false.into();
        }
        true.into()
    }
    let mut s = Search { frame_pid, found: 0 };
    unsafe {
        let _ = EnumChildWindows(Some(frame), Some(visit), LPARAM(&mut s as *mut Search as isize));
    }
    (s.found != 0).then_some(s.found)
}

static DESCRIPTIONS: Mutex<Option<HashMap<PathBuf, Option<String>>>> = Mutex::new(None);

/// Human app name: the executable's FileDescription ("Google Chrome", "Visual Studio Code"), falling back
/// to the ProductName and then the file stem.
pub fn app_display_name(path: &Path) -> String {
    if let Some(hit) = DESCRIPTIONS.lock().as_ref().and_then(|m| m.get(path).cloned()) {
        return hit.unwrap_or_else(|| file_stem(path));
    }
    let found = version_string(path, "FileDescription")
        .filter(|s| !generic_description(s))
        .or_else(|| version_string(path, "ProductName"));
    DESCRIPTIONS.lock().get_or_insert_with(HashMap::new).insert(path.to_path_buf(), found.clone());
    found.unwrap_or_else(|| file_stem(path))
}

fn file_stem(path: &Path) -> String {
    path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Some apps ship a useless description ("Electron", "Application").
fn generic_description(s: &str) -> bool {
    let t = s.trim().to_ascii_lowercase();
    t.is_empty() || t == "electron" || t == "application" || t.ends_with(".exe")
}

fn version_string(path: &Path, field: &str) -> Option<String> {
    unsafe {
        let wpath = wide(&path.to_string_lossy());
        let size = GetFileVersionInfoSizeW(PCWSTR(wpath.as_ptr()), None);
        if size == 0 {
            return None;
        }
        let mut data = vec![0u8; size as usize];
        GetFileVersionInfoW(PCWSTR(wpath.as_ptr()), None, size, data.as_mut_ptr() as *mut _).ok()?;

        let mut ptr: *mut std::ffi::c_void = std::ptr::null_mut();
        let mut len = 0u32;
        let mut langs: Vec<(u16, u16)> = Vec::new();
        let q = wide("\\VarFileInfo\\Translation");
        if VerQueryValueW(data.as_ptr() as *const _, PCWSTR(q.as_ptr()), &mut ptr, &mut len).as_bool() && !ptr.is_null() {
            let pairs = std::slice::from_raw_parts(ptr as *const u16, (len / 2) as usize);
            langs.extend(pairs.chunks_exact(2).map(|c| (c[0], c[1])));
        }
        // Prefer the user's UI language implicitly by order, then the common English / neutral fallbacks.
        langs.extend([(0x0409, 0x04B0), (0x0409, 0x04E4), (0x0000, 0x04B0)]);
        for (lang, cp) in langs {
            let q = wide(&format!("\\StringFileInfo\\{lang:04x}{cp:04x}\\{field}"));
            let mut p: *mut std::ffi::c_void = std::ptr::null_mut();
            let mut l = 0u32;
            if VerQueryValueW(data.as_ptr() as *const _, PCWSTR(q.as_ptr()), &mut p, &mut l).as_bool() && !p.is_null() && l > 1 {
                let s = from_wide(std::slice::from_raw_parts(p as *const u16, l as usize));
                let s = s.trim().to_string();
                if !s.is_empty() {
                    return Some(s);
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wide_roundtrip() {
        assert_eq!(from_wide(&wide("Hoshiro 语音")), "Hoshiro 语音");
    }

    #[test]
    fn process_ancestry() {
        let me = std::process::id();
        assert!(is_same_or_descendant(me, me));
        let parent = parent_pid(me).expect("test runner has a parent");
        assert!(is_same_or_descendant(me, parent));
        assert!(!is_same_or_descendant(parent, me));
    }

    #[test]
    fn modifier_bits_are_distinct() {
        let bits: Vec<u32> = MODIFIERS.iter().map(|m| modifier_bit(*m).unwrap()).collect();
        for (i, b) in bits.iter().enumerate() {
            assert!(!bits[i + 1..].contains(b));
        }
    }

    #[test]
    fn display_name_of_a_system_binary() {
        let notepad = PathBuf::from(std::env::var("SystemRoot").unwrap_or("C:\\Windows".into())).join("System32\\notepad.exe");
        if notepad.exists() {
            assert!(!app_display_name(&notepad).is_empty());
        }
        assert_eq!(app_id_for_path(Path::new("C:\\X\\app-1.2\\Slack.exe")).as_deref(), Some("slack.exe"));
    }
}
