//! Application icons for the usage page.

/// PNG of the app's icon (square, `px` pixels), or None if the app isn't installed.
/// `bundle_id` is the macOS bundle id, or on Windows the executable name (`code.exe`).
pub fn app_icon_png(bundle_id: &str, px: u32) -> Option<Vec<u8>> {
    #[cfg(target_os = "macos")]
    {
        mac_impl::app_icon_png(bundle_id, px)
    }
    #[cfg(windows)]
    {
        win_impl::app_icon_png(bundle_id, px)
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        let _ = (bundle_id, px);
        None
    }
}

#[cfg(target_os = "macos")]
mod mac_impl {
    use objc2::rc::autoreleasepool;
    use objc2::AnyThread;
    use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSWorkspace};
    use objc2_foundation::{NSDictionary, NSPoint, NSRect, NSSize, NSString};

    pub fn app_icon_png(bundle_id: &str, px: u32) -> Option<Vec<u8>> {
        autoreleasepool(|_| {
            let ws = NSWorkspace::sharedWorkspace();
            let url = ws.URLForApplicationWithBundleIdentifier(&NSString::from_str(bundle_id))?;
            let path = url.path()?;
            let image = ws.iconForFile(&path);
            let side = px as f64;
            let mut rect = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(side, side));
            // Picks the icon representation that best matches the requested size.
            let cg = unsafe { image.CGImageForProposedRect_context_hints(&mut rect, None, None) }?;
            let rep = NSBitmapImageRep::initWithCGImage(NSBitmapImageRep::alloc(), &cg);
            let data = unsafe { rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new()) }?;
            Some(data.to_vec())
        })
    }
}

#[cfg(windows)]
#[path = "icons_win.rs"]
mod win_impl;
