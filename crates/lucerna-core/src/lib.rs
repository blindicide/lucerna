//! Lucerna core: pure logic and domain types.
//!
//! This crate has no GTK, X11, D-Bus or async-runtime dependencies. The
//! architecture test in `lucerna-testkit` enforces that boundary.

pub mod backend;
pub mod bounded_log;
pub mod fsutil;
pub mod logging;
pub mod mpv;
pub mod paths;
pub mod renderer;
pub mod runtime;
pub mod types;
pub mod version;
