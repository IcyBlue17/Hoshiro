//! OS integration layer.
//! Everything OS-specific lives below this module; the rest of the app only uses these public APIs.
//! macOS is implemented now; other OSes get compiling stubs (cfg-gated) so Windows can be added later.

pub mod bar;
mod gesture;
#[cfg(target_os = "macos")]
mod mac;
pub mod context;
pub mod hotkey;
pub mod icons;
pub mod insert;
pub mod media;
pub mod permissions;

pub use context::AppContext;
