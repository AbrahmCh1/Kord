//! Decoder for Apple's "Proximity Pairing" Continuity message.
//!
//! AirPods and most Beats headphones broadcast this BLE advertisement a few
//! times per second while out of the case. It carries battery levels, charging
//! state, ear detection and lid state without needing a connection, which is
//! what makes it work the same on Windows and Linux.
//!
//! Layout of the manufacturer data (after the `0x004C` company id):
//!
//! | Byte | Meaning                                                     |
//! |------|-------------------------------------------------------------|
//! | 0    | Message type, `0x07` = Proximity Pairing                    |
//! | 1    | Payload length                                              |
//! | 2    | Prefix, `0x00` when the device isn't paired to anything     |
//! | 3–4  | Model id, big endian (e.g. `0x0E20` = AirPods Pro)          |
//! | 5    | Status bit field (primary pod, in-case, in-ear flags)       |
//! | 6    | Pod batteries, one nibble per pod                           |
//! | 7    | High nibble: charging flags, low nibble: case battery       |
//! | 8    | Lid open counter (bits 0–2) and lid state (bit 3)           |
//! | 9    | Colour                                                      |
//! | 10   | Connection state                                            |
//!
//! The rest of the payload is encrypted with a key only paired Apple devices
//! have. The bit interpretation follows the reverse-engineering work of the
//! OpenPods and LibrePods projects.

use crate::device::{Battery, DeviceKind, DeviceStatus, EarState, Vendor};

/// Bluetooth SIG company identifier of Apple, Inc.
pub const COMPANY_ID: u16 = 0x004C;

const PROXIMITY_PAIRING: u8 = 0x07;
/// Bytes up to and including the colour; the connection state is optional.
const MIN_LEN: usize = 10;

const STATUS_PRIMARY_LEFT: u8 = 0x20;
const STATUS_THIS_IN_CASE: u8 = 0x40;
const STATUS_THIS_IN_EAR: u8 = 0x02;
const STATUS_OTHER_IN_EAR: u8 = 0x08;

const CHARGING_PRIMARY: u8 = 0x01;
const CHARGING_SECONDARY: u8 = 0x02;
const CHARGING_CASE: u8 = 0x04;

/// A known Apple/Beats model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Model {
    pub id: u16,
    pub name: &'static str,
    pub kind: DeviceKind,
}

const fn model(id: u16, name: &'static str, kind: DeviceKind) -> Model {
    Model { id, name, kind }
}

/// Known model ids. The AirPods entries are cross-checked against LibrePods;
/// the Beats entries are community-sourced, so please report corrections.
pub const MODELS: &[Model] = &[
    model(0x0220, "AirPods", DeviceKind::Earbuds),
    model(0x0F20, "AirPods (2nd gen)", DeviceKind::Earbuds),
    model(0x1320, "AirPods (3rd gen)", DeviceKind::Earbuds),
    model(0x1920, "AirPods 4", DeviceKind::Earbuds),
    model(0x1B20, "AirPods 4 (ANC)", DeviceKind::Earbuds),
    model(0x0E20, "AirPods Pro", DeviceKind::Earbuds),
    model(0x1420, "AirPods Pro (2nd gen)", DeviceKind::Earbuds),
    model(0x2420, "AirPods Pro (2nd gen, USB-C)", DeviceKind::Earbuds),
    model(0x0A20, "AirPods Max", DeviceKind::Headphones),
    model(0x1F20, "AirPods Max (USB-C)", DeviceKind::Headphones),
    model(0x0320, "Powerbeats3", DeviceKind::Headphones),
    model(0x0520, "BeatsX", DeviceKind::Headphones),
    model(0x0620, "Beats Solo3", DeviceKind::Headphones),
    model(0x0920, "Beats Studio3", DeviceKind::Headphones),
    model(0x0B20, "Powerbeats Pro", DeviceKind::Earbuds),
    model(0x0C20, "Beats Solo Pro", DeviceKind::Headphones),
    model(0x1020, "Beats Flex", DeviceKind::Headphones),
    model(0x1120, "Beats Studio Buds", DeviceKind::Earbuds),
    model(0x1220, "Beats Fit Pro", DeviceKind::Earbuds),
    model(0x1620, "Beats Studio Buds+", DeviceKind::Earbuds),
];

pub fn lookup_model(id: u16) -> Option<&'static Model> {
    MODELS.iter().find(|m| m.id == id)
}

/// Decodes a Proximity Pairing payload. Returns `None` for any other
/// Continuity message (iPhones, Macs, AirTags… advertise under the same id).
pub fn decode(data: &[u8]) -> Option<DeviceStatus> {
    if data.len() < MIN_LEN || data[0] != PROXIMITY_PAIRING || data[2] == 0x00 {
        return None;
    }

    let model_id = u16::from_be_bytes([data[3], data[4]]);
    let status = data[5];
    let pods = data[6];
    let flags = data[7] >> 4;
    let lid = data[8];
    let color = data[9];

    // The advertisement is sent by whichever bud is "primary", and most fields
    // are expressed relative to it, so work out which side that is first.
    let primary_left = status & STATUS_PRIMARY_LEFT != 0;
    let this_in_case = status & STATUS_THIS_IN_CASE != 0;
    let (primary_nibble, secondary_nibble) = (pods & 0x0F, pods >> 4);
    let (left_nibble, right_nibble) =
        if primary_left { (primary_nibble, secondary_nibble) } else { (secondary_nibble, primary_nibble) };
    let (left_charging, right_charging) = if primary_left {
        (flags & CHARGING_PRIMARY != 0, flags & CHARGING_SECONDARY != 0)
    } else {
        (flags & CHARGING_SECONDARY != 0, flags & CHARGING_PRIMARY != 0)
    };

    let in_ear_this = status & STATUS_THIS_IN_EAR != 0;
    let in_ear_other = status & STATUS_OTHER_IN_EAR != 0;
    let (left_in_ear, right_in_ear) =
        if primary_left ^ this_in_case { (in_ear_this, in_ear_other) } else { (in_ear_other, in_ear_this) };

    let (model_name, kind) = match lookup_model(model_id) {
        Some(m) => (m.name.to_string(), m.kind),
        None => (format!("Apple device (0x{model_id:04X})"), DeviceKind::Earbuds),
    };

    let case = match kind {
        DeviceKind::Earbuds => Battery { level: nibble_to_percent(data[7] & 0x0F), charging: flags & CHARGING_CASE != 0 },
        DeviceKind::Headphones => Battery::default(),
    };

    Some(DeviceStatus {
        vendor: Vendor::Apple,
        model_id,
        model_name,
        kind,
        color,
        left: Battery { level: nibble_to_percent(left_nibble), charging: left_charging },
        right: Battery { level: nibble_to_percent(right_nibble), charging: right_charging },
        case,
        ears: EarState { left: Some(left_in_ear), right: Some(right_in_ear) },
        // The lid bit is only meaningful while the broadcasting bud sits in the case.
        lid_open: this_in_case.then_some(lid & 0x08 == 0),
        connected: data.get(10).and_then(|&state| match state {
            0x00 => Some(false),
            0xFF => None,
            _ => Some(true),
        }),
    })
}

/// Battery nibbles are in steps of 10 %; `0xF` means "not reporting".
fn nibble_to_percent(nibble: u8) -> Option<u8> {
    (nibble <= 10).then_some(nibble * 10)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload(model: [u8; 2], status: u8, pods: u8, flags_case: u8, lid: u8) -> Vec<u8> {
        let mut data = vec![0x07, 0x19, 0x01, model[0], model[1], status, pods, flags_case, lid, 0x00, 0x05];
        data.resize(27, 0xAA); // encrypted tail
        data
    }

    #[test]
    fn decodes_airpods_pro_with_left_primary() {
        let s = decode(&payload([0x0E, 0x20], 0x2B, 0x98, 0x45, 0x00)).unwrap();
        assert_eq!(s.model_name, "AirPods Pro");
        assert_eq!(s.kind, DeviceKind::Earbuds);
        assert_eq!(s.left, Battery { level: Some(80), charging: false });
        assert_eq!(s.right, Battery { level: Some(90), charging: false });
        assert_eq!(s.case, Battery { level: Some(50), charging: true });
        assert_eq!(s.ears, EarState { left: Some(true), right: Some(true) });
        assert_eq!(s.lid_open, None);
        assert_eq!(s.connected, Some(true));
        assert_eq!(s.pnp_id(), (0x004C, 0x200E));
    }

    #[test]
    fn swaps_sides_when_right_is_primary() {
        // Right bud primary, only the primary (right) bud in an ear and charging flag on primary.
        let s = decode(&payload([0x14, 0x20], 0x02, 0x37, 0x1F, 0x00)).unwrap();
        assert_eq!(s.model_name, "AirPods Pro (2nd gen)");
        assert_eq!(s.left.level, Some(30));
        assert_eq!(s.right.level, Some(70));
        assert!(s.right.charging && !s.left.charging);
        assert_eq!(s.case.level, None);
        assert_eq!(s.ears, EarState { left: Some(false), right: Some(true) });
    }

    #[test]
    fn reports_lid_only_when_primary_is_in_case() {
        let open = decode(&payload([0x0F, 0x20], 0x60, 0xA5, 0x08, 0x31)).unwrap();
        assert_eq!(open.lid_open, Some(true));
        assert_eq!(open.left.level, Some(50));
        assert_eq!(open.right.level, Some(100));
        let closed = decode(&payload([0x0F, 0x20], 0x60, 0xA5, 0x08, 0x39)).unwrap();
        assert_eq!(closed.lid_open, Some(false));
    }

    #[test]
    fn unknown_model_keeps_raw_id() {
        let s = decode(&payload([0x77, 0x20], 0x20, 0xFF, 0xFF, 0x00)).unwrap();
        assert_eq!(s.model_name, "Apple device (0x7720)");
        assert_eq!(s.left.level, None);
        assert_eq!(s.right.level, None);
    }

    #[test]
    fn rejects_other_continuity_messages() {
        assert!(decode(&[0x10, 0x05, 0x01, 0x18, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]).is_none()); // Nearby Info
        assert!(decode(&payload([0x0E, 0x20], 0, 0, 0, 0)[..8]).is_none()); // truncated
        let mut unpaired = payload([0x0E, 0x20], 0, 0, 0, 0);
        unpaired[2] = 0x00;
        assert!(decode(&unpaired).is_none());
    }

    #[test]
    fn headphones_have_no_case() {
        let s = decode(&payload([0x0A, 0x20], 0x22, 0xF8, 0x45, 0x00)).unwrap();
        assert_eq!(s.kind, DeviceKind::Headphones);
        assert_eq!(s.case, Battery::default());
        assert_eq!(s.lowest_level(), Some(80));
    }
}
