//! OS integration layer.
//! Everything OS-specific lives below this module; the rest of the app only uses these public APIs.
//! macOS, Windows and Linux are implemented; other OSes get compiling stubs (cfg-gated).

pub mod bar;
mod gesture;
#[cfg(target_os = "macos")]
mod mac;
#[cfg(windows)]
pub(crate) mod win;
#[cfg(target_os = "linux")]
pub(crate) mod linux;
pub mod context;
pub mod hotkey;
pub mod icons;
pub mod insert;
pub mod media;
pub mod permissions;

pub use context::AppContext;
