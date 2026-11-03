//! System-wide Bluetooth audio settings that need administrator rights.
//!
//! Kord itself never runs elevated: when a change needs it, Kord starts a
//! second copy of itself with `kord admin …` through the standard UAC prompt,
//! which makes the one change and exits.
//!
//! - Low latency: Windows sends AirPods audio as AAC, which adds delay.
//!   Turning AAC off for Bluetooth makes Windows use SBC instead.
//! - Hands-free: the headphones' microphone comes with the hands-free (HFP)
//!   profile, which also lowers audio quality while in use. Some people turn
//!   it off to keep stereo quality at all times.

use anyhow::{Result, bail};

/// Whether these settings exist on this platform.
pub const SUPPORTED: bool = cfg!(windows);

pub fn run_admin(args: &[String]) -> Result<()> {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        ["aac", state @ ("on" | "off")] => platform::write_aac(*state == "on"),
        ["hfp", address, state @ ("on" | "off")] => {
            let address = u64::from_str_radix(address, 16)?;
            platform::set_service(address, *state == "on")
        }
        _ => bail!("unknown admin command {args:?}"),
    }
}

/// `Some(true)` when AAC is off for Bluetooth (low latency).
pub fn low_latency() -> Option<bool> {
    platform::read_aac().map(|aac| !aac)
}

pub fn set_low_latency(on: bool) -> Result<()> {
    platform::elevate(&["admin", "aac", if on { "off" } else { "on" }])
}

/// Whether the hands-free profile (and so the microphone) is enabled for this device.
pub fn hands_free(address: u64) -> Option<bool> {
    platform::service_enabled(address)
}

pub fn set_hands_free(address: u64, on: bool) -> Result<()> {
    match platform::set_service(address, on) {
        Ok(()) => Ok(()),
        // Usually needs administrator rights.
        Err(_) => platform::elevate(&["admin", "hfp", &format!("{address:012x}"), if on { "on" } else { "off" }]),
    }
}

#[cfg(windows)]
mod platform {
    use anyhow::{Context, Result, bail};
    use windows::Win32::Devices::Bluetooth::{
        BLUETOOTH_DEVICE_INFO, BluetoothEnumerateInstalledServices, BluetoothGetDeviceInfo, BluetoothSetServiceState,
    };
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Registry::{
        HKEY_LOCAL_MACHINE, REG_DWORD, RRF_RT_REG_DWORD, RegGetValueW, RegSetKeyValueW,
    };
    use windows::Win32::System::Threading::{GetExitCodeProcess, INFINITE, WaitForSingleObject};
    use windows::Win32::UI::Shell::{SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW};
    use windows::core::{GUID, PCWSTR, w};

    const A2DP_KEY: PCWSTR = w!("SYSTEM\\CurrentControlSet\\Services\\BthA2dp\\Parameters");
    const AAC_VALUE: PCWSTR = w!("BluetoothAacEnable");
    /// Hands-Free profile service class.
    const HANDS_FREE: GUID = GUID::from_u128(0x0000111e_0000_1000_8000_00805f9b34fb);

    /// Whether AAC is enabled; it is unless explicitly turned off.
    pub fn read_aac() -> Option<bool> {
        let mut value = 1u32;
        let mut size = size_of::<u32>() as u32;
        // SAFETY: reads a DWORD into `value`, whose size we pass.
        let result = unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                A2DP_KEY,
                AAC_VALUE,
                RRF_RT_REG_DWORD,
                None,
                Some(&mut value as *mut u32 as *mut _),
                Some(&mut size),
            )
        };
        Some(result.is_err() || value != 0)
    }

    pub fn write_aac(enabled: bool) -> Result<()> {
        let value = u32::from(enabled);
        // SAFETY: writes a DWORD from `value`, whose size we pass.
        unsafe {
            RegSetKeyValueW(
                HKEY_LOCAL_MACHINE,
                A2DP_KEY,
                AAC_VALUE,
                REG_DWORD.0,
                Some(&value as *const u32 as *const _),
                size_of::<u32>() as u32,
            )
            .ok()
            .context("writing the Bluetooth AAC setting")?;
        }
        Ok(())
    }

    fn device_info(address: u64) -> Option<BLUETOOTH_DEVICE_INFO> {
        let mut info = BLUETOOTH_DEVICE_INFO { dwSize: size_of::<BLUETOOTH_DEVICE_INFO>() as u32, ..Default::default() };
        info.Address.Anonymous.ullLong = address;
        // SAFETY: `info` is a properly sized, initialised struct.
        (unsafe { BluetoothGetDeviceInfo(None, &mut info) } == 0).then_some(info)
    }

    pub fn service_enabled(address: u64) -> Option<bool> {
        let info = device_info(address)?;
        let mut services = [GUID::zeroed(); 64];
        let mut count = services.len() as u32;
        // SAFETY: `services` has room for `count` GUIDs.
        let result = unsafe { BluetoothEnumerateInstalledServices(None, &info, &mut count, Some(services.as_mut_ptr())) };
        (result == 0).then(|| services[..count as usize].contains(&HANDS_FREE))
    }

    pub fn set_service(address: u64, enabled: bool) -> Result<()> {
        let info = device_info(address).context("Bluetooth device not found")?;
        // SAFETY: valid device info and GUID pointers.
        let result = unsafe { BluetoothSetServiceState(None, &info, &HANDS_FREE, u32::from(enabled)) };
        if result != 0 {
            bail!("changing the hands-free service failed (error {result})");
        }
        Ok(())
    }

    /// Runs this executable with `args` as administrator and waits for it.
    pub fn elevate(args: &[&str]) -> Result<()> {
        let exe = std::env::current_exe()?;
        let wide = |s: &str| s.encode_utf16().chain([0]).collect::<Vec<u16>>();
        let file = wide(&exe.to_string_lossy());
        let parameters = wide(&args.join(" "));
        let mut info = SHELLEXECUTEINFOW {
            cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOCLOSEPROCESS,
            lpVerb: w!("runas"),
            lpFile: PCWSTR(file.as_ptr()),
            lpParameters: PCWSTR(parameters.as_ptr()),
            nShow: 0,
            ..Default::default()
        };
        // SAFETY: the strings outlive the call; the process handle is waited on and closed.
        unsafe {
            ShellExecuteExW(&mut info).context("administrator permission was not granted")?;
            WaitForSingleObject(info.hProcess, INFINITE);
            let mut code = 1u32;
            let _ = GetExitCodeProcess(info.hProcess, &mut code);
            let _ = CloseHandle(info.hProcess);
            if code != 0 {
                bail!("the change failed (exit code {code})");
            }
        }
        Ok(())
    }
}

#[cfg(not(windows))]
mod platform {
    use anyhow::{Result, bail};

    pub fn read_aac() -> Option<bool> {
        None
    }

    pub fn write_aac(_enabled: bool) -> Result<()> {
        bail!("not supported on this platform")
    }

    pub fn service_enabled(_address: u64) -> Option<bool> {
        None
    }

    pub fn set_service(_address: u64, _enabled: bool) -> Result<()> {
        bail!("not supported on this platform")
    }

    pub fn elevate(_args: &[&str]) -> Result<()> {
        bail!("not supported on this platform")
    }
}
