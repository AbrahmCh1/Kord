//! A window plus the software surface we paint it through.

use std::num::NonZeroU32;
use std::rc::Rc;

use anyhow::{Context, Result, anyhow};
use tao::window::{Theme, Window};
use tiny_skia::Pixmap;

pub struct Surface {
    pub window: Rc<Window>,
    surface: softbuffer::Surface<Rc<Window>, Rc<Window>>,
}

impl Surface {
    pub fn new(window: Window) -> Result<Self> {
        let window = Rc::new(window);
        let context = softbuffer::Context::new(window.clone()).map_err(|e| anyhow!("{e}"))?;
        let surface = softbuffer::Surface::new(&context, window.clone()).map_err(|e| anyhow!("{e}"))?;
        Ok(Self { window, surface })
    }

    pub fn scale(&self) -> f32 {
        self.window.scale_factor() as f32
    }

    pub fn dark(&self) -> bool {
        super::paint::dark(self.window.theme() == Theme::Dark)
    }

    /// Size of the drawable area in logical pixels.
    pub fn logical_size(&self) -> (f32, f32) {
        let size = self.window.inner_size();
        (size.width as f32 / self.scale(), size.height as f32 / self.scale())
    }

    /// An empty canvas matching the window's physical size.
    pub fn canvas(&self) -> Option<Pixmap> {
        let size = self.window.inner_size();
        Pixmap::new(size.width.max(1), size.height.max(1))
    }

    /// Copies an opaque canvas to the window.
    pub fn present(&mut self, pixmap: &Pixmap) -> Result<()> {
        let (Some(width), Some(height)) = (NonZeroU32::new(pixmap.width()), NonZeroU32::new(pixmap.height())) else {
            return Ok(());
        };
        self.surface.resize(width, height).map_err(|e| anyhow!("{e}"))?;
        let mut buffer = self.surface.buffer_mut().map_err(|e| anyhow!("{e}")).context("window buffer")?;
        for (dst, px) in buffer.iter_mut().zip(pixmap.pixels()) {
            // The background is opaque, so premultiplied equals straight colour.
            *dst = (u32::from(px.red()) << 16) | (u32::from(px.green()) << 8) | u32::from(px.blue());
        }
        buffer.present().map_err(|e| anyhow!("{e}"))?;
        Ok(())
    }
}

/// Windows 11 rounded corners for undecorated windows (ignored on Windows 10).
#[cfg(windows)]
pub fn round_corners(window: &Window) {
    use tao::platform::windows::WindowExtWindows;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Dwm::{DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmSetWindowAttribute};
    let preference = DWMWCP_ROUND;
    // SAFETY: valid window handle and a pointer/size pair describing `preference`.
    unsafe {
        let _ = DwmSetWindowAttribute(
            HWND(window.hwnd() as *mut _),
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &preference as *const _ as *const _,
            size_of_val(&preference) as u32,
        );
    }
}
