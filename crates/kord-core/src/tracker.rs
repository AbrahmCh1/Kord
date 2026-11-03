//! Picks *your* headphones out of everything broadcasting nearby.
//!
//! In an office or on a bus there can be a dozen AirPods in range, all sending
//! the same kind of advertisement with rotating addresses. The tracker locks on
//! to one device (by model and colour) based on signal strength and sticks with
//! it until it disappears or something much closer shows up.

use std::time::{Duration, Instant};

use crate::device::{DeviceKey, DeviceStatus};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackerConfig {
    /// Devices weaker than this (dBm) are ignored. Headphones in your ears
    /// next to the computer are typically above -60 dBm.
    pub min_rssi: i16,
    /// Forget the device after this long without advertisements.
    pub lost_after: Duration,
    /// How much stronger (dB) another device must be to take over.
    pub switch_margin_db: i16,
    /// Only ever track this model id, if set.
    pub only_model: Option<u16>,
    /// Only track these model ids (e.g. the ones paired with this computer);
    /// empty allows any.
    pub allowed_models: Vec<u16>,
}

impl Default for TrackerConfig {
    fn default() -> Self {
        Self { min_rssi: -70, lost_after: Duration::from_secs(15), switch_margin_db: 10, only_model: None, allowed_models: Vec::new() }
    }
}

/// What an observation meant for the tracked device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Observation {
    /// Not the tracked device; nothing changed.
    Ignored,
    /// New data for the device already being tracked.
    Updated,
    /// Started tracking this device (first lock or a takeover).
    Switched,
}

/// Both buds advertise, but only one of them reports the lid at a time, so a
/// lid reading is carried over to the other bud's messages for this long.
const LID_HOLD: Duration = Duration::from_secs(3);

#[derive(Debug, Clone)]
struct Tracked {
    status: DeviceStatus,
    rssi: f32,
    last_seen: Instant,
    lid_seen: Option<Instant>,
}

#[derive(Debug, Clone)]
pub struct Tracker {
    config: TrackerConfig,
    current: Option<Tracked>,
}

impl Tracker {
    pub fn new(config: TrackerConfig) -> Self {
        Self { config, current: None }
    }

    pub fn set_config(&mut self, config: TrackerConfig) {
        self.config = config;
    }

    pub fn current(&self) -> Option<&DeviceStatus> {
        self.current.as_ref().map(|t| &t.status)
    }

    pub fn current_key(&self) -> Option<DeviceKey> {
        self.current().map(DeviceStatus::key)
    }

    /// Feeds one decoded advertisement. Some platforms don't report RSSI; such
    /// readings pass the threshold but can never win a takeover.
    pub fn observe(&mut self, mut status: DeviceStatus, rssi: Option<i16>, now: Instant) -> Observation {
        let allowed = self.config.allowed_models.is_empty() || self.config.allowed_models.contains(&status.model_id);
        if !allowed || self.config.only_model.is_some_and(|id| id != status.model_id) {
            return Observation::Ignored;
        }
        let rssi = rssi.unwrap_or(self.config.min_rssi);

        if let Some(current) = self.current.as_mut()
            && current.status.key() == status.key()
        {
            if status.lid_open.is_some() {
                current.lid_seen = Some(now);
            } else if current.lid_seen.is_some_and(|seen| now.duration_since(seen) <= LID_HOLD) {
                status.lid_open = current.status.lid_open;
            }
            current.status = status;
            current.rssi = current.rssi * 0.7 + f32::from(rssi) * 0.3;
            current.last_seen = now;
            return Observation::Updated;
        }

        if rssi < self.config.min_rssi {
            return Observation::Ignored;
        }
        let takes_over = match &self.current {
            None => true,
            Some(current) => {
                now.duration_since(current.last_seen) > self.config.lost_after
                    || f32::from(rssi) > current.rssi + f32::from(self.config.switch_margin_db)
            }
        };
        if !takes_over {
            return Observation::Ignored;
        }
        let lid_seen = status.lid_open.map(|_| now);
        self.current = Some(Tracked { status, rssi: f32::from(rssi), last_seen: now, lid_seen });
        Observation::Switched
    }

    /// Drops the tracked device if it has gone quiet. Returns `true` when it
    /// was just lost.
    pub fn expire(&mut self, now: Instant) -> bool {
        let lost = self.current.as_ref().is_some_and(|t| now.duration_since(t.last_seen) > self.config.lost_after);
        if lost {
            self.current = None;
        }
        lost
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::{Battery, DeviceKind, EarState, Vendor};

    fn device(model_id: u16, color: u8) -> DeviceStatus {
        DeviceStatus {
            vendor: Vendor::Apple,
            model_id,
            model_name: format!("{model_id:04X}"),
            kind: DeviceKind::Earbuds,
            color,
            left: Battery::default(),
            right: Battery::default(),
            case: Battery::default(),
            ears: EarState::default(),
            lid_open: None,
            connected: Some(true),
        }
    }

    #[test]
    fn locks_on_strong_device_and_ignores_weak_ones() {
        let t0 = Instant::now();
        let mut tracker = Tracker::new(TrackerConfig::default());
        assert_eq!(tracker.observe(device(1, 0), Some(-90), t0), Observation::Ignored);
        assert_eq!(tracker.observe(device(2, 0), Some(-50), t0), Observation::Switched);
        assert_eq!(tracker.observe(device(2, 0), Some(-55), t0), Observation::Updated);
        assert_eq!(tracker.observe(device(3, 0), Some(-52), t0), Observation::Ignored, "not stronger by the margin");
        assert_eq!(tracker.current().unwrap().model_id, 2);
    }

    #[test]
    fn much_closer_device_takes_over() {
        let t0 = Instant::now();
        let mut tracker = Tracker::new(TrackerConfig::default());
        tracker.observe(device(1, 0), Some(-68), t0);
        assert_eq!(tracker.observe(device(1, 1), Some(-45), t0), Observation::Switched);
        assert_eq!(tracker.current_key().unwrap().color, 1);
    }

    #[test]
    fn expires_and_relocks() {
        let t0 = Instant::now();
        let mut tracker = Tracker::new(TrackerConfig::default());
        tracker.observe(device(1, 0), Some(-50), t0);
        assert!(!tracker.expire(t0 + Duration::from_secs(5)));
        assert!(tracker.expire(t0 + Duration::from_secs(20)));
        assert!(tracker.current().is_none());
        assert_eq!(tracker.observe(device(2, 0), Some(-65), t0 + Duration::from_secs(21)), Observation::Switched);
    }

    #[test]
    fn lid_reading_survives_the_other_buds_messages() {
        let t0 = Instant::now();
        let mut tracker = Tracker::new(TrackerConfig::default());
        let with_lid = DeviceStatus { lid_open: Some(true), ..device(1, 0) };
        tracker.observe(with_lid, Some(-50), t0);
        tracker.observe(device(1, 0), Some(-50), t0 + Duration::from_secs(1));
        assert_eq!(tracker.current().unwrap().lid_open, Some(true));
        // Buds taken out of the case: nobody reports the lid any more.
        tracker.observe(device(1, 0), Some(-50), t0 + Duration::from_secs(5));
        assert_eq!(tracker.current().unwrap().lid_open, None);
    }

    #[test]
    fn model_filter() {
        let t0 = Instant::now();
        let mut tracker = Tracker::new(TrackerConfig { only_model: Some(7), ..TrackerConfig::default() });
        assert_eq!(tracker.observe(device(1, 0), Some(-40), t0), Observation::Ignored);
        assert_eq!(tracker.observe(device(7, 0), Some(-60), t0), Observation::Switched);
    }
}
