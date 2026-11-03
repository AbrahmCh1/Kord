use anyhow::{Result, bail};

use super::TestState;

pub const SUPPORTED: bool = false;

pub fn default_output() -> Result<Option<String>> {
    Ok(None)
}

pub fn default_input() -> Result<Option<String>> {
    Ok(None)
}

pub fn other_output(_exclude: &[String]) -> Result<Option<String>> {
    Ok(None)
}

pub fn set_default(_id: &str) -> Result<()> {
    bail!("not supported on this platform")
}

pub fn volume(_id: &str) -> Result<u8> {
    bail!("not supported on this platform")
}

pub fn set_volume(_id: &str, _percent: u8) -> Result<()> {
    bail!("not supported on this platform")
}

pub struct MicTest;

impl MicTest {
    pub fn start(_input: String, _output: String, _seconds: u32) -> Self {
        MicTest
    }

    pub fn state(&self) -> TestState {
        TestState { phase: super::Phase::Failed("not supported on this platform".into()), level: 0.0 }
    }
}

pub struct KeepAlive;

impl KeepAlive {
    pub fn start(_output: String) -> Self {
        KeepAlive
    }
}
