//! Paints the popup: device name, floating device illustrations with their
//! batteries, and an action button. Laid out in logical pixels on a fixed
//! card and scaled to the display's DPI by one transform.
//!
//! Painting is split in two: the static layer (text, batteries, buttons) is
//! cached, and only the floating sprites are composited on each animation frame.

use kord_core::{Battery, DeviceKind};
use tiny_skia::{PathBuilder, Pixmap, Rect, Transform};

use crate::engine::Snapshot;
use crate::i18n::strings;
use crate::link::Link;
use crate::ui::art::{self, Art, Family, Sprite};
use crate::ui::paint::{self, Palette, contains, cross, fill, fill_rounded, gear, rect};
use crate::ui::text::{Align, Fonts, Weight};

pub const WIDTH: f32 = 320.0;
pub const HEIGHT: f32 = 272.0;
const BATTERY_Y: f32 = 196.0;
/// Where the floating illustrations rest.
const ART_BOTTOM: f32 = 166.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hover {
    None,
    Button,
    Close,
    Settings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonStyle {
    Primary,
    Secondary,
    Disabled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ButtonSpec {
    pub label: &'static str,
    pub style: ButtonStyle,
}

pub struct View<'a> {
    pub snapshot: Option<&'a Snapshot>,
    pub button: Option<ButtonSpec>,
    pub hover: Hover,
}

pub fn button_rect() -> Rect {
    rect(20.0, HEIGHT - 58.0, WIDTH - 40.0, 38.0)
}

fn close_rect() -> Rect {
    rect(WIDTH - 38.0, 10.0, 28.0, 28.0)
}

fn settings_rect() -> Rect {
    rect(10.0, 10.0, 28.0, 28.0)
}

pub fn hit(x: f32, y: f32, has_button: bool) -> Hover {
    if contains(close_rect(), x, y) {
        Hover::Close
    } else if contains(settings_rect(), x, y) {
        Hover::Settings
    } else if has_button && contains(button_rect(), x, y) {
        Hover::Button
    } else {
        Hover::None
    }
}

/// Which illustrations a snapshot shows, with each sprite's resting top-left corner.
pub fn layout(snapshot: Option<&Snapshot>) -> Vec<(Art, (f32, f32))> {
    let kind = snapshot.map_or(DeviceKind::Earbuds, |s| s.device.kind);
    let family = snapshot.map_or(Family::AirPodsPro, |s| Family::of(s.device.model_id));
    let lid_open = snapshot.is_some_and(|s| s.device.lid_open == Some(true));
    let place = |art: Art, center_x: f32| {
        let (w, h) = art.size();
        (art, (center_x - w / 2.0, ART_BOTTOM - h))
    };
    match art::arts(family, kind, lid_open).as_slice() {
        [buds, case] => vec![place(*buds, 92.0), place(*case, 230.0)],
        arts => arts.iter().map(|&art| place(art, WIDTH / 2.0)).collect(),
    }
}

/// Composites the floating sprites `elapsed` seconds into the animation.
pub fn render_art(canvas: &mut Pixmap, scale: f32, sprites: &[(Sprite, (f32, f32))], elapsed: f32, p: &Palette) {
    let t = Transform::from_scale(scale, scale);
    for (index, (sprite, (x, y))) in sprites.iter().enumerate() {
        // Gentle bobbing, each object slightly out of phase with the others.
        let phase = index as f32 * 1.9;
        let bob = (elapsed * std::f32::consts::TAU / 3.2 + phase).sin() * 3.0;
        let (w, h) = sprite.art.size();
        let shadow_y = y + h - 2.0;
        art::shadow(canvas, t, (x + w / 2.0, shadow_y), w * 0.62, 1.0 - bob / 8.0, p);
        sprite.draw_at(canvas, (*x, y + bob - 3.0), scale, 1.0);
    }
}

/// Paints everything except the floating illustrations.
pub fn render_static(pixmap: &mut Pixmap, scale: f32, fonts: &Fonts, view: &View, p: &Palette) {
    let t = Transform::from_scale(scale, scale);
    pixmap.fill(p.background);
    // Hairline border, except on Windows where the system draws a rounded one.
    if !cfg!(windows)
        && let Some(path) = paint::rounded_rect(rect(0.5, 0.5, WIDTH - 1.0, HEIGHT - 1.0), 0.0)
    {
        paint::stroke(pixmap, &path, p.border, 1.0, t);
    }

    let s = strings();
    let center = WIDTH / 2.0;

    for (r, hover, glyph) in [(close_rect(), Hover::Close, 0), (settings_rect(), Hover::Settings, 1)] {
        let (cx, cy) = (r.left() + 14.0, r.top() + 14.0);
        if view.hover == hover
            && let Some(path) = PathBuilder::from_circle(cx, cy, 13.0)
        {
            fill(pixmap, &path, p.neutral, t);
        }
        if glyph == 0 {
            cross(pixmap, (cx, cy), p.secondary, t);
        } else {
            gear(pixmap, (cx, cy), p.secondary, t);
        }
    }

    let Some(snapshot) = view.snapshot else {
        fonts.draw(pixmap, t, "Kord", (center, 40.0), 17.0, Weight::Bold, Align::Center, p.text);
        fonts.draw(pixmap, t, s.searching, (center, HEIGHT - 52.0), 13.0, Weight::Bold, Align::Center, p.text);
        fonts.draw(pixmap, t, s.searching_hint, (center, HEIGHT - 32.0), 12.0, Weight::Regular, Align::Center, p.secondary);
        return;
    };
    let device = &snapshot.device;

    let name = snapshot.display_name();
    let title = fonts.fit(name, 17.0, Weight::Bold, WIDTH - 96.0);
    fonts.draw(pixmap, t, &title, (center, 40.0), 17.0, Weight::Bold, Align::Center, p.text);
    let status = match &snapshot.link {
        Link::Paired(paired) if paired.connected => Some(s.connected),
        Link::Paired(_) => Some(s.not_connected),
        Link::NotPaired => Some(s.not_paired),
        Link::Unknown => None,
    };
    let subtitle = match status {
        Some(status) if name != device.model_name => format!("{} · {status}", device.model_name),
        Some(status) => status.to_string(),
        None => device.model_name.clone(),
    };
    let subtitle = fonts.fit(&subtitle, 12.0, Weight::Regular, WIDTH - 48.0);
    fonts.draw(pixmap, t, &subtitle, (center, 60.0), 12.0, Weight::Regular, Align::Center, p.secondary);

    match device.kind {
        DeviceKind::Earbuds => {
            paint::battery(pixmap, t, fonts, (50.0, BATTERY_Y), Some(s.left_short), device.left, p);
            paint::battery(pixmap, t, fonts, (136.0, BATTERY_Y), Some(s.right_short), device.right, p);
            paint::battery(pixmap, t, fonts, (228.0, BATTERY_Y), None, device.case, p);
        }
        DeviceKind::Headphones => {
            let battery = Battery { level: device.lowest_level(), charging: device.left.charging || device.right.charging };
            paint::battery(pixmap, t, fonts, (center, BATTERY_Y), None, battery, p);
        }
    }

    if let Some(button) = &view.button {
        let r = button_rect();
        let hovered = view.hover == Hover::Button;
        let (background, foreground) = match button.style {
            ButtonStyle::Primary => (if hovered { p.primary_hover } else { p.primary }, p.on_primary),
            ButtonStyle::Secondary => (if hovered { p.neutral_hover } else { p.neutral }, p.text),
            ButtonStyle::Disabled => (p.neutral, p.secondary),
        };
        fill_rounded(pixmap, r, 6.0, background, t);
        let label = fonts.fit(button.label, 14.0, Weight::Bold, r.width() - 16.0);
        let baseline = r.top() + r.height() / 2.0 + 5.0;
        fonts.draw(pixmap, t, &label, (center, baseline), 14.0, Weight::Bold, Align::Center, foreground);
    }
}
