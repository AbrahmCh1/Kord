//! Text drawing with the system UI font. Glyph outlines from `ab_glyph` are
//! filled as vector paths by tiny-skia, so text scales with the popup and is
//! anti-aliased like everything else.

use std::path::PathBuf;

use ab_glyph::{Font, FontVec, OutlineCurve};
use tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap, Transform};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Weight {
    Regular,
    Bold,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

pub struct Fonts {
    regular: FontVec,
    bold: FontVec,
}

impl Fonts {
    /// Loads the platform UI font. Fonts are only held while the popup is open.
    pub fn load() -> Option<Self> {
        let regular = load_first(candidates(Weight::Regular))?;
        let bold = load_first(candidates(Weight::Bold)).or_else(|| load_first(candidates(Weight::Regular)))?;
        Some(Self { regular, bold })
    }

    fn font(&self, weight: Weight) -> &FontVec {
        match weight {
            Weight::Regular => &self.regular,
            Weight::Bold => &self.bold,
        }
    }

    /// Width of `text` in pixels at `size` px (em size).
    pub fn measure(&self, text: &str, size: f32, weight: Weight) -> f32 {
        let font = self.font(weight);
        let scale = size / font.units_per_em().unwrap_or(1000.0);
        let mut width = 0.0;
        let mut previous = None;
        for c in text.chars() {
            let id = font.glyph_id(c);
            if let Some(prev) = previous {
                width += font.kern_unscaled(prev, id) * scale;
            }
            width += font.h_advance_unscaled(id) * scale;
            previous = Some(id);
        }
        width
    }

    /// Shortens `text` with an ellipsis until it fits in `max_width`.
    pub fn fit(&self, text: &str, size: f32, weight: Weight, max_width: f32) -> String {
        if self.measure(text, size, weight) <= max_width {
            return text.to_string();
        }
        let mut chars: Vec<char> = text.chars().collect();
        while !chars.is_empty() {
            chars.pop();
            let candidate = format!("{}…", chars.iter().collect::<String>().trim_end());
            if self.measure(&candidate, size, weight) <= max_width {
                return candidate;
            }
        }
        "…".to_string()
    }

    /// Breaks `text` into lines no wider than `max_width`, at spaces.
    pub fn wrap(&self, text: &str, size: f32, weight: Weight, max_width: f32) -> Vec<String> {
        let mut lines = Vec::new();
        let mut line = String::new();
        for word in text.split_whitespace() {
            let candidate = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
            if !line.is_empty() && self.measure(&candidate, size, weight) > max_width {
                lines.push(std::mem::replace(&mut line, word.to_string()));
            } else {
                line = candidate;
            }
        }
        if !line.is_empty() {
            lines.push(line);
        }
        lines
    }

    /// Draws `text` with its baseline at `y`, in logical pixels mapped through `transform`.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &self,
        pixmap: &mut Pixmap,
        transform: Transform,
        text: &str,
        (x, y): (f32, f32),
        size: f32,
        weight: Weight,
        align: Align,
        color: Color,
    ) {
        let font = self.font(weight);
        let scale = size / font.units_per_em().unwrap_or(1000.0);
        let mut pen = match align {
            Align::Left => x,
            Align::Center => x - self.measure(text, size, weight) / 2.0,
            Align::Right => x - self.measure(text, size, weight),
        };
        let mut paint = Paint::default();
        paint.set_color(color);
        paint.anti_alias = true;

        let mut previous = None;
        for c in text.chars() {
            let id = font.glyph_id(c);
            if let Some(prev) = previous {
                pen += font.kern_unscaled(prev, id) * scale;
            }
            if let Some(path) = font.outline(id).and_then(|outline| glyph_path(&outline.curves)) {
                // Font units have y pointing up; flip around the baseline.
                let glyph_transform = transform.pre_translate(pen, y).pre_scale(scale, -scale);
                pixmap.fill_path(&path, &paint, FillRule::Winding, glyph_transform, None);
            }
            pen += font.h_advance_unscaled(id) * scale;
            previous = Some(id);
        }
    }
}

fn glyph_path(curves: &[OutlineCurve]) -> Option<tiny_skia::Path> {
    let mut builder = PathBuilder::new();
    let mut last = None;
    for curve in curves {
        let (start, end) = match curve {
            OutlineCurve::Line(a, b) | OutlineCurve::Quad(a, _, b) | OutlineCurve::Cubic(a, _, _, b) => (*a, *b),
        };
        if last != Some(start) {
            if last.is_some() {
                builder.close();
            }
            builder.move_to(start.x, start.y);
        }
        match curve {
            OutlineCurve::Line(_, b) => builder.line_to(b.x, b.y),
            OutlineCurve::Quad(_, c, b) => builder.quad_to(c.x, c.y, b.x, b.y),
            OutlineCurve::Cubic(_, c1, c2, b) => builder.cubic_to(c1.x, c1.y, c2.x, c2.y, b.x, b.y),
        }
        last = Some(end);
    }
    builder.close();
    builder.finish()
}

fn load_first(candidates: Vec<(PathBuf, u32)>) -> Option<FontVec> {
    candidates.into_iter().find_map(|(path, index)| {
        let data = std::fs::read(&path).ok()?;
        FontVec::try_from_vec_and_index(data, index).ok()
    })
}

#[cfg(windows)]
fn candidates(weight: Weight) -> Vec<(PathBuf, u32)> {
    let fonts = PathBuf::from(std::env::var_os("WINDIR").unwrap_or_else(|| "C:\\Windows".into())).join("Fonts");
    let names: &[&str] = match weight {
        Weight::Regular => &["segoeui.ttf", "arial.ttf"],
        Weight::Bold => &["seguisb.ttf", "segoeuib.ttf", "arialbd.ttf"],
    };
    names.iter().map(|name| (fonts.join(name), 0)).collect()
}

#[cfg(not(windows))]
fn candidates(weight: Weight) -> Vec<(PathBuf, u32)> {
    let pattern = match weight {
        Weight::Regular => "sans-serif",
        Weight::Bold => "sans-serif:weight=200",
    };
    let mut found = Vec::new();
    // fontconfig knows the desktop's configured UI font.
    if let Ok(output) = std::process::Command::new("fc-match").args(["-f", "%{file}\n%{index}", pattern]).output() {
        let text = String::from_utf8_lossy(&output.stdout);
        let mut lines = text.lines();
        if let Some(file) = lines.next().filter(|f| !f.is_empty()) {
            found.push((PathBuf::from(file), lines.next().and_then(|i| i.parse().ok()).unwrap_or(0)));
        }
    }
    let fallbacks: &[&str] = match weight {
        Weight::Regular => &[
            "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
            "/usr/share/fonts/noto/NotoSans-Regular.ttf",
            "/usr/share/fonts/google-noto/NotoSans-Regular.ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            "/usr/share/fonts/TTF/DejaVuSans.ttf",
            "/usr/share/fonts/dejavu-sans-fonts/DejaVuSans.ttf",
        ],
        Weight::Bold => &[
            "/usr/share/fonts/truetype/noto/NotoSans-SemiBold.ttf",
            "/usr/share/fonts/noto/NotoSans-SemiBold.ttf",
            "/usr/share/fonts/google-noto/NotoSans-SemiBold.ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
            "/usr/share/fonts/TTF/DejaVuSans-Bold.ttf",
            "/usr/share/fonts/dejavu-sans-fonts/DejaVuSans-Bold.ttf",
        ],
    };
    found.extend(fallbacks.iter().map(|path| (PathBuf::from(path), 0)));
    found
}
