//! One Kord per user session. Starting Kord again (from the Start menu, a
//! shortcut…) asks the running one to open its window instead of starting a
//! second tray icon and a second scanner.

/// The running Kord, or a way to reach it.
pub enum Instance {
    /// This process is the only Kord; keep it alive to stay reachable.
    First(Listener),
    /// Another Kord is running.
    Second(Messenger),
}

pub fn acquire() -> Instance {
    match platform::acquire() {
        Ok(instance) => instance,
        Err(err) => {
            // Better two Kords than none.
            tracing::warn!("could not check for another running Kord: {err:#}");
            Instance::First(Listener(None))
        }
    }
}

pub struct Listener(Option<platform::Listener>);

impl Listener {
    /// Calls `on_show` on a background thread whenever another start asks for the window.
    pub fn listen(self, on_show: impl Fn() + Send + 'static) {
        if let Some(listener) = self.0 {
            platform::listen(listener, on_show);
        }
    }
}

pub struct Messenger(platform::Messenger);

impl Messenger {
    /// Asks the running Kord to open its window.
    pub fn show(self) {
        if let Err(err) = platform::show(self.0) {
            tracing::warn!("could not reach the running Kord: {err:#}");
        }
    }
}

#[cfg(windows)]
mod platform {
    use anyhow::Result;
    use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError, HANDLE};
    use windows::Win32::System::Threading::{CreateEventW, INFINITE, SetEvent, WaitForSingleObject};
    use windows::core::w;

    use super::{Instance, Messenger as Wrapped};

    /// A named auto-reset event: signalled by later starts, waited on by the first.
    pub struct Listener(usize);
    pub struct Messenger(usize);

    pub fn acquire() -> Result<Instance> {
        // SAFETY: a plain named-event creation; the handle is kept for the
        // process lifetime (or until signalled) and never closed twice.
        unsafe {
            let event = CreateEventW(None, false, false, w!("Local\\Kord.ShowWindow"))?;
            let existed = GetLastError() == ERROR_ALREADY_EXISTS;
            // Handles are plain integers; carried as such across threads.
            let raw = event.0 as usize;
            Ok(if existed { Instance::Second(Wrapped(Messenger(raw))) } else { Instance::First(super::Listener(Some(Listener(raw)))) })
        }
    }

    pub fn listen(listener: Listener, on_show: impl Fn() + Send + 'static) {
        let raw = listener.0;
        let spawned = std::thread::Builder::new().name("kord-instance".into()).spawn(move || {
            let event = HANDLE(raw as *mut _);
            // SAFETY: `event` stays open for the life of the process.
            while unsafe { WaitForSingleObject(event, INFINITE) }.0 == 0 {
                on_show();
            }
        });
        if let Err(err) = spawned {
            tracing::warn!("could not listen for other Kord starts: {err}");
        }
    }

    pub fn show(messenger: Messenger) -> Result<()> {
        // SAFETY: the handle came from CreateEventW in this process.
        unsafe { SetEvent(HANDLE(messenger.0 as *mut _))? };
        Ok(())
    }
}

#[cfg(unix)]
mod platform {
    use std::io::Write;
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::PathBuf;

    use anyhow::{Context, Result};

    use super::{Instance, Messenger as Wrapped};

    pub struct Listener(UnixListener);
    pub struct Messenger(UnixStream);

    fn socket_path() -> PathBuf {
        let dir = std::env::var_os("XDG_RUNTIME_DIR").map_or_else(std::env::temp_dir, PathBuf::from);
        dir.join("kord.sock")
    }

    pub fn acquire() -> Result<Instance> {
        let path = socket_path();
        if let Ok(stream) = UnixStream::connect(&path) {
            return Ok(Instance::Second(Wrapped(Messenger(stream))));
        }
        // Nobody answers: a stale socket from a Kord that didn't exit cleanly.
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path).with_context(|| format!("binding {}", path.display()))?;
        Ok(Instance::First(super::Listener(Some(Listener(listener)))))
    }

    pub fn listen(listener: Listener, on_show: impl Fn() + Send + 'static) {
        let spawned = std::thread::Builder::new().name("kord-instance".into()).spawn(move || {
            for stream in listener.0.incoming() {
                if stream.is_ok() {
                    on_show();
                }
            }
        });
        if let Err(err) = spawned {
            tracing::warn!("could not listen for other Kord starts: {err}");
        }
    }

    pub fn show(mut messenger: Messenger) -> Result<()> {
        messenger.0.write_all(b"show\n")?;
        Ok(())
    }
}

#[cfg(not(any(windows, unix)))]
mod platform {
    use anyhow::{Result, bail};

    use super::Instance;

    pub struct Listener;
    pub struct Messenger;

    pub fn acquire() -> Result<Instance> {
        bail!("not supported on this platform")
    }

    pub fn listen(_: Listener, _: impl Fn() + Send + 'static) {}

    pub fn show(_: Messenger) -> Result<()> {
        Ok(())
    }
}
