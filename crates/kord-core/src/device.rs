//! Vendor-neutral description of a pair of wireless headphones.

use std::fmt;

/// Who made the device. Used to pick the decoder and for display.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Vendor {
    Apple,
}

impl fmt::Display for Vendor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Vendor::Apple => f.write_str("Apple"),
        }
    }
}

/// Physical form factor, which decides how batteries are presented.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeviceKind {
    /// Two independent earbuds plus a charging case (AirPods, Powerbeats Pro…).
    Earbuds,
    /// A single unit with one battery (AirPods Max, Beats Solo…).
    Headphones,
}

/// Charge information for one component (left bud, right bud, case…).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Battery {
    /// Charge in percent (0–100), `None` when the component is not reporting,
    /// e.g. a bud that is in a closed case or out of range.
    pub level: Option<u8>,
    pub charging: bool,
}

/// Whether each bud is currently in an ear. `None` means "unknown".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EarState {
    pub left: Option<bool>,
    pub right: Option<bool>,
}

impl EarState {
    /// Number of buds known to be in an ear, or `None` if nothing is known.
    pub fn worn_count(&self) -> Option<u8> {
        if self.left.is_none() && self.right.is_none() {
            return None;
        }
        Some(u8::from(self.left == Some(true)) + u8::from(self.right == Some(true)))
    }
}

/// Stable identity of a device across advertisements.
///
/// Headphones rotate their BLE address every few minutes for privacy, so the
/// address can't be used to recognise them. Model and colour are stable and,
/// together with signal strength, good enough to tell your pair from others.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DeviceKey {
    pub vendor: Vendor,
    pub model_id: u16,
    pub color: u8,
}

/// Everything Kord knows about a device from a single advertisement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceStatus {
    pub vendor: Vendor,
    /// Vendor-specific model identifier.
    pub model_id: u16,
    /// Human-readable model name; for unknown models this includes the raw id
    /// so users can report it.
    pub model_name: String,
    pub kind: DeviceKind,
    pub color: u8,
    pub left: Battery,
    pub right: Battery,
    pub case: Battery,
    pub ears: EarState,
    /// Whether the charging case lid is open, when reported.
    pub lid_open: Option<bool>,
    /// Whether the device reports being connected to *some* host. It does not
    /// tell us which host (this computer, a phone…).
    pub connected: Option<bool>,
}

impl DeviceStatus {
    pub fn key(&self) -> DeviceKey {
        DeviceKey { vendor: self.vendor, model_id: self.model_id, color: self.color }
    }

    /// Bluetooth Device ID profile (vendor id, product id) of this model, as
    /// the OS reports it for the paired classic-Bluetooth device. This is how
    /// an advertisement is matched to a pairing on this computer.
    pub fn pnp_id(&self) -> (u16, u16) {
        match self.vendor {
            // The advertisement carries the product id with its bytes swapped.
            Vendor::Apple => (crate::apple::COMPANY_ID, self.model_id.swap_bytes()),
        }
    }

    /// Battery of a single-unit device, or the lowest bud for earbuds.
    /// Handy for a single tray indicator.
    pub fn lowest_level(&self) -> Option<u8> {
        [self.left.level, self.right.level].into_iter().flatten().min()
    }
}
