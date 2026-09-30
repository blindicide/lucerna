//! Lucerna core: pure logic and domain types.
//!
//! This crate has no GTK, X11, D-Bus or async-runtime dependencies. The
//! architecture test in `lucerna-testkit` enforces that boundary.

pub mod autostart;
pub mod backend;
pub mod bounded_log;
pub mod config;
pub mod doctor;
pub mod fsutil;
pub mod geometry;
pub mod identity;
pub mod library;
pub mod logging;
pub mod mpv;
pub mod paths;
pub mod plan;
pub mod policy;
pub mod renderer;
pub mod runtime;
pub mod session;
pub mod state;
#[cfg(feature = "test-support")]
pub mod testing;
pub mod timeutil;
pub mod types;
pub mod version;
