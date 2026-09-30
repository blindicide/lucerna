//! View models and formatting for the GUI: pure functions from D-Bus DTOs to plain data.
//!
//! Nothing here touches GTK, so all of it is unit-tested without a display (directive §48). The
//! architecture test enforces that this directory names no GTK, X11, D-Bus or async-runtime type.

pub mod about;
pub mod banner;
pub mod displays;
pub mod settings;
pub mod wallpapers;
