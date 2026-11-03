//! System tray front-end. The tray, popup and main window live on the
//! main thread (required by both Windows and GTK); the Bluetooth engine runs
//! on a background thread and talks to them through the event loop proxy.

use std::time::Instant;

use anyhow::{Context, Result};
use kord_core::{Battery, DeviceKind, DeviceStatus};
use tao::event::{Event, StartCause};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tokio::sync::{mpsc, watch};
use tracing::{debug, error, warn};
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

use crate::config::Config;
use crate::engine::{self, Snapshot};
use crate::i18n::strings;
use crate::icon;
use crate::instance::{self, Instance};
use crate::link;
use crate::ui::popup::{self, Popup};
use crate::ui::window::{self, MainWindow, Page};

enum UserEvent {
    Snapshot(Option<Snapshot>),
    Menu(MenuId),
    Tray(TrayIconEvent),
    /// Kord was started again: show the window.
    ShowWindow,
}

pub fn run(config: Config, show_popup: bool, open_window: bool) -> Result<()> {
    let listener = match instance::acquire() {
        Instance::First(listener) => listener,
        Instance::Second(running) => {
            if open_window {
                running.show();
            }
            return Ok(());
        }
    };
    crate::autostart::refresh();
    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();
    listener.listen(move || {
        let _ = proxy.send_event(UserEvent::ShowWindow);
    });

    let proxy = event_loop.create_proxy();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        let _ = proxy.send_event(UserEvent::Menu(event.id));
    }));
    let proxy = event_loop.create_proxy();
    TrayIconEvent::set_event_handler(Some(move |event| {
        let _ = proxy.send_event(UserEvent::Tray(event));
    }));

    crate::ui::paint::set_theme(config.general.theme);
    crate::i18n::set_language(config.general.language);
    let (config_tx, config_rx) = watch::channel(config);
    let (commands_tx, commands_rx) = mpsc::unbounded_channel();
    let proxy = event_loop.create_proxy();
    std::thread::Builder::new().name("kord-engine".into()).spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build();
        match runtime {
            Ok(runtime) => runtime.block_on(engine::run(config_rx, commands_rx, move |snapshot| {
                let _ = proxy.send_event(UserEvent::Snapshot(snapshot));
            })),
            Err(err) => error!("could not start the async runtime: {err}"),
        }
    })?;

    let mut ui: Option<TrayUi> = None;
    let mut popup = Popup::default();
    let mut window = MainWindow::default();
    // The popup appears when the case is *opened*: after it showed, wait for
    // the lid to close or the headphones to go away before showing it again.
    let mut lid_armed = true;

    event_loop.run(move |event, target, control_flow| {
        match event {
            Event::NewEvents(StartCause::Init) => match TrayUi::new(config_tx.borrow().ear_detection.enabled) {
                Ok(tray) => {
                    ui = Some(tray);
                    if show_popup {
                        popup.show(target, false);
                    }
                    if open_window {
                        window.show(target, &config_tx.borrow(), None);
                    }
                }
                Err(err) => {
                    error!("could not create the tray icon: {err:#}");
                    *control_flow = ControlFlow::Exit;
                    return;
                }
            },
            Event::NewEvents(StartCause::ResumeTimeReached { .. }) => {
                let now = Instant::now();
                popup.on_timer(now);
                if let Some(window::Action::Engine(command)) = window.on_timer(now) {
                    let _ = commands_tx.send(command);
                }
            }
            Event::UserEvent(UserEvent::Snapshot(snapshot)) => {
                if let Some(ui) = ui.as_mut() {
                    ui.show(snapshot.as_ref());
                }
                let lid = snapshot.as_ref().and_then(|s| s.device.lid_open);
                if snapshot.is_none() || lid == Some(false) {
                    lid_armed = true;
                }
                if lid == Some(true) && lid_armed && config_tx.borrow().popup.show_on_lid_open {
                    debug!("case opened");
                    lid_armed = false;
                    popup.show(target, true);
                }
                // Like on a phone, the card goes away once the earbuds leave the case.
                if lid != Some(true) && popup.is_auto_shown() {
                    popup.hide();
                }
                window.update(snapshot.clone());
                popup.update(snapshot);
            }
            Event::UserEvent(UserEvent::Tray(TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            })) => popup.toggle(target),
            Event::UserEvent(UserEvent::Tray(TrayIconEvent::DoubleClick { button: MouseButton::Left, .. })) => {
                popup.hide();
                window.show(target, &config_tx.borrow(), None);
            }
            Event::UserEvent(UserEvent::Tray(_)) => {}
            Event::UserEvent(UserEvent::ShowWindow) => window.show(target, &config_tx.borrow(), None),
            Event::UserEvent(UserEvent::Menu(id)) => {
                let Some(tray) = ui.as_ref() else { return };
                if id == tray.quit_id {
                    // Drop the icon first so it doesn't linger in the tray.
                    popup.hide();
                    window.close();
                    ui = None;
                    *control_flow = ControlFlow::Exit;
                    return;
                } else if id == tray.show_status_id {
                    popup.show(target, false);
                } else if id == tray.open_window_id {
                    window.show(target, &config_tx.borrow(), Some(Page::Devices));
                } else if id == tray.ear_detection_id {
                    let mut config = config_tx.borrow().clone();
                    config.ear_detection.enabled = tray.ear_detection.is_checked();
                    apply_config(&config_tx, config, true, &ui, &mut window);
                } else if id == tray.open_settings_id {
                    open_settings_file(&config_tx.borrow());
                }
            }
            Event::WindowEvent { window_id, event, .. } if Some(window_id) == popup.window_id() => {
                match popup.handle_event(&event) {
                    Some(popup::Action::Engine(command)) => {
                        let _ = commands_tx.send(command);
                    }
                    Some(popup::Action::OpenBluetoothSettings) => link::open_bluetooth_settings(),
                    Some(popup::Action::OpenSettings) => window.show_current_device(target, &config_tx.borrow()),
                    None => {}
                }
            }
            Event::WindowEvent { window_id, event, .. } if Some(window_id) == window.window_id() => {
                match window.handle_event(&event) {
                    Some(window::Action::Config { config, save }) => apply_config(&config_tx, config, save, &ui, &mut window),
                    Some(window::Action::Engine(command)) => {
                        let _ = commands_tx.send(command);
                    }
                    Some(window::Action::OpenBluetoothSettings) => link::open_bluetooth_settings(),
                    Some(window::Action::OpenConfigFile) => open_settings_file(&config_tx.borrow()),
                    None => {}
                }
            }
            Event::RedrawRequested(window_id) if Some(window_id) == popup.window_id() => popup.redraw(),
            Event::RedrawRequested(window_id) if Some(window_id) == window.window_id() => window.redraw(),
            _ => {}
        }
        *control_flow = match [popup.deadline(), window.deadline()].into_iter().flatten().min() {
            Some(deadline) => ControlFlow::WaitUntil(deadline),
            None => ControlFlow::Wait,
        };
    })
}

/// Applies settings changed from the UI and keeps every view in sync.
fn apply_config(config_tx: &watch::Sender<Config>, config: Config, save: bool, ui: &Option<TrayUi>, window: &mut MainWindow) {
    if let Some(ui) = ui {
        ui.ear_detection.set_checked(config.ear_detection.enabled);
    }
    crate::ui::paint::set_theme(config.general.theme);
    crate::i18n::set_language(config.general.language);
    window.set_config(&config);
    if save && let Err(err) = config.save() {
        warn!("could not save settings: {err:#}");
    }
    config_tx.send_replace(config);
}

struct TrayUi {
    tray: TrayIcon,
    menu: Menu,
    /// Status lines shown at the top of the menu; rebuilt when their number changes.
    lines: Vec<MenuItem>,
    ear_detection: CheckMenuItem,
    ear_detection_id: MenuId,
    show_status_id: MenuId,
    open_window_id: MenuId,
    open_settings_id: MenuId,
    quit_id: MenuId,
}

impl TrayUi {
    fn new(ear_detection_enabled: bool) -> Result<Self> {
        let s = strings();
        let show_status = MenuItem::new(s.show_status, true, None);
        let open_window = MenuItem::new(s.open_kord, true, None);
        let ear_detection = CheckMenuItem::new(s.ear_detection, true, ear_detection_enabled, None);
        let open_settings = MenuItem::new(s.open_settings, true, None);
        let quit = MenuItem::new(s.quit, true, None);
        let menu = Menu::with_items(&[
            &PredefinedMenuItem::separator(),
            &open_window,
            &show_status,
            &ear_detection,
            &open_settings,
            &PredefinedMenuItem::separator(),
            &quit,
        ])?;

        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu.clone()))
            // Left click opens the popup; the menu stays on right click.
            .with_menu_on_left_click(false)
            .with_tooltip("Kord")
            .with_icon(make_icon(None)?)
            .build()
            .context("tray icon")?;

        let mut ui = Self {
            tray,
            menu,
            lines: Vec::new(),
            ear_detection_id: ear_detection.id().clone(),
            ear_detection,
            show_status_id: show_status.id().clone(),
            open_window_id: open_window.id().clone(),
            open_settings_id: open_settings.id().clone(),
            quit_id: quit.id().clone(),
        };
        ui.set_lines(&[s.searching.to_string()]);
        Ok(ui)
    }

    fn show(&mut self, snapshot: Option<&Snapshot>) {
        let device = snapshot.map(|s| &s.device);
        let (lines, tooltip) = match snapshot {
            None => (vec![strings().searching.to_string()], "Kord".to_string()),
            Some(snapshot) => describe(snapshot),
        };
        if lines.len() != self.lines.len() {
            self.set_lines(&lines);
        } else {
            for (item, text) in self.lines.iter().zip(&lines) {
                item.set_text(text);
            }
        }
        let _ = self.tray.set_tooltip(Some(tooltip));
        match make_icon(device) {
            Ok(icon) => {
                let _ = self.tray.set_icon(Some(icon));
            }
            Err(err) => warn!("could not draw tray icon: {err:#}"),
        }
    }

    /// Replaces the disabled status items at the top of the menu.
    fn set_lines(&mut self, lines: &[String]) {
        for item in self.lines.drain(..) {
            let _ = self.menu.remove(&item);
        }
        for (position, text) in lines.iter().enumerate() {
            let item = MenuItem::new(text, false, None);
            if let Err(err) = self.menu.insert(&item, position) {
                warn!("could not update menu: {err}");
            }
            self.lines.push(item);
        }
    }
}

/// Menu lines and tooltip for the tracked headphones.
fn describe(snapshot: &Snapshot) -> (Vec<String>, String) {
    let s = strings();
    let device = &snapshot.device;
    let mut title = snapshot.display_name().to_string();
    if snapshot.link.is_connected() {
        title.push_str(&format!(" · {}", s.connected));
    }
    let mut lines = vec![title.clone()];
    let mut tooltip = title;
    match device.kind {
        DeviceKind::Earbuds => {
            for (label, battery, in_ear) in [
                (s.left, device.left, device.ears.left),
                (s.right, device.right, device.ears.right),
            ] {
                let mut line = format!("{label}: {}", battery_text(battery));
                if in_ear == Some(true) {
                    line.push_str(&format!(" · {}", s.in_ear));
                }
                lines.push(line);
            }
            let mut case = format!("{}: {}", s.case, battery_text(device.case));
            if device.lid_open == Some(true) {
                case.push_str(&format!(" · {}", s.lid_open));
            }
            lines.push(case);
            tooltip.push_str(&format!(
                "\n{} {} · {} {} · {} {}",
                s.left,
                level_text(device.left.level),
                s.right,
                level_text(device.right.level),
                s.case,
                level_text(device.case.level),
            ));
        }
        DeviceKind::Headphones => {
            let battery = Battery { level: device.lowest_level(), charging: device.left.charging || device.right.charging };
            lines.push(format!("{}: {}", s.battery, battery_text(battery)));
            tooltip.push_str(&format!("\n{} {}", s.battery, level_text(battery.level)));
        }
    }
    (lines, tooltip)
}

fn level_text(level: Option<u8>) -> String {
    level.map_or_else(|| strings().unavailable.to_string(), |l| format!("{l} %"))
}

fn battery_text(battery: Battery) -> String {
    let mut text = level_text(battery.level);
    if battery.charging {
        text.push_str(&format!(" ({})", strings().charging));
    }
    text
}

fn make_icon(device: Option<&DeviceStatus>) -> Result<Icon> {
    Ok(Icon::from_rgba(icon::tray(device), icon::SIZE, icon::SIZE)?)
}

/// Opens the settings file in the default editor, creating it first so there
/// is something to edit.
fn open_settings_file(config: &Config) {
    let path = match Config::path() {
        Some(path) if path.exists() => path,
        _ => match config.save() {
            Ok(path) => path,
            Err(err) => return warn!("could not create settings file: {err:#}"),
        },
    };
    #[cfg(windows)]
    let result = std::process::Command::new("notepad.exe").arg(&path).spawn();
    #[cfg(not(windows))]
    let result = std::process::Command::new("xdg-open").arg(&path).spawn();
    if let Err(err) = result {
        warn!("could not open {}: {err}", path.display());
    }
}
