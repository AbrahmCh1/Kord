//! Low-battery warnings: once per bud per discharge, re-armed when it charges.

use crate::device::{DeviceKind, DeviceStatus};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Component {
    Left,
    Right,
    /// Single-unit headphones.
    Headphones,
}

/// How far above the threshold a battery must recover before warning again.
const REARM_MARGIN: u8 = 10;

#[derive(Debug, Clone, Default)]
pub struct LowBatteryMonitor {
    /// Warn at or below this percentage; 0 disables warnings.
    threshold: u8,
    warned: [bool; 3],
}

impl LowBatteryMonitor {
    pub fn new(threshold: u8) -> Self {
        Self { threshold, warned: [false; 3] }
    }

    pub fn set_threshold(&mut self, threshold: u8) {
        self.threshold = threshold;
    }

    /// Returns the components that just dropped to the threshold.
    pub fn check(&mut self, status: &DeviceStatus) -> Vec<(Component, u8)> {
        let readings = match status.kind {
            DeviceKind::Earbuds => vec![(Component::Left, status.left), (Component::Right, status.right)],
            DeviceKind::Headphones => vec![(
                Component::Headphones,
                crate::Battery {
                    level: status.lowest_level(),
                    charging: status.left.charging || status.right.charging,
                },
            )],
        };
        let mut alerts = Vec::new();
        for (component, battery) in readings {
            let Some(level) = battery.level else { continue };
            let warned = &mut self.warned[component as usize];
            if battery.charging || level >= self.threshold.saturating_add(REARM_MARGIN) {
                *warned = false;
            } else if self.threshold > 0 && level <= self.threshold && !*warned {
                *warned = true;
                alerts.push((component, level));
            }
        }
        alerts
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::{Battery, EarState, Vendor};

    fn buds(left: u8, right: u8, charging: bool) -> DeviceStatus {
        DeviceStatus {
            vendor: Vendor::Apple,
            model_id: 0x1420,
            model_name: String::new(),
            kind: DeviceKind::Earbuds,
            color: 0,
            left: Battery { level: Some(left), charging },
            right: Battery { level: Some(right), charging },
            case: Battery::default(),
            ears: EarState::default(),
            lid_open: None,
            connected: Some(true),
        }
    }

    #[test]
    fn warns_once_and_rearms_after_charging() {
        let mut monitor = LowBatteryMonitor::new(20);
        assert!(monitor.check(&buds(50, 40, false)).is_empty());
        assert_eq!(monitor.check(&buds(20, 40, false)), vec![(Component::Left, 20)]);
        assert!(monitor.check(&buds(10, 40, false)).is_empty(), "already warned");
        assert!(monitor.check(&buds(10, 40, true)).is_empty());
        assert_eq!(monitor.check(&buds(20, 20, false)), vec![(Component::Left, 20), (Component::Right, 20)]);
    }

    #[test]
    fn zero_threshold_disables() {
        let mut monitor = LowBatteryMonitor::new(0);
        assert!(monitor.check(&buds(0, 0, false)).is_empty());
    }
}
