//! Kord's main window: a sidebar with the device list, app settings and
//! about pages, and a page per pair of headphones with its status and
//! options. Painted with the same lightweight renderer as the popup and, like
//! it, the window only exists while it's open.

mod draw;
mod entry;
mod rows;

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use tao::dpi::LogicalSize;
use tao::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use tao::event_loop::EventLoopWindowTarget;
use tao::keyboard::Key;
use tao::window::{CursorIcon, WindowBuilder, WindowId};
use tracing::warn;

use self::draw::{Content, DeviceButton, Hit, Nav, Sprites, Target, View};
use self::entry::{Entry, NEARBY};
use self::rows::{Control, DeviceFacts, Id, Mic, Section, SystemState, Value};
use crate::config::Config;
use crate::engine::{Command, Snapshot};
use crate::i18n::strings;
use crate::link::{self, KnownDevice, Link, PairedDevice};
use crate::ui::paint::palette;
use crate::ui::surface::Surface;
use crate::ui::text::Fonts;
use crate::{audio, autostart, system};

/// What the window asks the rest of the app to do.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Settings changed; `save` once the change is final (not mid-drag).
    Config { config: Config, save: bool },
    Engine(Command),
    OpenBluetoothSettings,
    OpenConfigFile,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Page {
    Devices,
    /// One pair of headphones, by [`Entry::id`].
    Device(String),
    Settings,
    About,
}

const PENDING_TIMEOUT: Duration = Duration::from_secs(15);
/// Refresh rate while something is running (microphone test, admin change).
const LIVE: Duration = Duration::from_millis(50);
/// How often the list of paired headphones is re-read while the window is open.
const DEVICES_POLL: Duration = Duration::from_secs(4);
const DEVICES_POLL_FAST: Duration = Duration::from_secs(1);
const MIC_TEST_SECONDS: u32 = 5;

struct Open {
    surface: Surface,
    fonts: Fonts,
    sprites: Sprites,
    hits: Vec<Hit>,
    hover: Option<Target>,
    cursor: (f32, f32),
    open_choice: Option<Id>,
    dragging: Option<Target>,
    scroll: f32,
    content_height: f32,
}

/// The result of work running on another thread, once it's done.
type Slot<T> = Arc<Mutex<Option<T>>>;

fn spawn<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Slot<T> {
    let slot = Arc::new(Mutex::new(None));
    let shared = slot.clone();
    std::thread::spawn(move || {
        let result = work();
        *shared.lock().unwrap_or_else(|e| e.into_inner()) = Some(result);
    });
    slot
}

fn take<T>(slot: &Slot<T>) -> Option<T> {
    slot.lock().unwrap_or_else(|e| e.into_inner()).take()
}

/// An OS change running in the background, usually waiting on a UAC prompt.
struct Job {
    id: Id,
    /// The headphones to reconnect once it's done, so they pick up the change.
    device: Option<String>,
    result: Slot<Result<(), String>>,
}

pub struct MainWindow {
    open: Option<Open>,
    page: Page,
    config: Config,
    snapshot: Option<Snapshot>,
    /// Pairings on this computer; kept while closed so reopening is instant.
    known: Vec<KnownDevice>,
    loading: Option<Slot<Vec<KnownDevice>>>,
    next_refresh: Instant,
    /// A connect/disconnect request: device id, command, when.
    pending: Option<(String, Command, Instant)>,
    system: SystemState,
    job: Option<Job>,
    mic_test: Option<audio::MicTest>,
}

impl Default for MainWindow {
    fn default() -> Self {
        Self {
            open: None,
            page: Page::Devices,
            config: Config::default(),
            snapshot: None,
            known: Vec::new(),
            loading: None,
            next_refresh: Instant::now(),
            pending: None,
            system: SystemState::default(),
            job: None,
            mic_test: None,
        }
    }
}

impl MainWindow {
    pub fn window_id(&self) -> Option<WindowId> {
        self.open.as_ref().map(|o| o.surface.window.id())
    }

    /// Opens the window (or brings it to the front), on `page` if given.
    pub fn show<T>(&mut self, target: &EventLoopWindowTarget<T>, config: &Config, page: Option<Page>) {
        self.config = config.clone();
        if let Some(page) = page {
            self.navigate(page);
        }
        if let Some(open) = &self.open {
            open.surface.window.set_minimized(false);
            open.surface.window.set_focus();
            self.request_redraw();
            return;
        }
        match create(target) {
            Ok(open) => {
                self.open = Some(open);
                self.refresh_devices();
                self.refresh_system();
                self.request_redraw();
            }
            Err(err) => warn!("could not open the Kord window: {err:#}"),
        }
    }

    /// Opens the page of the headphones the popup shows.
    pub fn show_current_device<T>(&mut self, target: &EventLoopWindowTarget<T>, config: &Config) {
        let page = self.snapshot.as_ref().map(|snapshot| match &snapshot.link {
            Link::Paired(paired) => Page::Device(paired.id()),
            _ => Page::Device(NEARBY.to_string()),
        });
        self.show(target, config, Some(page.unwrap_or(Page::Devices)));
    }

    pub fn close(&mut self) {
        self.open = None;
        self.mic_test = None;
    }

    /// Settings changed elsewhere (e.g. the tray menu).
    pub fn set_config(&mut self, config: &Config) {
        if *config != self.config {
            self.config = config.clone();
            self.request_redraw();
        }
    }

    pub fn update(&mut self, snapshot: Option<Snapshot>) {
        if snapshot == self.snapshot {
            return;
        }
        let link = |s: &Option<Snapshot>| s.as_ref().map(|s| s.link.clone());
        let link_changed = link(&snapshot) != link(&self.snapshot);
        let before = self.selected_paired();
        self.snapshot = snapshot;
        self.check_pending();
        if self.open.is_some() {
            if link_changed {
                // A new pairing, or one that (dis)connected: re-read the list soon.
                self.next_refresh = self.next_refresh.min(Instant::now() + DEVICES_POLL_FAST);
            }
            if self.selected_paired() != before {
                self.refresh_system();
            }
        }
        self.request_redraw();
    }

    pub fn deadline(&self) -> Option<Instant> {
        self.open.as_ref()?;
        let busy = self.job.is_some() || self.loading.is_some() || self.mic_test.as_ref().is_some_and(|t| t.state().running());
        let live = busy.then(|| Instant::now() + LIVE);
        let refresh = self.loading.is_none().then_some(self.next_refresh);
        let pending = self.pending.as_ref().map(|(_, _, since)| *since + PENDING_TIMEOUT);
        [live, refresh, pending].into_iter().flatten().min()
    }

    pub fn on_timer(&mut self, now: Instant) -> Option<Action> {
        self.open.as_ref()?;
        if self.pending.as_ref().is_some_and(|(_, _, since)| now >= *since + PENDING_TIMEOUT) {
            self.pending = None;
            self.request_redraw();
        }
        if self.mic_test.is_some() {
            self.request_redraw();
        }
        if let Some(known) = self.loading.as_ref().and_then(take) {
            self.loading = None;
            let fast = self.pending.is_some();
            self.next_refresh = now + if fast { DEVICES_POLL_FAST } else { DEVICES_POLL };
            if known != self.known {
                let before = self.selected_paired();
                self.known = known;
                self.check_pending();
                if self.selected_paired() != before {
                    self.refresh_system();
                }
                self.request_redraw();
            }
        } else if self.loading.is_none() && now >= self.next_refresh {
            self.refresh_devices();
        }

        let result = take(&self.job.as_ref()?.result)?;
        let job = self.job.take()?;
        self.refresh_system();
        self.request_redraw();
        match result {
            // New codec or profile: the headphones must reconnect to pick it up.
            Ok(()) => self.reconnect(job.device.as_deref()?),
            Err(err) => {
                warn!("system change failed: {err}");
                None
            }
        }
    }

    pub fn handle_event(&mut self, event: &WindowEvent) -> Option<Action> {
        let open = self.open.as_mut()?;
        let scale = open.surface.scale();
        match event {
            WindowEvent::CloseRequested => self.close(),
            WindowEvent::KeyboardInput { event, .. }
                if event.state == ElementState::Pressed && event.logical_key == Key::Escape =>
            {
                if open.open_choice.take().is_some() {
                    self.request_redraw();
                } else if matches!(self.page, Page::Device(_)) {
                    self.navigate(Page::Devices);
                } else {
                    self.close();
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                open.cursor = ((position.x as f32) / scale, (position.y as f32) / scale);
                if let Some(Target::Slider(id, left, right)) = open.dragging {
                    return self.slide(id, left, right, false);
                }
                let hover = draw::hit_test(&open.hits, open.cursor.0, open.cursor.1);
                if hover != open.hover {
                    open.hover = hover;
                    let icon = if hover.is_some() { CursorIcon::Hand } else { CursorIcon::Default };
                    open.surface.window.set_cursor_icon(icon);
                    open.surface.window.request_redraw();
                }
            }
            WindowEvent::CursorLeft { .. } => {
                if open.hover.take().is_some() {
                    open.surface.window.request_redraw();
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let lines = match delta {
                    MouseScrollDelta::LineDelta(_, y) => *y * 48.0,
                    MouseScrollDelta::PixelDelta(position) => position.y as f32 / scale,
                    _ => 0.0,
                };
                let (_, height) = open.surface.logical_size();
                let max = (open.content_height - height).max(0.0);
                open.scroll = (open.scroll - lines).clamp(0.0, max);
                open.open_choice = None;
                open.surface.window.request_redraw();
            }
            WindowEvent::MouseInput { state: ElementState::Pressed, button: MouseButton::Left, .. } => {
                let target = draw::hit_test(&open.hits, open.cursor.0, open.cursor.1);
                if open.open_choice.take().is_some() {
                    open.surface.window.request_redraw();
                    // A click with a list open either picks an option or just closes it.
                    return match target {
                        Some(Target::Option(id, index)) => self.change(id, Value::Choice(index), true),
                        _ => None,
                    };
                }
                return match target? {
                    Target::Nav(Nav::Devices) => self.navigate(Page::Devices),
                    Target::Nav(Nav::Settings) => self.navigate(Page::Settings),
                    Target::Nav(Nav::About) => self.navigate(Page::About),
                    Target::Back => self.navigate(Page::Devices),
                    Target::Card(index) => {
                        let id = self.entries().get(index)?.id.clone();
                        self.navigate(Page::Device(id))
                    }
                    Target::AddDevice => Some(Action::OpenBluetoothSettings),
                    Target::DeviceButton => self.click_device_button(),
                    Target::Control(id) => self.click_control(id),
                    Target::Slider(id, left, right) => {
                        open.dragging = Some(Target::Slider(id, left, right));
                        self.slide(id, left, right, false)
                    }
                    Target::Option(..) => None,
                };
            }
            WindowEvent::MouseInput { state: ElementState::Released, button: MouseButton::Left, .. } => {
                if let Some(Target::Slider(id, left, right)) = open.dragging.take() {
                    return self.slide(id, left, right, true);
                }
            }
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } | WindowEvent::ThemeChanged(_) => {
                open.surface.window.request_redraw();
            }
            _ => {}
        }
        None
    }

    fn navigate(&mut self, page: Page) -> Option<Action> {
        if page != self.page {
            self.page = page;
            self.mic_test = None;
            if let Some(open) = self.open.as_mut() {
                open.scroll = 0.0;
                open.open_choice = None;
                open.hover = None;
            }
            if self.open.is_some() {
                self.refresh_system();
            }
        }
        self.request_redraw();
        None
    }

    fn entries(&self) -> Vec<Entry> {
        entry::entries(&self.known, self.snapshot.as_ref())
    }

    /// The headphones whose page is open.
    fn selected(&self) -> Option<Entry> {
        let Page::Device(id) = &self.page else { return None };
        self.entries().into_iter().find(|e| &e.id == id)
    }

    fn selected_paired(&self) -> Option<PairedDevice> {
        self.selected().and_then(|e| e.paired)
    }

    /// Whether these are the headphones the engine follows, which it then
    /// connects itself (keeping its own state in step).
    fn is_tracked(&self, id: &str) -> bool {
        matches!(self.snapshot.as_ref().map(|s| &s.link), Some(Link::Paired(p)) if p.id() == id)
    }

    fn refresh_devices(&mut self) {
        if self.loading.is_none() {
            self.loading = Some(spawn(|| {
                link::paired_devices().unwrap_or_else(|err| {
                    warn!("could not list paired headphones: {err:#}");
                    Vec::new()
                })
            }));
        }
    }

    fn check_pending(&mut self) {
        let Some((id, command, _)) = &self.pending else { return };
        let connected = self.entries().iter().find(|e| &e.id == id).map(Entry::connected);
        let done = match command {
            Command::Connect => connected == Some(true),
            Command::Disconnect => connected != Some(true),
            Command::Reconnect => false,
        };
        if done {
            self.pending = None;
        }
    }

    fn sections(&self) -> Vec<Section> {
        let busy = self.job.as_ref().map(|j| j.id);
        match &self.page {
            Page::Device(_) => {
                let Some(entry) = self.selected() else { return Vec::new() };
                let facts = DeviceFacts {
                    apple: entry.apple(),
                    model: &entry.model,
                    address: entry.paired.as_ref().and_then(PairedDevice::address),
                };
                let test = self.mic_test.as_ref().map(|t| t.state());
                rows::device_sections(&self.config, &self.system, &facts, test.as_ref(), busy)
            }
            Page::Settings => rows::app_sections(&self.config, &self.system),
            Page::Devices | Page::About => Vec::new(),
        }
    }

    fn control(&self, id: Id) -> Option<Control> {
        self.sections().into_iter().flat_map(|s| s.rows).find(|row| row.id == id).map(|row| row.control)
    }

    fn click_control(&mut self, id: Id) -> Option<Action> {
        match self.control(id)? {
            Control::Toggle(on) => self.change(id, Value::Toggle(!on), true),
            Control::Choice { .. } => {
                let open = self.open.as_mut()?;
                open.open_choice = Some(id);
                open.surface.window.request_redraw();
                None
            }
            Control::Button { enabled: true, .. } | Control::Test { enabled: true, .. } => {
                self.change(id, Value::Press, true)
            }
            _ => None,
        }
    }

    /// Sets a slider from the cursor position.
    fn slide(&mut self, id: Id, left: f32, right: f32, save: bool) -> Option<Action> {
        let cursor = self.open.as_ref()?.cursor.0;
        let Some(Control::Slider { max, step, .. }) = self.control(id) else { return None };
        let fraction = ((cursor - left) / (right - left)).clamp(0.0, 1.0);
        let raw = fraction * f32::from(max);
        let value = ((raw / f32::from(step)).round() * f32::from(step)).min(f32::from(max)) as u8;
        self.change(id, Value::Slider(value), save)
    }

    fn change(&mut self, id: Id, value: Value, save: bool) -> Option<Action> {
        self.request_redraw();
        match id {
            Id::BluetoothSettings => return Some(Action::OpenBluetoothSettings),
            Id::ConfigFile => return Some(Action::OpenConfigFile),
            _ if id.is_system() => {
                self.change_system(id, value);
                return None;
            }
            _ => {}
        }
        let before = self.config.clone();
        rows::apply(&mut self.config, id, value);
        (self.config != before || save).then(|| Action::Config { config: self.config.clone(), save })
    }

    /// Settings that live in the OS rather than in Kord's file.
    fn change_system(&mut self, id: Id, value: Value) {
        let selected = self.selected();
        let device = selected.as_ref().map(|e| e.id.clone());
        let paired = selected.and_then(|e| e.paired);
        let mic = self.system.mic.clone();
        match (id, value) {
            (Id::Autostart, Value::Toggle(on)) => {
                if let Err(err) = autostart::set_enabled(on) {
                    warn!("could not change autostart: {err:#}");
                }
                self.system.autostart = autostart::is_enabled();
            }
            (Id::LowLatency, Value::Toggle(on)) => self.run_job(id, device, move || system::set_low_latency(on)),
            (Id::HandsFree, Value::Toggle(on)) => {
                if let Some(address) = paired.as_ref().and_then(PairedDevice::address) {
                    self.run_job(id, device, move || system::set_hands_free(address, on));
                }
            }
            (Id::MicDefault, Value::Press) => {
                if let Some(Mic::Ready { input, .. }) = mic {
                    if let Err(err) = audio::set_default(&input) {
                        warn!("could not change the default microphone: {err:#}");
                    }
                    self.refresh_system();
                }
            }
            (Id::MicVolume, Value::Slider(volume)) => {
                if let Some(Mic::Ready { input, .. }) = &mic {
                    if let Err(err) = audio::set_volume(input, volume) {
                        warn!("could not change the microphone volume: {err:#}");
                    }
                    if let Some(Mic::Ready { volume: current, .. }) = &mut self.system.mic {
                        *current = volume;
                    }
                }
            }
            (Id::MicTest, Value::Press) => {
                if let Some(Mic::Ready { input, output, .. }) = mic {
                    self.mic_test = Some(audio::MicTest::start(input, output, MIC_TEST_SECONDS));
                }
            }
            _ => {}
        }
    }

    fn run_job(&mut self, id: Id, device: Option<String>, work: impl FnOnce() -> Result<()> + Send + 'static) {
        if self.job.is_none() {
            let result = spawn(move || work().map_err(|err| format!("{err:#}")));
            self.job = Some(Job { id, device, result });
        }
    }

    /// Re-reads the settings that live in the OS, for the open page.
    fn refresh_system(&mut self) {
        let paired = self.selected_paired();
        let mic = audio::SUPPORTED.then(|| match &paired {
            Some(paired) if paired.connected => match paired.input_endpoint() {
                Some(input) => Mic::Ready {
                    output: paired.output_endpoint().unwrap_or_default(),
                    is_default: audio::default_input().ok().flatten().as_ref() == Some(&input),
                    volume: audio::volume(&input).unwrap_or(0),
                    input,
                },
                None => Mic::NeedsHandsFree,
            },
            _ => Mic::NotConnected,
        });
        self.system = SystemState {
            autostart: autostart::is_enabled(),
            low_latency: if system::SUPPORTED { system::low_latency() } else { None },
            hands_free: paired.as_ref().and_then(PairedDevice::address).and_then(system::hands_free),
            mic,
        };
    }

    fn click_device_button(&mut self) -> Option<Action> {
        let entry = self.selected()?;
        if self.pending.is_some() {
            return None;
        }
        let Some(paired) = entry.paired else { return Some(Action::OpenBluetoothSettings) };
        let command = if paired.connected { Command::Disconnect } else { Command::Connect };
        self.pending = Some((entry.id.clone(), command, Instant::now()));
        self.request_redraw();
        if self.is_tracked(&entry.id) {
            return Some(Action::Engine(command));
        }
        // Headphones the engine doesn't follow (another brand, or not nearby).
        spawn(move || {
            let result = if command == Command::Connect { link::connect(&paired) } else { link::disconnect(&paired) };
            if let Err(err) = result {
                warn!("{command:?} failed: {err:#}");
            }
        });
        self.next_refresh = Instant::now() + DEVICES_POLL_FAST;
        None
    }

    fn reconnect(&mut self, id: &str) -> Option<Action> {
        if self.is_tracked(id) {
            return Some(Action::Engine(Command::Reconnect));
        }
        let paired = self.entries().into_iter().find(|e| e.id == id)?.paired.filter(|p| p.connected)?;
        spawn(move || {
            let result = link::disconnect(&paired).and_then(|()| {
                std::thread::sleep(Duration::from_secs(2));
                link::connect(&paired)
            });
            if let Err(err) = result {
                warn!("reconnect failed: {err:#}");
            }
        });
        None
    }

    fn device_button(&self, entry: &Entry) -> Option<DeviceButton> {
        let s = strings();
        if let Some((id, command, _)) = &self.pending
            && *id == entry.id
        {
            let label = if *command == Command::Connect { s.connecting } else { s.disconnecting };
            return Some(DeviceButton { label, primary: false, enabled: false });
        }
        let enabled = self.pending.is_none();
        Some(match &entry.paired {
            Some(paired) if paired.connected => DeviceButton { label: s.disconnect, primary: false, enabled },
            Some(_) => DeviceButton { label: s.connect, primary: true, enabled },
            None => DeviceButton { label: s.pair_short, primary: true, enabled: true },
        })
    }

    fn request_redraw(&self) {
        if let Some(open) = &self.open {
            open.surface.window.request_redraw();
        }
    }

    pub fn redraw(&mut self) {
        if matches!(self.page, Page::Device(_)) && self.selected().is_none() {
            // The headphones went away (unpaired, or no longer nearby).
            self.navigate(Page::Devices);
        }
        let entries = self.entries();
        let selected = self.selected();
        let sections = self.sections();
        let button = selected.as_ref().and_then(|entry| self.device_button(entry));
        let Some(open) = self.open.as_mut() else { return };
        let content = match (&self.page, &selected) {
            (Page::Device(_), Some(entry)) => Content::Device { entry, button, sections: &sections },
            (Page::Settings, _) => Content::Settings { sections: &sections },
            (Page::About, _) => Content::About,
            _ => Content::Devices { entries: &entries },
        };
        let view = View {
            content,
            connected: entries.iter().filter(|e| e.connected()).count(),
            hover: open.hover,
            open_choice: open.open_choice,
            scroll: open.scroll,
        };
        let result = (|| -> Result<()> {
            let mut canvas = open.surface.canvas().context("allocating the window")?;
            let p = palette(open.surface.dark());
            let size = open.surface.logical_size();
            let (hits, content_height) =
                draw::render(&mut canvas, open.surface.scale(), size, &open.fonts, &mut open.sprites, &view, &p);
            open.hits = hits;
            open.content_height = content_height;
            // Keep the scroll position valid after the content shrank.
            let max = (content_height - size.1).max(0.0);
            if open.scroll > max {
                open.scroll = max;
                open.surface.window.request_redraw();
            }
            open.surface.present(&canvas)
        })();
        if let Err(err) = result {
            warn!("could not draw the Kord window: {err:#}");
        }
    }
}

/// Renders a page of the window to a PNG file, for screenshots and design work.
pub fn render_to_png(
    config: &Config,
    snapshot: Option<&Snapshot>,
    known: &[KnownDevice],
    page: Page,
    dark: bool,
    path: &std::path::Path,
) -> Result<()> {
    let fonts = Fonts::load().context("no usable system font found")?;
    let window = MainWindow {
        page,
        config: config.clone(),
        snapshot: snapshot.cloned(),
        known: known.to_vec(),
        system: SystemState {
            autostart: true,
            low_latency: system::SUPPORTED.then_some(false),
            hands_free: system::SUPPORTED.then_some(true),
            mic: audio::SUPPORTED.then(|| Mic::Ready {
                input: String::new(),
                output: String::new(),
                is_default: false,
                volume: 80,
            }),
        },
        ..MainWindow::default()
    };
    let entries = window.entries();
    let selected = window.selected();
    let sections = window.sections();
    let content = match (&window.page, &selected) {
        (Page::Device(_), Some(entry)) => {
            Content::Device { entry, button: window.device_button(entry), sections: &sections }
        }
        (Page::Settings, _) => Content::Settings { sections: &sections },
        (Page::About, _) => Content::About,
        _ => Content::Devices { entries: &entries },
    };
    let view = View {
        content,
        connected: entries.iter().filter(|e| e.connected()).count(),
        hover: None,
        open_choice: None,
        scroll: 0.0,
    };
    let (width, scale) = (980.0, 1.5);
    // Measure first, then paint at the height the page needs.
    let mut probe = tiny_skia::Pixmap::new((width * scale) as u32, (700.0 * scale) as u32).context("allocating")?;
    let p = palette(dark);
    let mut sprites = Sprites::default();
    let (_, content_height) = draw::render(&mut probe, scale, (width, 700.0), &fonts, &mut sprites, &view, &p);
    let height = content_height.max(700.0);
    let mut pixmap = tiny_skia::Pixmap::new((width * scale) as u32, (height * scale) as u32).context("allocating")?;
    draw::render(&mut pixmap, scale, (width, height), &fonts, &mut sprites, &view, &p);
    pixmap.save_png(path)?;
    Ok(())
}

fn create<T>(target: &EventLoopWindowTarget<T>) -> Result<Open> {
    let fonts = Fonts::load().context("no usable system font found")?;
    let icon = tao::window::Icon::from_rgba(crate::icon::app_rgba(64), 64, 64).ok();
    let window = WindowBuilder::new()
        .with_title("Kord")
        .with_window_icon(icon)
        .with_inner_size(LogicalSize::new(980.0, 700.0))
        .with_min_inner_size(LogicalSize::new(760.0, 520.0))
        .build(target)
        .context("creating the Kord window")?;
    Ok(Open {
        surface: Surface::new(window)?,
        fonts,
        sprites: Sprites::default(),
        hits: Vec::new(),
        hover: None,
        cursor: (0.0, 0.0),
        open_choice: None,
        dragging: None,
        scroll: 0.0,
        content_height: 0.0,
    })
}
