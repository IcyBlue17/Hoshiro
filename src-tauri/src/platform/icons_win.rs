//! Windows app icons: resolve the executable of an app id (`code.exe`), render its shell icon, encode PNG.

use crate::platform::win::{self, ensure_com, wide};
use std::path::{Path, PathBuf};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, SIZE};
use windows::Win32::Graphics::Gdi::{
    DeleteObject, GetDC, GetDIBits, GetObjectW, ReleaseDC, BITMAP, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
    HGDIOBJ,
};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ};
use windows::Win32::UI::Shell::{IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_BIGGERSIZEOK, SIIGBF_ICONONLY};

pub fn app_icon_png(id: &str, px: u32) -> Option<Vec<u8>> {
    if !id.to_ascii_lowercase().ends_with(".exe") {
        return None; // a macOS bundle id (history imported from a Mac)
    }
    let path = resolve(id)?;
    let rgba = shell_icon(&path, px)?;
    encode_png(&rgba, px)
}

fn resolve(id: &str) -> Option<PathBuf> {
    if let Some(p) = win::known_path(id) {
        return Some(p);
    }
    let p = running_path(id).or_else(|| app_paths(id))?;
    win::remember_path(id, &p);
    Some(p)
}

fn running_path(id: &str) -> Option<PathBuf> {
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).ok()?;
        let mut entry = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut found = None;
        if Process32FirstW(snap, &mut entry).is_ok() {
            loop {
                if win::from_wide(&entry.szExeFile).eq_ignore_ascii_case(id) {
                    if let Some(p) = win::process_path(entry.th32ProcessID) {
                        found = Some(p);
                        break;
                    }
                }
                if Process32NextW(snap, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
        found
    }
}

/// `HKxx\SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\<exe>` (default value).
fn app_paths(id: &str) -> Option<PathBuf> {
    let sub = wide(&format!("SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\App Paths\\{id}"));
    for root in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
        let mut buf = vec![0u16; 1024];
        let mut len = (buf.len() * 2) as u32;
        let r = unsafe {
            RegGetValueW(
                root,
                PCWSTR(sub.as_ptr()),
                PCWSTR::null(),
                RRF_RT_REG_SZ,
                None,
                Some(buf.as_mut_ptr() as *mut _),
                Some(&mut len),
            )
        };
        if r.is_ok() {
            let p = PathBuf::from(win::from_wide(&buf).trim_matches('"'));
            if p.exists() {
                return Some(p);
            }
        }
    }
    None
}

/// Straight (non-premultiplied) RGBA, top-down, `px` x `px`.
fn shell_icon(path: &Path, px: u32) -> Option<Vec<u8>> {
    ensure_com();
    unsafe {
        let wpath = wide(&path.to_string_lossy());
        let factory: IShellItemImageFactory = SHCreateItemFromParsingName(PCWSTR(wpath.as_ptr()), None).ok()?;
        let hbmp = factory.GetImage(SIZE { cx: px as i32, cy: px as i32 }, SIIGBF_ICONONLY | SIIGBF_BIGGERSIZEOK).ok()?;
        let obj = HGDIOBJ(hbmp.0);
        let mut bm = BITMAP::default();
        let ok = GetObjectW(obj, std::mem::size_of::<BITMAP>() as i32, Some(&mut bm as *mut _ as *mut _)) != 0;
        let result = (|| {
            if !ok || bm.bmWidth <= 0 || bm.bmHeight <= 0 {
                return None;
            }
            let (w, h) = (bm.bmWidth as u32, bm.bmHeight as u32);
            let mut info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w as i32,
                    biHeight: -(h as i32), // top-down
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bgra = vec![0u8; (w * h * 4) as usize];
            let dc = GetDC(None);
            let lines = GetDIBits(dc, hbmp, 0, h, Some(bgra.as_mut_ptr() as *mut _), &mut info, DIB_RGB_COLORS);
            ReleaseDC(None, dc);
            if lines == 0 {
                return None;
            }
            Some(fit(&bgra_to_rgba(&bgra), w, h, px))
        })();
        let _ = DeleteObject(obj);
        result
    }
}

/// Shell bitmaps are premultiplied BGRA; icons without any alpha are opaque.
fn bgra_to_rgba(bgra: &[u8]) -> Vec<u8> {
    let has_alpha = bgra.chunks_exact(4).any(|p| p[3] != 0);
    let mut rgba = Vec::with_capacity(bgra.len());
    for p in bgra.chunks_exact(4) {
        let (b, g, r) = (p[0], p[1], p[2]);
        if !has_alpha {
            rgba.extend_from_slice(&[r, g, b, 255]);
            continue;
        }
        let a = p[3];
        let un = |c: u8| if a == 0 { 0 } else { ((c as u32 * 255 + a as u32 / 2) / a as u32).min(255) as u8 };
        rgba.extend_from_slice(&[un(r), un(g), un(b), a]);
    }
    rgba
}

/// Center (shrinking by nearest neighbour if needed) the bitmap into a `px` x `px` canvas.
fn fit(rgba: &[u8], w: u32, h: u32, px: u32) -> Vec<u8> {
    if w == px && h == px {
        return rgba.to_vec();
    }
    let mut out = vec![0u8; (px * px * 4) as usize];
    let scale = (w.max(h) as f64 / px as f64).max(1.0);
    let (sw, sh) = (((w as f64 / scale) as u32).min(px), ((h as f64 / scale) as u32).min(px));
    let (ox, oy) = ((px - sw) / 2, (px - sh) / 2);
    for y in 0..sh {
        for x in 0..sw {
            let sx = ((x as f64 * scale) as u32).min(w - 1);
            let sy = ((y as f64 * scale) as u32).min(h - 1);
            let si = ((sy * w + sx) * 4) as usize;
            let di = (((y + oy) * px + x + ox) * 4) as usize;
            out[di..di + 4].copy_from_slice(&rgba[si..si + 4]);
        }
    }
    out
}

fn encode_png(rgba: &[u8], px: u32) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, px, px);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header().ok()?;
        w.write_image_data(rgba).ok()?;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    #[test]
    fn unpremultiply_and_opaque_fallback() {
        assert_eq!(super::bgra_to_rgba(&[10, 20, 30, 0]), vec![30, 20, 10, 255]);
        assert_eq!(super::bgra_to_rgba(&[0, 0, 64, 128, 0, 0, 0, 0]), vec![128, 0, 0, 128, 0, 0, 0, 0]);
    }

    #[test]
    fn fit_centers_small_and_shrinks_large() {
        let small = vec![255u8; 2 * 2 * 4];
        let out = super::fit(&small, 2, 2, 4);
        assert_eq!(out.len(), 64);
        assert_eq!(out[(4 + 1) * 4 + 3], 255);
        assert_eq!(out[3], 0);
        let big = vec![7u8; 8 * 8 * 4];
        assert_eq!(super::fit(&big, 8, 8, 4), vec![7u8; 64]);
    }

    #[test]
    fn icon_of_explorer() {
        let png = super::app_icon_png("explorer.exe", 64).expect("explorer icon");
        assert_eq!(&png[1..4], b"PNG");
        assert!(super::app_icon_png("com.apple.Notes", 64).is_none());
    }
}
