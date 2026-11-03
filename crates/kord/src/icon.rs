//! Kord's logo, drawn at runtime: two earbuds joined by a cord, in greys.
//! Generating it avoids shipping image files and keeps it crisp at any size.

use kord_core::DeviceStatus;
use tiny_skia::{Color, LineCap, Paint, PathBuilder, Pixmap, Stroke, Transform};

use crate::ui::paint::{rect, rgb, rounded_rect, vertical_gradient};

pub const SIZE: u32 = 32;

/// The mark on a 32×32 grid: two earbud heads whose stems meet in a "U".
pub fn glyph(pixmap: &mut Pixmap, t: Transform, color: Color) {
    let mut cord = PathBuilder::new();
    cord.move_to(10.0, 10.0);
    cord.line_to(10.0, 18.0);
    cord.cubic_to(10.0, 26.0, 22.0, 26.0, 22.0, 18.0);
    cord.line_to(22.0, 10.0);
    let mut paint = Paint::default();
    paint.set_color(color);
    paint.anti_alias = true;
    if let Some(path) = cord.finish() {
        let stroke = Stroke { width: 2.6, line_cap: LineCap::Round, ..Stroke::default() };
        pixmap.stroke_path(&path, &paint, &stroke, t, None);
    }
    for x in [10.0, 22.0] {
        if let Some(head) = PathBuilder::from_circle(x, 8.0, 4.0) {
            pixmap.fill_path(&head, &paint, tiny_skia::FillRule::Winding, t, None);
        }
    }
}

/// The tray icon: the bare mark in the taskbar's text colour, dimmed while
/// no headphones are around.
pub fn tray(device: Option<&DeviceStatus>) -> Vec<u8> {
    let Some(mut pixmap) = Pixmap::new(SIZE, SIZE) else { return vec![0; (SIZE * SIZE * 4) as usize] };
    let color = match (device.is_some(), taskbar_is_light()) {
        (false, _) => rgb(0x8A8A8E),
        (true, true) => rgb(0x1C1C1E),
        (true, false) => rgb(0xF2F2F4),
    };
    glyph(&mut pixmap, Transform::identity(), color);
    straight_rgba(&pixmap)
}

/// The app icon: white mark on a near-black rounded square.
pub fn app(size: u32) -> Pixmap {
    let mut pixmap = Pixmap::new(size, size).expect("icon size");
    let scale = size as f32 / 32.0;
    let t = Transform::from_scale(scale, scale);
    if let Some(tile) = rounded_rect(rect(1.0, 1.0, 30.0, 30.0), 7.5) {
        let paint = Paint {
            shader: vertical_gradient(1.0, 31.0, rgb(0x2E2F33), rgb(0x0E0E10)),
            anti_alias: true,
            ..Paint::default()
        };
        pixmap.fill_path(&tile, &paint, tiny_skia::FillRule::Winding, t, None);
    }
    // Draw the mark slightly smaller, centred on the tile.
    let inner = t.pre_translate(16.0, 16.5).pre_scale(0.78, 0.78).pre_translate(-16.0, -16.0);
    glyph(&mut pixmap, inner, rgb(0xF5F5F7));
    pixmap
}

/// RGBA pixels of the app icon, for window icons.
pub fn app_rgba(size: u32) -> Vec<u8> {
    straight_rgba(&app(size))
}

/// tiny-skia stores premultiplied alpha; icons want straight alpha.
fn straight_rgba(pixmap: &Pixmap) -> Vec<u8> {
    pixmap
        .pixels()
        .iter()
        .flat_map(|px| {
            let c = px.demultiply();
            [c.red(), c.green(), c.blue(), c.alpha()]
        })
        .collect()
}

/// Writes a Windows .ico with PNG images of the app icon at common sizes.
pub fn write_ico(path: &std::path::Path) -> anyhow::Result<()> {
    let sizes = [16u32, 24, 32, 48, 64, 128, 256];
    let images: Vec<Vec<u8>> = sizes.iter().map(|&s| app(s).encode_png()).collect::<Result<_, _>>()?;
    let mut ico = Vec::new();
    ico.extend([0u8, 0, 1, 0]);
    ico.extend((sizes.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * sizes.len() as u32;
    for (size, image) in sizes.iter().zip(&images) {
        let dim = if *size >= 256 { 0 } else { *size as u8 };
        ico.extend([dim, dim, 0, 0]);
        ico.extend(1u16.to_le_bytes()); // colour planes
        ico.extend(32u16.to_le_bytes()); // bits per pixel
        ico.extend((image.len() as u32).to_le_bytes());
        ico.extend(offset.to_le_bytes());
        offset += image.len() as u32;
    }
    for image in images {
        ico.extend(image);
    }
    std::fs::write(path, ico)?;
    Ok(())
}

/// Whether the taskbar uses the light theme (so the icon must be dark).
#[cfg(windows)]
fn taskbar_is_light() -> bool {
    use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
    use windows::core::w;
    let mut value = 0u32;
    let mut size = size_of::<u32>() as u32;
    // SAFETY: reads a DWORD into `value`, whose size we pass.
    let result = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            w!("SystemUsesLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut value as *mut u32 as *mut _),
            Some(&mut size),
        )
    };
    result.is_ok() && value != 0
}

/// Most Linux panels are dark.
#[cfg(not(windows))]
fn taskbar_is_light() -> bool {
    false
}
