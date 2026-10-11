//! Windows clipboard + synthetic paste.
//!
//! Insert = snapshot every clipboard format, put the text on the clipboard (flagged so Windows clipboard
//! history, cloud clipboard and clipboard managers skip it), press Ctrl+V, restore the snapshot ~500 ms later
//! unless the clipboard changed meanwhile (GetClipboardSequenceNumber).
//!
//! Terminals need care: Ctrl+C there means "interrupt", and some (PuTTY, mintty ...) paste with Shift+Insert
//! only, so those get the classic Ctrl+Insert / Shift+Insert pair.

use crate::platform::win::{self, post_shortcut, wide};
use parking_lot::Mutex;
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL, HWND};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, EnumClipboardFormats, GetClipboardData, GetClipboardSequenceNumber,
    IsClipboardFormatAvailable, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::Ole::{CF_LOCALE, CF_OEMTEXT, CF_TEXT, CF_UNICODETEXT};
use windows::Win32::UI::Input::KeyboardAndMouse::{VK_C, VK_INSERT, VK_LCONTROL, VK_LSHIFT, VK_V};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DispatchMessageW, GetClassNameW, GetMessageW, TranslateMessage, HWND_MESSAGE, MSG,
    WINDOW_EX_STYLE, WINDOW_STYLE,
};

const RESTORE_DELAY: Duration = Duration::from_millis(500);
/// Refuse to snapshot absurdly large individual formats (would stall the paste).
const MAX_FORMAT_BYTES: usize = 64 * 1024 * 1024;
/// Delay-rendered formats (Excel, Word ...) are produced on demand by their owner and can be slow.
const SNAPSHOT_BUDGET: Duration = Duration::from_millis(400);

/// Everything that was on the clipboard: (format, bytes), in the original enumeration order.
#[derive(Default)]
pub(crate) struct Snapshot {
    formats: Vec<(u32, Vec<u8>)>,
}

struct Pending {
    snapshot: Snapshot,
    generation: u64,
    expected_seq: u32,
}

static PENDING: Mutex<Option<Pending>> = Mutex::new(None);
static GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

// ───────────────────────── clipboard plumbing ─────────────────────────

/// Clipboard owner: a message-only window on its own thread. The owner must pump messages, otherwise any
/// other app that empties the clipboard hangs on WM_DESTROYCLIPBOARD.
fn owner_window() -> Option<HWND> {
    static OWNER: OnceLock<Option<usize>> = OnceLock::new();
    let raw = *OWNER.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::channel::<Option<usize>>();
        let spawned = std::thread::Builder::new().name("hoshiro-clipboard-owner".into()).spawn(move || unsafe {
            let class = wide("STATIC");
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                PCWSTR(class.as_ptr()),
                PCWSTR::null(),
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                None,
                None,
            );
            match hwnd {
                Ok(h) => {
                    let _ = tx.send(Some(h.0 as usize));
                    let mut msg = MSG::default();
                    while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
                        let _ = TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                }
                Err(e) => {
                    log::warn!("cannot create the clipboard owner window: {e}");
                    let _ = tx.send(None);
                }
            }
        });
        if spawned.is_err() {
            return None;
        }
        rx.recv_timeout(Duration::from_secs(2)).ok().flatten()
    });
    raw.map(|h| HWND(h as *mut _))
}

/// RAII guard for OpenClipboard/CloseClipboard. Retries for a while: other apps hold the clipboard briefly.
struct Clipboard;

impl Clipboard {
    fn open() -> anyhow::Result<Self> {
        let owner = owner_window();
        let deadline = Instant::now() + Duration::from_millis(500);
        loop {
            if unsafe { OpenClipboard(owner) }.is_ok() {
                return Ok(Self);
            }
            if Instant::now() > deadline {
                anyhow::bail!("the clipboard is locked by another application");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for Clipboard {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseClipboard();
        }
    }
}

fn registered(name: &str) -> u32 {
    let w = wide(name);
    unsafe { RegisterClipboardFormatW(PCWSTR(w.as_ptr())) }
}

/// Formats whose handle is not global memory (GDI objects, private handles) or that Windows synthesizes.
fn skip_format(fmt: u32, has_unicode: bool) -> bool {
    const CF_BITMAP: u32 = 2;
    const CF_METAFILEPICT: u32 = 3;
    const CF_PALETTE: u32 = 9;
    const CF_ENHMETAFILE: u32 = 14;
    const CF_OWNERDISPLAY: u32 = 0x80;
    const CF_DSPBITMAP: u32 = 0x82;
    const CF_DSPMETAFILEPICT: u32 = 0x83;
    const CF_DSPENHMETAFILE: u32 = 0x8E;
    matches!(
        fmt,
        CF_BITMAP | CF_METAFILEPICT | CF_PALETTE | CF_ENHMETAFILE | CF_OWNERDISPLAY | CF_DSPBITMAP | CF_DSPMETAFILEPICT
            | CF_DSPENHMETAFILE
    ) || (0x200..=0x3FF).contains(&fmt) // CF_PRIVATEFIRST..CF_GDIOBJLAST
        || (has_unicode && (fmt == CF_TEXT.0 as u32 || fmt == CF_OEMTEXT.0 as u32 || fmt == CF_LOCALE.0 as u32))
}

unsafe fn read_global(h: HANDLE) -> Option<Vec<u8>> {
    let g = HGLOBAL(h.0);
    let size = GlobalSize(g);
    if size == 0 || size > MAX_FORMAT_BYTES {
        return None;
    }
    let p = GlobalLock(g);
    if p.is_null() {
        return None;
    }
    let bytes = std::slice::from_raw_parts(p as *const u8, size).to_vec();
    let _ = GlobalUnlock(g);
    Some(bytes)
}

/// Clipboard must be open.
unsafe fn set_bytes(fmt: u32, bytes: &[u8]) -> bool {
    let Ok(g) = GlobalAlloc(GMEM_MOVEABLE, bytes.len().max(1)) else {
        return false;
    };
    let p = GlobalLock(g);
    if p.is_null() {
        let _ = GlobalFree(Some(g));
        return false;
    }
    std::ptr::copy_nonoverlapping(bytes.as_ptr(), p as *mut u8, bytes.len());
    let _ = GlobalUnlock(g);
    if SetClipboardData(fmt, Some(HANDLE(g.0))).is_err() {
        let _ = GlobalFree(Some(g));
        return false;
    }
    true // the system owns the memory now
}

fn utf16_bytes(text: &str) -> Vec<u8> {
    text.encode_utf16().chain(std::iter::once(0)).flat_map(|u| u.to_le_bytes()).collect()
}

pub(crate) fn snapshot() -> Snapshot {
    let mut snap = Snapshot::default();
    let Ok(_cb) = Clipboard::open() else {
        log::warn!("clipboard snapshot skipped: clipboard busy");
        return snap;
    };
    let start = Instant::now();
    unsafe {
        let has_unicode = IsClipboardFormatAvailable(CF_UNICODETEXT.0 as u32).is_ok();
        let mut fmt = 0u32;
        loop {
            fmt = EnumClipboardFormats(fmt);
            if fmt == 0 {
                break;
            }
            if skip_format(fmt, has_unicode) {
                continue;
            }
            // Plain text first and always; the rest only within the time budget.
            if fmt != CF_UNICODETEXT.0 as u32 && start.elapsed() > SNAPSHOT_BUDGET {
                log::debug!("clipboard snapshot over budget; format {fmt} skipped");
                continue;
            }
            if let Ok(h) = GetClipboardData(fmt) {
                if let Some(bytes) = read_global(h) {
                    snap.formats.push((fmt, bytes));
                }
            }
        }
    }
    snap
}

pub(crate) fn restore(snap: &Snapshot) -> anyhow::Result<()> {
    let _cb = Clipboard::open()?;
    unsafe {
        EmptyClipboard()?;
        for (fmt, bytes) in &snap.formats {
            if !set_bytes(*fmt, bytes) {
                log::debug!("could not restore clipboard format {fmt}");
            }
        }
        if !snap.formats.is_empty() {
            // The item is already in clipboard history from when the user copied it; don't add it twice.
            mark_transient(snap);
        }
    }
    Ok(())
}

/// Clipboard must be open. Flags the current content so clipboard history, cloud clipboard and clipboard
/// managers ignore it (formats already present in `existing` are left alone).
unsafe fn mark_transient(existing: &Snapshot) {
    let zero = 0u32.to_le_bytes();
    for name in [
        "ExcludeClipboardContentFromMonitorProcessing",
        "CanIncludeInClipboardHistory",
        "CanUploadToCloudClipboard",
        "Clipboard Viewer Ignore",
    ] {
        let fmt = registered(name);
        if fmt != 0 && !existing.formats.iter().any(|(f, _)| *f == fmt) {
            set_bytes(fmt, &zero);
        }
    }
}

/// Put `text` on the clipboard. `transient` marks it so clipboard history / cloud sync / managers skip it.
fn write_text(text: &str, transient: bool) -> anyhow::Result<()> {
    let _cb = Clipboard::open()?;
    unsafe {
        EmptyClipboard()?;
        if !set_bytes(CF_UNICODETEXT.0 as u32, &utf16_bytes(text)) {
            anyhow::bail!("SetClipboardData failed");
        }
        if transient {
            mark_transient(&Snapshot::default());
        }
    }
    Ok(())
}

fn read_text() -> Option<String> {
    let _cb = Clipboard::open().ok()?;
    unsafe {
        let h = GetClipboardData(CF_UNICODETEXT.0 as u32).ok()?;
        let bytes = read_global(h)?;
        let units: Vec<u16> = bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        Some(win::from_wide(&units)).filter(|s| !s.is_empty())
    }
}

fn sequence() -> u32 {
    unsafe { GetClipboardSequenceNumber() }
}

// ───────────────────────── terminals ─────────────────────────

/// Apps where Ctrl+C interrupts and/or Ctrl+V is not paste.
const TERMINALS: &[&str] = &[
    "windowsterminal.exe", "cmd.exe", "powershell.exe", "pwsh.exe", "conhost.exe", "openconsole.exe", "wsl.exe",
    "wezterm-gui.exe", "alacritty.exe", "mintty.exe", "putty.exe", "kitty.exe", "tabby.exe", "hyper.exe",
    "mobaxterm.exe", "xshell.exe", "securecrt.exe", "termius.exe", "cmder.exe", "conemu64.exe", "conemu.exe",
    "fluentterminal.app.exe", "warp.exe", "rio.exe", "ghostty.exe",
];
/// Terminals that only paste with Shift+Insert (Ctrl+V types a literal ^V).
const SHIFT_INSERT_PASTE: &[&str] = &["putty.exe", "mintty.exe", "kitty.exe", "mobaxterm.exe", "securecrt.exe", "xshell.exe"];

struct Target {
    exe: Option<String>,
    console: bool,
}

fn target() -> Target {
    let Some(fg) = win::foreground() else {
        return Target { exe: None, console: false };
    };
    let exe = fg.path.as_deref().and_then(win::app_id_for_path);
    let mut class = [0u16; 64];
    let n = unsafe { GetClassNameW(fg.hwnd, &mut class) };
    let console = n > 0 && win::from_wide(&class[..n as usize]) == "ConsoleWindowClass";
    Target { exe, console }
}

impl Target {
    fn is_terminal(&self) -> bool {
        self.console || self.exe.as_deref().is_some_and(|e| TERMINALS.contains(&e))
    }
    fn shift_insert_paste(&self) -> bool {
        self.exe.as_deref().is_some_and(|e| SHIFT_INSERT_PASTE.contains(&e))
    }
}

fn press_paste() -> anyhow::Result<()> {
    if target().shift_insert_paste() {
        post_shortcut(&[VK_LSHIFT], VK_INSERT)
    } else {
        // VK_V is right on every layout: Windows apps match shortcuts by virtual key, and layouts map their
        // "v" key to VK_V (non-Latin layouts keep the US virtual keys).
        post_shortcut(&[VK_LCONTROL], VK_V)
    }
}

// ───────────────────────── public API ─────────────────────────

pub fn insert_text(text: &str) -> anyhow::Result<()> {
    if text.is_empty() {
        return Ok(());
    }
    let generation;
    {
        let mut pending = PENDING.lock();
        // A previous paste may still be waiting to restore: its snapshot is the real clipboard, not what we wrote.
        let snap = match pending.take() {
            Some(p) => p.snapshot,
            None => snapshot(),
        };
        if let Err(e) = write_text(text, true) {
            let _ = restore(&snap);
            return Err(e.context("could not write the text to the clipboard"));
        }
        generation = GENERATION.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
        *pending = Some(Pending { snapshot: snap, generation, expected_seq: sequence() });
    }

    if let Err(e) = press_paste() {
        finish_restore(generation, true);
        return Err(e);
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

/// Restore the clipboard if `generation` is still the latest paste. `force` skips the sequence check.
fn finish_restore(generation: u64, force: bool) {
    let mut pending = PENDING.lock();
    match pending.as_ref() {
        Some(p) if p.generation == generation => {}
        _ => return, // superseded by a newer paste or a user-initiated copy
    }
    let p = pending.take().expect("checked above");
    if force || sequence() == p.expected_seq {
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

/// Copy round trip to read the selection: snapshot, Ctrl+C (Ctrl+Insert in terminals, where Ctrl+C would
/// interrupt the running program), read, restore. None if the app copied nothing.
pub(crate) fn copy_selection_via_ctrl_c() -> Option<String> {
    let t = target();
    let snap = snapshot();
    let before = sequence();
    let r = if t.is_terminal() { post_shortcut(&[VK_LCONTROL], VK_INSERT) } else { post_shortcut(&[VK_LCONTROL], VK_C) };
    if let Err(e) = r {
        log::warn!("copy fallback failed: {e:#}");
        return None;
    }
    let deadline = Instant::now() + Duration::from_millis(180);
    let mut changed = false;
    while Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(8));
        if sequence() != before {
            changed = true;
            break;
        }
    }
    if !changed {
        return None;
    }
    // Give the app a moment to finish writing all formats.
    std::thread::sleep(Duration::from_millis(10));
    let text = read_text();
    if let Err(e) = restore(&snap) {
        log::warn!("restoring the clipboard after copy failed: {e:#}");
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_payload_is_nul_terminated() {
        let b = utf16_bytes("a语");
        assert_eq!(b, vec![b'a', 0, 0xED, 0x8B, 0, 0]);
    }

    #[test]
    fn gdi_and_synthesized_formats_are_skipped() {
        assert!(skip_format(2, false)); // CF_BITMAP
        assert!(skip_format(0x300, false));
        assert!(skip_format(CF_TEXT.0 as u32, true));
        assert!(!skip_format(CF_TEXT.0 as u32, false));
        assert!(!skip_format(CF_UNICODETEXT.0 as u32, true));
        assert!(!skip_format(8, true)); // CF_DIB
        assert!(!skip_format(0xC123, true)); // registered formats
    }

    /// Touches the real clipboard (and restores it): run with `--ignored`.
    #[test]
    #[ignore = "touches the real clipboard"]
    fn snapshot_write_restore_roundtrip() {
        let original = "hoshiro-test-original 原文";
        copy_to_clipboard(original).unwrap();
        let snap = snapshot();
        copy_to_clipboard("something else").unwrap();
        assert_eq!(read_text().as_deref(), Some("something else"));
        restore(&snap).unwrap();
        assert_eq!(read_text().as_deref(), Some(original));
    }
}
