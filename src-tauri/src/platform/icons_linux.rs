//! Linux app icons: the desktop entry's `Icon=` resolved through the icon themes (hicolor first), rendered
//! with gdk-pixbuf so SVG-only apps work too.

use crate::platform::linux;
use gtk::gdk_pixbuf::Pixbuf;
use std::path::{Path, PathBuf};

/// `bundle_id` is the lowercased WM_CLASS (or process name) recorded with the transcript.
pub fn app_icon_png(bundle_id: &str, px: u32) -> Option<Vec<u8>> {
    let icon = linux::desktop_entry(bundle_id).and_then(|e| e.icon).unwrap_or_else(|| bundle_id.to_string());
    let path = find_icon(&icon, px)?;
    let side = px.max(16) as i32;
    let pb = Pixbuf::from_file_at_size(&path, side, side).ok()?;
    pb.save_to_bufferv("png", &[]).ok()
}

/// Score a candidate: an exact or larger bitmap beats a vector, which beats a smaller bitmap.
fn score(size: Option<u32>, px: u32) -> u32 {
    match size {
        Some(s) if s >= px => 10_000 - (s - px).min(9_000),
        None => 5_000, // scalable
        Some(s) => s,
    }
}

fn size_of_dir(name: &str) -> Option<Option<u32>> {
    if name == "scalable" {
        return Some(None);
    }
    let (w, rest) = name.split_once('x')?;
    let h = rest.split('@').next()?;
    let w: u32 = w.parse().ok()?;
    (h.parse::<u32>().ok()? == w).then_some(Some(w))
}

fn find_icon(name: &str, px: u32) -> Option<PathBuf> {
    let p = Path::new(name);
    if p.is_absolute() {
        return p.exists().then(|| p.to_path_buf());
    }
    let files = [format!("{name}.png"), format!("{name}.svg")];
    let mut best: Option<(u32, PathBuf)> = None;
    for data in linux::data_dirs() {
        let icons = data.join("icons");
        let mut themes: Vec<PathBuf> = vec![icons.join("hicolor")];
        if let Ok(rd) = std::fs::read_dir(&icons) {
            let mut others: Vec<PathBuf> =
                rd.flatten().map(|e| e.path()).filter(|t| !t.ends_with("hicolor") && t.is_dir()).collect();
            others.sort();
            themes.extend(others);
        }
        for (i, theme) in themes.iter().enumerate() {
            let Ok(sizes) = std::fs::read_dir(theme) else { continue };
            for size_dir in sizes.flatten() {
                let Some(size) = size_dir.file_name().to_str().and_then(size_of_dir) else { continue };
                for f in &files {
                    let cand = size_dir.path().join("apps").join(f);
                    if cand.exists() {
                        // hicolor is the app's own icon; other themes only break ties.
                        let s = score(size, px) * 2 + u32::from(i == 0);
                        if best.as_ref().is_none_or(|(b, _)| s > *b) {
                            best = Some((s, cand));
                        }
                    }
                }
            }
        }
        if best.is_some() {
            return best.map(|(_, p)| p);
        }
        for f in &files {
            let cand = data.join("pixmaps").join(f);
            if cand.exists() {
                return Some(cand);
            }
        }
    }
    best.map(|(_, p)| p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefers_the_closest_larger_bitmap() {
        assert!(score(Some(64), 48) > score(Some(256), 48));
        assert!(score(Some(48), 48) > score(Some(64), 48));
        assert!(score(Some(256), 48) > score(None, 48));
        assert!(score(None, 48) > score(Some(32), 48));
    }

    #[test]
    fn parses_size_dirs() {
        assert_eq!(size_of_dir("48x48"), Some(Some(48)));
        assert_eq!(size_of_dir("32x32@2"), Some(Some(32)));
        assert_eq!(size_of_dir("scalable"), Some(None));
        assert_eq!(size_of_dir("symbolic"), None);
        assert_eq!(size_of_dir("16x24"), None);
    }
}
