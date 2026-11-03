//! The classic-Bluetooth side: is the pairing on this computer, is it
//! connected right now, and connecting/disconnecting it.
//!
//! BLE advertisements tell us the headphones are *nearby*; this tells us
//! whether they are actually playing *this computer's* audio, which is what
//! ear detection and the Connect button need.
//!
//! All calls block for a few milliseconds; run them off the async executor.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(windows)]
mod windows;

#[cfg(target_os = "linux")]
use self::linux as platform;
#[cfg(windows)]
use self::windows as platform;

#[cfg(any(windows, target_os = "linux"))]
type Handle = platform::Handle;
#[cfg(not(any(windows, target_os = "linux")))]
type Handle = ();

/// How the headphones relate to this computer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Link {
    /// Not looked up yet, or this platform can't tell.
    Unknown,
    /// No pairing for this model on this computer.
    NotPaired,
    Paired(PairedDevice),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairedDevice {
    /// The name the user gave the headphones, e.g. "Abraham's AirPods".
    pub name: String,
    pub connected: bool,
    /// Platform-specific way to reach the device.
    pub(crate) handle: Handle,
}

impl PairedDevice {
    /// Bluetooth address, where the platform reports it.
    pub fn address(&self) -> Option<u64> {
        #[cfg(windows)]
        return self.handle.address();
        #[cfg(not(windows))]
        None
    }

    /// Id of the audio output endpoint while connected (see [`crate::audio`]).
    pub fn output_endpoint(&self) -> Option<String> {
        #[cfg(windows)]
        return self.handle.output_endpoint();
        #[cfg(not(windows))]
        None
    }

    /// Id of the microphone endpoint while connected with hands-free enabled.
    pub fn input_endpoint(&self) -> Option<String> {
        #[cfg(windows)]
        return self.handle.input_endpoint();
        #[cfg(not(windows))]
        None
    }

    /// Stable identity of the pairing, to tell devices of the same model apart.
    pub fn id(&self) -> String {
        #[cfg(windows)]
        return format!("{:012X}", self.handle.address().unwrap_or_default());
        #[cfg(target_os = "linux")]
        return self.handle.clone();
        #[cfg(not(any(windows, target_os = "linux")))]
        self.name.clone()
    }

    /// A device that only exists for previews and tests.
    #[allow(dead_code)]
    pub fn sample(name: &str, connected: bool) -> Self {
        Self { name: name.to_string(), connected, handle: Handle::default() }
    }
}

/// Headphones paired with this computer, with their Bluetooth Device ID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownDevice {
    pub vendor_id: u16,
    pub product_id: u16,
    pub device: PairedDevice,
}

impl KnownDevice {
    /// The model id Apple advertises over BLE, for Apple and Beats headphones.
    pub fn model_id(&self) -> Option<u16> {
        // The advertised id is the Device ID's product id with its bytes swapped.
        (self.vendor_id == kord_core::apple::COMPANY_ID).then(|| self.product_id.swap_bytes())
    }
}

impl Link {
    pub fn is_connected(&self) -> bool {
        matches!(self, Link::Paired(device) if device.connected)
    }

    /// Whether audio may be playing through these headphones from here.
    /// Platforms that can't tell get the benefit of the doubt.
    pub fn may_play_here(&self) -> bool {
        match self {
            Link::Unknown => true,
            Link::NotPaired => false,
            Link::Paired(device) => device.connected,
        }
    }
}

/// Looks up the paired device with this Bluetooth Device ID.
pub fn find(vendor_id: u16, product_id: u16) -> anyhow::Result<Link> {
    #[cfg(any(windows, target_os = "linux"))]
    return platform::find(vendor_id, product_id);
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = (vendor_id, product_id);
        Ok(Link::Unknown)
    }
}

/// Device IDs (vendor, product) of all headphones paired with this computer.
pub fn paired_ids() -> anyhow::Result<Vec<(u16, u16)>> {
    #[cfg(any(windows, target_os = "linux"))]
    return platform::paired_ids();
    #[cfg(not(any(windows, target_os = "linux")))]
    Ok(Vec::new())
}

/// Every pair of headphones paired with this computer.
pub fn paired_devices() -> anyhow::Result<Vec<KnownDevice>> {
    #[cfg(any(windows, target_os = "linux"))]
    return platform::paired_devices();
    #[cfg(not(any(windows, target_os = "linux")))]
    Ok(Vec::new())
}

pub fn connect(device: &PairedDevice) -> anyhow::Result<()> {
    #[cfg(any(windows, target_os = "linux"))]
    return platform::connect(device);
    #[cfg(not(any(windows, target_os = "linux")))]
    anyhow::bail!("connecting {} is not supported on this platform", device.name)
}

pub fn disconnect(device: &PairedDevice) -> anyhow::Result<()> {
    #[cfg(any(windows, target_os = "linux"))]
    return platform::disconnect(device);
    #[cfg(not(any(windows, target_os = "linux")))]
    anyhow::bail!("disconnecting {} is not supported on this platform", device.name)
}

/// Sets the output volume of the headphones themselves (not the default device).
pub fn set_volume(device: &PairedDevice, percent: u8) -> anyhow::Result<()> {
    #[cfg(any(windows, target_os = "linux"))]
    return platform::set_volume(device, percent);
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = percent;
        anyhow::bail!("setting the volume of {} is not supported on this platform", device.name)
    }
}

/// Opens the OS Bluetooth settings so the user can pair.
pub fn open_bluetooth_settings() {
    #[cfg(windows)]
    let result = std::process::Command::new("explorer.exe").arg("ms-settings:bluetooth").spawn();
    #[cfg(not(windows))]
    let result = [
        ("gnome-control-center", &["bluetooth"][..]),
        ("systemsettings", &["kcm_bluetooth"][..]),
        ("blueman-manager", &[][..]),
    ]
    .iter()
    .find_map(|(program, args)| std::process::Command::new(program).args(*args).spawn().ok())
    .ok_or_else(|| std::io::Error::other("no Bluetooth settings app found"));
    if let Err(err) = result {
        tracing::warn!("could not open Bluetooth settings: {err}");
    }
}
