//! The status card that pops up when you open the case (like on a phone) or
//! click the tray icon. The window only exists while it's visible, so it
//! costs nothing while hidden.

mod draw;

use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use tao::dpi::{LogicalSize, PhysicalPosition};
use tao::event::{ElementState, MouseButton, WindowEvent};
use tao::event_loop::EventLoopWindowTarget;
use tao::keyboard::Key;
use tao::window::{CursorIcon, Window, WindowBuilder, WindowId};
use tiny_skia::Pixmap;
use tracing::{debug, warn};

use self::draw::{ButtonSpec, ButtonStyle, Hover, View};
use crate::engine::{Command, Snapshot};
use crate::i18n::strings;
use crate::link::Link;
use crate::ui::art::{Art, Sprite};
use crate::ui::paint::palette;
use crate::ui::surface::Surface;
use crate::ui::text::Fonts;

/// What the user asked for by clicking the popup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Engine(Command),
    OpenBluetoothSettings,
    OpenSettings,
}

/// Auto-shown popups go away by themselves after this long without the mouse over them.
const AUTO_HIDE: Duration = Duration::from_secs(6);
const AUTO_HIDE_AFTER_LEAVE: Duration = Duration::from_secs(2);
/// Give up showing "Connecting…" after this long.
const PENDING_TIMEOUT: Duration = Duration::from_secs(15);
const FRAME: Duration = Duration::from_millis(33);

struct Shown {
    surface: Surface,
    fonts: Fonts,
    hover: Hover,
    /// Shown automatically (lid opened) rather than by the user.
    auto: bool,
    hide_at: Option<Instant>,
    started: Instant,
    next_frame: Instant,
    /// Cached static layer; `None` when it needs repainting.
    background: Option<Pixmap>,
    sprites: Vec<(Sprite, (f32, f32))>,
    sprites_key: Option<(Vec<Art>, u32, bool)>,
}

#[derive(Default)]
pub struct Popup {
    shown: Option<Shown>,
    snapshot: Option<Snapshot>,
    pending: Option<(Command, Instant)>,
    hidden_at: Option<Instant>,
}

impl Popup {
    pub fn window_id(&self) -> Option<WindowId> {
        self.shown.as_ref().map(|s| s.surface.window.id())
    }

    pub fn is_auto_shown(&self) -> bool {
        self.shown.as_ref().is_some_and(|s| s.auto)
    }

    /// Shows the popup. `auto` popups don't take focus and hide by themselves.
    pub fn show<T>(&mut self, target: &EventLoopWindowTarget<T>, auto: bool) {
        if let Some(shown) = &mut self.shown {
            if !auto {
                shown.auto = false;
                shown.hide_at = None;
                shown.surface.window.set_focus();
            }
            return;
        }
        match create(target, auto) {
            Ok(shown) => {
                debug!("popup shown (auto: {auto})");
                self.shown = Some(shown);
                self.redraw();
            }
            Err(err) => warn!("could not open the popup: {err:#}"),
        }
    }

    pub fn hide(&mut self) {
        if self.shown.take().is_some() {
            debug!("popup hidden");
            self.hidden_at = Some(Instant::now());
        }
    }

    /// Tray clicks toggle the popup. Clicking the tray icon first takes focus
    /// away from the popup (hiding it), so a hide that just happened counts as
    /// the toggle.
    pub fn toggle<T>(&mut self, target: &EventLoopWindowTarget<T>) {
        let just_hidden = self.hidden_at.is_some_and(|at| at.elapsed() < Duration::from_millis(400));
        if self.shown.is_some() {
            self.hide();
        } else if !just_hidden {
            self.show(target, false);
        }
    }

    pub fn update(&mut self, snapshot: Option<Snapshot>) {
        if let (Some((command, _)), Some(snapshot)) = (self.pending, &snapshot) {
            let done = match command {
                Command::Connect | Command::Reconnect => snapshot.link.is_connected(),
                Command::Disconnect => !snapshot.link.is_connected(),
            };
            if done {
                self.pending = None;
            }
        }
        if snapshot != self.snapshot {
            self.snapshot = snapshot;
            self.invalidate();
        }
    }

    /// When the event loop should wake us up next, if ever.
    pub fn deadline(&self) -> Option<Instant> {
        let shown = self.shown.as_ref();
        let frame = shown.map(|s| s.next_frame);
        let hide = shown.and_then(|s| s.hide_at);
        let pending = self.pending.map(|(_, since)| since + PENDING_TIMEOUT);
        [frame, hide, pending].into_iter().flatten().min()
    }

    pub fn on_timer(&mut self, now: Instant) {
        if self.pending.is_some_and(|(_, since)| now >= since + PENDING_TIMEOUT) {
            self.pending = None;
            self.invalidate();
        }
        if self.shown.as_ref().and_then(|s| s.hide_at).is_some_and(|at| now >= at) {
            self.hide();
            return;
        }
        if self.shown.as_ref().is_some_and(|s| now >= s.next_frame) {
            self.redraw();
        }
    }

    pub fn handle_event(&mut self, event: &WindowEvent) -> Option<Action> {
        let has_button = self.button().is_some();
        let shown = self.shown.as_mut()?;
        match event {
            WindowEvent::CursorMoved { position, .. } => {
                let scale = shown.surface.window.scale_factor();
                let hover = draw::hit((position.x / scale) as f32, (position.y / scale) as f32, has_button);
                shown.hide_at = None;
                if hover != shown.hover {
                    shown.hover = hover;
                    let icon = if hover == Hover::None { CursorIcon::Default } else { CursorIcon::Hand };
                    shown.surface.window.set_cursor_icon(icon);
                    shown.background = None;
                }
            }
            WindowEvent::CursorLeft { .. } => {
                if shown.auto {
                    shown.hide_at = Some(Instant::now() + AUTO_HIDE_AFTER_LEAVE);
                }
                if shown.hover != Hover::None {
                    shown.hover = Hover::None;
                    shown.background = None;
                }
            }
            WindowEvent::MouseInput { state: ElementState::Released, button: MouseButton::Left, .. } => {
                match shown.hover {
                    Hover::Close => self.hide(),
                    Hover::Settings => {
                        self.hide();
                        return Some(Action::OpenSettings);
                    }
                    Hover::Button => return self.click_button(),
                    Hover::None => {}
                }
            }
            WindowEvent::Focused(true) => {
                // The user is interacting: stay until they click elsewhere.
                shown.auto = false;
                shown.hide_at = None;
            }
            WindowEvent::Focused(false) if !shown.auto => self.hide(),
            WindowEvent::KeyboardInput { event, .. }
                if event.state == ElementState::Pressed && event.logical_key == Key::Escape =>
            {
                self.hide()
            }
            WindowEvent::CloseRequested => self.hide(),
            WindowEvent::ScaleFactorChanged { .. } | WindowEvent::Resized(_) | WindowEvent::ThemeChanged(_) => {
                shown.background = None;
                shown.sprites_key = None;
            }
            _ => {}
        }
        None
    }

    fn click_button(&mut self) -> Option<Action> {
        let snapshot = self.snapshot.as_ref()?;
        if self.pending.is_some() {
            return None;
        }
        let action = match &snapshot.link {
            Link::Paired(paired) if paired.connected => Action::Engine(Command::Disconnect),
            Link::Paired(_) => Action::Engine(Command::Connect),
            Link::NotPaired => Action::OpenBluetoothSettings,
            Link::Unknown => return None,
        };
        match action {
            Action::Engine(command) => {
                self.pending = Some((command, Instant::now()));
                self.invalidate();
            }
            _ => self.hide(),
        }
        Some(action)
    }

    fn button(&self) -> Option<ButtonSpec> {
        let s = strings();
        let snapshot = self.snapshot.as_ref()?;
        if let Some((command, _)) = self.pending {
            let label = match command {
                Command::Connect | Command::Reconnect => s.connecting,
                Command::Disconnect => s.disconnecting,
            };
            return Some(ButtonSpec { label, style: ButtonStyle::Disabled });
        }
        match &snapshot.link {
            Link::Paired(paired) if paired.connected => {
                Some(ButtonSpec { label: s.disconnect, style: ButtonStyle::Secondary })
            }
            Link::Paired(_) => Some(ButtonSpec { label: s.connect, style: ButtonStyle::Primary }),
            Link::NotPaired => Some(ButtonSpec { label: s.pair, style: ButtonStyle::Primary }),
            Link::Unknown => None,
        }
    }

    fn invalidate(&mut self) {
        if let Some(shown) = &mut self.shown {
            shown.background = None;
        }
    }

    /// Paints the next frame (also called for the OS's redraw requests).
    pub fn redraw(&mut self) {
        let button = self.button();
        let Some(shown) = self.shown.as_mut() else { return };
        if let Err(err) = paint(shown, self.snapshot.as_ref(), button) {
            warn!("could not draw the popup: {err:#}");
        }
        shown.next_frame = Instant::now() + FRAME;
    }
}

fn paint(shown: &mut Shown, snapshot: Option<&Snapshot>, button: Option<ButtonSpec>) -> Result<()> {
    let scale = shown.surface.scale();
    let dark = shown.surface.dark();
    let p = palette(dark);

    let layout = draw::layout(snapshot);
    let key = (layout.iter().map(|(art, _)| *art).collect::<Vec<_>>(), (scale * 100.0) as u32, dark);
    if shown.sprites_key.as_ref() != Some(&key) {
        shown.sprites = layout
            .iter()
            .filter_map(|(art, position)| Some((Sprite::render(*art, scale, &p)?, *position)))
            .collect();
        shown.sprites_key = Some(key);
    }

    if shown.background.is_none() {
        let mut background = shown.surface.canvas().context("allocating the popup")?;
        draw::render_static(&mut background, scale, &shown.fonts, &View { snapshot, button, hover: shown.hover }, &p);
        shown.background = Some(background);
    }
    let Some(background) = &shown.background else { return Ok(()) };
    let mut canvas = background.clone();
    draw::render_art(&mut canvas, scale, &shown.sprites, shown.started.elapsed().as_secs_f32(), &p);
    shown.surface.present(&canvas)
}

/// Renders the popup for `snapshot` to a PNG file, for screenshots and design work.
pub fn render_to_png(snapshot: Option<&Snapshot>, dark: bool, scale: f32, path: &std::path::Path) -> Result<()> {
    let fonts = Fonts::load().context("no usable system font found")?;
    let p = palette(dark);
    let popup = Popup { snapshot: snapshot.cloned(), ..Popup::default() };
    let view = View { snapshot, button: popup.button(), hover: Hover::None };
    let mut pixmap =
        Pixmap::new((draw::WIDTH * scale) as u32, (draw::HEIGHT * scale) as u32).context("allocating")?;
    draw::render_static(&mut pixmap, scale, &fonts, &view, &p);
    let sprites: Vec<_> = draw::layout(snapshot)
        .into_iter()
        .filter_map(|(art, position)| Some((Sprite::render(art, scale, &p)?, position)))
        .collect();
    draw::render_art(&mut pixmap, scale, &sprites, 0.0, &p);
    pixmap.save_png(path)?;
    Ok(())
}

fn create<T>(target: &EventLoopWindowTarget<T>, auto: bool) -> Result<Shown> {
    let fonts = Fonts::load().context("no usable system font found")?;
    #[allow(unused_mut)]
    let mut builder = WindowBuilder::new()
        .with_title("Kord")
        .with_decorations(false)
        .with_resizable(false)
        .with_always_on_top(true)
        .with_visible(false)
        .with_focused(!auto)
        .with_inner_size(LogicalSize::new(f64::from(draw::WIDTH), f64::from(draw::HEIGHT)));
    #[cfg(windows)]
    {
        use tao::platform::windows::WindowBuilderExtWindows;
        builder = builder.with_skip_taskbar(true).with_undecorated_shadow(true);
    }
    #[cfg(target_os = "linux")]
    {
        use tao::platform::unix::WindowBuilderExtUnix;
        builder = builder.with_skip_taskbar(true);
    }
    let window = builder.build(target).context("creating the popup window")?;
    place(&window);
    #[cfg(windows)]
    crate::ui::surface::round_corners(&window);

    let surface = Surface::new(window)?;
    surface.window.set_visible(true);
    if !auto {
        surface.window.set_focus();
    }
    let now = Instant::now();
    Ok(Shown {
        surface,
        fonts,
        hover: Hover::None,
        auto,
        hide_at: auto.then(|| now + AUTO_HIDE),
        started: now,
        next_frame: now,
        background: None,
        sprites: Vec::new(),
        sprites_key: None,
    })
}

/// Bottom-right corner, just above the taskbar, where tray flyouts appear.
fn place(window: &Window) {
    let size = window.outer_size();
    let margin = (12.0 * window.scale_factor()) as i32;
    #[cfg(windows)]
    let area = work_area();
    #[cfg(not(windows))]
    let area = window.current_monitor().or_else(|| window.primary_monitor()).map(|monitor| {
        let (pos, size) = (monitor.position(), monitor.size());
        // Leave room for a bottom panel.
        let panel = (48.0 * monitor.scale_factor()) as i32;
        (pos.x + size.width as i32, pos.y + size.height as i32 - panel)
    });
    if let Some((right, bottom)) = area {
        window.set_outer_position(PhysicalPosition::new(
            right - size.width as i32 - margin,
            bottom - size.height as i32 - margin,
        ));
    }
}

/// Right and bottom edges of the primary monitor's area not covered by the taskbar.
#[cfg(windows)]
fn work_area() -> Option<(i32, i32)> {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::UI::WindowsAndMessaging::{
        SPI_GETWORKAREA, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
    };
    let mut rect = RECT::default();
    // SAFETY: SPI_GETWORKAREA writes a RECT into the buffer we pass.
    unsafe {
        SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            Some(&mut rect as *mut RECT as *mut _),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
        .ok()?;
    }
    Some((rect.right, rect.bottom))
}
