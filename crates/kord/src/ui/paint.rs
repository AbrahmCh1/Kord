//! Drawing primitives and colours shared by the popup and the settings window.

use std::sync::atomic::{AtomicU8, Ordering};

use kord_core::Battery;
use tiny_skia::{
    Color, FillRule, GradientStop, LinearGradient, Paint, Path, PathBuilder, Pixmap, Point, Rect, Shader, SpreadMode,
    Stroke, Transform,
};

use super::text::{Align, Fonts, Weight};
use crate::config::Theme;
use crate::i18n::strings;

pub struct Palette {
    pub background: Color,
    /// Grouped settings sit on cards slightly lighter/darker than the background.
    pub card: Color,
    pub border: Color,
    pub text: Color,
    pub secondary: Color,
    pub art_outline: Color,
    pub primary: Color,
    pub primary_hover: Color,
    pub on_primary: Color,
    pub neutral: Color,
    pub neutral_hover: Color,
    pub shadow: Color,
}

/// Theme chosen in the settings; 0 follows the system.
static THEME: AtomicU8 = AtomicU8::new(0);

pub fn set_theme(theme: Theme) {
    let value = match theme {
        Theme::System => 0,
        Theme::Light => 1,
        Theme::Dark => 2,
    };
    THEME.store(value, Ordering::Relaxed);
}

/// Whether to paint dark, given what the system prefers.
pub fn dark(system_dark: bool) -> bool {
    match THEME.load(Ordering::Relaxed) {
        1 => false,
        2 => true,
        _ => system_dark,
    }
}

pub fn rgb(hex: u32) -> Color {
    Color::from_rgba8((hex >> 16) as u8, (hex >> 8) as u8, hex as u8, 255)
}

pub fn palette(dark: bool) -> Palette {
    if dark {
        Palette {
            background: rgb(0x202024),
            card: rgb(0x2B2B30),
            border: rgb(0x3A3A40),
            text: rgb(0xF5F5F7),
            secondary: rgb(0xA1A1A8),
            art_outline: rgb(0x8E8E96),
            primary: rgb(0xE8E8EB),
            primary_hover: rgb(0xFFFFFF),
            on_primary: rgb(0x141416),
            neutral: rgb(0x38383E),
            neutral_hover: rgb(0x44444B),
            shadow: Color::from_rgba8(0, 0, 0, 120),
        }
    } else {
        Palette {
            background: rgb(0xF3F3F5),
            card: rgb(0xFFFFFF),
            border: rgb(0xE1E1E6),
            text: rgb(0x1B1B1F),
            secondary: rgb(0x6B6B73),
            art_outline: rgb(0xC9C9D0),
            primary: rgb(0x1C1C1E),
            primary_hover: rgb(0x3A3A3E),
            on_primary: rgb(0xFFFFFF),
            neutral: rgb(0xE6E6EA),
            neutral_hover: rgb(0xDBDBE0),
            shadow: Color::from_rgba8(0, 0, 0, 60),
        }
    }
}

pub fn rounded_rect(rect: Rect, radius: f32) -> Option<Path> {
    let r = radius.min(rect.width() / 2.0).min(rect.height() / 2.0);
    if r <= 0.0 {
        return Some(PathBuilder::from_rect(rect));
    }
    // Cubic approximation of a quarter circle.
    let k = r * 0.552_284_8;
    let (l, t, rt, b) = (rect.left(), rect.top(), rect.right(), rect.bottom());
    let mut pb = PathBuilder::new();
    pb.move_to(l + r, t);
    pb.line_to(rt - r, t);
    pb.cubic_to(rt - r + k, t, rt, t + r - k, rt, t + r);
    pb.line_to(rt, b - r);
    pb.cubic_to(rt, b - r + k, rt - r + k, b, rt - r, b);
    pb.line_to(l + r, b);
    pb.cubic_to(l + r - k, b, l, b - r + k, l, b - r);
    pb.line_to(l, t + r);
    pb.cubic_to(l, t + r - k, l + r - k, t, l + r, t);
    pb.close();
    pb.finish()
}

pub fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
    Rect::from_xywh(x, y, w.max(0.01), h.max(0.01)).unwrap_or_else(|| Rect::from_xywh(0.0, 0.0, 0.01, 0.01).unwrap())
}

pub fn contains(r: Rect, x: f32, y: f32) -> bool {
    x >= r.left() && x <= r.right() && y >= r.top() && y <= r.bottom()
}

pub fn fill(pixmap: &mut Pixmap, path: &Path, color: Color, t: Transform) {
    fill_shader(pixmap, path, Shader::SolidColor(color), t);
}

pub fn fill_shader(pixmap: &mut Pixmap, path: &Path, shader: Shader, t: Transform) {
    let paint = Paint { shader, anti_alias: true, ..Paint::default() };
    pixmap.fill_path(path, &paint, FillRule::Winding, t, None);
}

pub fn fill_rounded(pixmap: &mut Pixmap, r: Rect, radius: f32, color: Color, t: Transform) {
    if let Some(path) = rounded_rect(r, radius) {
        fill(pixmap, &path, color, t);
    }
}

pub fn stroke(pixmap: &mut Pixmap, path: &Path, color: Color, width: f32, t: Transform) {
    let mut paint = Paint::default();
    paint.set_color(color);
    paint.anti_alias = true;
    let stroke = Stroke { width, line_cap: tiny_skia::LineCap::Round, ..Stroke::default() };
    pixmap.stroke_path(path, &paint, &stroke, t, None);
}

pub fn line(pixmap: &mut Pixmap, from: (f32, f32), to: (f32, f32), color: Color, width: f32, t: Transform) {
    let mut pb = PathBuilder::new();
    pb.move_to(from.0, from.1);
    pb.line_to(to.0, to.1);
    if let Some(path) = pb.finish() {
        stroke(pixmap, &path, color, width, t);
    }
}

pub fn vertical_gradient(y0: f32, y1: f32, from: Color, to: Color) -> Shader<'static> {
    LinearGradient::new(
        Point::from_xy(0.0, y0),
        Point::from_xy(0.0, y1),
        vec![GradientStop::new(0.0, from), GradientStop::new(1.0, to)],
        SpreadMode::Pad,
        Transform::identity(),
    )
    .unwrap_or(Shader::SolidColor(from))
}

/// A "×" close glyph centred on (x, y).
pub fn cross(pixmap: &mut Pixmap, (x, y): (f32, f32), color: Color, t: Transform) {
    line(pixmap, (x - 4.5, y - 4.5), (x + 4.5, y + 4.5), color, 1.4, t);
    line(pixmap, (x + 4.5, y - 4.5), (x - 4.5, y + 4.5), color, 1.4, t);
}

/// A gear glyph centred on (x, y).
pub fn gear(pixmap: &mut Pixmap, (x, y): (f32, f32), color: Color, t: Transform) {
    let mut pb = PathBuilder::new();
    for i in 0..8 {
        let angle = i as f32 * std::f32::consts::PI / 4.0;
        let (sin, cos) = angle.sin_cos();
        pb.move_to(x + cos * 4.6, y + sin * 4.6);
        pb.line_to(x + cos * 7.2, y + sin * 7.2);
    }
    if let Some(path) = pb.finish() {
        stroke(pixmap, &path, color, 2.2, t);
    }
    if let Some(ring) = PathBuilder::from_circle(x, y, 4.6) {
        stroke(pixmap, &ring, color, 1.6, t);
    }
}

/// Width of a [`battery`] indicator without side label, to align several.
pub fn battery_width(fonts: &Fonts, battery: Battery) -> f32 {
    let label = battery.level.map_or_else(|| strings().unavailable.to_string(), |l| format!("{l}%"));
    22.0 + 2.0 + if battery.charging { 9.0 } else { 0.0 } + 5.0 + fonts.measure(&label, 13.0, Weight::Regular)
}

/// A battery indicator centred on `x` with its text baseline at `y`:
/// outline, fill, optional bolt and percentage.
pub fn battery(
    pixmap: &mut Pixmap,
    t: Transform,
    fonts: &Fonts,
    (x, y): (f32, f32),
    side: Option<&str>,
    battery: Battery,
    p: &Palette,
) {
    let label = battery.level.map_or_else(|| strings().unavailable.to_string(), |l| format!("{l}%"));
    let (body_w, body_h, bolt_w, gap) = (22.0, 11.0, if battery.charging { 9.0 } else { 0.0 }, 5.0);
    let side_w = side.map_or(0.0, |s| fonts.measure(s, 11.0, Weight::Bold) + 5.0);
    let text_w = fonts.measure(&label, 13.0, Weight::Regular);
    let total = side_w + body_w + 2.0 + bolt_w + gap + text_w;
    if let Some(side) = side {
        fonts.draw(pixmap, t, side, (x - total / 2.0, y), 11.0, Weight::Bold, Align::Left, p.secondary);
    }
    let left = x - total / 2.0 + side_w;
    let top = y - body_h / 2.0 - 4.0;

    if let Some(path) = rounded_rect(rect(left, top, body_w, body_h), 3.0) {
        stroke(pixmap, &path, p.secondary, 1.1, t);
    }
    fill_rounded(pixmap, rect(left + body_w + 0.8, top + 3.5, 1.8, body_h - 7.0), 0.8, p.secondary, t);
    if let Some(level) = battery.level {
        // Monochrome: a low battery is a lighter grey rather than red.
        let color = if level <= 20 { p.secondary } else { p.text };
        let inner = (body_w - 4.0) * f32::from(level.min(100)) / 100.0;
        if inner > 0.5 {
            fill_rounded(pixmap, rect(left + 2.0, top + 2.0, inner, body_h - 4.0), 1.5, color, t);
        }
    }
    if battery.charging {
        let bx = left + body_w + 4.0;
        let mut bolt = PathBuilder::new();
        bolt.move_to(bx + 4.5, top - 1.5);
        bolt.line_to(bx, top + 6.5);
        bolt.line_to(bx + 3.2, top + 6.5);
        bolt.line_to(bx + 2.0, top + body_h + 1.5);
        bolt.line_to(bx + 7.0, top + 3.8);
        bolt.line_to(bx + 3.8, top + 3.8);
        bolt.close();
        if let Some(path) = bolt.finish() {
            fill(pixmap, &path, p.text, t);
        }
    }
    fonts.draw(pixmap, t, &label, (left + body_w + 2.0 + bolt_w + gap, y), 13.0, Weight::Regular, Align::Left, p.text);
}
