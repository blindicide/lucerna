//! Lucerna core: pure logic and domain types.
//!
//! This crate has no GTK, X11, D-Bus or async-runtime dependencies. The
//! architecture test in `lucerna-testkit` enforces that boundary.

pub mod logging;
pub mod paths;
pub mod version;
