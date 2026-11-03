//! The options the window offers, grouped in sections, and how a change is
//! written back to the settings file.

use crate::audio::{Phase, TestState};
use crate::config::{Config, Language, PauseWhen, Theme};
use crate::i18n::strings;

/// Every option the window offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Id {
    EarMode,
    Resume,
    SwitchOnRemove,
    AutoConnect,
    AutoPlay,
    PauseOnDisconnect,
    DefaultVolume,
    SwitchOnConnect,
    NotifyConnection,
    LowLatency,
    KeepAlive,
    HandsFree,
    MicDefault,
    MicVolume,
    MicTest,
    LowBattery,
    PopupOnLid,
    Model,
    Address,
    BluetoothSettings,
    OnlyPaired,
    Range,
    Theme,
    Language,
    Autostart,
    ConfigFile,
}

impl Id {
    /// Settings that live in the OS rather than in Kord's settings file.
    pub fn is_system(self) -> bool {
        matches!(
            self,
            Id::LowLatency | Id::HandsFree | Id::MicDefault | Id::MicVolume | Id::MicTest | Id::Autostart
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Control {
    Toggle(bool),
    Choice { selected: usize, options: Vec<&'static str> },
    Slider { value: u8, max: u8, step: u8 },
    Button { label: &'static str, enabled: bool },
    /// A level meter next to a button (microphone test).
    Test { label: &'static str, enabled: bool, level: f32 },
    /// Read-only text.
    Info(String),
    /// An OS change in progress.
    Busy,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Value {
    Toggle(bool),
    Choice(usize),
    Slider(u8),
    Press,
}

pub struct Row {
    pub id: Id,
    pub title: &'static str,
    pub description: String,
    pub control: Control,
}

pub struct Section {
    pub title: &'static str,
    pub rows: Vec<Row>,
}

/// The headphones' microphone, as far as the OS is concerned.
#[derive(Debug, Clone, Default, PartialEq)]
pub enum Mic {
    #[default]
    NotConnected,
    NeedsHandsFree,
    Ready { input: String, output: String, is_default: bool, volume: u8 },
}

/// Settings owned by the OS, read when a page opens and after changes.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SystemState {
    pub autostart: bool,
    /// `None` where the platform (or the current device) doesn't have it.
    pub low_latency: Option<bool>,
    pub hands_free: Option<bool>,
    pub mic: Option<Mic>,
}

/// What a device page needs to know about its headphones.
pub struct DeviceFacts<'a> {
    /// AirPods and Beats: Kord reads their battery and ear sensors.
    pub apple: bool,
    pub model: &'a str,
    pub address: Option<u64>,
}

/// Signal-strength presets for the "search range" choice, in dBm.
const RANGES: [i16; 3] = [-60, -70, -80];

fn row(busy: Option<Id>, id: Id, title: &'static str, description: &str, control: Control) -> Row {
    Row { id, title, description: description.to_string(), control: if busy == Some(id) { Control::Busy } else { control } }
}

/// Options of one pair of headphones.
pub fn device_sections(
    config: &Config,
    system: &SystemState,
    device: &DeviceFacts,
    test: Option<&TestState>,
    busy: Option<Id>,
) -> Vec<Section> {
    let s = strings();
    let row = |id, title, description: &str, control| row(busy, id, title, description, control);
    let mut sections = Vec::new();

    if device.apple {
        let ear = &config.ear_detection;
        let ear_mode = match (ear.enabled, ear.pause_when) {
            (false, _) => 0,
            (true, PauseWhen::Any) => 1,
            (true, PauseWhen::All) => 2,
        };
        sections.push(Section {
            title: s.section_ear,
            rows: vec![
                row(
                    Id::EarMode,
                    s.ear_mode_title,
                    s.ear_mode_description,
                    Control::Choice { selected: ear_mode, options: vec![s.ear_off, s.ear_any, s.ear_all] },
                ),
                row(Id::Resume, s.resume_title, s.resume_description, Control::Toggle(ear.resume)),
                row(
                    Id::SwitchOnRemove,
                    s.switch_on_remove_title,
                    s.switch_on_remove_description,
                    Control::Toggle(ear.switch_output_when_removed),
                ),
            ],
        });
        let connection = &config.connection;
        sections.push(Section {
            title: s.section_connection,
            rows: vec![
                row(Id::AutoConnect, s.auto_connect_title, s.auto_connect_description, Control::Toggle(connection.auto_connect)),
                row(Id::AutoPlay, s.auto_play_title, s.auto_play_description, Control::Toggle(connection.auto_play)),
                row(
                    Id::PauseOnDisconnect,
                    s.pause_on_disconnect_title,
                    s.pause_on_disconnect_description,
                    Control::Toggle(connection.pause_on_disconnect),
                ),
                row(
                    Id::SwitchOnConnect,
                    s.switch_on_connect_title,
                    s.switch_on_connect_description,
                    Control::Toggle(connection.switch_output_on_connect),
                ),
                row(
                    Id::DefaultVolume,
                    s.default_volume_title,
                    s.default_volume_description,
                    Control::Slider { value: connection.default_volume, max: 100, step: 5 },
                ),
            ],
        });
    }

    if crate::audio::SUPPORTED {
        let mut audio = Vec::new();
        if let Some(on) = system.low_latency {
            audio.push(row(Id::LowLatency, s.low_latency_title, s.low_latency_description, Control::Toggle(on)));
        }
        if device.apple {
            audio.push(row(Id::KeepAlive, s.keep_alive_title, s.keep_alive_description, Control::Toggle(config.audio.keep_alive)));
        }
        if let Some(on) = system.hands_free {
            audio.push(row(Id::HandsFree, s.hands_free_title, s.hands_free_description, Control::Toggle(on)));
        }
        if !audio.is_empty() {
            sections.push(Section { title: s.section_audio, rows: audio });
        }
    }

    if let Some(mic) = &system.mic {
        sections.push(Section { title: s.section_microphone, rows: mic_rows(mic, test, busy) });
    }

    if device.apple {
        sections.push(Section {
            title: s.section_notifications,
            rows: vec![
                row(
                    Id::NotifyConnection,
                    s.notify_connection_title,
                    s.notify_connection_description,
                    Control::Toggle(config.connection.notify_connection),
                ),
                row(
                    Id::LowBattery,
                    s.low_battery_setting_title,
                    s.low_battery_description,
                    Control::Slider { value: config.notifications.low_battery, max: 50, step: 5 },
                ),
                row(Id::PopupOnLid, s.popup_title, s.popup_description, Control::Toggle(config.popup.show_on_lid_open)),
            ],
        });
    }

    let mut info = vec![row(Id::Model, s.model_title, "", Control::Info(device.model.to_string()))];
    if let Some(address) = device.address {
        info.push(row(Id::Address, s.address_title, "", Control::Info(format_address(address))));
    }
    info.push(row(
        Id::BluetoothSettings,
        s.bluetooth_settings_title,
        s.bluetooth_settings_description,
        Control::Button { label: s.open, enabled: true },
    ));
    sections.push(Section { title: s.section_info, rows: info });
    sections
}

fn mic_rows(mic: &Mic, test: Option<&TestState>, busy: Option<Id>) -> Vec<Row> {
    let s = strings();
    let row = |id, title, description: &str, control| row(busy, id, title, description, control);
    match mic {
        Mic::NotConnected | Mic::NeedsHandsFree => {
            let status = if *mic == Mic::NotConnected { s.mic_needs_connection } else { s.mic_needs_hands_free };
            vec![row(Id::MicDefault, s.mic_title, status, Control::Button { label: s.mic_make_default, enabled: false })]
        }
        Mic::Ready { is_default, volume, .. } => {
            let status = if *is_default { s.mic_is_default } else { s.mic_ready };
            let (label, running, level) = match test.map(|t| (&t.phase, t.level)) {
                Some((Phase::Recording { .. }, level)) => (s.mic_recording, true, level),
                Some((Phase::Playing, _)) => (s.mic_playing, true, 0.0),
                _ => (s.mic_test_start, false, 0.0),
            };
            let description = match test.map(|t| &t.phase) {
                Some(Phase::Recording { seconds_left }) => format!("{} {seconds_left} s", s.mic_test_description),
                Some(Phase::Failed(error)) => format!("{}: {error}", s.mic_failed),
                _ => s.mic_test_description.to_string(),
            };
            vec![
                row(Id::MicDefault, s.mic_title, status, Control::Button { label: s.mic_make_default, enabled: !is_default }),
                row(
                    Id::MicVolume,
                    s.mic_volume_title,
                    s.mic_volume_description,
                    Control::Slider { value: *volume, max: 100, step: 5 },
                ),
                Row { id: Id::MicTest, title: s.mic_test_title, description, control: Control::Test { label, enabled: !running, level } },
            ]
        }
    }
}

/// Options of Kord itself.
pub fn app_sections(config: &Config, system: &SystemState) -> Vec<Section> {
    let s = strings();
    let row = |id, title, description: &str, control| row(None, id, title, description, control);
    let theme = match config.general.theme {
        Theme::System => 0,
        Theme::Light => 1,
        Theme::Dark => 2,
    };
    let language = match config.general.language {
        Language::Auto => 0,
        Language::En => 1,
        Language::Es => 2,
    };
    let range = RANGES
        .iter()
        .enumerate()
        .min_by_key(|(_, rssi)| (**rssi - config.tracking.min_rssi).abs())
        .map_or(1, |(index, _)| index);
    vec![
        Section {
            title: s.section_appearance,
            rows: vec![
                row(
                    Id::Theme,
                    s.theme_title,
                    s.theme_description,
                    Control::Choice { selected: theme, options: vec![s.theme_system, s.theme_light, s.theme_dark] },
                ),
                row(
                    Id::Language,
                    s.language_title,
                    s.language_description,
                    Control::Choice { selected: language, options: vec![s.language_auto, "English", "Español"] },
                ),
            ],
        },
        Section {
            title: s.section_startup,
            rows: vec![row(Id::Autostart, s.autostart_title, s.autostart_description, Control::Toggle(system.autostart))],
        },
        Section {
            title: s.section_search,
            rows: vec![
                row(Id::OnlyPaired, s.only_paired_title, s.only_paired_description, Control::Toggle(config.tracking.only_paired)),
                row(
                    Id::Range,
                    s.range_title,
                    s.range_description,
                    Control::Choice { selected: range, options: vec![s.range_near, s.range_normal, s.range_far] },
                ),
            ],
        },
        Section {
            title: s.section_advanced,
            rows: vec![row(
                Id::ConfigFile,
                s.config_file_title,
                s.config_file_description,
                Control::Button { label: s.open, enabled: true },
            )],
        },
    ]
}

/// Applies a changed control to the settings file. System settings and
/// buttons are handled by the caller.
pub fn apply(config: &mut Config, id: Id, value: Value) {
    match (id, value) {
        (Id::EarMode, Value::Choice(index)) => {
            config.ear_detection.enabled = index != 0;
            if index == 2 {
                config.ear_detection.pause_when = PauseWhen::All;
            } else if index == 1 {
                config.ear_detection.pause_when = PauseWhen::Any;
            }
        }
        (Id::Resume, Value::Toggle(on)) => config.ear_detection.resume = on,
        (Id::SwitchOnRemove, Value::Toggle(on)) => config.ear_detection.switch_output_when_removed = on,
        (Id::AutoConnect, Value::Toggle(on)) => config.connection.auto_connect = on,
        (Id::AutoPlay, Value::Toggle(on)) => config.connection.auto_play = on,
        (Id::PauseOnDisconnect, Value::Toggle(on)) => config.connection.pause_on_disconnect = on,
        (Id::SwitchOnConnect, Value::Toggle(on)) => config.connection.switch_output_on_connect = on,
        (Id::DefaultVolume, Value::Slider(v)) => config.connection.default_volume = v,
        (Id::NotifyConnection, Value::Toggle(on)) => config.connection.notify_connection = on,
        (Id::KeepAlive, Value::Toggle(on)) => config.audio.keep_alive = on,
        (Id::LowBattery, Value::Slider(v)) => config.notifications.low_battery = v,
        (Id::PopupOnLid, Value::Toggle(on)) => config.popup.show_on_lid_open = on,
        (Id::OnlyPaired, Value::Toggle(on)) => config.tracking.only_paired = on,
        (Id::Range, Value::Choice(index)) => config.tracking.min_rssi = RANGES[index.min(RANGES.len() - 1)],
        (Id::Theme, Value::Choice(index)) => {
            config.general.theme = [Theme::System, Theme::Light, Theme::Dark][index.min(2)];
        }
        (Id::Language, Value::Choice(index)) => {
            config.general.language = [Language::Auto, Language::En, Language::Es][index.min(2)];
        }
        _ => {}
    }
}

/// "30:82:16:A2:06:15".
pub fn format_address(address: u64) -> String {
    (0..6).rev().map(|byte| format!("{:02X}", (address >> (byte * 8)) & 0xFF)).collect::<Vec<_>>().join(":")
}

#[cfg(test)]
mod tests {
    #[test]
    fn formats_bluetooth_addresses() {
        assert_eq!(super::format_address(0x308216A20615), "30:82:16:A2:06:15");
    }
}
