//! Types shared between the daemon and any wallpaper backend.
//!
//! The full `WallpaperBackend` trait arrives with the X11 backend milestone; the identity and
//! embedding types live here first because the renderer already needs them.

use std::fmt;

/// Stable identity of a physical display, for example `edid:DEL-a0b1-7XJ2K3` or `conn:HDMI-1`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct OutputId(String);

impl OutputId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Eight hex characters derived from the ID (FNV-1a 64), used to keep runtime socket
    /// paths short and free of characters that are awkward in file names.
    pub fn slot(&self) -> String {
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for byte in self.0.bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        format!("{hash:016x}")[..8].to_owned()
    }
}

impl fmt::Display for OutputId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// What a renderer needs in order to draw into a surface.
///
/// Non-exhaustive so a future Wayland backend can add a variant without changing callers.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EmbedTarget {
    /// An X11 window id for mpv's `--wid`.
    X11Window(u32),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slot_is_eight_hex_chars_and_stable() {
        let a = OutputId::new("edid:DEL-a0b1-7XJ2K3").slot();
        assert_eq!(a.len(), 8);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(a, OutputId::new("edid:DEL-a0b1-7XJ2K3").slot());
        assert_ne!(a, OutputId::new("conn:HDMI-1").slot());
    }

    #[test]
    fn slot_matches_fnv1a_reference_vector() {
        // FNV-1a 64 of "a" is af63dc4c8601ec8c.
        assert_eq!(OutputId::new("a").slot(), "af63dc4c");
    }
}
