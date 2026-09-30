//! Configuration schema v1 (directive §18, §53).
//!
//! The daemon is the only writer of `config.toml`. Reading is lenient: unknown keys are ignored,
//! unknown or ill-typed values fall back to defaults with a warning, and the original
//! [`toml_edit::DocumentMut`] is kept so that a save rewrites only the fields that actually
//! changed, preserving unknown keys and comments.

mod doc;
mod load;
mod migrate;

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;

pub use doc::{apply_to_doc, parse_doc};
pub use load::{
    ConfigError, ConfigState, Inspection, LoadedConfig, inspect, load, load_with, save,
};
pub use migrate::{Migration, MigrationError, migrate_with};

use crate::backend::OutputId;
use crate::types::{FpsLimit, HwDecode, MediaType, ScalingMode};

/// The schema version this build reads and writes.
pub const CURRENT_SCHEMA: u32 = 1;

/// A UUID identifying a library entry.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WallpaperId(String);

impl WallpaperId {
    pub fn new_random() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }

    /// Accepts any non-empty string without control characters; UUIDs are what Lucerna writes.
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        (!text.is_empty() && !text.chars().any(char::is_control)).then(|| Self(text.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for WallpaperId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// How the X11 backend stacks surfaces (`[x11] stacking`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StackingSetting {
    #[default]
    Auto,
    OverrideRedirect,
    DesktopWindow,
}

impl StackingSetting {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::OverrideRedirect => "override-redirect",
            Self::DesktopWindow => "desktop-window",
        }
    }
}

impl std::str::FromStr for StackingSetting {
    type Err = crate::types::ParseEnumError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" => Ok(Self::Auto),
            "override-redirect" => Ok(Self::OverrideRedirect),
            "desktop-window" => Ok(Self::DesktopWindow),
            _ => Err(crate::types::ParseEnumError {
                what: "stacking strategy",
                value: s.to_owned(),
                expected: "auto, override-redirect, desktop-window",
            }),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeneralSettings {
    pub pause_on_fullscreen: bool,
    pub pause_on_lock: bool,
    /// Never enabled silently (§16).
    pub audio: bool,
    pub hardware_decode: HwDecode,
    pub fps_limit: FpsLimit,
}

impl Default for GeneralSettings {
    fn default() -> Self {
        Self {
            pause_on_fullscreen: true,
            pause_on_lock: true,
            audio: false,
            hardware_decode: HwDecode::Auto,
            fps_limit: FpsLimit::Native,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RendererSettings {
    /// Clamped to 1..=10 when used.
    pub max_restarts: u32,
    /// Clamped to 10..=3600 when used.
    pub restart_window_secs: u64,
}

impl Default for RendererSettings {
    fn default() -> Self {
        Self {
            max_restarts: 3,
            restart_window_secs: 60,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct X11Settings {
    pub stacking: StackingSetting,
}

/// Mode 1 of §13: one wallpaper on every display that has no override.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AllDisplays {
    pub wallpaper: Option<WallpaperId>,
    pub scaling: ScalingMode,
}

/// Mode 2 of §13: per-display override, keyed by stable display id.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DisplayConfig {
    pub wallpaper: Option<WallpaperId>,
    pub scaling: Option<ScalingMode>,
    /// Informational label shown for displays that are currently absent.
    pub last_seen: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Wallpaper {
    pub id: WallpaperId,
    pub name: String,
    /// Absolute, canonical, valid UTF-8.
    pub path: PathBuf,
    pub media_type: MediaType,
    /// RFC 3339 UTC.
    pub added: String,
    /// Last known existence state (§11).
    pub available: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Config {
    pub general: GeneralSettings,
    pub renderer: RendererSettings,
    pub x11: X11Settings,
    pub all_displays: AllDisplays,
    pub displays: BTreeMap<OutputId, DisplayConfig>,
    pub wallpapers: Vec<Wallpaper>,
}

/// Where the effective wallpaper of a display comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WallpaperSource {
    Display,
    All,
    None,
}

impl WallpaperSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Display => "display",
            Self::All => "all",
            Self::None => "none",
        }
    }
}

impl Config {
    /// The effective wallpaper of `output`: its own assignment, else the all-displays one.
    pub fn effective_wallpaper(
        &self,
        output: &OutputId,
    ) -> (Option<&WallpaperId>, WallpaperSource) {
        if let Some(id) = self.displays.get(output).and_then(|d| d.wallpaper.as_ref()) {
            return (Some(id), WallpaperSource::Display);
        }
        match &self.all_displays.wallpaper {
            Some(id) => (Some(id), WallpaperSource::All),
            None => (None, WallpaperSource::None),
        }
    }

    pub fn effective_scaling(&self, output: &OutputId) -> ScalingMode {
        self.displays
            .get(output)
            .and_then(|d| d.scaling)
            .unwrap_or(self.all_displays.scaling)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_documented_ones() {
        let c = Config::default();
        assert!(c.general.pause_on_fullscreen);
        assert!(c.general.pause_on_lock);
        assert!(!c.general.audio, "audio is off by default (§16)");
        assert_eq!(c.general.hardware_decode, HwDecode::Auto);
        assert_eq!(c.general.fps_limit, FpsLimit::Native);
        assert_eq!(
            c.renderer,
            RendererSettings {
                max_restarts: 3,
                restart_window_secs: 60
            }
        );
        assert_eq!(c.x11.stacking, StackingSetting::Auto);
        assert_eq!(c.all_displays.scaling, ScalingMode::Fill);
    }

    #[test]
    fn effective_wallpaper_prefers_the_display_override() {
        let mut c = Config::default();
        let global = WallpaperId::parse("global").unwrap();
        let local = WallpaperId::parse("local").unwrap();
        let a = OutputId::new("edid:A");
        let b = OutputId::new("edid:B");
        c.all_displays.wallpaper = Some(global.clone());
        c.displays.insert(
            a.clone(),
            DisplayConfig {
                wallpaper: Some(local.clone()),
                ..Default::default()
            },
        );
        assert_eq!(
            c.effective_wallpaper(&a),
            (Some(&local), WallpaperSource::Display)
        );
        assert_eq!(
            c.effective_wallpaper(&b),
            (Some(&global), WallpaperSource::All)
        );
        c.all_displays.wallpaper = None;
        assert_eq!(c.effective_wallpaper(&b), (None, WallpaperSource::None));
    }

    #[test]
    fn effective_scaling_inherits() {
        let mut c = Config::default();
        c.all_displays.scaling = ScalingMode::Fit;
        let a = OutputId::new("a");
        assert_eq!(c.effective_scaling(&a), ScalingMode::Fit);
        c.displays.insert(
            a.clone(),
            DisplayConfig {
                scaling: Some(ScalingMode::Center),
                ..Default::default()
            },
        );
        assert_eq!(c.effective_scaling(&a), ScalingMode::Center);
    }

    #[test]
    fn wallpaper_ids_reject_blank_and_control_characters() {
        assert!(WallpaperId::parse("").is_none());
        assert!(WallpaperId::parse("  ").is_none());
        assert!(WallpaperId::parse("a\nb").is_none());
        assert!(WallpaperId::parse(" x ").is_some_and(|i| i.as_str() == "x"));
        assert_ne!(WallpaperId::new_random(), WallpaperId::new_random());
    }
}

#[cfg(test)]
mod tests_file;
