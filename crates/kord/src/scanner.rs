//! Listens for headphone advertisements on the first Bluetooth adapter.

use std::collections::HashMap;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use btleplug::api::{BDAddr, Central, CentralEvent, Manager as _, Peripheral as _, ScanFilter};
use btleplug::platform::{Adapter, Manager, PeripheralId};
use futures::StreamExt;
use kord_core::DeviceStatus;
use tokio::sync::mpsc;
use tracing::{info, warn};

/// One decoded advertisement.
#[derive(Debug, Clone)]
pub struct Sighting {
    pub status: DeviceStatus,
    pub rssi: Option<i16>,
    /// Current (rotating) BLE address, only useful for debugging.
    pub address: Option<BDAddr>,
    pub company_id: u16,
    pub raw: Vec<u8>,
}

/// Scans forever, sending every headphone advertisement to `tx`. Survives the
/// adapter being turned off or missing by retrying every few seconds.
pub async fn run(tx: mpsc::Sender<Sighting>) {
    loop {
        match scan(&tx).await {
            Ok(()) => return, // receiver gone
            Err(err) => warn!("Bluetooth scan stopped: {err:#}"),
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}

async fn scan(tx: &mpsc::Sender<Sighting>) -> Result<()> {
    let manager = Manager::new().await.context("Bluetooth is not available")?;
    let adapter = manager.adapters().await?.into_iter().next().context("no Bluetooth adapter found")?;
    let mut events = adapter.events().await?;
    adapter.start_scan(ScanFilter::default()).await.context("could not start scanning")?;
    info!("scanning for headphones");

    while let Some(event) = events.next().await {
        let (id, data) = match event {
            CentralEvent::ManufacturerDataAdvertisement { id, manufacturer_data } => (id, Some(manufacturer_data)),
            // BlueZ only reports manufacturer data when it changes, so also
            // re-read it on generic updates (e.g. RSSI) to know the device is
            // still around.
            CentralEvent::DeviceUpdated(id) if cfg!(target_os = "linux") => (id, None),
            _ => continue,
        };
        for sighting in decode(&adapter, &id, data).await {
            if tx.send(sighting).await.is_err() {
                return Ok(());
            }
        }
    }
    bail!("adapter event stream ended")
}

async fn decode(adapter: &Adapter, id: &PeripheralId, data: Option<HashMap<u16, Vec<u8>>>) -> Vec<Sighting> {
    let mut properties = None;
    let data = match data {
        Some(data) => data,
        None => {
            properties = read_properties(adapter, id).await;
            match &properties {
                Some(p) => p.manufacturer_data.clone(),
                None => return Vec::new(),
            }
        }
    };

    let mut sightings = Vec::new();
    for (company_id, raw) in data {
        let Some(status) = kord_core::decode_advertisement(company_id, &raw) else { continue };
        // Only look up signal strength for advertisements we care about.
        if properties.is_none() {
            properties = read_properties(adapter, id).await;
        }
        sightings.push(Sighting {
            status,
            rssi: properties.as_ref().and_then(|p| p.rssi),
            address: properties.as_ref().map(|p| p.address),
            company_id,
            raw,
        });
    }
    sightings
}

async fn read_properties(adapter: &Adapter, id: &PeripheralId) -> Option<btleplug::api::PeripheralProperties> {
    adapter.peripheral(id).await.ok()?.properties().await.ok()?
}
