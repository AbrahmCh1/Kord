//! Windows has no public "connect this Bluetooth device" API. What the
//! Settings app does for headphones is send a one-shot reconnect request to
//! the Bluetooth audio driver through the audio endpoint's kernel-streaming
//! filter, and that's what we do too. The filter's device path also carries
//! the Device ID (`_vid&0001004c_pid&2014`) and Bluetooth address, which is
//! how we find the right headphones, and the endpoint's state tells us
//! whether they are connected.

use anyhow::{Context, Result, bail};
use windows::Devices::Bluetooth::BluetoothDevice;
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::{
    DEVICE_STATE, DEVICE_STATE_ACTIVE, DEVICE_STATEMASK_ALL, IDeviceTopology, IMMDevice, IMMDeviceEnumerator,
    IMMEndpoint, IPart, MMDeviceEnumerator, eAll, eRender,
};
use windows::Win32::Media::KernelStreaming::{
    IKsControl, KSIDENTIFIER, KSIDENTIFIER_0, KSIDENTIFIER_0_0, KSPROPERTY_BTAUDIO, KSPROPERTY_ONESHOT_DISCONNECT,
    KSPROPERTY_ONESHOT_RECONNECT, KSPROPERTY_TYPE_GET, KSPROPSETID_BtAudio,
};
use windows::Win32::System::Com::{CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree};
use windows::core::{HSTRING, Interface, PWSTR};

use super::{KnownDevice, Link, PairedDevice};

/// The audio endpoints (A2DP headphones, hands-free headset…) of one device.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Handle {
    endpoints: Vec<EndpointRef>,
    address: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EndpointRef {
    endpoint_id: String,
    filter_id: String,
    render: bool,
    active: bool,
}

impl Handle {
    pub fn address(&self) -> Option<u64> {
        Some(self.address)
    }

    /// The endpoint audio plays through: the active one, preferring stereo
    /// (A2DP) over hands-free, which Windows lists first.
    pub fn output_endpoint(&self) -> Option<String> {
        self.endpoint(true)
    }

    /// The hands-free microphone, if the device has one right now.
    pub fn input_endpoint(&self) -> Option<String> {
        self.endpoint(false)
    }

    fn endpoint(&self, render: bool) -> Option<String> {
        let mut candidates: Vec<&EndpointRef> = self.endpoints.iter().filter(|e| e.render == render && e.active).collect();
        // A2DP filters are the 0000110b service; hands-free ones are 0000111e.
        candidates.sort_by_key(|e| !e.filter_id.to_ascii_lowercase().contains("0000110b"));
        candidates.first().map(|e| e.endpoint_id.clone())
    }
}

struct Endpoint {
    reference: EndpointRef,
    vendor_id: u16,
    product_id: u16,
    address: u64,
}

pub fn find(vendor_id: u16, product_id: u16) -> Result<Link> {
    let matching: Vec<Endpoint> =
        endpoints()?.into_iter().filter(|e| e.vendor_id == vendor_id && e.product_id == product_id).collect();
    Ok(paired_device(matching).map_or(Link::NotPaired, Link::Paired))
}

pub fn paired_devices() -> Result<Vec<KnownDevice>> {
    let mut by_address: Vec<Vec<Endpoint>> = Vec::new();
    for endpoint in endpoints()? {
        match by_address.iter_mut().find(|group| group[0].address == endpoint.address) {
            Some(group) => group.push(endpoint),
            None => by_address.push(vec![endpoint]),
        }
    }
    Ok(by_address
        .into_iter()
        .filter_map(|group| {
            let (vendor_id, product_id) = (group[0].vendor_id, group[0].product_id);
            Some(KnownDevice { vendor_id, product_id, device: paired_device(group)? })
        })
        .collect())
}

/// One device from the audio endpoints that belong to it.
fn paired_device(endpoints: Vec<Endpoint>) -> Option<PairedDevice> {
    let address = endpoints.first()?.address;
    let name = BluetoothDevice::FromBluetoothAddressAsync(address)
        .and_then(|op| op.join())
        .and_then(|device| device.Name())
        .map(|name| name.to_string().trim().to_owned())
        .unwrap_or_default();
    Some(PairedDevice {
        name,
        connected: endpoints.iter().any(|e| e.reference.active),
        handle: Handle { endpoints: endpoints.into_iter().map(|e| e.reference).collect(), address },
    })
}

pub fn paired_ids() -> Result<Vec<(u16, u16)>> {
    let mut ids: Vec<(u16, u16)> = endpoints()?.iter().map(|e| (e.vendor_id, e.product_id)).collect();
    ids.sort_unstable();
    ids.dedup();
    Ok(ids)
}

pub fn connect(device: &PairedDevice) -> Result<()> {
    oneshot(device, KSPROPERTY_ONESHOT_RECONNECT)
}

pub fn disconnect(device: &PairedDevice) -> Result<()> {
    oneshot(device, KSPROPERTY_ONESHOT_DISCONNECT)
}

pub fn set_volume(device: &PairedDevice, percent: u8) -> Result<()> {
    let enumerator = enumerator()?;
    let level = f32::from(percent.min(100)) / 100.0;
    for endpoint in device.handle.endpoints.iter().filter(|e| e.render) {
        // SAFETY: plain COM calls; a null event context is allowed.
        unsafe {
            let volume: IAudioEndpointVolume =
                enumerator.GetDevice(&HSTRING::from(endpoint.endpoint_id.as_str()))?.Activate(CLSCTX_ALL, None)?;
            volume.SetMasterVolumeLevelScalar(level, std::ptr::null())?;
        }
    }
    Ok(())
}

fn enumerator() -> Result<IMMDeviceEnumerator> {
    // SAFETY: COM initialisation and object creation have no preconditions;
    // initialising an already initialised thread is harmless.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        Ok(CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?)
    }
}

fn endpoints() -> Result<Vec<Endpoint>> {
    let enumerator = enumerator()?;
    let mut found = Vec::new();
    // SAFETY: plain COM calls on valid interfaces.
    unsafe {
        let collection = enumerator.EnumAudioEndpoints(eAll, DEVICE_STATE(DEVICE_STATEMASK_ALL))?;
        for index in 0..collection.GetCount()? {
            let Ok(device) = collection.Item(index) else { continue };
            let Some(filter_id) = ks_filter_id(&device) else { continue };
            let Some((vendor_id, product_id, address)) = parse_filter_id(&filter_id) else { continue };
            let Some(endpoint_id) = device.GetId().ok().and_then(|id| take_string(id)) else { continue };
            let render = device.cast::<IMMEndpoint>().and_then(|e| e.GetDataFlow()).is_ok_and(|flow| flow == eRender);
            let active = device.GetState().is_ok_and(|state| state == DEVICE_STATE_ACTIVE);
            found.push(Endpoint {
                reference: EndpointRef { endpoint_id, filter_id, render, active },
                vendor_id,
                product_id,
                address,
            });
        }
    }
    Ok(found)
}

/// Converts and frees a COM-allocated string.
unsafe fn take_string(text: PWSTR) -> Option<String> {
    // SAFETY: `text` was allocated by COM and is freed exactly once here.
    unsafe {
        let value = text.to_string().ok();
        CoTaskMemFree(Some(text.0 as *const _));
        value
    }
}

/// Device id of the kernel-streaming filter (the driver side) behind an endpoint.
unsafe fn ks_filter_id(endpoint: &IMMDevice) -> Option<String> {
    // SAFETY: plain COM calls on valid interfaces.
    unsafe {
        let topology: IDeviceTopology = endpoint.Activate(CLSCTX_ALL, None).ok()?;
        let filter = topology.GetConnector(0).ok()?.GetConnectedTo().ok()?.cast::<IPart>().ok()?.GetTopologyObject().ok()?;
        take_string(filter.GetDeviceId().ok()?)
    }
}

fn oneshot(device: &PairedDevice, request: KSPROPERTY_BTAUDIO) -> Result<()> {
    let enumerator = enumerator()?;
    let mut filters: Vec<&str> = device.handle.endpoints.iter().map(|e| e.filter_id.as_str()).collect();
    filters.dedup();
    let mut last_error = None;
    for filter_id in filters {
        // SAFETY: the property struct lives across the call, has the size we
        // pass, and this property takes no data buffer.
        let result = unsafe {
            enumerator.GetDevice(&HSTRING::from(filter_id)).and_then(|filter| {
                let control: IKsControl = filter.Activate(CLSCTX_ALL, None)?;
                let property = KSIDENTIFIER {
                    Anonymous: KSIDENTIFIER_0 {
                        Anonymous: KSIDENTIFIER_0_0 {
                            Set: KSPROPSETID_BtAudio,
                            Id: request.0 as u32,
                            Flags: KSPROPERTY_TYPE_GET,
                        },
                    },
                };
                let mut returned = 0u32;
                control.KsProperty(
                    &property,
                    size_of::<KSIDENTIFIER>() as u32,
                    std::ptr::null_mut(),
                    0,
                    &mut returned,
                )
            })
        };
        match result {
            // One accepted request is enough: the driver (re)connects the whole device.
            Ok(()) => return Ok(()),
            Err(err) => last_error = Some(err),
        }
    }
    match last_error {
        Some(err) => Err(err).with_context(|| format!("Bluetooth request for {}", device.name)),
        None => bail!("{} has no audio endpoint", device.name),
    }
}

/// Extracts (vendor id, product id, Bluetooth address) from a filter id like
/// `{2}.\\?\bthenum#{0000110b-…}_vid&0001004c_pid&2014#b&63b6b4e&0&308216a20615_c00000000#{…}`.
fn parse_filter_id(id: &str) -> Option<(u16, u16, u64)> {
    let id = id.to_ascii_lowercase();
    if !id.contains("bthenum") {
        return None;
    }
    let hex_after = |marker: &str, len: usize| -> Option<&str> {
        let start = id.find(marker)? + marker.len();
        id.get(start..start + len)
    };
    // The vendor id is prefixed by its source (0001 = Bluetooth SIG).
    let vendor = u16::from_str_radix(hex_after("_vid&", 8)?.get(4..)?, 16).ok()?;
    let product = u16::from_str_radix(hex_after("_pid&", 4)?, 16).ok()?;
    let instance = id.split('#').nth(2)?;
    let address = instance.rsplit('&').next()?.split('_').next()?;
    let address = u64::from_str_radix(address, 16).ok().filter(|_| address.len() == 12)?;
    Some((vendor, product, address))
}

#[cfg(test)]
mod tests {
    #[test]
    fn parses_bluetooth_filter_ids() {
        let id = r"{2}.\\?\bthenum#{0000110b-0000-1000-8000-00805f9b34fb}_vid&0001004c_pid&2014#b&63b6b4e&0&308216a20615_c00000000#{6994ad04-93ef-11d0-a3cc-00a0c9223196}\a2dp";
        assert_eq!(super::parse_filter_id(id), Some((0x004C, 0x2014, 0x308216A20615)));
        assert_eq!(super::parse_filter_id(r"{2}.\\?\hdaudio#func_01&ven_10ec#4&1&0001#{6994ad04}"), None);
    }
}
