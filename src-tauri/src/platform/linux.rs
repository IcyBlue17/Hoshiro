//! Linux helpers shared by the platform modules.
//!
//! - Input goes through the kernel, not the display server, so it works the same on X11 and Wayland:
//!   the hotkey engine reads `/dev/input/event*` (evdev) and keystrokes are injected through a uinput
//!   virtual keyboard. Both need access to those device nodes (see `permissions_linux.rs`).
//! - Window information comes from X11 (`_NET_ACTIVE_WINDOW`). The app runs on the X11 GDK backend (XWayland
//!   under Wayland, see `prefer_x11_backend`), so the floating bar can be positioned and kept on top.
//!   Native Wayland apps are invisible to X11: their context is simply unknown.

use once_cell::sync::{Lazy, OnceCell};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::{Duration, Instant};

// ───────────────────────── evdev key codes ─────────────────────────

pub const KEY_LEFTCTRL: u16 = 29;
pub const KEY_LEFTSHIFT: u16 = 42;
pub const KEY_RIGHTSHIFT: u16 = 54;
pub const KEY_LEFTALT: u16 = 56;
pub const KEY_RIGHTCTRL: u16 = 97;
pub const KEY_RIGHTALT: u16 = 100;
pub const KEY_LEFTMETA: u16 = 125;
pub const KEY_RIGHTMETA: u16 = 126;
pub const KEY_C: u16 = 46;
pub const KEY_V: u16 = 47;
pub const KEY_INSERT: u16 = 110;
/// Tapped while Super/Alt are held after they took part in a shortcut, so their release is not a "tap"
/// (GNOME opens the Activities overview on a lone Super tap, Firefox and KDE apps show the menu bar on Alt).
pub const KEY_MASK: u16 = 194; // KEY_F24

// ───────────────────────── physical modifier state ─────────────────────────

/// Modifiers currently held on the physical keyboards, maintained by the hotkey listener (bit per key).
pub static PHYSICAL_MODS: AtomicU32 = AtomicU32::new(0);
/// The evdev listener is running, so `PHYSICAL_MODS` is meaningful.
pub static LISTENING: AtomicBool = AtomicBool::new(false);

pub fn modifier_bit(code: u16) -> Option<u32> {
    Some(match code {
        KEY_LEFTCTRL => 1 << 0,
        KEY_RIGHTCTRL => 1 << 1,
        KEY_LEFTSHIFT => 1 << 2,
        KEY_RIGHTSHIFT => 1 << 3,
        KEY_LEFTALT => 1 << 4,
        KEY_RIGHTALT => 1 << 5,
        KEY_LEFTMETA => 1 << 6,
        KEY_RIGHTMETA => 1 << 7,
        _ => return None,
    })
}

pub fn is_alt_or_super(code: u16) -> bool {
    matches!(code, KEY_LEFTALT | KEY_RIGHTALT | KEY_LEFTMETA | KEY_RIGHTMETA)
}

/// Injected shortcuts are merged with whatever the user still holds (X11 and Wayland compositors combine all
/// keyboards), so Ctrl+V while Super is down would arrive as Super+Ctrl+V. Wait until the user let go.
pub fn wait_modifiers_released(max: Duration) {
    if !LISTENING.load(Ordering::Acquire) {
        return;
    }
    let deadline = Instant::now() + max;
    while PHYSICAL_MODS.load(Ordering::Acquire) != 0 {
        if Instant::now() >= deadline {
            log::debug!("modifiers still held after {max:?}; injecting anyway");
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

// ───────────────────────── uinput virtual keyboard ─────────────────────────

/// Our virtual keyboard; the hotkey listener skips it so injected keys never look like the user's.
pub const VIRTUAL_KEYBOARD_NAME: &str = "Hoshiro Virtual Keyboard";
/// The display server needs a moment to pick up a new input device; events sent earlier are lost.
const DEVICE_SETTLE: Duration = Duration::from_millis(300);
const KEY_GAP: Duration = Duration::from_millis(6);

static UINPUT: Mutex<Option<evdev::uinput::VirtualDevice>> = Mutex::new(None);

pub fn uinput_writable() -> bool {
    std::fs::OpenOptions::new().write(true).open("/dev/uinput").is_ok()
}

fn create_device() -> anyhow::Result<evdev::uinput::VirtualDevice> {
    use evdev::{AttributeSet, KeyCode};
    let mut keys = AttributeSet::<KeyCode>::new();
    for code in [
        KEY_LEFTCTRL, KEY_LEFTSHIFT, KEY_RIGHTSHIFT, KEY_LEFTALT, KEY_RIGHTCTRL, KEY_RIGHTALT, KEY_LEFTMETA,
        KEY_RIGHTMETA, KEY_C, KEY_V, KEY_INSERT, KEY_MASK,
    ] {
        keys.insert(KeyCode::new(code));
    }
    let dev = evdev::uinput::VirtualDevice::builder()
        .map_err(|e| anyhow::anyhow!("cannot open /dev/uinput: {e}"))?
        .name(VIRTUAL_KEYBOARD_NAME)
        .with_keys(&keys)
        .map_err(|e| anyhow::anyhow!("uinput keys: {e}"))?
        .build()
        .map_err(|e| anyhow::anyhow!("cannot create the virtual keyboard: {e}"))?;
    std::thread::sleep(DEVICE_SETTLE);
    log::info!("uinput virtual keyboard created");
    Ok(dev)
}

/// Create the virtual keyboard ahead of the first paste (creation takes ~300 ms). Never blocks.
pub fn warm_up() {
    if UINPUT.lock().is_some() || !uinput_writable() {
        return;
    }
    let _ = std::thread::Builder::new().name("hoshiro-uinput".into()).spawn(|| {
        let mut slot = UINPUT.lock();
        if slot.is_none() {
            match create_device() {
                Ok(d) => *slot = Some(d),
                Err(e) => log::warn!("{e:#}"),
            }
        }
    });
}

/// Send key transitions `(code, down)` through the virtual keyboard, one report per transition.
pub fn send_keys(seq: &[(u16, bool)]) -> anyhow::Result<()> {
    use evdev::{EventType, InputEvent};
    let mut slot = UINPUT.lock();
    if slot.is_none() {
        *slot = Some(create_device()?);
    }
    let dev = slot.as_mut().expect("created above");
    for (i, &(code, down)) in seq.iter().enumerate() {
        if i > 0 {
            std::thread::sleep(KEY_GAP);
        }
        if let Err(e) = dev.emit(&[InputEvent::new(EventType::KEY.0, code, down as i32)]) {
            // The device may have been removed (e.g. udev reload): start over next time.
            *slot = None;
            anyhow::bail!("uinput write failed: {e}");
        }
    }
    Ok(())
}

/// Press `mods`, tap `key`, release `mods`.
pub fn shortcut(mods: &[u16], key: u16) -> anyhow::Result<()> {
    let mut seq: Vec<(u16, bool)> = mods.iter().map(|m| (*m, true)).collect();
    seq.push((key, true));
    seq.push((key, false));
    seq.extend(mods.iter().rev().map(|m| (*m, false)));
    send_keys(&seq)
}

// ───────────────────────── session / backend ─────────────────────────

pub fn session_is_wayland() -> bool {
    std::env::var("XDG_SESSION_TYPE").is_ok_and(|v| v.eq_ignore_ascii_case("wayland"))
        || std::env::var_os("WAYLAND_DISPLAY").is_some()
}

/// Must run before GTK initializes. Under Wayland the app uses XWayland: a native Wayland client can neither
/// position its windows nor keep them above others, which the floating bar needs. `HOSHIRO_WAYLAND=1` opts out.
pub fn prefer_x11_backend() {
    if std::env::var_os("GDK_BACKEND").is_some() || std::env::var("HOSHIRO_WAYLAND").is_ok_and(|v| v == "1") {
        return;
    }
    if session_is_wayland() && std::env::var_os("DISPLAY").is_some() {
        std::env::set_var("GDK_BACKEND", "x11");
    }
    // WebKitGTK's DMA-BUF renderer shows blank windows on some GPU drivers (notably NVIDIA).
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }
}

pub fn desktop_name() -> String {
    std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default()
}

// ───────────────────────── X11 active window ─────────────────────────

struct Atoms {
    active: u32,
    pid: u32,
    name: u32,
    utf8: u32,
}

struct X {
    conn: x11rb::rust_connection::RustConnection,
    root: u32,
    atoms: Atoms,
}

static X11: OnceCell<Option<X>> = OnceCell::new();

fn x11() -> Option<&'static X> {
    X11.get_or_init(|| {
        use x11rb::connection::Connection;
        use x11rb::protocol::xproto::ConnectionExt;
        let (conn, screen) = match x11rb::connect(None) {
            Ok(c) => c,
            Err(e) => {
                log::info!("no X11 display ({e}); window context unavailable");
                return None;
            }
        };
        let root = conn.setup().roots.get(screen)?.root;
        let atom = |name: &[u8]| -> Option<u32> { Some(conn.intern_atom(false, name).ok()?.reply().ok()?.atom) };
        let atoms = Atoms {
            active: atom(b"_NET_ACTIVE_WINDOW")?,
            pid: atom(b"_NET_WM_PID")?,
            name: atom(b"_NET_WM_NAME")?,
            utf8: atom(b"UTF8_STRING")?,
        };
        Some(X { conn, root, atoms })
    })
    .as_ref()
}

#[derive(Clone, Debug, Default)]
pub struct ActiveWindow {
    pub pid: Option<u32>,
    /// WM_CLASS class part, e.g. "firefox", "Code", "org.gnome.Nautilus".
    pub class: Option<String>,
    pub title: Option<String>,
}

impl ActiveWindow {
    /// Stable app identifier (what macOS calls the bundle id): the lowercased WM_CLASS, else the process name.
    pub fn app_id(&self) -> Option<String> {
        self.class
            .as_deref()
            .filter(|c| !c.is_empty())
            .map(|c| c.to_lowercase())
            .or_else(|| self.pid.and_then(process_name))
    }
}

fn property(x: &X, window: u32, prop: u32, ty: u32) -> Option<x11rb::protocol::xproto::GetPropertyReply> {
    use x11rb::protocol::xproto::ConnectionExt;
    x.conn.get_property(false, window, prop, ty, 0, 4096).ok()?.reply().ok().filter(|r| r.value_len > 0)
}

pub fn active_window() -> Option<ActiveWindow> {
    use x11rb::protocol::xproto::AtomEnum;
    let x = x11()?;
    let id = property(x, x.root, x.atoms.active, AtomEnum::WINDOW.into())?.value32()?.next()?;
    if id == 0 {
        return None; // a native Wayland window (or the desktop) has the focus
    }
    let pid = property(x, id, x.atoms.pid, AtomEnum::CARDINAL.into()).and_then(|r| r.value32()?.next());
    // WM_CLASS = "instance\0class\0"
    let class = property(x, id, AtomEnum::WM_CLASS.into(), AtomEnum::STRING.into()).and_then(|r| {
        let mut parts = r.value.split(|b| *b == 0).map(|p| String::from_utf8_lossy(p).into_owned());
        parts.nth(1).filter(|s| !s.is_empty())
    });
    let title = property(x, id, x.atoms.name, x.atoms.utf8)
        .or_else(|| property(x, id, AtomEnum::WM_NAME.into(), AtomEnum::STRING.into()))
        .map(|r| String::from_utf8_lossy(&r.value).into_owned())
        .filter(|s| !s.is_empty());
    Some(ActiveWindow { pid, class, title })
}

// ───────────────────────── processes ─────────────────────────

pub fn process_name(pid: u32) -> Option<String> {
    let exe = std::fs::read_link(format!("/proc/{pid}/exe")).ok();
    exe.and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .or_else(|| std::fs::read_to_string(format!("/proc/{pid}/comm")).ok().map(|s| s.trim().to_string()))
        .filter(|s| !s.is_empty())
}

/// Pids of the current user's processes whose executable (or comm) is `name` (case-insensitive).
pub fn pids_named(name: &str) -> Vec<i32> {
    let me = unsafe { libc::getuid() };
    let mut out = Vec::new();
    let Ok(dir) = std::fs::read_dir("/proc") else { return out };
    for entry in dir.flatten() {
        let Some(pid) = entry.file_name().to_str().and_then(|s| s.parse::<u32>().ok()) else { continue };
        let owned = std::fs::metadata(entry.path()).is_ok_and(|m| std::os::unix::fs::MetadataExt::uid(&m) == me);
        if owned && process_name(pid).is_some_and(|n| n.eq_ignore_ascii_case(name)) {
            out.push(pid as i32);
        }
    }
    out
}

// ───────────────────────── desktop entries ─────────────────────────

#[derive(Clone, Debug, Default)]
pub struct DesktopEntry {
    pub name: String,
    pub icon: Option<String>,
}

#[derive(Clone, Debug, Default)]
struct ParsedEntry {
    id: String,
    name: Option<String>,
    localized_name: Option<String>,
    icon: Option<String>,
    wm_class: Option<String>,
    exec: Option<String>,
}

pub fn data_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(d) = dirs::data_dir() {
        dirs.push(d);
    }
    let sys = std::env::var("XDG_DATA_DIRS").ok().filter(|s| !s.is_empty());
    for d in sys.as_deref().unwrap_or("/usr/local/share:/usr/share").split(':') {
        dirs.push(PathBuf::from(d));
    }
    if let Some(h) = dirs::home_dir() {
        dirs.push(h.join(".local/share/flatpak/exports/share"));
    }
    dirs.push(PathBuf::from("/var/lib/flatpak/exports/share"));
    dirs.push(PathBuf::from("/var/lib/snapd/desktop"));
    let mut seen = std::collections::HashSet::new();
    dirs.retain(|d| seen.insert(d.clone()));
    dirs
}

fn wants_chinese() -> bool {
    ["LC_ALL", "LC_MESSAGES", "LANG", "LANGUAGE"]
        .iter()
        .filter_map(|v| std::env::var(v).ok().filter(|s| !s.is_empty()))
        .next()
        .is_some_and(|l| l.starts_with("zh"))
}

fn parse_entry(path: &Path, zh: bool) -> Option<ParsedEntry> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut e = ParsedEntry { id: path.file_stem()?.to_string_lossy().into_owned(), ..Default::default() };
    let mut in_main = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_main = line == "[Desktop Entry]";
            continue;
        }
        if !in_main {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else { continue };
        let (k, v) = (k.trim(), v.trim().to_string());
        match k {
            "Name" => e.name = Some(v),
            "Name[zh_CN]" if zh => e.localized_name = Some(v),
            "Name[zh]" if zh && e.localized_name.is_none() => e.localized_name = Some(v),
            "Icon" => e.icon = Some(v),
            "StartupWMClass" => e.wm_class = Some(v),
            "Exec" => e.exec = Some(v),
            _ => {}
        }
    }
    Some(e)
}

/// Parsed desktop entries and when they were read.
type EntryCache = Option<(Instant, Vec<ParsedEntry>)>;
static ENTRIES: Lazy<Mutex<EntryCache>> = Lazy::new(|| Mutex::new(None));

fn all_entries() -> Vec<ParsedEntry> {
    let mut cache = ENTRIES.lock();
    if let Some((t, v)) = cache.as_ref() {
        if t.elapsed() < Duration::from_secs(300) {
            return v.clone();
        }
    }
    let zh = wants_chinese();
    let mut out = Vec::new();
    let mut ids = std::collections::HashSet::new();
    for dir in data_dirs() {
        let Ok(rd) = std::fs::read_dir(dir.join("applications")) else { continue };
        for f in rd.flatten() {
            let p = f.path();
            if p.extension().is_some_and(|e| e == "desktop") {
                if let Some(e) = parse_entry(&p, zh) {
                    if ids.insert(e.id.clone()) {
                        out.push(e); // earlier dirs (the user's) win
                    }
                }
            }
        }
    }
    *cache = Some((Instant::now(), out.clone()));
    out
}

fn exec_basename(exec: &str) -> Option<String> {
    let mut rest = exec.trim();
    loop {
        // First token, honoring a quoted program path.
        let (token, after) = match rest.strip_prefix('"') {
            Some(q) => {
                let end = q.find('"')?;
                (&q[..end], &q[end + 1..])
            }
            None => rest.split_once(char::is_whitespace).unwrap_or((rest, "")),
        };
        // Skip `env` and `VAR=value` prefixes.
        if token == "env" || (token.contains('=') && !token.contains('/')) {
            rest = after.trim_start();
            if rest.is_empty() {
                return None;
            }
            continue;
        }
        return Path::new(token).file_name().map(|n| n.to_string_lossy().to_lowercase());
    }
}

/// Desktop entry for an app id (lowercased WM_CLASS or process name).
pub fn desktop_entry(app_id: &str) -> Option<DesktopEntry> {
    static MEMO: Lazy<Mutex<HashMap<String, Option<DesktopEntry>>>> = Lazy::new(|| Mutex::new(HashMap::new()));
    let key = app_id.to_lowercase();
    if let Some(hit) = MEMO.lock().get(&key) {
        return hit.clone();
    }
    let entries = all_entries();
    let last_segment = |id: &str| id.rsplit('.').next().unwrap_or(id).to_lowercase();
    let found = entries
        .iter()
        .find(|e| e.wm_class.as_deref().is_some_and(|c| c.eq_ignore_ascii_case(&key)))
        .or_else(|| entries.iter().find(|e| e.id.eq_ignore_ascii_case(&key)))
        .or_else(|| entries.iter().find(|e| last_segment(&e.id) == key))
        .or_else(|| entries.iter().find(|e| e.exec.as_deref().and_then(exec_basename).is_some_and(|b| b == key)))
        .and_then(|e| {
            let name = e.localized_name.clone().or_else(|| e.name.clone())?;
            Some(DesktopEntry { name, icon: e.icon.clone() })
        });
    MEMO.lock().insert(key, found.clone());
    found
}

/// Open a URL or file with the desktop's default handler.
pub fn open_url(url: &str) {
    if let Err(e) = std::process::Command::new("xdg-open").arg(url).spawn() {
        log::warn!("failed to open {url}: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exec_basename_skips_env_and_args() {
        assert_eq!(exec_basename("/usr/bin/code --unity-launch %F").as_deref(), Some("code"));
        assert_eq!(exec_basename("env BAMF_DESKTOP_FILE_HINT=x /snap/bin/firefox %u").as_deref(), Some("firefox"));
        assert_eq!(exec_basename("\"/opt/My App/App\" %U").as_deref(), Some("app"));
    }

    #[test]
    fn modifier_bits_are_distinct() {
        let codes = [
            KEY_LEFTCTRL, KEY_RIGHTCTRL, KEY_LEFTSHIFT, KEY_RIGHTSHIFT, KEY_LEFTALT, KEY_RIGHTALT, KEY_LEFTMETA,
            KEY_RIGHTMETA,
        ];
        let mut all = 0;
        for c in codes {
            let b = modifier_bit(c).unwrap();
            assert_eq!(all & b, 0);
            all |= b;
        }
        assert_eq!(modifier_bit(KEY_V), None);
    }

    /// Run with `--ignored` while another app has the focus on an X display.
    #[test]
    #[ignore = "needs an X display with a focused window"]
    fn reads_the_active_window() {
        let w = active_window().expect("an active X window");
        let id = w.app_id().expect("an app id");
        let entry = desktop_entry(&id);
        println!("{w:?} app_id={id} entry={entry:?}");
        assert!(w.pid.is_some());
    }

    #[test]
    fn parses_localized_desktop_entries() {
        let dir = std::env::temp_dir().join(format!("hoshiro-desktop-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("org.example.Editor.desktop");
        std::fs::write(
            &p,
            "[Desktop Entry]\nName=Editor\nName[zh_CN]=编辑器\nIcon=org.example.Editor\nStartupWMClass=editor\n\n[Desktop Action new]\nName=New Window\n",
        )
        .unwrap();
        let e = parse_entry(&p, true).unwrap();
        assert_eq!(e.id, "org.example.Editor");
        assert_eq!(e.localized_name.as_deref(), Some("编辑器"));
        assert_eq!(e.name.as_deref(), Some("Editor"));
        assert_eq!(e.wm_class.as_deref(), Some("editor"));
        assert_eq!(parse_entry(&p, false).unwrap().localized_name, None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
