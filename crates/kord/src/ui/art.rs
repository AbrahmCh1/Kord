//! Device illustrations, as sprites that can float around cheaply in
//! animations. Models with a 3D render (see `assets/sprites/CREDITS.md`) use
//! it; everything else falls back to simple original vector drawings.

use kord_core::DeviceKind;
use tiny_skia::{Color, FilterQuality, PathBuilder, Pixmap, PixmapPaint, Transform};

use super::paint::{Palette, fill, fill_shader, rect, rgb, rounded_rect, stroke, vertical_gradient};

/// Which family of artwork fits a model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    AirPodsPro,
    AirPods,
    AirPodsMax,
    Beats,
    GalaxyBuds,
    Sony,
    Jbl,
    Jabra,
    Logitech,
    Generic,
}

impl Family {
    /// The family of an Apple or Beats model id.
    pub fn of(model_id: u16) -> Self {
        match model_id {
            0x0E20 | 0x1420 | 0x2420 => Family::AirPodsPro,
            0x0220 | 0x0F20 | 0x1320 | 0x1920 | 0x1B20 => Family::AirPods,
            0x0A20 | 0x1F20 => Family::AirPodsMax,
            // Over-ear Beats: Solo3, Studio3, Solo Pro.
            0x0620 | 0x0920 | 0x0C20 => Family::Beats,
            _ => Family::Generic,
        }
    }

    /// Guesses the family of other brands from the name the headphones
    /// pair with, e.g. "Galaxy Buds2 Pro", "WH-1000XM5" or "Zone Vibe 100".
    pub fn from_name(name: &str) -> Self {
        let name = name.to_lowercase();
        let has = |words: &[&str]| words.iter().any(|w| name.contains(w));
        if has(&["airpods max"]) {
            Family::AirPodsMax
        } else if has(&["airpods pro"]) {
            Family::AirPodsPro
        } else if has(&["airpods"]) {
            Family::AirPods
        } else if has(&["beats solo", "beats studio"]) {
            Family::Beats
        } else if has(&["galaxy buds", "buds2", "buds3", "buds live", "buds pro", "buds fe", "buds+"]) {
            Family::GalaxyBuds
        } else if has(&["wh-", "wh1000", "mdr-"]) {
            Family::Sony
        } else if has(&["jbl"]) && !has(&["buds", "tws", "flip", "charge", "go "]) {
            Family::Jbl
        } else if has(&["jabra"]) && !has(&["85h", "45h", "evolve"]) {
            Family::Jabra
        } else if has(&["zone vibe", "logi", "g435"]) {
            Family::Logitech
        } else {
            Family::Generic
        }
    }

    /// The brand, for headphones Kord doesn't know by model.
    pub fn brand(self) -> Option<&'static str> {
        match self {
            Family::GalaxyBuds => Some("Samsung Galaxy Buds"),
            Family::Sony => Some("Sony"),
            Family::Jbl => Some("JBL"),
            Family::Jabra => Some("Jabra"),
            Family::Logitech => Some("Logitech"),
            _ => None,
        }
    }

    /// Worn over the head rather than in the ears.
    pub fn over_ear(self) -> bool {
        matches!(self, Family::AirPodsMax | Family::Beats | Family::Sony | Family::Jbl | Family::Logitech)
    }
}

/// The illustrations for a pair of headphones, drawn side by side.
pub fn arts(family: Family, kind: DeviceKind, lid_open: bool) -> Vec<Art> {
    match family {
        Family::AirPodsPro => vec![Art::Buds(family), Art::Case { family, lid_open }],
        _ if Art::Device(family).photo().is_some() => vec![Art::Device(family)],
        _ if family.over_ear() || kind == DeviceKind::Headphones => vec![Art::Headphones],
        _ => vec![Art::Earbuds],
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Art {
    Buds(Family),
    Case { family: Family, lid_open: bool },
    /// Over-ear headphones without art of their own.
    Headphones,
    /// Earbuds in their case, without art of their own.
    Earbuds,
    /// One picture of the whole product, for models rendered that way.
    Device(Family),
}

impl Art {
    /// Size of the sprite in logical pixels.
    pub fn size(self) -> (f32, f32) {
        match self {
            Art::Buds(Family::AirPodsPro) => (120.0, 90.0),
            Art::Buds(_) => (110.0, 66.0),
            Art::Case { family: Family::AirPodsPro, .. } => (100.0, 78.0),
            Art::Case { .. } => (90.0, 70.0),
            Art::Headphones => (110.0, 100.0),
            Art::Earbuds => (100.0, 90.0),
            Art::Device(Family::AirPods) => (100.0, 100.0),
            Art::Device(Family::GalaxyBuds) => (100.0, 70.0),
            Art::Device(Family::Jabra) => (120.0, 70.0),
            Art::Device(_) => (110.0, 100.0),
        }
    }

    /// The pre-rendered image for this art, if there is one. Renders are
    /// three times the logical size; see `assets/sprites/CREDITS.md`.
    fn photo(self) -> Option<&'static [u8]> {
        macro_rules! sprite {
            ($path:literal) => {
                Some(include_bytes!(concat!("../../assets/sprites/", $path)))
            };
        }
        match self {
            Art::Buds(Family::AirPodsPro) => sprite!("airpods-pro/buds.png"),
            Art::Case { family: Family::AirPodsPro, .. } => sprite!("airpods-pro/case.png"),
            Art::Device(Family::AirPods) => sprite!("airpods/set.png"),
            Art::Device(Family::AirPodsMax) => sprite!("airpods-max/headphones.png"),
            Art::Device(Family::Beats) => sprite!("beats/headphones.png"),
            Art::Device(Family::GalaxyBuds) => sprite!("galaxy-buds/case.png"),
            Art::Device(Family::Sony) => sprite!("sony/headphones.png"),
            Art::Device(Family::Jbl) => sprite!("jbl/headphones.png"),
            Art::Device(Family::Jabra) => sprite!("jabra/buds.png"),
            // A Logitech G435: no Zone Vibe model is downloadable, and the shape is close.
            Art::Device(Family::Logitech) => sprite!("logitech/headphones.png"),
            Art::Headphones => sprite!("generic/headphones.png"),
            Art::Earbuds => sprite!("generic/earbuds.png"),
            _ => None,
        }
    }
}

/// An illustration rendered for one display scale.
pub struct Sprite {
    pub art: Art,
    pub pixmap: Pixmap,
    /// Physical pixels per logical pixel the sprite was rendered at.
    pub scale: f32,
}

impl Sprite {
    pub fn render(art: Art, scale: f32, p: &Palette) -> Option<Self> {
        let (w, h) = art.size();
        let mut pixmap = Pixmap::new((w * scale).ceil() as u32, (h * scale).ceil() as u32)?;
        match art.photo().and_then(|bytes| Pixmap::decode_png(bytes).ok()) {
            Some(photo) => {
                // Resample once to the display scale with a good filter.
                let fit = (pixmap.width() as f32 / photo.width() as f32).min(pixmap.height() as f32 / photo.height() as f32);
                let paint = PixmapPaint { quality: FilterQuality::Bicubic, ..PixmapPaint::default() };
                pixmap.draw_pixmap(0, 0, photo.as_ref(), &paint, Transform::from_scale(fit, fit), None);
                // Keep the UI monochrome (the case's status light is green).
                for px in pixmap.pixels_mut() {
                    let luma = (u16::from(px.red()) * 54 + u16::from(px.green()) * 183 + u16::from(px.blue()) * 19) >> 8;
                    let grey = (luma as u8).min(px.alpha());
                    if let Some(gray) = tiny_skia::PremultipliedColorU8::from_rgba(grey, grey, grey, px.alpha()) {
                        *px = gray;
                    }
                }
            }
            None => draw(&mut pixmap, Transform::from_scale(scale, scale), art, p),
        }
        Some(Self { art, pixmap, scale })
    }

    /// Draws the sprite with its top-left corner at logical (x, y), at `zoom` times its size.
    pub fn draw_at(&self, target: &mut Pixmap, (x, y): (f32, f32), scale: f32, zoom: f32) {
        let paint = PixmapPaint { quality: FilterQuality::Bilinear, ..PixmapPaint::default() };
        let ratio = scale / self.scale * zoom;
        let t = Transform::from_translate(x * scale, y * scale).pre_scale(ratio, ratio);
        target.draw_pixmap(0, 0, self.pixmap.as_ref(), &paint, t, None);
    }
}

/// Draws the vector version of `art` with its box's top-left corner at the origin of `t`.
fn draw(pixmap: &mut Pixmap, t: Transform, art: Art, p: &Palette) {
    match art {
        Art::Buds(_) => {
            bud(pixmap, t.pre_translate(31.0, 2.0), p);
            // The right bud is the mirror image of the left one.
            bud(pixmap, t.pre_translate(79.0, 2.0).pre_scale(-1.0, 1.0), p);
        }
        Art::Case { lid_open, .. } => case(pixmap, t.pre_translate(3.0, 3.0), lid_open, p),
        Art::Headphones | Art::Device(_) => headphones(pixmap, t.pre_translate(55.0, 14.0), p),
        Art::Earbuds => case(pixmap, t.pre_translate(8.0, 13.0), false, p),
    }
}

/// A soft oval shadow centred on (x, y), drawn as stacked translucent ellipses.
pub fn shadow(pixmap: &mut Pixmap, t: Transform, (x, y): (f32, f32), width: f32, strength: f32, p: &Palette) {
    let base = p.shadow;
    for step in 0..4 {
        let grow = step as f32 * 4.0;
        let (w, h) = (width + grow, 6.0 + grow * 0.5);
        let alpha = base.alpha() * strength * (0.32 - step as f32 * 0.07);
        let color = Color::from_rgba(base.red(), base.green(), base.blue(), alpha.clamp(0.0, 1.0)).unwrap_or(base);
        if let Some(path) = PathBuilder::from_oval(rect(x - w / 2.0, y - h / 2.0, w, h)) {
            fill(pixmap, &path, color, t);
        }
    }
}

/// One earbud with its tip pointing to +x; origin at the head's centre top.
fn bud(pixmap: &mut Pixmap, t: Transform, p: &Palette) {
    let white = vertical_gradient(0.0, 60.0, rgb(0xFFFFFF), rgb(0xE2E2E7));

    // Stem, slightly darker at the bottom where the microphone grille is.
    if let Some(stem) = rounded_rect(rect(-9.0, 12.0, 9.5, 48.0), 4.75) {
        fill_shader(pixmap, &stem, white.clone(), t);
        stroke(pixmap, &stem, p.art_outline, 0.9, t);
    }
    if let Some(cap) = rounded_rect(rect(-7.6, 54.0, 6.7, 4.0), 2.0) {
        fill(pixmap, &cap, rgb(0x9FA1A8), t);
    }
    // Head.
    if let Some(head) = PathBuilder::from_oval(rect(-13.0, 0.0, 26.0, 23.0)) {
        fill_shader(pixmap, &head, white, t);
        stroke(pixmap, &head, p.art_outline, 0.9, t);
    }
    // Glossy highlight.
    if let Some(gloss) = PathBuilder::from_oval(rect(-9.5, 2.5, 10.0, 6.0)) {
        fill(pixmap, &gloss, Color::from_rgba8(255, 255, 255, 210), t);
    }
    // Silicone tip, pointing at the other bud.
    if let Some(tip) = PathBuilder::from_oval(rect(6.0, 3.5, 11.0, 14.0)) {
        fill_shader(pixmap, &tip, vertical_gradient(3.0, 18.0, rgb(0xEDEEF2), rgb(0xC3C5CC)), t);
        stroke(pixmap, &tip, p.art_outline, 0.9, t);
    }
    // Speaker mesh and sensor.
    if let Some(mesh) = PathBuilder::from_oval(rect(-7.0, 9.0, 5.0, 2.6)) {
        fill(pixmap, &mesh, rgb(0x3A3A40), t);
    }
    if let Some(sensor) = PathBuilder::from_circle(-1.0, 16.5, 1.1) {
        fill(pixmap, &sensor, rgb(0x55575E), t);
    }
}

fn case(pixmap: &mut Pixmap, t: Transform, lid_open: bool, p: &Palette) {
    let (w, h) = (84.0, 64.0);
    let body = rect(0.0, 0.0, w, h);
    if let Some(path) = rounded_rect(body, 18.0) {
        fill_shader(pixmap, &path, vertical_gradient(0.0, h, rgb(0xFFFFFF), rgb(0xDEDEE4)), t);
        stroke(pixmap, &path, p.art_outline, 0.9, t);
    }
    // Soft highlight across the lid.
    if let Some(gloss) = rounded_rect(rect(10.0, 3.0, w - 20.0, 9.0), 4.5) {
        fill(pixmap, &gloss, Color::from_rgba8(255, 255, 255, 170), t);
    }
    // Seam between lid and body.
    super::paint::line(pixmap, (1.5, 22.0), (w - 1.5, 22.0), p.art_outline, 0.9, t);
    // Status light: lit while the lid is open.
    if let Some(led) = PathBuilder::from_circle(w / 2.0, 40.0, 2.4) {
        fill(pixmap, &led, if lid_open { rgb(0x6E6E74) } else { rgb(0xCFCFD6) }, t);
    }
}

/// Over-ear headphones centred on the origin's x.
fn headphones(pixmap: &mut Pixmap, t: Transform, p: &Palette) {
    let mut band = PathBuilder::new();
    band.move_to(-30.0, 38.0);
    band.cubic_to(-30.0, -4.0, 30.0, -4.0, 30.0, 38.0);
    if let Some(path) = band.finish() {
        stroke(pixmap, &path, p.art_outline, 7.0, t);
        stroke(pixmap, &path, rgb(0xF1F1F4), 5.0, t);
    }
    for cup_x in [-44.0, 22.0] {
        if let Some(cup) = rounded_rect(rect(cup_x, 28.0, 22.0, 42.0), 9.0) {
            fill_shader(pixmap, &cup, vertical_gradient(28.0, 70.0, rgb(0xFFFFFF), rgb(0xDADAE0)), t);
            stroke(pixmap, &cup, p.art_outline, 0.9, t);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Family;

    #[test]
    fn recognizes_brands_by_name() {
        for (name, family) in [
            ("Galaxy Buds2 Pro", Family::GalaxyBuds),
            ("Alex's Galaxy Buds Live (5C1A)", Family::GalaxyBuds),
            ("WH-1000XM5", Family::Sony),
            ("LE_WH-1000XM4", Family::Sony),
            ("JBL TUNE720BT", Family::Jbl),
            ("JBL Tune Buds", Family::Generic),
            ("Jabra Elite 75t", Family::Jabra),
            ("Zone Vibe 100", Family::Logitech),
            ("Alex's AirPods Max", Family::AirPodsMax),
            ("Some Speaker", Family::Generic),
        ] {
            assert_eq!(Family::from_name(name), family, "{name}");
        }
    }
}
