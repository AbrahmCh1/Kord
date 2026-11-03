//! Start Kord when the user logs in: a `Run` registry value on Windows, an
//! XDG autostart entry on Linux. The OS is the source of truth, so this is
//! not part of the settings file.

use anyhow::{Context, Result};

pub fn is_enabled() -> bool {
    platform::is_enabled()
}

pub fn set_enabled(enabled: bool) -> Result<()> {
    let exe = std::env::current_exe().context("locating kord")?;
    platform::set_enabled(enabled, &exe)
}

/// Rewrites an existing entry, so it points at this executable and starts
/// Kord in the tray only (older versions opened the window at login).
pub fn refresh() {
    if is_enabled()
        && let Err(err) = set_enabled(true)
    {
        tracing::warn!("could not update autostart: {err:#}");
    }
}

#[cfg(windows)]
mod platform {
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    use anyhow::Result;
    use windows::Win32::System::Registry::{
        HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW,
    };
    use windows::core::w;

    const RUN_KEY: windows::core::PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
    const VALUE: windows::core::PCWSTR = w!("Kord");

    pub fn is_enabled() -> bool {
        // SAFETY: only queries whether the value exists; no buffers passed.
        unsafe { RegGetValueW(HKEY_CURRENT_USER, RUN_KEY, VALUE, RRF_RT_REG_SZ, None, None, None).is_ok() }
    }

    pub fn set_enabled(enabled: bool, exe: &Path) -> Result<()> {
        if !enabled {
            // SAFETY: plain registry call with static strings.
            let result = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN_KEY, VALUE) };
            // Already absent is fine.
            if result.is_err() && is_enabled() {
                result.ok()?;
            }
            return Ok(());
        }
        let command: Vec<u16> =
            std::ffi::OsString::from(format!("\"{}\" tray", exe.display())).encode_wide().chain([0]).collect();
        // SAFETY: `command` is a NUL-terminated UTF-16 buffer of the size we pass.
        unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                RUN_KEY,
                VALUE,
                REG_SZ.0,
                Some(command.as_ptr().cast()),
                (command.len() * 2) as u32,
            )
            .ok()?;
        }
        Ok(())
    }
}

#[cfg(not(windows))]
mod platform {
    use std::path::{Path, PathBuf};

    use anyhow::{Context, Result};

    fn entry() -> Option<PathBuf> {
        directories::BaseDirs::new().map(|dirs| dirs.config_dir().join("autostart").join("kord.desktop"))
    }

    pub fn is_enabled() -> bool {
        entry().is_some_and(|path| path.exists())
    }

    pub fn set_enabled(enabled: bool, exe: &Path) -> Result<()> {
        let path = entry().context("no config directory")?;
        if !enabled {
            if path.exists() {
                std::fs::remove_file(&path)?;
            }
            return Ok(());
        }
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let desktop = format!(
            "[Desktop Entry]\nType=Application\nName=Kord\nComment=AirPods and wireless headphones companion\nExec=\"{}\" tray\nX-GNOME-Autostart-enabled=true\n",
            exe.display()
        );
        std::fs::write(&path, desktop)?;
        Ok(())
    }
}
