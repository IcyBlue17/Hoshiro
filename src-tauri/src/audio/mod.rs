//! Audio capture, encoding and UI sounds.

pub mod recorder;
pub mod resample;
pub mod sounds;
pub mod wav;

pub use recorder::{AudioChunk, Recorder, RecorderSink, StopResult};

/// Sample rate of everything handed to the rest of the app.
pub const TARGET_SAMPLE_RATE: u32 = 16_000;
