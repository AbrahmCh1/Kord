// Release builds are tray apps without a console window on Windows.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod autostart;
mod config;
mod engine;
mod i18n;
mod icon;
mod instance;
mod link;
mod media;
mod notify;
mod audio;
mod scanner;
mod system;
mod tray;
mod ui;

use std::collections::HashMap;
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use kord_core::{Battery, DeviceStatus};
use tokio::sync::{mpsc, watch};
use tracing::info;
use tracing_subscriber::EnvFilter;

use crate::config::Config;

/// Lightweight companion for AirPods and other wireless headphones.
#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Run in the system tray. Without a command, Kord also opens its window.
    Tray {
        /// Open the status popup right away.
        #[arg(long)]
        show: bool,
        /// Open the main window right away.
        #[arg(long, alias = "settings")]
        open: bool,
    },
    /// Run ear detection without a tray icon, logging to the terminal.
    Headless,
    /// Print every headphone advertisement nearby. Useful to find your model id
    /// or to report an unsupported device.
    Scan {
        /// Print every advertisement with its arrival time, not only changes.
        #[arg(long)]
        all: bool,
    },
    /// Show whether the headphones of a model are paired/connected here, and
    /// optionally connect or disconnect them.
    Device {
        /// Model id as shown by `kord scan`, e.g. 0x1420.
        model: String,
        #[arg(long, conflicts_with = "disconnect")]
        connect: bool,
        #[arg(long)]
        disconnect: bool,
    },
    /// Print the settings file location.
    ConfigPath,
    /// Render sample popups to PNG files in a directory (for screenshots).
    #[command(hide = true)]
    RenderPopup { dir: std::path::PathBuf },
    /// Write the logo as PNG and the Windows .ico into a directory.
    #[command(hide = true)]
    RenderIcons { dir: std::path::PathBuf },
    /// Privileged system changes, run elevated by Kord itself (UAC prompt).
    #[command(hide = true)]
    Admin {
        #[arg(trailing_var_arg = true)]
        args: Vec<String>,
    },
}

fn main() -> ExitCode {
    #[cfg(windows)]
    attach_parent_console();

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("kord=info,warn")))
        .init();

    let cli = Cli::parse();
    let result = match cli.command.unwrap_or(Command::Tray { show: false, open: true }) {
        Command::Tray { show, open } => tray::run(Config::load(), show, open),
        Command::Headless => block_on(headless(Config::load())),
        Command::Scan { all } => block_on(scan(all)),
        Command::Device { model, connect, disconnect } => device(&model, connect, disconnect),
        Command::RenderPopup { dir } => render_popups(&dir),
        Command::RenderIcons { dir } => render_icons(&dir),
        Command::Admin { args } => system::run_admin(&args),
        Command::ConfigPath => {
            match Config::path() {
                Some(path) => println!("{}", path.display()),
                None => println!("no config directory on this system"),
            }
            Ok(())
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err:#}");
            ExitCode::FAILURE
        }
    }
}

fn render_icons(dir: &std::path::Path) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    icon::app(256).save_png(dir.join("logo.png"))?;
    icon::write_ico(&dir.join("kord.ico"))?;
    println!("{}", dir.display());
    Ok(())
}

fn block_on(future: impl Future<Output = Result<()>>) -> Result<()> {
    tokio::runtime::Builder::new_current_thread().enable_all().build()?.block_on(future)
}

async fn headless(config: Config) -> Result<()> {
    let (_config_tx, config_rx) = watch::channel(config);
    let (_commands_tx, commands_rx) = mpsc::unbounded_channel();
    engine::run(config_rx, commands_rx, |snapshot| match snapshot {
        Some(engine::Snapshot { device, link }) => {
            let here = match link {
                link::Link::Paired(paired) if paired.connected => format!("connected here as {:?}", paired.name),
                link::Link::Paired(paired) => format!("paired here as {:?}, not connected", paired.name),
                link::Link::NotPaired => "not paired with this computer".to_string(),
                link::Link::Unknown => "pairing unknown".to_string(),
            };
            info!("{} ({here})", summary(&device));
        }
        None => info!("no headphones nearby"),
    })
    .await;
    Ok(())
}

async fn scan(all: bool) -> Result<()> {
    println!("Scanning... press Ctrl+C to stop.");
    let (tx, mut rx) = mpsc::channel(64);
    tokio::spawn(scanner::run(tx));
    let start = std::time::Instant::now();
    // Only print when a device's payload changes, otherwise it floods the terminal.
    let mut last_seen: HashMap<String, Vec<u8>> = HashMap::new();
    while let Some(sighting) = rx.recv().await {
        let address = sighting.address.map_or_else(|| "?".to_string(), |a| a.to_string());
        if all {
            println!("{:>7.2}s  {address}  rssi {:?}  {}", start.elapsed().as_secs_f32(), sighting.rssi, summary(&sighting.status));
            continue;
        }
        if last_seen.get(&address) == Some(&sighting.raw) {
            continue;
        }
        let raw: String = sighting.raw.iter().map(|b| format!("{b:02X}")).collect();
        println!(
            "{address}  rssi {:>4}  model 0x{:04X}  {}\n    raw [{:04X}] {raw}",
            sighting.rssi.map_or_else(|| "?".to_string(), |r| r.to_string()),
            sighting.status.model_id,
            summary(&sighting.status),
            sighting.company_id,
        );
        last_seen.insert(address, sighting.raw);
    }
    Ok(())
}

fn device(model: &str, connect: bool, disconnect: bool) -> Result<()> {
    let model_id = config::parse_model_id(model).with_context(|| format!("invalid model id {model:?}"))?;
    // The pairing's Device ID is the advertised model id with its bytes swapped.
    let (vendor_id, product_id) = (kord_core::apple::COMPANY_ID, model_id.swap_bytes());
    let link::Link::Paired(device) = link::find(vendor_id, product_id)? else {
        println!("no pairing found for model 0x{model_id:04X} on this computer");
        return Ok(());
    };
    println!("{}: {}", device.name, if device.connected { "connected" } else { "not connected" });
    if let Some(address) = device.address() {
        println!("  address:      {address:012X}");
        println!("  hands-free:   {:?}", system::hands_free(address));
    }
    println!("  low latency:  {:?}", system::low_latency());
    println!("  output:       {:?}", device.output_endpoint());
    println!("  microphone:   {:?}", device.input_endpoint());
    println!("  default out:  {:?}", audio::default_output().ok().flatten());
    println!("  default in:   {:?}", audio::default_input().ok().flatten());
    if connect {
        link::connect(&device)?;
        println!("connect request sent");
    } else if disconnect {
        link::disconnect(&device)?;
        println!("disconnect request sent");
    }
    Ok(())
}

fn render_popups(dir: &std::path::Path) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    // A real advertisement: AirPods Pro 2, both buds charging in an open case.
    let raw = [0x07, 0x19, 0x01, 0x14, 0x20, 0x55, 0xAA, 0xB6, 0x31, 0x00, 0x04];
    let device = kord_core::apple::decode(&raw).context("sample payload")?;
    let paired = |connected| link::Link::Paired(link::PairedDevice::sample("Alex's AirPods", connected));
    let max = DeviceStatus {
        model_id: 0x0A20,
        model_name: "AirPods Max".into(),
        kind: kord_core::DeviceKind::Headphones,
        left: Battery { level: Some(40), charging: false },
        right: Battery::default(),
        ..device.clone()
    };
    let samples = [
        ("disconnected", Some(engine::Snapshot { device: device.clone(), link: paired(false) })),
        ("connected", Some(engine::Snapshot { device: device.clone(), link: paired(true) })),
        ("not-paired", Some(engine::Snapshot { device, link: link::Link::NotPaired })),
        ("headphones", Some(engine::Snapshot { device: max, link: paired(true) })),
        ("searching", None),
    ];
    for (name, snapshot) in &samples {
        for (theme, dark) in [("light", false), ("dark", true)] {
            let path = dir.join(format!("popup-{name}-{theme}.png"));
            ui::popup::render_to_png(snapshot.as_ref(), dark, 2.0, &path)?;
            println!("{}", path.display());
        }
    }
    let connected = samples[1].1.as_ref();
    let known = [
        link::KnownDevice {
            vendor_id: kord_core::apple::COMPANY_ID,
            product_id: 0x2014,
            device: link::PairedDevice::sample("Alex's AirPods", true),
        },
        link::KnownDevice {
            vendor_id: kord_core::apple::COMPANY_ID,
            product_id: 0x200A,
            device: link::PairedDevice::sample("AirPods Max", false),
        },
        link::KnownDevice { vendor_id: 0x0075, product_id: 0x0101, device: link::PairedDevice::sample("Galaxy Buds2 Pro", false) },
        link::KnownDevice { vendor_id: 0x012D, product_id: 0x0DF0, device: link::PairedDevice::sample("WH-1000XM5", false) },
        link::KnownDevice { vendor_id: 0x046D, product_id: 0x0AFC, device: link::PairedDevice::sample("Zone Vibe 100", false) },
        link::KnownDevice { vendor_id: 0x0A12, product_id: 0x0001, device: link::PairedDevice::sample("Studio Headphones", false) },
        link::KnownDevice { vendor_id: 0x0A12, product_id: 0x0002, device: link::PairedDevice::sample("TWS Earbuds", false) },
    ];
    let device_page = ui::window::Page::Device(known[0].device.id());
    let pages = [
        ("devices", ui::window::Page::Devices),
        ("device", device_page),
        ("settings", ui::window::Page::Settings),
        ("about", ui::window::Page::About),
    ];
    for (name, page) in pages {
        for (theme, dark) in [("light", false), ("dark", true)] {
            let path = dir.join(format!("window-{name}-{theme}.png"));
            ui::window::render_to_png(&Config::default(), connected, &known, page.clone(), dark, &path)?;
            println!("{}", path.display());
        }
    }
    Ok(())
}

fn summary(device: &DeviceStatus) -> String {
    fn battery(b: Battery) -> String {
        let level = b.level.map_or_else(|| "--".to_string(), |l| format!("{l}%"));
        if b.charging { format!("{level}+") } else { level }
    }
    fn ear(in_ear: Option<bool>) -> &'static str {
        if in_ear == Some(true) { " (in ear)" } else { "" }
    }
    format!(
        "{}  L {}{}  R {}{}  case {}{}",
        device.model_name,
        battery(device.left),
        ear(device.ears.left),
        battery(device.right),
        ear(device.ears.right),
        battery(device.case),
        if device.lid_open == Some(true) { " (lid open)" } else { "" },
    )
}

/// Lets the console-less release build print to the terminal it was started from.
#[cfg(windows)]
fn attach_parent_console() {
    use windows::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
    // SAFETY: plain Win32 call without pointers; failing just means no terminal.
    unsafe {
        let _ = AttachConsole(ATTACH_PARENT_PROCESS);
    }
}
