//! The wallpaper backend abstraction (directive §7).
//!
//! The daemon talks to the desktop only through [`WallpaperBackend`]. X11 types never cross this
//! boundary, so a Wayland backend can be added later without touching configuration, policy,
//! IPC or the GUI.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Stable identity of a physical display, for example `edid:DEL-a0b1-7XJ2K3` or `conn:HDMI-1`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
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

/// A rectangle in root-window coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    pub fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn area(&self) -> u64 {
        u64::from(self.width) * u64::from(self.height)
    }

    /// Area of the overlap with `other` (0 if they do not intersect).
    pub fn intersection_area(&self, other: &Rect) -> u64 {
        let left = i64::from(self.x).max(i64::from(other.x));
        let top = i64::from(self.y).max(i64::from(other.y));
        let right = (i64::from(self.x) + i64::from(self.width))
            .min(i64::from(other.x) + i64::from(other.width));
        let bottom = (i64::from(self.y) + i64::from(self.height))
            .min(i64::from(other.y) + i64::from(other.height));
        if right <= left || bottom <= top {
            0
        } else {
            u64::try_from((right - left) * (bottom - top)).unwrap_or(0)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Rotation {
    Normal,
    Left,
    Inverted,
    Right,
}

impl Rotation {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Left => "left",
            Self::Inverted => "inverted",
            Self::Right => "right",
        }
    }
}

/// What the monitor's EDID says about itself. The serial number is deliberately optional: many
/// panels report none.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EdidIdentity {
    /// Three-letter PNP manufacturer ID, e.g. `DEL`.
    pub manufacturer: String,
    pub product_code: u16,
    pub serial: Option<String>,
    pub model_name: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutputInfo {
    pub id: OutputId,
    /// Connector name, e.g. `HDMI-1` or `eDP-1`.
    pub connector: String,
    /// Root-window coordinates, after rotation and transforms.
    pub geometry: Rect,
    pub primary: bool,
    pub rotation: Rotation,
    pub refresh_mhz: Option<u32>,
    pub edid: Option<EdidIdentity>,
}

impl OutputInfo {
    /// A human-readable label such as `HDMI-1 — 1920×1080` or `eDP-1 — 1920×1080 — Primary`.
    pub fn label(&self) -> String {
        let mut label = format!(
            "{} — {}×{}",
            self.connector, self.geometry.width, self.geometry.height
        );
        if self.primary {
            label.push_str(" — Primary");
        }
        label
    }
}

/// Backend-local surface handle; never reused within a process.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SurfaceId(pub u64);

/// What a renderer needs in order to draw into a surface.
///
/// Non-exhaustive so a future Wayland backend can add a variant without changing callers.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EmbedTarget {
    /// An X11 window id for mpv's `--wid`.
    X11Window(u32),
}

#[derive(Clone, Debug)]
pub struct SurfaceHandle {
    pub id: SurfaceId,
    pub output: OutputId,
    pub geometry: Rect,
    pub embed: EmbedTarget,
}

#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackendKind {
    /// Cinnamon (Muffin) on X11: the release acceptance target.
    #[serde(rename = "cinnamon-x11")]
    CinnamonX11,
    /// Any other EWMH-compatible X11 window manager, best effort.
    #[serde(rename = "x11-ewmh")]
    X11Ewmh,
}

impl BackendKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CinnamonX11 => "cinnamon-x11",
            Self::X11Ewmh => "x11-ewmh",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct BackendCapabilities {
    /// EWMH client list and `_NET_WM_STATE` are available, so fullscreen pause can work.
    pub fullscreen_detection: bool,
    /// The SHAPE extension is available, so surfaces can be click-through.
    pub input_passthrough: bool,
    /// RandR change notifications are available.
    pub hotplug_events: bool,
    /// EDID could be read from at least one output.
    pub stable_monitor_identity: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct BackendProbe {
    pub kind: BackendKind,
    pub capabilities: BackendCapabilities,
    /// Backend-specific diagnostic facts (window manager, RandR version, Nemo, ...).
    pub facts: serde_json::Value,
}

#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BackendEvent {
    /// Outputs changed; re-enumerate. Already debounced by the backend.
    OutputsChanged,
    /// Geometry of every visible fullscreen client on the current desktop.
    FullscreenChanged(Vec<Rect>),
    /// Something may have moved above or below our surfaces.
    StackingDisturbed,
    /// The display server went away (logout, crash).
    ConnectionLost(String),
}

/// Receives events from a backend-owned thread.
pub type EventSink = Box<dyn Fn(BackendEvent) + Send + Sync + 'static>;

#[derive(Debug, thiserror::Error)]
pub enum BackendError {
    #[error(
        "Lucerna could not connect to the X11 display {display:?}: {reason}\n\
         This release supports X11 sessions. Log in with a \"Cinnamon\" (X11) session."
    )]
    Connect { display: String, reason: String },
    #[error("The display server lacks a required feature: {0}")]
    MissingFeature(&'static str),
    #[error("internal error: unknown surface {0:?}")]
    UnknownSurface(SurfaceId),
    #[error("The connection to the display server was lost.")]
    ConnectionLost,
    #[error("The display server rejected a request: {0}")]
    Protocol(String),
}

/// `Sync` as well as `Send` so the daemon's engine future can move between threads even though it
/// only ever uses the backend from one task at a time.
pub trait WallpaperBackend: Send + Sync {
    fn kind(&self) -> BackendKind;
    /// Collect environment facts and capabilities. Idempotent and read-only.
    fn probe(&mut self) -> Result<BackendProbe, BackendError>;
    /// Currently connected, active outputs. Disabled or disconnected outputs are omitted.
    fn enumerate_outputs(&mut self) -> Result<Vec<OutputInfo>, BackendError>;
    /// Create, configure, map and bottom-stack a surface covering `output.geometry`.
    fn create_surface(&mut self, output: &OutputInfo) -> Result<SurfaceHandle, BackendError>;
    fn resize_surface(&mut self, surface: SurfaceId, geometry: Rect) -> Result<(), BackendError>;
    /// Idempotent: destroying an unknown or already destroyed surface returns `Ok`.
    fn destroy_surface(&mut self, surface: SurfaceId) -> Result<(), BackendError>;
    /// Re-assert stacking and hints on all live surfaces (rate-limited by the caller).
    fn refresh(&mut self) -> Result<(), BackendError>;
    /// Start delivering events to `sink` from a backend-owned thread. Called once.
    fn subscribe(&mut self, sink: EventSink) -> Result<(), BackendError>;
    /// Structured diagnostic data (window ids, stacking positions, hints) for `doctor`.
    fn diagnostics(&mut self) -> serde_json::Value;
    /// Destroy all surfaces, stop the event thread, close the connection. Idempotent.
    fn shutdown(&mut self) -> Result<(), BackendError>;
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

    #[test]
    fn rect_intersection() {
        let a = Rect::new(0, 0, 100, 100);
        assert_eq!(a.intersection_area(&Rect::new(50, 50, 100, 100)), 2500);
        assert_eq!(a.intersection_area(&Rect::new(100, 0, 10, 10)), 0);
        assert_eq!(a.intersection_area(&Rect::new(-10, -10, 500, 500)), 10_000);
        assert_eq!(a.area(), 10_000);
    }

    #[test]
    fn labels_match_the_spec_examples() {
        let mut o = OutputInfo {
            id: OutputId::new("conn:HDMI-1"),
            connector: "HDMI-1".into(),
            geometry: Rect::new(0, 0, 1920, 1080),
            primary: false,
            rotation: Rotation::Normal,
            refresh_mhz: None,
            edid: None,
        };
        assert_eq!(o.label(), "HDMI-1 — 1920×1080");
        o.connector = "eDP-1".into();
        o.primary = true;
        assert_eq!(o.label(), "eDP-1 — 1920×1080 — Primary");
    }

    #[test]
    fn output_id_serialises_as_a_plain_string() {
        let json = serde_json::to_string(&OutputId::new("conn:X")).unwrap();
        assert_eq!(json, "\"conn:X\"");
    }
}
