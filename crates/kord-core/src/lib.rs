//! Platform-independent core of Kord.
//!
//! This crate knows how to turn raw Bluetooth LE advertisements into a
//! vendor-neutral [`DeviceStatus`], how to pick *your* headphones out of all the
//! ones broadcasting nearby ([`tracker`]), and when to pause/resume media based on
//! ear detection ([`ear`]). It performs no I/O, so every piece is unit-testable.
//!
//! Adding support for a new family of headphones means writing a decoder for its
//! advertisement format and registering it in [`decode_advertisement`].

pub mod apple;
pub mod battery;
pub mod device;
pub mod ear;
pub mod tracker;

pub use device::{Battery, DeviceKey, DeviceKind, DeviceStatus, EarState, Vendor};

/// Decodes one manufacturer-specific data entry of a BLE advertisement.
///
/// `company_id` is the Bluetooth SIG company identifier the payload was
/// advertised under and `data` is the payload without that identifier.
/// Returns `None` when the payload is not a headphone status message Kord
/// understands.
pub fn decode_advertisement(company_id: u16, data: &[u8]) -> Option<DeviceStatus> {
    match company_id {
        apple::COMPANY_ID => apple::decode(data),
        _ => None,
    }
}
