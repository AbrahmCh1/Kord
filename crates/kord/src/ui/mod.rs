//! Windows Kord draws itself: the status popup and the main window.
//! Everything is software-rendered with tiny-skia into a plain window, which
//! keeps the app small and lets windows exist only while visible.

pub mod art;
pub mod paint;
pub mod popup;
pub mod window;
pub mod surface;
pub mod text;
