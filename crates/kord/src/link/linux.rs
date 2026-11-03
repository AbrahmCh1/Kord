//! BlueZ over D-Bus: every paired device is an `org.bluez.Device1` object
//! whose `Modalias` ("bluetooth:v004Cp2014d…") carries the Device ID.

use std::collections::HashMap;

use anyhow::{Context, Result};
use zbus::blocking::fdo::ObjectManagerProxy;
use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{OwnedValue, Value};

use super::{KnownDevice, Link, PairedDevice};

/// The device's D-Bus object path, e.g. `/org/bluez/hci0/dev_30_82_16_A2_06_15`.
pub type Handle = String;

const DEVICE_INTERFACE: &str = "org.bluez.Device1";

/// Properties of every paired BlueZ device, by object path.
fn paired_objects(conn: &Connection) -> Result<Vec<(String, HashMap<String, OwnedValue>)>> {
    let objects = ObjectManagerProxy::builder(conn).destination("org.bluez")?.path("/")?.build()?.get_managed_objects()?;
    Ok(objects
        .into_iter()
        .filter_map(|(path, interfaces)| {
            let props = interfaces.into_iter().find(|(name, _)| name.as_str() == DEVICE_INTERFACE)?.1;
            let paired = props.get("Paired").and_then(|v| bool::try_from(&**v).ok()).unwrap_or(false);
            paired.then(|| (path.to_string(), props))
        })
        .collect())
}

fn device_ids(props: &HashMap<String, OwnedValue>) -> Option<(u16, u16)> {
    props.get("Modalias").and_then(|v| <&str>::try_from(&**v).ok()).and_then(parse_modalias)
}

pub fn find(vendor_id: u16, product_id: u16) -> Result<Link> {
    let conn = Connection::system().context("D-Bus system bus")?;
    for (path, props) in paired_objects(&conn)? {
        if device_ids(&props) == Some((vendor_id, product_id)) {
            return Ok(Link::Paired(paired_device(path, &props)));
        }
    }
    Ok(Link::NotPaired)
}

pub fn paired_devices() -> Result<Vec<KnownDevice>> {
    let conn = Connection::system().context("D-Bus system bus")?;
    Ok(paired_objects(&conn)?
        .into_iter()
        .filter(|(_, props)| {
            // Only audio devices: BlueZ icons are "audio-headphones", "audio-headset"…
            props.get("Icon").and_then(|v| <&str>::try_from(&**v).ok()).is_some_and(|icon| icon.starts_with("audio"))
        })
        .filter_map(|(path, props)| {
            let (vendor_id, product_id) = device_ids(&props)?;
            Some(KnownDevice { vendor_id, product_id, device: paired_device(path, &props) })
        })
        .collect())
}

fn paired_device(path: String, props: &HashMap<String, OwnedValue>) -> PairedDevice {
    let get = |key: &str| props.get(key).map(|v| &**v);
    let text = |v: &Value<'_>| <&str>::try_from(v).ok().map(str::to_owned);
    let name = get("Alias").and_then(text).or_else(|| get("Name").and_then(text)).unwrap_or_default();
    let connected = get("Connected").and_then(|v| bool::try_from(v).ok()).unwrap_or(false);
    PairedDevice { name, connected, handle: path }
}

pub fn paired_ids() -> Result<Vec<(u16, u16)>> {
    let conn = Connection::system().context("D-Bus system bus")?;
    Ok(paired_objects(&conn)?.iter().filter_map(|(_, props)| device_ids(props)).collect())
}

pub fn connect(device: &PairedDevice) -> Result<()> {
    call(device, "Connect")
}

pub fn disconnect(device: &PairedDevice) -> Result<()> {
    call(device, "Disconnect")
}

/// Sets the volume of the headphones' PulseAudio/PipeWire sink, whose name
/// contains the Bluetooth address (`bluez_output.30_82_16_A2_06_15.1`).
pub fn set_volume(device: &PairedDevice, percent: u8) -> Result<()> {
    let address = device.handle.rsplit("dev_").next().unwrap_or_default();
    let sinks = std::process::Command::new("pactl").args(["list", "short", "sinks"]).output().context("running pactl")?;
    let sinks = String::from_utf8_lossy(&sinks.stdout);
    let sink = sinks
        .lines()
        .filter_map(|line| line.split_whitespace().nth(1))
        .find(|name| name.starts_with("bluez") && name.contains(address))
        .with_context(|| format!("no audio sink for {}", device.name))?;
    std::process::Command::new("pactl").args(["set-sink-volume", sink, &format!("{}%", percent.min(100))]).status()?;
    Ok(())
}

fn call(device: &PairedDevice, method: &str) -> Result<()> {
    let conn = Connection::system()?;
    let proxy = Proxy::new(&conn, "org.bluez", device.handle.as_str(), DEVICE_INTERFACE)?;
    proxy.call_method(method, &()).with_context(|| format!("{method} {}", device.name))?;
    Ok(())
}

/// Parses "bluetooth:v004Cp2014d0B15" into (0x004C, 0x2014).
fn parse_modalias(modalias: &str) -> Option<(u16, u16)> {
    let rest = modalias.strip_prefix("bluetooth:v")?;
    let vendor = u16::from_str_radix(rest.get(0..4)?, 16).ok()?;
    let product = u16::from_str_radix(rest.get(4..)?.strip_prefix('p')?.get(0..4)?, 16).ok()?;
    Some((vendor, product))
}

#[cfg(test)]
mod tests {
    #[test]
    fn parses_modalias() {
        assert_eq!(super::parse_modalias("bluetooth:v004Cp2014d0B15"), Some((0x004C, 0x2014)));
        assert_eq!(super::parse_modalias("usb:v1D6Bp0246d0555"), None);
    }
}
