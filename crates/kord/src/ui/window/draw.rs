//! Paints the main window: the sidebar, and the device list, device, settings
//! and about pages.
//!
//! Painting also produces the clickable regions ([`Hit`]s), so layout and
//! hit-testing can never disagree.

use kord_core::{Battery, DeviceKind};
use tiny_skia::{Color, ColorU8, FilterQuality, PathBuilder, Pixmap, PixmapPaint, Rect, Transform};

use super::entry::Entry;
use super::rows::{Control, Id, Section};
use crate::i18n::strings;
use crate::ui::art::{self, Art, Sprite};
use crate::ui::paint::{self, Palette, contains, fill, fill_rounded, gear, line, rect, rounded_rect, stroke};
use crate::ui::text::{Align, Fonts, Weight};

/// The pages reachable from the sidebar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nav {
    Devices,
    Settings,
    About,
}

/// Something clickable, in logical window coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Target {
    Nav(Nav),
    /// A device card, by its index in the entry list.
    Card(usize),
    AddDevice,
    Back,
    DeviceButton,
    Control(Id),
    Option(Id, usize),
    /// A slider and its track (left, right) in logical x.
    Slider(Id, f32, f32),
}

impl Target {
    /// Hover identity, ignoring geometry.
    pub fn same(&self, other: &Target) -> bool {
        match (self, other) {
            (Target::Slider(a, ..), Target::Slider(b, ..)) => a == b,
            _ => self == other,
        }
    }
}

pub struct Hit {
    pub rect: Rect,
    pub target: Target,
}

pub fn hit_test(hits: &[Hit], x: f32, y: f32) -> Option<Target> {
    // Later hits (the sidebar, open dropdown lists) are drawn on top, so they win.
    hits.iter().rev().find(|hit| contains(hit.rect, x, y)).map(|hit| hit.target)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceButton {
    pub label: &'static str,
    pub primary: bool,
    pub enabled: bool,
}

/// What a page shows.
pub enum Content<'a> {
    Devices { entries: &'a [Entry] },
    Device { entry: &'a Entry, button: Option<DeviceButton>, sections: &'a [Section] },
    Settings { sections: &'a [Section] },
    About,
}

pub struct View<'a> {
    pub content: Content<'a>,
    pub connected: usize,
    pub hover: Option<Target>,
    pub open_choice: Option<Id>,
    pub scroll: f32,
}

/// Device illustrations rendered once per scale and theme, reused across frames.
#[derive(Default)]
pub struct Sprites {
    cache: Vec<(Art, u32, ColorU8, Sprite)>,
}

impl Sprites {
    fn get(&mut self, art: Art, scale: f32, p: &Palette) -> Option<&Sprite> {
        let key = (art, (scale * 100.0) as u32, p.art_outline.to_color_u8());
        let index = match self.cache.iter().position(|(a, s, c, _)| (*a, *s, *c) == key) {
            Some(index) => index,
            None => {
                self.cache.push((key.0, key.1, key.2, Sprite::render(art, scale, p)?));
                self.cache.len() - 1
            }
        };
        self.cache.get(index).map(|(.., sprite)| sprite)
    }
}

pub const SIDEBAR: f32 = 232.0;
const MARGIN: f32 = 36.0;
const MAX_CONTENT: f32 = 820.0;
const ROW_PAD: f32 = 16.0;
const CARD_MIN_W: f32 = 236.0;
const CARD_H: f32 = 218.0;
const GAP: f32 = 16.0;

struct Ctx<'a> {
    pixmap: &'a mut Pixmap,
    t: Transform,
    scale: f32,
    fonts: &'a Fonts,
    sprites: &'a mut Sprites,
    p: &'a Palette,
    hits: Vec<Hit>,
    hover: Option<Target>,
}

impl Ctx<'_> {
    fn is_hover(&self, target: Target) -> bool {
        self.hover.is_some_and(|h| h.same(&target))
    }

    fn text(&mut self, text: &str, at: (f32, f32), size: f32, weight: Weight, align: Align, color: Color) {
        self.fonts.draw(self.pixmap, self.t, text, at, size, weight, align, color);
    }

    fn hit(&mut self, rect: Rect, target: Target) {
        self.hits.push(Hit { rect, target });
    }
}

/// Paints the window and returns its clickable regions and the content height.
#[allow(clippy::too_many_arguments)]
pub fn render(
    pixmap: &mut Pixmap,
    scale: f32,
    (width, height): (f32, f32),
    fonts: &Fonts,
    sprites: &mut Sprites,
    view: &View,
    p: &Palette,
) -> (Vec<Hit>, f32) {
    pixmap.fill(p.background);
    let mut cx = Ctx {
        pixmap,
        t: Transform::from_scale(scale, scale),
        scale,
        fonts,
        sprites,
        p,
        hits: Vec::new(),
        hover: view.hover,
    };
    let content_w = (width - SIDEBAR - 2.0 * MARGIN).clamp(280.0, MAX_CONTENT);
    let x = SIDEBAR + MARGIN;
    let top = 28.0 - view.scroll;
    let mut dropdown = None;
    let bottom = match &view.content {
        Content::Devices { entries } => devices_page(&mut cx, x, top, content_w, entries),
        Content::Device { entry, button, sections } => {
            let y = device_page(&mut cx, x, top, content_w, entry, *button);
            sections_block(&mut cx, x, y, content_w, sections, view.open_choice, &mut dropdown)
        }
        Content::Settings { sections } => {
            let s = strings();
            let y = page_title(&mut cx, x, top, content_w, s.nav_settings, s.settings_subtitle);
            sections_block(&mut cx, x, y, content_w, sections, view.open_choice, &mut dropdown)
        }
        Content::About => about_page(&mut cx, x, top, content_w),
    };
    let content_h = bottom + 32.0 + view.scroll;

    sidebar(&mut cx, height, view);
    if let Some((id, anchor, selected, options)) = dropdown {
        dropdown_list(&mut cx, height, id, anchor, selected, &options);
    }
    (cx.hits, content_h)
}

fn sidebar(cx: &mut Ctx, height: f32, view: &View) {
    let p = cx.p;
    let t = cx.t;
    fill(cx.pixmap, &PathBuilder::from_rect(rect(0.0, 0.0, SIDEBAR, height)), p.background, t);
    line(cx.pixmap, (SIDEBAR - 0.5, 0.0), (SIDEBAR - 0.5, height), p.border, 1.0, t);

    // Logo and name.
    let logo = crate::icon::app((36.0 * cx.scale).round() as u32);
    let paint = PixmapPaint { quality: FilterQuality::Bilinear, ..PixmapPaint::default() };
    cx.pixmap.draw_pixmap(0, 0, logo.as_ref(), &paint, Transform::from_translate(20.0 * cx.scale, 22.0 * cx.scale), None);
    cx.text("Kord", (66.0, 46.0), 20.0, Weight::Bold, Align::Left, p.text);

    let s = strings();
    let selected = match view.content {
        Content::Devices { .. } | Content::Device { .. } => Nav::Devices,
        Content::Settings { .. } => Nav::Settings,
        Content::About => Nav::About,
    };
    for (index, (nav, label)) in
        [(Nav::Devices, s.nav_devices), (Nav::Settings, s.nav_settings), (Nav::About, s.nav_about)].into_iter().enumerate()
    {
        let r = rect(12.0, 84.0 + index as f32 * 44.0, SIDEBAR - 24.0, 38.0);
        let target = Target::Nav(nav);
        if nav == selected {
            fill_rounded(cx.pixmap, r, 7.0, p.card, t);
            if let Some(border) = rounded_rect(r, 7.0) {
                stroke(cx.pixmap, &border, p.border, 1.0, t);
            }
            fill_rounded(cx.pixmap, rect(r.left() + 1.0, r.top() + 11.0, 3.0, 16.0), 1.5, p.primary, t);
        } else if cx.is_hover(target) {
            fill_rounded(cx.pixmap, r, 7.0, p.neutral, t);
        }
        let middle = r.top() + r.height() / 2.0;
        let color = if nav == selected { p.text } else { p.secondary };
        nav_glyph(cx, nav, (r.left() + 24.0, middle), color);
        let weight = if nav == selected { Weight::Bold } else { Weight::Regular };
        cx.text(label, (r.left() + 46.0, middle + 5.0), 14.0, weight, Align::Left, p.text);
        cx.hit(r, target);
    }

    // How many headphones play here right now, and the version.
    let status = format!("{} {}", view.connected, s.connected_count);
    let dot = (24.0, height - 47.0);
    if let Some(circle) = PathBuilder::from_circle(dot.0, dot.1, 3.5) {
        if view.connected > 0 {
            fill(cx.pixmap, &circle, p.text, t);
        } else {
            stroke(cx.pixmap, &circle, p.secondary, 1.2, t);
        }
    }
    cx.text(&status, (36.0, height - 43.0), 12.0, Weight::Regular, Align::Left, p.secondary);
    let version = format!("Kord {}", env!("CARGO_PKG_VERSION"));
    cx.text(&version, (20.0, height - 22.0), 12.0, Weight::Regular, Align::Left, p.secondary);
}

fn nav_glyph(cx: &mut Ctx, nav: Nav, (x, y): (f32, f32), color: Color) {
    let t = cx.t;
    match nav {
        Nav::Devices => crate::icon::glyph(cx.pixmap, t.pre_translate(x - 9.6, y - 9.6).pre_scale(0.6, 0.6), color),
        Nav::Settings => gear(cx.pixmap, (x, y), color, t),
        Nav::About => {
            if let Some(ring) = PathBuilder::from_circle(x, y, 7.5) {
                stroke(cx.pixmap, &ring, color, 1.5, t);
            }
            line(cx.pixmap, (x, y - 0.5), (x, y + 3.5), color, 1.7, t);
            if let Some(dot) = PathBuilder::from_circle(x, y - 3.6, 1.1) {
                fill(cx.pixmap, &dot, color, t);
            }
        }
    }
}

/// Big page title and subtitle; returns where the content starts.
fn page_title(cx: &mut Ctx, x: f32, y: f32, width: f32, title: &str, subtitle: &str) -> f32 {
    let p = cx.p;
    let title = cx.fonts.fit(title, 26.0, Weight::Bold, width);
    cx.text(&title, (x, y + 30.0), 26.0, Weight::Bold, Align::Left, p.text);
    let mut bottom = y + 40.0;
    for (index, text) in cx.fonts.wrap(subtitle, 13.0, Weight::Regular, width).iter().enumerate() {
        bottom = y + 58.0 + index as f32 * 18.0;
        cx.text(text, (x, bottom), 13.0, Weight::Regular, Align::Left, p.secondary);
    }
    bottom + 22.0
}

fn devices_page(cx: &mut Ctx, x: f32, y: f32, width: f32, entries: &[Entry]) -> f32 {
    let s = strings();
    let y = page_title(cx, x, y, width, s.nav_devices, s.devices_subtitle);
    let columns = (((width + GAP) / (CARD_MIN_W + GAP)).floor() as usize).max(1);
    let card_w = (width - GAP * (columns - 1) as f32) / columns as f32;
    let place = |index: usize| {
        let (column, row) = (index % columns, index / columns);
        rect(x + column as f32 * (card_w + GAP), y + row as f32 * (CARD_H + GAP), card_w, CARD_H)
    };
    for (index, entry) in entries.iter().enumerate() {
        device_card(cx, place(index), index, entry);
    }
    let add = place(entries.len());
    add_card(cx, add);
    add.bottom()
}

fn card_frame(cx: &mut Ctx, r: Rect, target: Target) {
    let p = cx.p;
    fill_rounded(cx.pixmap, r, 12.0, p.card, cx.t);
    let hovered = cx.is_hover(target);
    if let Some(border) = rounded_rect(r, 12.0) {
        let (color, width) = if hovered { (p.secondary, 1.4) } else { (p.border, 1.0) };
        stroke(cx.pixmap, &border, color, width, cx.t);
    }
    cx.hit(r, target);
}

fn device_card(cx: &mut Ctx, r: Rect, index: usize, entry: &Entry) {
    let p = cx.p;
    let t = cx.t;
    card_frame(cx, r, Target::Card(index));
    let panel = rect(r.left() + 10.0, r.top() + 10.0, r.width() - 20.0, 116.0);
    fill_rounded(cx.pixmap, panel, 8.0, p.background, t);
    draw_arts(cx, &entry.arts(), panel);

    let (left, right) = (r.left() + 16.0, r.right() - 16.0);
    let name = cx.fonts.fit(&entry.name, 15.0, Weight::Bold, r.width() - 52.0);
    cx.text(&name, (left, r.top() + 152.0), 15.0, Weight::Bold, Align::Left, p.text);
    chevron_right(cx.pixmap, t, (right - 3.0, r.top() + 147.0), p.secondary);
    let model = cx.fonts.fit(&entry.model, 12.0, Weight::Regular, r.width() - 32.0);
    cx.text(&model, (left, r.top() + 171.0), 12.0, Weight::Regular, Align::Left, p.secondary);

    let baseline = r.top() + 199.0;
    status_dot(cx, (left + 4.0, baseline - 4.0), entry.connected());
    let battery = entry.status.as_ref().map(|status| match status.kind {
        DeviceKind::Earbuds => {
            Battery { level: status.lowest_level(), charging: status.left.charging || status.right.charging }
        }
        DeviceKind::Headphones => Battery { level: status.lowest_level(), charging: status.left.charging },
    });
    let room = if battery.is_some() { r.width() - 120.0 } else { r.width() - 44.0 };
    let status = cx.fonts.fit(&entry.status_text(), 12.0, Weight::Regular, room);
    cx.text(&status, (left + 14.0, baseline), 12.0, Weight::Regular, Align::Left, p.text);
    if let Some(battery) = battery.filter(|b| b.level.is_some()) {
        let center = right - paint::battery_width(cx.fonts, battery) / 2.0;
        paint::battery(cx.pixmap, t, cx.fonts, (center, baseline), None, battery, p);
    }
}

fn add_card(cx: &mut Ctx, r: Rect) {
    let p = cx.p;
    let t = cx.t;
    card_frame(cx, r, Target::AddDevice);
    let panel = rect(r.left() + 10.0, r.top() + 10.0, r.width() - 20.0, 116.0);
    let (x, y) = (panel.left() + panel.width() / 2.0, panel.top() + panel.height() / 2.0);
    if let Some(circle) = PathBuilder::from_circle(x, y, 24.0) {
        fill(cx.pixmap, &circle, if cx.is_hover(Target::AddDevice) { p.neutral_hover } else { p.neutral }, t);
    }
    line(cx.pixmap, (x - 8.0, y), (x + 8.0, y), p.text, 2.0, t);
    line(cx.pixmap, (x, y - 8.0), (x, y + 8.0), p.text, 2.0, t);
    let s = strings();
    let left = r.left() + 16.0;
    cx.text(s.add_device, (left, r.top() + 152.0), 15.0, Weight::Bold, Align::Left, p.text);
    for (index, text) in cx.fonts.wrap(s.add_device_description, 12.0, Weight::Regular, r.width() - 32.0).iter().take(2).enumerate() {
        cx.text(text, (left, r.top() + 171.0 + index as f32 * 16.0), 12.0, Weight::Regular, Align::Left, p.secondary);
    }
}

fn status_dot(cx: &mut Ctx, (x, y): (f32, f32), on: bool) {
    let Some(circle) = PathBuilder::from_circle(x, y, 3.5) else { return };
    if on {
        fill(cx.pixmap, &circle, cx.p.text, cx.t);
    } else {
        stroke(cx.pixmap, &circle, cx.p.secondary, 1.2, cx.t);
    }
}

/// Draws the illustrations side by side, as large as fits in `area`, each
/// resting on a soft shadow.
fn draw_arts(cx: &mut Ctx, arts: &[Art], area: Rect) {
    const SPACING: f32 = 10.0;
    let total_w: f32 = arts.iter().map(|a| a.size().0).sum::<f32>() + SPACING * arts.len().saturating_sub(1) as f32;
    let max_h = arts.iter().map(|a| a.size().1).fold(0.0, f32::max);
    if total_w <= 0.0 || max_h <= 0.0 {
        return;
    }
    let zoom = (area.height() * 0.8 / max_h).min(area.width() * 0.86 / total_w);
    let mut x = area.left() + (area.width() - total_w * zoom) / 2.0;
    let floor = area.top() + area.height() / 2.0 + max_h * zoom / 2.0;
    for &art in arts {
        let (w, h) = art.size();
        art::shadow(cx.pixmap, cx.t, (x + w * zoom / 2.0, floor + 1.0), w * zoom * 0.55, 0.8, cx.p);
        let (scale, p) = (cx.scale, cx.p);
        // One sharp rendering serves every zoom up to the hero's.
        if let Some(sprite) = cx.sprites.get(art, scale * 1.5, p) {
            sprite.draw_at(cx.pixmap, (x, floor - h * zoom), scale, zoom);
        }
        x += (w + SPACING) * zoom;
    }
}

/// Back link, name, the illustration with batteries and the connect button.
/// Returns where the option sections start.
fn device_page(cx: &mut Ctx, x: f32, y: f32, width: f32, entry: &Entry, button: Option<DeviceButton>) -> f32 {
    let p = cx.p;
    let t = cx.t;
    let s = strings();

    let back_w = cx.fonts.measure(s.nav_devices, 13.0, Weight::Regular) + 22.0;
    let back = rect(x - 6.0, y - 4.0, back_w + 12.0, 26.0);
    let color = if cx.is_hover(Target::Back) { p.text } else { p.secondary };
    chevron_left(cx.pixmap, t, (x + 3.0, y + 9.0), color);
    cx.text(s.nav_devices, (x + 16.0, y + 14.0), 13.0, Weight::Regular, Align::Left, color);
    cx.hit(back, Target::Back);

    let subtitle = format!("{} · {}", entry.model, entry.status_text());
    let y = page_title(cx, x, y + 26.0, width, &entry.name, &subtitle);

    // Hero card.
    let hero = rect(x, y, width, 212.0);
    fill_rounded(cx.pixmap, hero, 12.0, p.card, t);
    let panel_w = (width * 0.48).min(340.0);
    let panel = rect(hero.left() + 12.0, hero.top() + 12.0, panel_w, hero.height() - 24.0);
    fill_rounded(cx.pixmap, panel, 9.0, p.background, t);
    draw_arts(cx, &entry.arts(), panel);

    let left = panel.right() + 24.0;
    let right = hero.right() - 20.0;
    let mut row_y = hero.top() + 40.0;
    match &entry.status {
        Some(status) => {
            let rows: Vec<(&str, Battery)> = match status.kind {
                DeviceKind::Earbuds => vec![(s.left, status.left), (s.right, status.right), (s.case, status.case)],
                DeviceKind::Headphones => vec![(s.battery, Battery { level: status.lowest_level(), ..status.left })],
            };
            for (label, battery) in rows {
                cx.text(label, (left, row_y), 13.0, Weight::Regular, Align::Left, p.secondary);
                let center = left + 100.0 + paint::battery_width(cx.fonts, battery) / 2.0;
                paint::battery(cx.pixmap, t, cx.fonts, (center, row_y), None, battery, p);
                if let Some(ear) = in_ear(status, label) {
                    cx.text(ear, (right, row_y), 12.0, Weight::Regular, Align::Right, p.secondary);
                }
                row_y += 30.0;
            }
        }
        None if entry.apple() => {
            for text in cx.fonts.wrap(s.battery_hint, 13.0, Weight::Regular, right - left).iter().take(3) {
                cx.text(text, (left, row_y), 13.0, Weight::Regular, Align::Left, p.secondary);
                row_y += 19.0;
            }
        }
        None => {}
    }

    if let Some(button) = button {
        let bw = (cx.fonts.measure(button.label, 13.0, Weight::Bold) + 40.0).max(150.0);
        let r = rect(left, hero.bottom() - 58.0, bw.min(right - left), 36.0);
        let hovered = button.enabled && cx.is_hover(Target::DeviceButton);
        let (background, foreground) = match (button.enabled, button.primary) {
            (false, _) => (p.neutral, p.secondary),
            (true, true) => (if hovered { p.primary_hover } else { p.primary }, p.on_primary),
            (true, false) => (if hovered { p.neutral_hover } else { p.neutral }, p.text),
        };
        fill_rounded(cx.pixmap, r, 6.0, background, t);
        cx.text(button.label, (r.left() + r.width() / 2.0, r.top() + 23.0), 13.0, Weight::Bold, Align::Center, foreground);
        if button.enabled {
            cx.hit(r, Target::DeviceButton);
        }
    }

    let mut y = hero.bottom() + 22.0;
    let note = if entry.apple() { s.shared_settings_note } else { s.limited_support_note };
    for text in cx.fonts.wrap(note, 12.0, Weight::Regular, width - 8.0) {
        cx.text(&text, (x + 4.0, y), 12.0, Weight::Regular, Align::Left, p.secondary);
        y += 17.0;
    }
    y - 12.0
}

/// "In ear" next to an earbud that is in one.
fn in_ear(status: &kord_core::DeviceStatus, label: &str) -> Option<&'static str> {
    let s = strings();
    let worn = if label == s.left {
        status.ears.left
    } else if label == s.right {
        status.ears.right
    } else {
        None
    };
    (worn == Some(true)).then_some(s.in_ear)
}

fn about_page(cx: &mut Ctx, x: f32, y: f32, width: f32) -> f32 {
    let p = cx.p;
    let t = cx.t;
    let s = strings();

    let hero = rect(x, y, width, 150.0);
    fill_rounded(cx.pixmap, hero, 12.0, p.card, t);
    let logo = crate::icon::app((88.0 * cx.scale).round() as u32);
    let paint = PixmapPaint { quality: FilterQuality::Bilinear, ..PixmapPaint::default() };
    let at = Transform::from_translate((hero.left() + 30.0) * cx.scale, (hero.top() + 31.0) * cx.scale);
    cx.pixmap.draw_pixmap(0, 0, logo.as_ref(), &paint, at, None);
    let left = hero.left() + 146.0;
    cx.text("Kord", (left, hero.top() + 56.0), 28.0, Weight::Bold, Align::Left, p.text);
    let version = format!("{} · MIT", env!("CARGO_PKG_VERSION"));
    cx.text(&version, (left, hero.top() + 80.0), 13.0, Weight::Regular, Align::Left, p.secondary);
    for (index, text) in cx.fonts.wrap(s.about_tagline, 13.0, Weight::Regular, hero.right() - left - 20.0).iter().take(2).enumerate() {
        cx.text(text, (left, hero.top() + 108.0 + index as f32 * 18.0), 13.0, Weight::Regular, Align::Left, p.text);
    }

    let mut y = hero.bottom() + 34.0;
    cx.text(s.about_features_title, (x + 4.0, y), 13.0, Weight::Bold, Align::Left, p.text);
    y += 10.0;
    let features: Vec<&str> = s.about_features.split(" · ").collect();
    let card = rect(x, y, width, features.len() as f32 * 34.0 + 16.0);
    fill_rounded(cx.pixmap, card, 12.0, p.card, t);
    for (index, feature) in features.iter().enumerate() {
        let baseline = card.top() + 31.0 + index as f32 * 34.0;
        if let Some(dot) = PathBuilder::from_circle(x + 24.0, baseline - 4.5, 2.5) {
            fill(cx.pixmap, &dot, p.text, t);
        }
        cx.text(feature, (x + 38.0, baseline), 13.0, Weight::Regular, Align::Left, p.text);
    }

    let mut y = card.bottom() + 34.0;
    cx.text(s.about_credits_title, (x + 4.0, y), 13.0, Weight::Bold, Align::Left, p.text);
    y += 10.0;
    let lines: Vec<String> =
        [s.about_license, s.about_credits].iter().flat_map(|text| cx.fonts.wrap(text, 12.0, Weight::Regular, width - 40.0)).collect();
    let card = rect(x, y, width, lines.len() as f32 * 18.0 + 30.0);
    fill_rounded(cx.pixmap, card, 12.0, p.card, t);
    for (index, text) in lines.iter().enumerate() {
        cx.text(text, (x + 20.0, card.top() + 27.0 + index as f32 * 18.0), 12.0, Weight::Regular, Align::Left, p.secondary);
    }
    card.bottom()
}

type Dropdown = Option<(Id, Rect, usize, Vec<&'static str>)>;

/// Option sections, each on a card. Returns the bottom of the last one.
fn sections_block(
    cx: &mut Ctx,
    x: f32,
    mut y: f32,
    width: f32,
    sections: &[Section],
    open_choice: Option<Id>,
    dropdown: &mut Dropdown,
) -> f32 {
    let p = cx.p;
    let t = cx.t;
    let s = strings();
    for section in sections {
        y += 30.0;
        cx.text(section.title, (x + 4.0, y), 13.0, Weight::Bold, Align::Left, p.text);
        y += 10.0;
        let heights: Vec<(f32, Vec<String>)> = section
            .rows
            .iter()
            .map(|row| {
                if row.description.is_empty() {
                    return (52.0, Vec::new());
                }
                let lines = cx.fonts.wrap(&row.description, 12.0, Weight::Regular, text_width(width, &row.control));
                ((14.0 + 20.0 + lines.len() as f32 * 17.0 + 12.0).max(60.0), lines)
            })
            .collect();
        let card_h: f32 = heights.iter().map(|(h, _)| h).sum();
        fill_rounded(cx.pixmap, rect(x, y, width, card_h), 12.0, p.card, t);

        for (index, (row, (row_h, lines))) in section.rows.iter().zip(&heights).enumerate() {
            if index > 0 {
                line(cx.pixmap, (x + ROW_PAD, y), (x + width - ROW_PAD, y), p.border, 1.0, t);
            }
            let left = x + ROW_PAD;
            let middle = y + row_h / 2.0;
            let title_y = if lines.is_empty() { middle + 5.0 } else { y + 28.0 };
            cx.text(row.title, (left, title_y), 14.0, Weight::Regular, Align::Left, p.text);
            for (line_index, text) in lines.iter().enumerate() {
                cx.text(text, (left, y + 48.0 + line_index as f32 * 17.0), 12.0, Weight::Regular, Align::Left, p.secondary);
            }
            let right = x + width - ROW_PAD;
            let target = Target::Control(row.id);
            match &row.control {
                Control::Toggle(on) => {
                    let r = rect(right - 40.0, middle - 10.0, 40.0, 20.0);
                    toggle(cx.pixmap, t, r, *on, cx.is_hover(target), p);
                    let label = if *on { s.on } else { s.off };
                    cx.text(label, (right - 52.0, middle + 5.0), 13.0, Weight::Regular, Align::Right, p.text);
                    cx.hit(rect(right - 130.0, middle - 16.0, 130.0, 32.0), target);
                }
                Control::Choice { selected, options } => {
                    let r = rect(right - 230.0, middle - 16.0, 230.0, 32.0);
                    let open = open_choice == Some(row.id);
                    let hovered = cx.is_hover(target) || open;
                    fill_rounded(cx.pixmap, r, 6.0, if hovered { p.neutral_hover } else { p.neutral }, t);
                    let label = cx.fonts.fit(options.get(*selected).copied().unwrap_or(""), 13.0, Weight::Regular, 190.0);
                    cx.text(&label, (r.left() + 12.0, middle + 5.0), 13.0, Weight::Regular, Align::Left, p.text);
                    chevron_down(cx.pixmap, t, (r.right() - 16.0, middle), p.secondary);
                    cx.hit(r, target);
                    if open {
                        *dropdown = Some((row.id, r, *selected, options.clone()));
                    }
                }
                Control::Slider { value, max, .. } => {
                    let (left, track_right) = (right - 190.0, right - 8.0);
                    let label = if *value == 0 { s.slider_off.to_string() } else { format!("{value}%") };
                    cx.text(&label, (left - 14.0, middle + 5.0), 13.0, Weight::Regular, Align::Right, p.text);
                    let fraction = f32::from(*value) / f32::from((*max).max(1));
                    let knob_x = left + (track_right - left) * fraction;
                    line(cx.pixmap, (left, middle), (track_right, middle), p.secondary, 3.0, t);
                    line(cx.pixmap, (left, middle), (knob_x, middle), p.primary, 3.0, t);
                    let target = Target::Slider(row.id, left, track_right);
                    let radius = if cx.is_hover(target) { 8.5 } else { 7.5 };
                    if let Some(knob) = PathBuilder::from_circle(knob_x, middle, radius) {
                        fill(cx.pixmap, &knob, p.primary, t);
                        stroke(cx.pixmap, &knob, p.card, 2.0, t);
                    }
                    cx.hit(rect(left - 10.0, middle - 14.0, track_right - left + 20.0, 28.0), target);
                }
                Control::Button { label, enabled } => {
                    let r = button(cx, (right, middle), label, *enabled, cx.is_hover(target));
                    if *enabled {
                        cx.hit(r, target);
                    }
                }
                Control::Test { label, enabled, level } => {
                    let r = button(cx, (right, middle), label, *enabled, cx.is_hover(target));
                    if *enabled {
                        cx.hit(r, target);
                    }
                    // Input level meter.
                    let meter = rect(r.left() - 136.0, middle - 4.0, 120.0, 8.0);
                    fill_rounded(cx.pixmap, meter, 4.0, p.neutral, t);
                    if *level > 0.01 {
                        let filled = rect(meter.left(), meter.top(), meter.width() * level.min(1.0), 8.0);
                        fill_rounded(cx.pixmap, filled, 4.0, p.primary, t);
                    }
                }
                Control::Info(text) => {
                    cx.text(text, (right, middle + 5.0), 13.0, Weight::Regular, Align::Right, p.secondary);
                }
                Control::Busy => {
                    cx.text(s.applying, (right, middle + 5.0), 13.0, Weight::Regular, Align::Right, p.secondary);
                }
            }
            y += row_h;
        }
    }
    y
}

fn dropdown_list(cx: &mut Ctx, height: f32, id: Id, anchor: Rect, selected: usize, options: &[&'static str]) {
    let p = cx.p;
    let t = cx.t;
    let list_h = options.len() as f32 * 34.0 + 8.0;
    let below = anchor.bottom() + 4.0;
    let top = if below + list_h > height - 8.0 { anchor.top() - 4.0 - list_h } else { below };
    let list = rect(anchor.left(), top, anchor.width(), list_h);
    if let Some(shadow) = rounded_rect(rect(list.left() + 1.0, list.top() + 3.0, list.width(), list.height()), 8.0) {
        fill(cx.pixmap, &shadow, p.shadow, t);
    }
    fill_rounded(cx.pixmap, list, 8.0, p.card, t);
    if let Some(border) = rounded_rect(list, 8.0) {
        stroke(cx.pixmap, &border, p.border, 1.0, t);
    }
    for (index, option) in options.iter().enumerate() {
        let r = rect(list.left() + 4.0, list.top() + 4.0 + index as f32 * 34.0, list.width() - 8.0, 34.0);
        let target = Target::Option(id, index);
        let hovered = cx.is_hover(target);
        if hovered || index == selected {
            fill_rounded(cx.pixmap, r, 5.0, if hovered { p.neutral_hover } else { p.neutral }, t);
        }
        if index == selected {
            fill_rounded(cx.pixmap, rect(r.left() + 2.0, r.top() + 10.0, 3.0, 14.0), 1.5, p.primary, t);
        }
        cx.text(option, (r.left() + 14.0, r.top() + 22.0), 13.0, Weight::Regular, Align::Left, p.text);
        cx.hit(r, target);
    }
}

/// A neutral push button right-aligned at `right`, vertically centred on `middle`.
fn button(cx: &mut Ctx, (right, middle): (f32, f32), label: &str, enabled: bool, hovered: bool) -> Rect {
    let p = cx.p;
    let w = (cx.fonts.measure(label, 13.0, Weight::Regular) + 32.0).max(110.0);
    let r = rect(right - w, middle - 16.0, w, 32.0);
    let background = if enabled && hovered { p.neutral_hover } else { p.neutral };
    fill_rounded(cx.pixmap, r, 6.0, background, cx.t);
    let color = if enabled { p.text } else { p.secondary };
    cx.text(label, (r.left() + w / 2.0, middle + 5.0), 13.0, Weight::Regular, Align::Center, color);
    r
}

/// Room for a row's text next to its control.
fn text_width(content_w: f32, control: &Control) -> f32 {
    let control_w = match control {
        Control::Toggle(_) | Control::Busy => 140.0,
        Control::Choice { .. } => 250.0,
        Control::Slider { .. } => 260.0,
        Control::Button { .. } | Control::Info(_) => 170.0,
        Control::Test { .. } => 300.0,
    };
    (content_w - 2.0 * ROW_PAD - control_w - 16.0).max(120.0)
}

fn toggle(pixmap: &mut Pixmap, t: Transform, r: Rect, on: bool, hovered: bool, p: &Palette) {
    let Some(pill) = rounded_rect(r, r.height() / 2.0) else { return };
    let knob_r = if hovered { 6.0 } else { 5.0 };
    let cy = r.top() + r.height() / 2.0;
    if on {
        fill(pixmap, &pill, if hovered { p.primary_hover } else { p.primary }, t);
        if let Some(knob) = PathBuilder::from_circle(r.right() - 10.0, cy, knob_r) {
            fill(pixmap, &knob, p.on_primary, t);
        }
    } else {
        stroke(pixmap, &pill, p.secondary, 1.2, t);
        if let Some(knob) = PathBuilder::from_circle(r.left() + 10.0, cy, knob_r) {
            fill(pixmap, &knob, p.secondary, t);
        }
    }
}

fn chevron_down(pixmap: &mut Pixmap, t: Transform, (x, y): (f32, f32), color: Color) {
    line(pixmap, (x - 4.0, y - 2.0), (x, y + 2.0), color, 1.3, t);
    line(pixmap, (x, y + 2.0), (x + 4.0, y - 2.0), color, 1.3, t);
}

fn chevron_right(pixmap: &mut Pixmap, t: Transform, (x, y): (f32, f32), color: Color) {
    line(pixmap, (x - 2.0, y - 4.0), (x + 2.0, y), color, 1.4, t);
    line(pixmap, (x + 2.0, y), (x - 2.0, y + 4.0), color, 1.4, t);
}

fn chevron_left(pixmap: &mut Pixmap, t: Transform, (x, y): (f32, f32), color: Color) {
    line(pixmap, (x + 2.0, y - 4.5), (x - 2.5, y), color, 1.5, t);
    line(pixmap, (x - 2.5, y), (x + 2.0, y + 4.5), color, 1.5, t);
}
