//! Mute other audio while dictating.

/// Restores the previous output mute state when dropped.
pub struct MuteGuard {
    #[allow(dead_code)]
    was_muted: bool,
    #[cfg(target_os = "macos")]
    state: mac_impl::Muted,
    #[cfg(windows)]
    state: win_impl::Muted,
    #[cfg(target_os = "linux")]
    state: linux_impl::Muted,
}

#[cfg(target_os = "macos")]
impl Drop for MuteGuard {
    fn drop(&mut self) {
        mac_impl::restore(&self.state);
    }
}

#[cfg(windows)]
impl Drop for MuteGuard {
    fn drop(&mut self) {
        win_impl::restore(&self.state);
    }
}

#[cfg(target_os = "linux")]
impl Drop for MuteGuard {
    fn drop(&mut self) {
        linux_impl::restore(&self.state);
    }
}

#[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
impl Drop for MuteGuard {
    fn drop(&mut self) {}
}

/// Mute the default output device (only if it is not already muted). Returns None if nothing was changed.
///
/// Uses the device's mute switch when it has one; otherwise sets the virtual main volume to 0.
/// The guard restores exactly what it changed (and only if nobody else changed it in the meantime).
pub fn mute_system_output() -> Option<MuteGuard> {
    #[cfg(target_os = "macos")]
    {
        mac_impl::mute().map(|state| MuteGuard { was_muted: false, state })
    }
    #[cfg(windows)]
    {
        win_impl::mute().map(|state| MuteGuard { was_muted: false, state })
    }
    #[cfg(target_os = "linux")]
    {
        linux_impl::mute().map(|state| MuteGuard { was_muted: false, state })
    }
    #[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
    {
        None
    }
}

#[cfg(target_os = "linux")]
mod linux_impl {
    //! Default sink mute through `pactl`, which talks to PulseAudio and to PipeWire's pulse server alike.
    use std::process::Command;

    pub struct Muted {
        sink: String,
    }

    fn pactl(args: &[&str]) -> Option<String> {
        let out = Command::new("pactl").args(args).output().ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    fn is_muted(sink: &str) -> Option<bool> {
        // "Mute: yes" (the label is localized on some versions, the value is not)
        let out = pactl(&["get-sink-mute", sink])?;
        Some(out.split_whitespace().last()? == "yes")
    }

    pub fn mute() -> Option<Muted> {
        let sink = pactl(&["get-default-sink"]).filter(|s| !s.is_empty())?;
        if is_muted(&sink)? {
            return None; // already muted by the user
        }
        pactl(&["set-sink-mute", &sink, "1"])?;
        log::debug!("muted sink {sink}");
        Some(Muted { sink })
    }

    pub fn restore(state: &Muted) {
        // Only undo our own change, on the sink we muted even if it is no longer the default.
        if is_muted(&state.sink) == Some(true) {
            let _ = pactl(&["set-sink-mute", &state.sink, "0"]);
        }
    }
}

#[cfg(windows)]
mod win_impl {
    //! Default render endpoint mute via IAudioEndpointVolume (what the volume flyout's mute button does).
    use crate::platform::win::ensure_com;
    use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
    use windows::Win32::Media::Audio::{eConsole, eRender, IMMDeviceEnumerator, MMDeviceEnumerator};
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};

    pub struct Muted {
        device_id: String,
    }

    fn default_endpoint() -> Option<(String, IAudioEndpointVolume)> {
        ensure_com();
        unsafe {
            let e: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
            let dev = e.GetDefaultAudioEndpoint(eRender, eConsole).ok()?;
            let id = dev.GetId().ok().map(|p| {
                let s = p.to_string().unwrap_or_default();
                windows::Win32::System::Com::CoTaskMemFree(Some(p.0 as *const _));
                s
            })?;
            let vol: IAudioEndpointVolume = dev.Activate(CLSCTX_ALL, None).ok()?;
            Some((id, vol))
        }
    }

    pub fn mute() -> Option<Muted> {
        let (device_id, vol) = default_endpoint()?;
        unsafe {
            if vol.GetMute().ok()?.as_bool() {
                return None; // already muted by the user
            }
            vol.SetMute(true, std::ptr::null()).ok()?;
        }
        log::debug!("muted output endpoint {device_id}");
        Some(Muted { device_id })
    }

    /// The device we muted, even if it is no longer the default output.
    fn device_volume(id: &str) -> Option<IAudioEndpointVolume> {
        ensure_com();
        unsafe {
            let e: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
            let wid = crate::platform::win::wide(id);
            let dev = e.GetDevice(windows::core::PCWSTR(wid.as_ptr())).ok()?;
            dev.Activate(CLSCTX_ALL, None).ok()
        }
    }

    pub fn restore(state: &Muted) {
        let Some(vol) = device_volume(&state.device_id) else { return };
        unsafe {
            // Only undo our own change.
            if vol.GetMute().is_ok_and(|m| m.as_bool()) {
                let _ = vol.SetMute(false, std::ptr::null());
            }
        }
    }

    #[cfg(test)]
    mod tests {
        /// Briefly mutes the real output device: run with `--ignored`.
        #[test]
        #[ignore = "changes the real system mute for a moment"]
        fn mute_and_restore() {
            if let Some(state) = super::mute() {
                std::thread::sleep(std::time::Duration::from_millis(300));
                super::restore(&state);
                let again = super::mute().expect("mutable again");
                super::restore(&again);
            }
        }
    }
}

#[cfg(target_os = "macos")]
mod mac_impl {
    use std::ffi::c_void;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Address {
        selector: u32,
        scope: u32,
        element: u32,
    }

    type OsStatus = i32;

    #[link(name = "CoreAudio", kind = "framework")]
    extern "C" {
        fn AudioObjectGetPropertyData(
            object: u32,
            address: *const Address,
            qualifier_size: u32,
            qualifier: *const c_void,
            io_size: *mut u32,
            data: *mut c_void,
        ) -> OsStatus;
        fn AudioObjectSetPropertyData(
            object: u32,
            address: *const Address,
            qualifier_size: u32,
            qualifier: *const c_void,
            size: u32,
            data: *const c_void,
        ) -> OsStatus;
        fn AudioObjectIsPropertySettable(object: u32, address: *const Address, out: *mut u8) -> OsStatus;
        fn AudioObjectHasProperty(object: u32, address: *const Address) -> u8;
    }

    const fn fourcc(s: &[u8; 4]) -> u32 {
        ((s[0] as u32) << 24) | ((s[1] as u32) << 16) | ((s[2] as u32) << 8) | s[3] as u32
    }

    const SYSTEM_OBJECT: u32 = 1;
    const SCOPE_GLOBAL: u32 = fourcc(b"glob");
    const SCOPE_OUTPUT: u32 = fourcc(b"outp");
    const SEL_DEFAULT_OUTPUT: u32 = fourcc(b"dOut");
    const SEL_MUTE: u32 = fourcc(b"mute");
    const SEL_VIRTUAL_MAIN_VOLUME: u32 = fourcc(b"vmvc");

    pub enum Muted {
        Mute { device: u32, elements: Vec<u32> },
        Volume { device: u32, previous: f32 },
    }

    fn get<T: Copy + Default>(object: u32, selector: u32, scope: u32, element: u32) -> Option<T> {
        let addr = Address { selector, scope, element };
        let mut value = T::default();
        let mut size = std::mem::size_of::<T>() as u32;
        let status = unsafe {
            AudioObjectGetPropertyData(object, &addr, 0, std::ptr::null(), &mut size, &mut value as *mut T as *mut c_void)
        };
        (status == 0).then_some(value)
    }

    fn set<T: Copy>(object: u32, selector: u32, scope: u32, element: u32, value: T) -> bool {
        let addr = Address { selector, scope, element };
        let status = unsafe {
            AudioObjectSetPropertyData(
                object,
                &addr,
                0,
                std::ptr::null(),
                std::mem::size_of::<T>() as u32,
                &value as *const T as *const c_void,
            )
        };
        status == 0
    }

    fn settable(object: u32, selector: u32, scope: u32, element: u32) -> bool {
        let addr = Address { selector, scope, element };
        if unsafe { AudioObjectHasProperty(object, &addr) } == 0 {
            return false;
        }
        let mut s: u8 = 0;
        unsafe { AudioObjectIsPropertySettable(object, &addr, &mut s) == 0 && s != 0 }
    }

    pub fn mute() -> Option<Muted> {
        let device: u32 = get(SYSTEM_OBJECT, SEL_DEFAULT_OUTPUT, SCOPE_GLOBAL, 0)?;
        if device == 0 {
            return None;
        }

        // 1) Device mute switch: main element first, then individual channels.
        let mut elements = Vec::new();
        if settable(device, SEL_MUTE, SCOPE_OUTPUT, 0) {
            match get::<u32>(device, SEL_MUTE, SCOPE_OUTPUT, 0) {
                Some(0) => elements.push(0),
                Some(_) => return None, // already muted by the user
                None => {}
            }
        } else {
            for ch in 1..=2u32 {
                if settable(device, SEL_MUTE, SCOPE_OUTPUT, ch) && get::<u32>(device, SEL_MUTE, SCOPE_OUTPUT, ch) == Some(0) {
                    elements.push(ch);
                }
            }
        }
        let mut muted = Vec::new();
        for el in elements {
            if set(device, SEL_MUTE, SCOPE_OUTPUT, el, 1u32) {
                muted.push(el);
            }
        }
        if !muted.is_empty() {
            log::debug!("muted output device {device} via mute switch {muted:?}");
            return Some(Muted::Mute { device, elements: muted });
        }

        // 2) No mute switch: drop the virtual main volume to zero.
        if settable(device, SEL_VIRTUAL_MAIN_VOLUME, SCOPE_OUTPUT, 0) {
            let previous: f32 = get(device, SEL_VIRTUAL_MAIN_VOLUME, SCOPE_OUTPUT, 0)?;
            if previous <= 0.001 {
                return None; // already silent
            }
            if set(device, SEL_VIRTUAL_MAIN_VOLUME, SCOPE_OUTPUT, 0, 0.0f32) {
                log::debug!("muted output device {device} via volume (was {previous})");
                return Some(Muted::Volume { device, previous });
            }
        }
        log::debug!("output device {device} offers neither mute nor volume control");
        None
    }

    pub fn restore(state: &Muted) {
        match state {
            Muted::Mute { device, elements } => {
                for el in elements {
                    // Only undo our own change.
                    if get::<u32>(*device, SEL_MUTE, SCOPE_OUTPUT, *el) == Some(1) {
                        set(*device, SEL_MUTE, SCOPE_OUTPUT, *el, 0u32);
                    }
                }
            }
            Muted::Volume { device, previous } => {
                if get::<f32>(*device, SEL_VIRTUAL_MAIN_VOLUME, SCOPE_OUTPUT, 0).is_some_and(|v| v <= 0.001) {
                    set(*device, SEL_VIRTUAL_MAIN_VOLUME, SCOPE_OUTPUT, 0, *previous);
                }
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::fourcc;

        #[test]
        fn fourcc_values() {
            assert_eq!(fourcc(b"glob"), 0x676c6f62);
            assert_eq!(fourcc(b"mute"), 0x6d757465);
        }
    }
}

#[cfg(all(test, target_os = "macos"))]
mod live_tests {
    /// Briefly mutes the real output device: run with `--ignored`.
    #[test]
    #[ignore = "changes the real system volume/mute for a moment"]
    fn mute_and_restore() {
        let before = super::mac_impl::mute();
        eprintln!("mute applied: {}", before.is_some());
        if let Some(state) = before {
            std::thread::sleep(std::time::Duration::from_millis(300));
            super::mac_impl::restore(&state);
            // a second mute must work again after restoring
            let again = super::mac_impl::mute().expect("mutable again");
            super::mac_impl::restore(&again);
        }
    }
}
