//! User settings, stored as TOML in the platform config directory
//! (`%APPDATA%\kord\Kord\config\config.toml`, `~/.config/kord/config.toml`).

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use kord_core::ear::{EarConfig, PauseMode};
use kord_core::tracker::TrackerConfig;
use serde::{Deserialize, Serialize};
use tracing::warn;

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub general: General,
    pub ear_detection: EarDetection,
    pub connection: Connection,
    pub audio: Audio,
    pub notifications: Notifications,
    pub tracking: Tracking,
    pub popup: PopupSettings,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct General {
    pub theme: Theme,
    pub language: Language,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    #[default]
    Auto,
    En,
    Es,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Audio {
    /// Keep the Bluetooth audio link awake with an inaudible stream, so the
    /// start of short sounds isn't cut off. Uses a little more battery.
    pub keep_alive: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EarDetection {
    /// Pause media when you take an earbud out.
    pub enabled: bool,
    /// `"any"`: pause when any earbud is removed. `"all"`: only when both are.
    pub pause_when: PauseWhen,
    /// Resume when the earbuds go back in.
    pub resume: bool,
    /// Send sound to the speakers while both earbuds are out, and back when one goes in.
    pub switch_output_when_removed: bool,
    /// Milliseconds a new in-ear reading must hold before acting on it.
    pub debounce_ms: u64,
    /// Don't resume if the earbuds go back in after this many seconds (0 = always resume).
    pub resume_window_secs: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PauseWhen {
    Any,
    All,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Connection {
    /// Connect the headphones to this computer when you put one in your ear.
    pub auto_connect: bool,
    /// Send "play" after the headphones connect.
    pub auto_play: bool,
    /// Pause playback when the headphones disconnect, so it doesn't continue on the speakers.
    pub pause_on_disconnect: bool,
    /// Volume (percent) applied after connecting; 0 leaves it alone.
    pub default_volume: u8,
    /// Make the headphones the default output when they connect.
    pub switch_output_on_connect: bool,
    /// Show a notification when the headphones connect or disconnect.
    pub notify_connection: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Notifications {
    /// Notify when an earbud drops to this percentage; 0 disables.
    pub low_battery: u8,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Tracking {
    /// Ignore headphones with a weaker signal than this (dBm). Raise it (e.g. -55)
    /// if Kord picks up someone else's AirPods.
    pub min_rssi: i16,
    /// Seconds without hearing from the headphones before they count as gone.
    pub lost_after_secs: u64,
    /// Only track models paired with this computer (when any are).
    pub only_paired: bool,
    /// Only track this model id, e.g. "0x1420". Run `kord scan` to find yours.
    pub only_model: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PopupSettings {
    /// Show the battery card when you open the case, like a phone does.
    pub show_on_lid_open: bool,
}

impl Default for EarDetection {
    fn default() -> Self {
        let core = EarConfig::default();
        Self {
            enabled: true,
            pause_when: PauseWhen::Any,
            resume: true,
            switch_output_when_removed: false,
            debounce_ms: core.debounce.as_millis() as u64,
            resume_window_secs: core.resume_window.map_or(0, |w| w.as_secs()),
        }
    }
}

impl Default for Connection {
    fn default() -> Self {
        Self {
            auto_connect: false,
            auto_play: false,
            pause_on_disconnect: true,
            default_volume: 0,
            switch_output_on_connect: false,
            notify_connection: false,
        }
    }
}

impl Default for Notifications {
    fn default() -> Self {
        Self { low_battery: 20 }
    }
}

impl Default for Tracking {
    fn default() -> Self {
        let core = TrackerConfig::default();
        Self { min_rssi: core.min_rssi, lost_after_secs: core.lost_after.as_secs(), only_paired: true, only_model: None }
    }
}

impl Default for PopupSettings {
    fn default() -> Self {
        Self { show_on_lid_open: true }
    }
}

impl Config {
    pub fn path() -> Option<PathBuf> {
        directories::ProjectDirs::from("", "kord", "Kord").map(|dirs| dirs.config_dir().join("config.toml"))
    }

    /// Loads the settings, falling back to defaults when the file is missing
    /// or invalid so a typo never keeps Kord from starting.
    pub fn load() -> Self {
        let Some(path) = Self::path() else { return Self::default() };
        match std::fs::read_to_string(&path) {
            Ok(text) => toml::from_str(&text).unwrap_or_else(|err| {
                warn!("ignoring invalid {}: {err}", path.display());
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) -> Result<PathBuf> {
        let path = Self::path().context("no config directory on this system")?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&path, toml::to_string_pretty(self)?).with_context(|| format!("writing {}", path.display()))?;
        Ok(path)
    }

    pub fn ear_config(&self) -> EarConfig {
        let ear = &self.ear_detection;
        EarConfig {
            mode: match ear.pause_when {
                PauseWhen::Any => PauseMode::AnyRemoved,
                PauseWhen::All => PauseMode::AllRemoved,
            },
            debounce: Duration::from_millis(ear.debounce_ms),
            resume_window: (ear.resume_window_secs > 0).then(|| Duration::from_secs(ear.resume_window_secs)),
        }
    }

    pub fn tracker_config(&self) -> TrackerConfig {
        let tracking = &self.tracking;
        TrackerConfig {
            min_rssi: tracking.min_rssi,
            lost_after: Duration::from_secs(tracking.lost_after_secs),
            only_model: tracking.only_model.as_deref().and_then(|id| {
                let parsed = parse_model_id(id);
                if parsed.is_none() {
                    warn!("ignoring invalid tracking.only_model {id:?}");
                }
                parsed
            }),
            ..TrackerConfig::default()
        }
    }
}

/// Parses a hexadecimal model id such as "0x1420" or "1420".
pub fn parse_model_id(text: &str) -> Option<u16> {
    let text = text.trim();
    let digits = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")).unwrap_or(text);
    u16::from_str_radix(digits, 16).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_file_keeps_defaults() {
        let config: Config = toml::from_str("[tracking]\nonly_model = \"0x1420\"\n").unwrap();
        assert!(config.ear_detection.enabled);
        assert!(config.connection.pause_on_disconnect);
        assert_eq!(config.tracker_config().only_model, Some(0x1420));
        assert_eq!(config.tracker_config().min_rssi, TrackerConfig::default().min_rssi);
    }

    #[test]
    fn round_trips() {
        let config = Config::default();
        let text = toml::to_string_pretty(&config).unwrap();
        assert_eq!(toml::from_str::<Config>(&text).unwrap(), config);
        assert_eq!(config.ear_config(), EarConfig::default());
    }
}
