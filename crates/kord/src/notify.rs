//! Desktop notifications.

use kord_core::battery::Component;
use tracing::warn;

use crate::i18n::strings;

pub fn connection(device_name: &str, connected: bool) {
    let s = strings();
    show(device_name, if connected { s.connected } else { s.disconnected });
}

fn show(summary: &str, body: &str) {
    if let Err(err) = notify_rust::Notification::new().appname("Kord").summary(summary).body(body).show() {
        warn!("could not show notification: {err}");
    }
}

pub fn low_battery(device_name: &str, component: Component, level: u8) {
    let s = strings();
    let part = match component {
        Component::Left => s.left,
        Component::Right => s.right,
        Component::Headphones => s.battery,
    };
    show(&format!("{device_name}: {}", s.low_battery_title), &format!("{part}: {level}%"));
}
