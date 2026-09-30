//! Typed views of the `a{sv}` payloads (docs/IMPLEMENTATION-PLAN.md §7.4).
//!
//! Every DTO converts to and from a [`Dict`] and serialises to JSON for `lucernactl --json`.
//! Readers tolerate missing keys, so an older client keeps working when the daemon adds fields.

use serde::Serialize;

use crate::dict::{Dict, DictBuilder, DictReader};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct RendererDto {
    pub display_id: String,
    pub connector: String,
    pub wallpaper_id: String,
    /// `stopped`, `starting`, `playing`, `paused`, `stopping` or `failed`.
    pub state: String,
    /// Subset of `user`, `lock`, `fullscreen`.
    pub pause_reasons: Vec<String>,
    pub failure_code: String,
    pub failure_message: String,
    pub restarts_in_window: u32,
    /// 0 if there is no process.
    pub pid: u32,
    pub scaling: String,
}

impl RendererDto {
    pub fn to_dict(&self) -> Dict {
        DictBuilder::new()
            .str("display_id", &self.display_id)
            .str("connector", &self.connector)
            .str("wallpaper_id", &self.wallpaper_id)
            .str("state", &self.state)
            .strings("pause_reasons", &self.pause_reasons)
            .str("failure_code", &self.failure_code)
            .str("failure_message", &self.failure_message)
            .u32("restarts_in_window", self.restarts_in_window)
            .u32("pid", self.pid)
            .str("scaling", &self.scaling)
            .build()
    }

    pub fn from_dict(d: &Dict) -> Self {
        let r = DictReader(d);
        Self {
            display_id: r.str_or_empty("display_id"),
            connector: r.str_or_empty("connector"),
            wallpaper_id: r.str_or_empty("wallpaper_id"),
            state: r.str_or_empty("state"),
            pause_reasons: r.strings("pause_reasons"),
            failure_code: r.str_or_empty("failure_code"),
            failure_message: r.str_or_empty("failure_message"),
            restarts_in_window: r.u32("restarts_in_window").unwrap_or(0),
            pid: r.u32("pid").unwrap_or(0),
            scaling: r.str_or_empty("scaling"),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct StatusDto {
    pub daemon_version: String,
    pub api_version: u32,
    /// `cinnamon-x11`, `x11-ewmh` or `none`.
    pub backend: String,
    /// `x11`, `wayland`, `tty` or `unknown`.
    pub session_type: String,
    pub supported: bool,
    /// Empty, `wayland`, `no-display`, `x11-connect-failed`, `missing-feature` or `runtime-dir-unavailable`.
    pub unsupported_reason: String,
    pub unsupported_message: String,
    /// Aggregate: `playing`, `paused`, `stopped`, `idle` or `failed`.
    pub playback: String,
    pub user_paused: bool,
    pub user_stopped: bool,
    pub session_locked: bool,
    pub lock_detection: bool,
    pub fullscreen_detection: bool,
    pub mpv_available: bool,
    pub mpv_version: String,
    pub schema_version: u32,
    /// `ok`, `defaults-after-corruption`, `read-only-newer-schema` or `unreadable`.
    pub config_state: String,
    pub config_notice: String,
    pub config_warnings: Vec<String>,
    pub renderers: Vec<RendererDto>,
}

impl StatusDto {
    pub fn to_dict(&self) -> Dict {
        DictBuilder::new()
            .str("daemon_version", &self.daemon_version)
            .u32("api_version", self.api_version)
            .str("backend", &self.backend)
            .str("session_type", &self.session_type)
            .bool("supported", self.supported)
            .str("unsupported_reason", &self.unsupported_reason)
            .str("unsupported_message", &self.unsupported_message)
            .str("playback", &self.playback)
            .bool("user_paused", self.user_paused)
            .bool("user_stopped", self.user_stopped)
            .bool("session_locked", self.session_locked)
            .bool("lock_detection", self.lock_detection)
            .bool("fullscreen_detection", self.fullscreen_detection)
            .bool("mpv_available", self.mpv_available)
            .str("mpv_version", &self.mpv_version)
            .u32("schema_version", self.schema_version)
            .str("config_state", &self.config_state)
            .str("config_notice", &self.config_notice)
            .strings("config_warnings", &self.config_warnings)
            .dicts(
                "renderers",
                self.renderers.iter().map(RendererDto::to_dict).collect(),
            )
            .build()
    }

    pub fn from_dict(d: &Dict) -> Self {
        let r = DictReader(d);
        Self {
            daemon_version: r.str_or_empty("daemon_version"),
            api_version: r.u32("api_version").unwrap_or(0),
            backend: r.str_or_empty("backend"),
            session_type: r.str_or_empty("session_type"),
            supported: r.bool_or_false("supported"),
            unsupported_reason: r.str_or_empty("unsupported_reason"),
            unsupported_message: r.str_or_empty("unsupported_message"),
            playback: r.str_or_empty("playback"),
            user_paused: r.bool_or_false("user_paused"),
            user_stopped: r.bool_or_false("user_stopped"),
            session_locked: r.bool_or_false("session_locked"),
            lock_detection: r.bool_or_false("lock_detection"),
            fullscreen_detection: r.bool_or_false("fullscreen_detection"),
            mpv_available: r.bool_or_false("mpv_available"),
            mpv_version: r.str_or_empty("mpv_version"),
            schema_version: r.u32("schema_version").unwrap_or(0),
            config_state: r.str_or_empty("config_state"),
            config_notice: r.str_or_empty("config_notice"),
            config_warnings: r.strings("config_warnings"),
            renderers: r
                .dicts("renderers")
                .iter()
                .map(RendererDto::from_dict)
                .collect(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct DisplayDto {
    pub id: String,
    pub connector: String,
    /// For example `eDP-1 — 1920×1080 — Primary`.
    pub label: String,
    /// False for a configured display that is currently absent.
    pub connected: bool,
    pub primary: bool,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub rotation: String,
    pub edid_manufacturer: String,
    pub edid_model: String,
    /// Empty unless a diagnostics call asks for it.
    pub edid_serial: String,
    /// The effective wallpaper id, or empty.
    pub wallpaper_id: String,
    /// `display`, `all` or `none`.
    pub wallpaper_source: String,
    pub scaling: String,
}

impl DisplayDto {
    pub fn to_dict(&self) -> Dict {
        DictBuilder::new()
            .str("id", &self.id)
            .str("connector", &self.connector)
            .str("label", &self.label)
            .bool("connected", self.connected)
            .bool("primary", self.primary)
            .i32("x", self.x)
            .i32("y", self.y)
            .u32("width", self.width)
            .u32("height", self.height)
            .str("rotation", &self.rotation)
            .str("edid_manufacturer", &self.edid_manufacturer)
            .str("edid_model", &self.edid_model)
            .str("edid_serial", &self.edid_serial)
            .str("wallpaper_id", &self.wallpaper_id)
            .str("wallpaper_source", &self.wallpaper_source)
            .str("scaling", &self.scaling)
            .build()
    }

    pub fn from_dict(d: &Dict) -> Self {
        let r = DictReader(d);
        Self {
            id: r.str_or_empty("id"),
            connector: r.str_or_empty("connector"),
            label: r.str_or_empty("label"),
            connected: r.bool_or_false("connected"),
            primary: r.bool_or_false("primary"),
            x: r.i32("x").unwrap_or(0),
            y: r.i32("y").unwrap_or(0),
            width: r.u32("width").unwrap_or(0),
            height: r.u32("height").unwrap_or(0),
            rotation: r.str_or_empty("rotation"),
            edid_manufacturer: r.str_or_empty("edid_manufacturer"),
            edid_model: r.str_or_empty("edid_model"),
            edid_serial: r.str_or_empty("edid_serial"),
            wallpaper_id: r.str_or_empty("wallpaper_id"),
            wallpaper_source: r.str_or_empty("wallpaper_source"),
            scaling: r.str_or_empty("scaling"),
        }
    }
}

/// One entry of `GetAssignments`: `display_id` is a stable display id or `*` for all displays.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct AssignmentDto {
    pub display_id: String,
    pub wallpaper_id: String,
    pub scaling: String,
    /// Informational label for displays that are currently absent.
    pub last_seen: String,
}

impl AssignmentDto {
    pub fn to_dict(&self) -> Dict {
        DictBuilder::new()
            .str("display_id", &self.display_id)
            .str("wallpaper_id", &self.wallpaper_id)
            .str("scaling", &self.scaling)
            .str("last_seen", &self.last_seen)
            .build()
    }

    pub fn from_dict(d: &Dict) -> Self {
        let r = DictReader(d);
        Self {
            display_id: r.str_or_empty("display_id"),
            wallpaper_id: r.str_or_empty("wallpaper_id"),
            scaling: r.str_or_empty("scaling"),
            last_seen: r.str_or_empty("last_seen"),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct WallpaperDto {
    pub id: String,
    pub name: String,
    pub path: String,
    /// `video`, `animated-image` or `unknown`.
    pub media_type: String,
    pub available: bool,
    /// RFC 3339.
    pub added: String,
    pub duration_ms: Option<u64>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub codec: Option<String>,
}

impl WallpaperDto {
    pub fn to_dict(&self) -> Dict {
        let mut b = DictBuilder::new()
            .str("id", &self.id)
            .str("name", &self.name)
            .str("path", &self.path)
            .str("media_type", &self.media_type)
            .bool("available", self.available)
            .str("added", &self.added);
        if let Some(v) = self.duration_ms {
            b = b.u64("duration_ms", v);
        }
        if let Some(v) = self.width {
            b = b.u32("width", v);
        }
        if let Some(v) = self.height {
            b = b.u32("height", v);
        }
        if let Some(v) = &self.codec {
            b = b.str("codec", v);
        }
        b.build()
    }

    pub fn from_dict(d: &Dict) -> Self {
        let r = DictReader(d);
        Self {
            id: r.str_or_empty("id"),
            name: r.str_or_empty("name"),
            path: r.str_or_empty("path"),
            media_type: r.str_or_empty("media_type"),
            available: r.bool_or_false("available"),
            added: r.str_or_empty("added"),
            duration_ms: r.u64("duration_ms"),
            width: r.u32("width"),
            height: r.u32("height"),
            codec: r.str("codec"),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct SettingsDto {
    pub pause_on_fullscreen: bool,
    pub pause_on_lock: bool,
    pub audio: bool,
    /// `auto` or `disabled`.
    pub hardware_decode: String,
    /// `native`, `60`, `30` or `15`.
    pub fps_limit: String,
    /// `auto`, `override-redirect` or `desktop-window`.
    pub stacking: String,
    pub max_restarts: u32,
    pub restart_window_secs: u32,
    /// Derived from the autostart file, not from `config.toml`.
    pub autostart: bool,
}

impl SettingsDto {
    pub fn to_dict(&self) -> Dict {
        DictBuilder::new()
            .bool("pause_on_fullscreen", self.pause_on_fullscreen)
            .bool("pause_on_lock", self.pause_on_lock)
            .bool("audio", self.audio)
            .str("hardware_decode", &self.hardware_decode)
            .str("fps_limit", &self.fps_limit)
            .str("stacking", &self.stacking)
            .u32("max_restarts", self.max_restarts)
            .u32("restart_window_secs", self.restart_window_secs)
            .bool("autostart", self.autostart)
            .build()
    }

    pub fn from_dict(d: &Dict) -> Self {
        let r = DictReader(d);
        Self {
            pause_on_fullscreen: r.bool_or_false("pause_on_fullscreen"),
            pause_on_lock: r.bool_or_false("pause_on_lock"),
            audio: r.bool_or_false("audio"),
            hardware_decode: r.str_or_empty("hardware_decode"),
            fps_limit: r.str_or_empty("fps_limit"),
            stacking: r.str_or_empty("stacking"),
            max_restarts: r.u32("max_restarts").unwrap_or(0),
            restart_window_secs: r.u32("restart_window_secs").unwrap_or(0),
            autostart: r.bool_or_false("autostart"),
        }
    }
}

/// A partial update for `SetSettings`. Only the keys present in the dictionary are changed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SettingsPatch {
    pub pause_on_fullscreen: Option<bool>,
    pub pause_on_lock: Option<bool>,
    pub audio: Option<bool>,
    pub hardware_decode: Option<String>,
    pub fps_limit: Option<String>,
    pub stacking: Option<String>,
    pub max_restarts: Option<u32>,
    pub restart_window_secs: Option<u32>,
    pub autostart: Option<bool>,
}

impl SettingsPatch {
    pub const KEYS: &'static [&'static str] = &[
        "pause_on_fullscreen",
        "pause_on_lock",
        "audio",
        "hardware_decode",
        "fps_limit",
        "stacking",
        "max_restarts",
        "restart_window_secs",
        "autostart",
    ];

    /// Parse a `SetSettings` argument. Unknown keys and wrongly typed values are errors, so a typo
    /// can never be silently ignored; the whole update is validated before anything is applied.
    pub fn from_dict(d: &Dict) -> Result<Self, String> {
        let r = DictReader(d);
        if let Some(unknown) = d.keys().find(|k| !Self::KEYS.contains(&k.as_str())) {
            return Err(format!(
                "'{unknown}' is not a setting. Valid settings: {}.",
                Self::KEYS.join(", ")
            ));
        }
        let typed = |key: &str, ok: bool| -> Result<(), String> {
            if d.contains_key(key) && !ok {
                Err(format!("The value of '{key}' has the wrong type."))
            } else {
                Ok(())
            }
        };
        for key in ["pause_on_fullscreen", "pause_on_lock", "audio", "autostart"] {
            typed(key, r.bool(key).is_some())?;
        }
        for key in ["hardware_decode", "fps_limit", "stacking"] {
            typed(key, r.str(key).is_some())?;
        }
        for key in ["max_restarts", "restart_window_secs"] {
            typed(key, r.u32(key).is_some())?;
        }
        Ok(Self {
            pause_on_fullscreen: r.bool("pause_on_fullscreen"),
            pause_on_lock: r.bool("pause_on_lock"),
            audio: r.bool("audio"),
            hardware_decode: r.str("hardware_decode"),
            fps_limit: r.str("fps_limit"),
            stacking: r.str("stacking"),
            max_restarts: r.u32("max_restarts"),
            restart_window_secs: r.u32("restart_window_secs"),
            autostart: r.bool("autostart"),
        })
    }

    pub fn to_dict(&self) -> Dict {
        let mut b = DictBuilder::new();
        if let Some(v) = self.pause_on_fullscreen {
            b = b.bool("pause_on_fullscreen", v);
        }
        if let Some(v) = self.pause_on_lock {
            b = b.bool("pause_on_lock", v);
        }
        if let Some(v) = self.audio {
            b = b.bool("audio", v);
        }
        if let Some(v) = &self.hardware_decode {
            b = b.str("hardware_decode", v);
        }
        if let Some(v) = &self.fps_limit {
            b = b.str("fps_limit", v);
        }
        if let Some(v) = &self.stacking {
            b = b.str("stacking", v);
        }
        if let Some(v) = self.max_restarts {
            b = b.u32("max_restarts", v);
        }
        if let Some(v) = self.restart_window_secs {
            b = b.u32("restart_window_secs", v);
        }
        if let Some(v) = self.autostart {
            b = b.bool("autostart", v);
        }
        b.build()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn renderer(n: u32) -> RendererDto {
        RendererDto {
            display_id: format!("conn:HDMI-{n}"),
            connector: format!("HDMI-{n}"),
            wallpaper_id: "w".into(),
            state: "failed".into(),
            pause_reasons: vec!["user".into(), "fullscreen".into()],
            failure_code: "crashed".into(),
            failure_message: "line one\nline two — ü".into(),
            restarts_in_window: 2,
            pid: 4242,
            scaling: "fill".into(),
        }
    }

    #[test]
    fn status_round_trips_including_nested_renderers() {
        let status = StatusDto {
            daemon_version: "0.3.0".into(),
            api_version: 1,
            backend: "cinnamon-x11".into(),
            session_type: "x11".into(),
            supported: true,
            playback: "playing".into(),
            user_paused: true,
            mpv_available: true,
            mpv_version: "0.37.0".into(),
            schema_version: 1,
            config_state: "ok".into(),
            config_warnings: vec!["w1".into(), "w2".into()],
            renderers: vec![renderer(1), renderer(2)],
            ..StatusDto::default()
        };
        assert_eq!(StatusDto::from_dict(&status.to_dict()), status);
    }

    #[test]
    fn display_wallpaper_assignment_settings_round_trip() {
        let display = DisplayDto {
            id: "edid:DEL-a0b1-7XJ2K3".into(),
            connector: "DP-1".into(),
            label: "DP-1 — 2560×1440 — Primary".into(),
            connected: true,
            primary: true,
            x: -1920,
            y: 0,
            width: 2560,
            height: 1440,
            rotation: "normal".into(),
            edid_manufacturer: "DEL".into(),
            edid_model: "U2720Q".into(),
            edid_serial: String::new(),
            wallpaper_id: "w".into(),
            wallpaper_source: "all".into(),
            scaling: "fit".into(),
        };
        assert_eq!(DisplayDto::from_dict(&display.to_dict()), display);

        let assignment = AssignmentDto {
            display_id: "*".into(),
            wallpaper_id: "w".into(),
            scaling: "fill".into(),
            last_seen: String::new(),
        };
        assert_eq!(AssignmentDto::from_dict(&assignment.to_dict()), assignment);

        let mut wallpaper = WallpaperDto {
            id: "w".into(),
            name: "Rain".into(),
            path: "/v/rain.webm".into(),
            media_type: "video".into(),
            available: true,
            added: "2026-09-30T10:00:00Z".into(),
            ..WallpaperDto::default()
        };
        assert_eq!(WallpaperDto::from_dict(&wallpaper.to_dict()), wallpaper);
        wallpaper.duration_ms = Some(12_000);
        wallpaper.width = Some(1920);
        wallpaper.height = Some(1080);
        wallpaper.codec = Some("vp9".into());
        assert_eq!(WallpaperDto::from_dict(&wallpaper.to_dict()), wallpaper);

        let settings = SettingsDto {
            pause_on_fullscreen: true,
            pause_on_lock: true,
            audio: false,
            hardware_decode: "auto".into(),
            fps_limit: "30".into(),
            stacking: "auto".into(),
            max_restarts: 3,
            restart_window_secs: 60,
            autostart: true,
        };
        assert_eq!(SettingsDto::from_dict(&settings.to_dict()), settings);
    }

    #[test]
    fn readers_ignore_unknown_keys_and_default_missing_ones() {
        let mut d = renderer(1).to_dict();
        d.insert(
            "added_in_the_future".into(),
            zbus::zvariant::OwnedValue::from(1u32),
        );
        assert_eq!(RendererDto::from_dict(&d), renderer(1));
        assert_eq!(StatusDto::from_dict(&Dict::new()), StatusDto::default());
    }

    #[test]
    fn dtos_serialise_to_json_with_plain_field_names() {
        let json = serde_json::to_value(renderer(1)).unwrap();
        assert_eq!(json["display_id"], "conn:HDMI-1");
        assert_eq!(json["pause_reasons"][1], "fullscreen");
    }

    #[test]
    fn settings_patch_round_trips_and_only_changes_what_it_names() {
        let patch = SettingsPatch {
            audio: Some(true),
            fps_limit: Some("30".into()),
            autostart: Some(false),
            ..Default::default()
        };
        let dict = patch.to_dict();
        assert_eq!(dict.len(), 3);
        assert_eq!(SettingsPatch::from_dict(&dict).unwrap(), patch);
        assert_eq!(
            SettingsPatch::from_dict(&Dict::new()).unwrap(),
            SettingsPatch::default()
        );
    }

    #[test]
    fn settings_patch_rejects_unknown_keys_and_wrong_types() {
        let typo = DictBuilder::new().bool("pause_on_fullscren", true).build();
        let err = SettingsPatch::from_dict(&typo).unwrap_err();
        assert!(
            err.contains("pause_on_fullscren") && err.contains("Valid settings"),
            "{err}"
        );

        let wrong = DictBuilder::new().str("audio", "yes").build();
        assert!(
            SettingsPatch::from_dict(&wrong)
                .unwrap_err()
                .contains("wrong type")
        );
        let wrong = DictBuilder::new().bool("max_restarts", true).build();
        assert!(SettingsPatch::from_dict(&wrong).is_err());
    }
}
