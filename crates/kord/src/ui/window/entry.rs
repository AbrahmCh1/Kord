//! The headphones listed in the window: every pairing on this computer, plus
//! the live status of the ones advertising nearby.

use kord_core::{DeviceKind, DeviceStatus, apple};

use crate::engine::Snapshot;
use crate::i18n::strings;
use crate::link::{KnownDevice, Link, PairedDevice};
use crate::ui::art::{self, Art, Family};

/// Id of the entry for headphones that are nearby but not paired here.
pub const NEARBY: &str = "nearby";

#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub id: String,
    pub name: String,
    pub model: String,
    pub model_id: Option<u16>,
    pub kind: DeviceKind,
    /// `None` for headphones that are only nearby.
    pub paired: Option<PairedDevice>,
    /// What the headphones advertise right now, if they're nearby.
    pub status: Option<DeviceStatus>,
}

impl Entry {
    fn from_known(known: &KnownDevice) -> Self {
        let model_id = known.model_id();
        let model = model_id.and_then(apple::lookup_model);
        let device = &known.device;
        let family = Family::from_name(&device.name);
        // Other brands: the name tells in-ear from over-ear, at best.
        let in_ear = match family {
            Family::Generic => {
                let name = device.name.to_lowercase();
                ["buds", "tws", "earphone", "in-ear", "pods"].iter().any(|word| name.contains(word))
            }
            family => !family.over_ear(),
        };
        let kind = if in_ear { DeviceKind::Earbuds } else { DeviceKind::Headphones };
        Self {
            id: device.id(),
            name: if device.name.is_empty() { model.map_or(strings().generic_headphones, |m| m.name).to_string() } else { device.name.clone() },
            model: model.map_or_else(|| family.brand().unwrap_or(strings().generic_headphones), |m| m.name).to_string(),
            model_id: model.map(|m| m.id),
            kind: model.map_or(kind, |m| m.kind),
            paired: Some(device.clone()),
            status: None,
        }
    }

    /// AirPods and Beats: Kord reads their battery and ear sensors.
    pub fn apple(&self) -> bool {
        self.model_id.is_some()
    }

    pub fn connected(&self) -> bool {
        self.paired.as_ref().is_some_and(|p| p.connected)
    }

    /// "Connected", "Not connected · Nearby"…
    pub fn status_text(&self) -> String {
        let s = strings();
        let link = match &self.paired {
            Some(paired) if paired.connected => s.connected,
            Some(_) => s.not_connected,
            None => s.not_paired,
        };
        if self.status.is_some() && !self.connected() { format!("{link} · {}", s.nearby) } else { link.to_string() }
    }

    /// The illustrations for this model, drawn side by side.
    pub fn arts(&self) -> Vec<Art> {
        let family = match self.model_id.map(Family::of) {
            Some(Family::Generic) | None => Family::from_name(&self.name),
            Some(family) => family,
        };
        let lid_open = self.status.as_ref().is_some_and(|s| s.lid_open == Some(true));
        art::arts(family, self.kind, lid_open)
    }
}

/// Merges the pairings with the engine's view of the nearby headphones.
/// Connected headphones come first, then nearby ones, then the rest.
pub fn entries(known: &[KnownDevice], snapshot: Option<&Snapshot>) -> Vec<Entry> {
    let mut entries: Vec<Entry> = known.iter().map(Entry::from_known).collect();
    if let Some(snapshot) = snapshot {
        match &snapshot.link {
            Link::Paired(paired) => {
                let id = paired.id();
                match entries.iter_mut().find(|e| e.id == id) {
                    Some(entry) => {
                        // The engine polls the link more often than the list is refreshed.
                        entry.paired = Some(paired.clone());
                        entry.status = Some(snapshot.device.clone());
                    }
                    None => entries.push(nearby(snapshot, Some(paired.clone()), id)),
                }
            }
            Link::NotPaired | Link::Unknown => entries.push(nearby(snapshot, None, NEARBY.to_string())),
        }
    }
    entries.sort_by_key(|e| (!e.connected(), e.status.is_none(), e.name.to_lowercase()));
    entries
}

fn nearby(snapshot: &Snapshot, paired: Option<PairedDevice>, id: String) -> Entry {
    let device = &snapshot.device;
    Entry {
        id,
        name: snapshot.display_name().to_string(),
        model: device.model_name.clone(),
        model_id: Some(device.model_id),
        kind: device.kind,
        paired,
        status: Some(device.clone()),
    }
}
