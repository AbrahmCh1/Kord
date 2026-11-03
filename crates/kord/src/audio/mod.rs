//! Audio devices: choosing the default output/input, volumes, a microphone
//! test and keeping a Bluetooth audio link awake.
//!
//! Only implemented on Windows (Core Audio) so far; elsewhere every call
//! reports that it's unsupported and the UI hides the related options.

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use self::windows::*;

#[cfg(not(windows))]
mod unsupported;
#[cfg(not(windows))]
pub use self::unsupported::*;

/// Progress of a microphone test.
#[derive(Debug, Clone, PartialEq)]
pub enum Phase {
    Recording { seconds_left: u32 },
    Playing,
    Done,
    Failed(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct TestState {
    pub phase: Phase,
    /// Input level, 0–1, while recording.
    pub level: f32,
}

impl TestState {
    pub fn running(&self) -> bool {
        matches!(self.phase, Phase::Recording { .. } | Phase::Playing)
    }
}
