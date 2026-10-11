//! Linux text insertion: the text goes through the clipboard and is pasted with a synthetic shortcut typed on
//! our uinput virtual keyboard, then the previous clipboard comes back.
//!
//! The clipboard is the X11 CLIPBOARD selection (the app runs on XWayland under Wayland, and XWayland
//! bridges it to native Wayland apps). Text we write for a paste is flagged so clipboard managers skip it.
//! Terminals paste with Ctrl+Shift+V (Ctrl+V types a literal ^V), the xterm family with Shift+Insert.

use crate::platform::linux::{self, KEY_C, KEY_INSERT, KEY_LEFTCTRL, KEY_LEFTSHIFT, KEY_V};
use arboard::{GetExtLinux, ImageData, LinuxClipboardKind, SetExtLinux};
use parking_lot::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// How long the pasted text stays on the clipboard: the target app reads it asynchronously after Ctrl+V.
const RESTORE_DELAY: Duration = Duration::from_millis(500);
/// Wait at most this long for the user to release the shortcut's modifiers before typing ours.
const MODIFIER_WAIT: Duration = Duration::from_millis(1500);

/// Kept for the whole run: on X11 the owner of a selection has to keep serving it.
static CLIPBOARD: Mutex<Option<arboard::Clipboard>> = Mutex::new(None);

fn with_clipboard<T>(f: impl FnOnce(&mut arboard::Clipboard) -> Result<T, arboard::Error>) -> anyhow::Result<T> {
    let mut slot = CLIPBOARD.lock();
    if slot.is_none() {
        *slot = Some(arboard::Clipboard::new().map_err(|e| anyhow::anyhow!("clipboard unavailable: {e}"))?);
    }
    f(slot.as_mut().expect("created above")).map_err(|e| anyhow::anyhow!("clipboard: {e}"))
}

enum Snapshot {
    Empty,
    Text(String),
    Image(ImageData<'static>),
}

fn snapshot() -> Snapshot {
    if let Ok(t) = with_clipboard(|c| c.get_text()) {
        return Snapshot::Text(t);
    }
    if let Ok(img) = with_clipboard(|c| c.get_image()) {
        return Snapshot::Image(img.to_owned_img());
    }
    Snapshot::Empty
}

fn restore(s: &Snapshot) -> anyhow::Result<()> {
    match s {
        Snapshot::Empty => with_clipboard(|c| c.clear()),
        Snapshot::Text(t) => with_clipboard(|c| c.set_text(t.clone())),
        Snapshot::Image(img) => with_clipboard(|c| c.set_image(img.clone())),
    }
}

fn write_text(text: &str, transient: bool) -> anyhow::Result<()> {
    if transient {
        with_clipboard(|c| c.set().exclude_from_history().text(text.to_string()))
    } else {
        with_clipboard(|c| c.set_text(text.to_string()))
    }
}

fn read_text() -> Option<String> {
    with_clipboard(|c| c.get_text()).ok()
}

/// Taking the selection over is asynchronous, and right after a `clear()` the next write can be lost
/// entirely (the release and the new claim race in the X server). A paste must never send stale or empty
/// data: write until the text reads back.
fn write_confirmed(text: &str, transient: bool) -> anyhow::Result<()> {
    for attempt in 0..5 {
        write_text(text, transient)?;
        let deadline = Instant::now() + Duration::from_millis(50);
        loop {
            if read_text().as_deref() == Some(text) {
                if attempt > 0 {
                    log::debug!("clipboard write took {} attempts", attempt + 1);
                }
                return Ok(());
            }
            if Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    log::warn!("clipboard did not read back our text; pasting anyway");
    Ok(())
}

struct Pending {
    snapshot: Snapshot,
    generation: u64,
    written: String,
}

static PENDING: Mutex<Option<Pending>> = Mutex::new(None);
static GENERATION: AtomicU64 = AtomicU64::new(0);

// ───────────────────────── target app ─────────────────────────

/// WM_CLASS (lowercased) of terminal emulators.
const TERMINALS: &[&str] = &[
    "gnome-terminal-server", "gnome-terminal", "org.gnome.console", "kgx", "org.gnome.ptyxis", "ptyxis", "konsole",
    "org.kde.konsole", "yakuake", "xfce4-terminal", "mate-terminal", "lxterminal", "qterminal", "tilix",
    "terminator", "guake", "terminology", "deepin-terminal", "alacritty", "kitty", "wezterm", "org.wezfurlong.wezterm",
    "wezterm-gui", "foot", "footclient", "ghostty", "com.mitchellh.ghostty", "tabby", "hyper", "warp",
    "dev.warp.warp", "rio", "contour", "blackbox", "com.raggesilver.blackbox", "cool-retro-term", "termite",
    "xterm", "uxterm", "urxvt", "rxvt", "st", "st-256color", "sakura", "lxterm", "eterm",
];
/// Paste with Shift+Insert and read the selection from PRIMARY (no clipboard shortcuts by default).
const XTERM_FAMILY: &[&str] = &["xterm", "uxterm", "lxterm", "urxvt", "rxvt", "st", "st-256color", "eterm"];

struct Target {
    class: Option<String>,
}

fn target() -> Target {
    Target { class: linux::active_window().and_then(|w| w.class).map(|c| c.to_lowercase()) }
}

impl Target {
    fn is_terminal(&self) -> bool {
        self.class.as_deref().is_some_and(|c| TERMINALS.contains(&c))
    }
    fn xterm_family(&self) -> bool {
        self.class.as_deref().is_some_and(|c| XTERM_FAMILY.contains(&c))
    }
}

fn press_paste(t: &Target) -> anyhow::Result<()> {
    if t.xterm_family() {
        linux::shortcut(&[KEY_LEFTSHIFT], KEY_INSERT)
    } else if t.is_terminal() {
        linux::shortcut(&[KEY_LEFTCTRL, KEY_LEFTSHIFT], KEY_V)
    } else {
        linux::shortcut(&[KEY_LEFTCTRL], KEY_V)
    }
}

// ───────────────────────── public API ─────────────────────────

pub fn insert_text(text: &str) -> anyhow::Result<()> {
    if text.is_empty() {
        return Ok(());
    }
    linux::wait_modifiers_released(MODIFIER_WAIT);
    let t = target();
    let generation;
    {
        let mut pending = PENDING.lock();
        // A previous paste may still be waiting to restore: its snapshot is the real clipboard, not what we wrote.
        let snap = match pending.take() {
            Some(p) => p.snapshot,
            None => snapshot(),
        };
        if let Err(e) = write_confirmed(text, true) {
            let _ = restore(&snap);
            return Err(e.context("could not write the text to the clipboard"));
        }
        if t.xterm_family() {
            // Shift+Insert pastes PRIMARY there.
            let _ = with_clipboard(|c| c.set().clipboard(LinuxClipboardKind::Primary).text(text.to_string()));
        }
        generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
        *pending = Some(Pending { snapshot: snap, generation, written: text.to_string() });
    }

    if let Err(e) = press_paste(&t) {
        finish_restore(generation, true);
        return Err(e.context("could not type the paste shortcut (is /dev/uinput accessible?)"));
    }

    std::thread::Builder::new()
        .name("hoshiro-clipboard-restore".into())
        .spawn(move || {
            std::thread::sleep(RESTORE_DELAY);
            finish_restore(generation, false);
        })
        .map_err(|e| anyhow::anyhow!("failed to spawn clipboard restore thread: {e}"))?;
    Ok(())
}

/// Restore the clipboard if `generation` is still the latest paste. `force` skips the "unchanged" check.
fn finish_restore(generation: u64, force: bool) {
    let mut pending = PENDING.lock();
    match pending.as_ref() {
        Some(p) if p.generation == generation => {}
        _ => return, // superseded by a newer paste or a user-initiated copy
    }
    let p = pending.take().expect("checked above");
    if force || read_text().as_deref() == Some(p.written.as_str()) {
        if let Err(e) = restore(&p.snapshot) {
            log::warn!("restoring the clipboard failed: {e:#}");
        }
    } else {
        log::debug!("clipboard changed during paste; leaving the user's newer clipboard alone");
    }
}

pub fn copy_to_clipboard(text: &str) -> anyhow::Result<()> {
    *PENDING.lock() = None; // never let a delayed restore overwrite an explicit copy
    write_text(text, false)
}

/// Copy round trip to read the selection: snapshot, Ctrl+C (Ctrl+Shift+C in terminals, where Ctrl+C would
/// interrupt the running program), read, restore. None if the app copied nothing.
pub(crate) fn copy_selection_via_ctrl_c() -> Option<String> {
    linux::wait_modifiers_released(MODIFIER_WAIT);
    let t = target();
    if t.xterm_family() {
        // Selecting in xterm owns PRIMARY; there is no copy shortcut.
        return with_clipboard(|c| c.get().clipboard(LinuxClipboardKind::Primary).text())
            .ok()
            .filter(|s| !s.trim().is_empty());
    }
    let snap = snapshot();
    let marker = format!("\u{2063}hoshiro-copy-{}", GENERATION.fetch_add(1, Ordering::SeqCst));
    if let Err(e) = write_confirmed(&marker, true) {
        log::warn!("copy fallback: {e:#}");
        return None;
    }
    let keys = if t.is_terminal() {
        linux::shortcut(&[KEY_LEFTCTRL, KEY_LEFTSHIFT], KEY_C)
    } else {
        linux::shortcut(&[KEY_LEFTCTRL], KEY_C)
    };
    let mut text = None;
    if let Err(e) = keys {
        log::warn!("copy fallback failed: {e:#}");
    } else {
        let deadline = Instant::now() + Duration::from_millis(250);
        while Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(15));
            match read_text() {
                Some(s) if s != marker => {
                    text = Some(s);
                    break;
                }
                _ => {}
            }
        }
    }
    if let Err(e) = restore(&snap) {
        log::warn!("restoring the clipboard after copy failed: {e:#}");
    }
    text.filter(|s| !s.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xterm_family_are_terminals() {
        for t in XTERM_FAMILY {
            assert!(TERMINALS.contains(t), "{t}");
        }
        assert!(Target { class: Some("gnome-terminal-server".into()) }.is_terminal());
        assert!(!Target { class: Some("firefox".into()) }.is_terminal());
        assert!(!Target { class: None }.xterm_family());
    }

    /// Needs an X display: run with `--ignored`.
    #[test]
    #[ignore = "uses the real clipboard"]
    fn clipboard_write_read_restore() {
        let before = snapshot();
        write_text("hoshiro 剪贴板测试", true).unwrap();
        assert_eq!(read_text().as_deref(), Some("hoshiro 剪贴板测试"));
        restore(&before).unwrap();
        // Right after a restore (possibly a clear): the confirmed write must read back at once.
        write_confirmed("after restore", true).unwrap();
        assert_eq!(read_text().as_deref(), Some("after restore"));
        copy_to_clipboard("explicit copy").unwrap();
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(read_text().as_deref(), Some("explicit copy"));
        restore(&before).unwrap();
    }

    /// Types into the focused window through /dev/uinput: run with `--ignored` with a text field focused.
    #[test]
    #[ignore = "types into the focused window"]
    fn pastes_into_the_focused_window() {
        insert_text("你好 hoshiro").unwrap();
        std::thread::sleep(Duration::from_millis(800));
    }
}
