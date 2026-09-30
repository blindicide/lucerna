//! The X11 wallpaper backend (directive §8): Cinnamon/Muffin first, generic EWMH as best effort.
//!
//! Everything X11-specific lives here. The daemon sees only `lucerna_core::backend`.
//!
//! **Nothing in this crate can prove that a wallpaper *looks* right.** The tests in
//! `lucerna-testkit` run against Xvfb and verify protocol behaviour only (window properties,
//! event routing, RandR data). How a real Cinnamon session stacks the surfaces is documented in
//! `docs/X11-CINNAMON-NOTES.md` and left to the manual acceptance campaign.

mod atoms;
mod backend;
mod conn;
mod events;
mod facts;
mod outputs;
mod surface;

pub use backend::{StackingMode, X11Backend, X11Options, connect};
pub use facts::{X11Probe, probe_display};
