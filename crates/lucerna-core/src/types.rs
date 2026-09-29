//! Small domain enums shared by configuration, the renderer and the user interfaces.

use std::fmt;
use std::path::Path;
use std::str::FromStr;

/// How a video is fitted to a display (directive §13). Deterministic and documented in
/// `docs/CONFIGURATION.md`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ScalingMode {
    /// Cover the whole output, keep aspect ratio, crop the overflow.
    #[default]
    Fill,
    /// Show the whole video, keep aspect ratio, letterbox in black.
    Fit,
    /// Cover the whole output and distort the aspect ratio.
    Stretch,
    /// Native pixel size, centred.
    Center,
}

impl ScalingMode {
    pub const ALL: [ScalingMode; 4] = [Self::Fill, Self::Fit, Self::Stretch, Self::Center];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fill => "fill",
            Self::Fit => "fit",
            Self::Stretch => "stretch",
            Self::Center => "center",
        }
    }

    /// The three mpv properties that implement this mode, as `(name, value)` strings.
    ///
    /// The same values are used for launch options (`--keepaspect=yes`) and for live
    /// `set_property` commands.
    pub fn mpv_properties(self) -> [(&'static str, &'static str); 3] {
        let (keepaspect, panscan, unscaled) = match self {
            Self::Fill => ("yes", "1.0", "no"),
            Self::Fit => ("yes", "0.0", "no"),
            Self::Stretch => ("no", "0.0", "no"),
            Self::Center => ("yes", "0.0", "yes"),
        };
        [
            ("keepaspect", keepaspect),
            ("panscan", panscan),
            ("video-unscaled", unscaled),
        ]
    }
}

impl fmt::Display for ScalingMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Error for an unrecognised enum spelling.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("'{value}' is not a valid {what}; expected one of: {expected}")]
pub struct ParseEnumError {
    pub what: &'static str,
    pub value: String,
    pub expected: &'static str,
}

impl FromStr for ScalingMode {
    type Err = ParseEnumError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "fill" => Ok(Self::Fill),
            "fit" => Ok(Self::Fit),
            "stretch" => Ok(Self::Stretch),
            "center" | "centre" => Ok(Self::Center),
            _ => Err(ParseEnumError {
                what: "scaling mode",
                value: s.to_owned(),
                expected: "fill, fit, stretch, center",
            }),
        }
    }
}

/// Hardware decoding preference (directive §17).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum HwDecode {
    /// Maps to mpv's `auto-safe`.
    #[default]
    Auto,
    Disabled,
}

impl HwDecode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Disabled => "disabled",
        }
    }
}

impl FromStr for HwDecode {
    type Err = ParseEnumError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" => Ok(Self::Auto),
            "disabled" | "off" | "no" => Ok(Self::Disabled),
            _ => Err(ParseEnumError {
                what: "hardware decoding setting",
                value: s.to_owned(),
                expected: "auto, disabled",
            }),
        }
    }
}

/// Frame-rate cap presets (directive §17).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum FpsLimit {
    #[default]
    Native,
    Fps60,
    Fps30,
    Fps15,
}

impl FpsLimit {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Native => "native",
            Self::Fps60 => "60",
            Self::Fps30 => "30",
            Self::Fps15 => "15",
        }
    }

    /// The mpv video filter that implements the cap, if any.
    pub fn video_filter(self) -> Option<&'static str> {
        match self {
            Self::Native => None,
            Self::Fps60 => Some("fps=60"),
            Self::Fps30 => Some("fps=30"),
            Self::Fps15 => Some("fps=15"),
        }
    }
}

impl FromStr for FpsLimit {
    type Err = ParseEnumError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "native" => Ok(Self::Native),
            "60" => Ok(Self::Fps60),
            "30" => Ok(Self::Fps30),
            "15" => Ok(Self::Fps15),
            _ => Err(ParseEnumError {
                what: "FPS limit",
                value: s.to_owned(),
                expected: "native, 60, 30, 15",
            }),
        }
    }
}

/// Informational media classification, derived from the file extension only.
///
/// Lucerna deliberately has no codec whitelist (directive §10): if mpv can open a file, it may
/// be used, whatever this says.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MediaType {
    Video,
    AnimatedImage,
    Unknown,
}

impl MediaType {
    pub fn from_path(path: &Path) -> Self {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase);
        match ext.as_deref() {
            Some("mp4" | "webm" | "mkv" | "mov" | "avi" | "m4v" | "ogv" | "flv" | "wmv") => {
                Self::Video
            }
            Some("gif" | "apng" | "webp") => Self::AnimatedImage,
            _ => Self::Unknown,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Video => "video",
            Self::AnimatedImage => "animated-image",
            Self::Unknown => "unknown",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaling_round_trips_through_strings() {
        for mode in ScalingMode::ALL {
            assert_eq!(mode.as_str().parse::<ScalingMode>().unwrap(), mode);
        }
        assert!("zoom".parse::<ScalingMode>().is_err());
    }

    #[test]
    fn scaling_properties_are_the_documented_ones() {
        assert_eq!(
            ScalingMode::Fill.mpv_properties(),
            [
                ("keepaspect", "yes"),
                ("panscan", "1.0"),
                ("video-unscaled", "no")
            ]
        );
        assert_eq!(
            ScalingMode::Fit.mpv_properties(),
            [
                ("keepaspect", "yes"),
                ("panscan", "0.0"),
                ("video-unscaled", "no")
            ]
        );
        assert_eq!(
            ScalingMode::Stretch.mpv_properties(),
            [
                ("keepaspect", "no"),
                ("panscan", "0.0"),
                ("video-unscaled", "no")
            ]
        );
        assert_eq!(
            ScalingMode::Center.mpv_properties(),
            [
                ("keepaspect", "yes"),
                ("panscan", "0.0"),
                ("video-unscaled", "yes")
            ]
        );
    }

    #[test]
    fn fps_and_hwdec_parse() {
        assert_eq!("60".parse::<FpsLimit>().unwrap(), FpsLimit::Fps60);
        assert_eq!("Native".parse::<FpsLimit>().unwrap(), FpsLimit::Native);
        assert!("144".parse::<FpsLimit>().is_err());
        assert_eq!(FpsLimit::Fps15.video_filter(), Some("fps=15"));
        assert_eq!(FpsLimit::Native.video_filter(), None);
        assert_eq!("disabled".parse::<HwDecode>().unwrap(), HwDecode::Disabled);
        assert!("gpu".parse::<HwDecode>().is_err());
    }

    #[test]
    fn media_type_from_extension() {
        assert_eq!(
            MediaType::from_path(Path::new("/a/b.MP4")),
            MediaType::Video
        );
        assert_eq!(
            MediaType::from_path(Path::new("/a/b.gif")),
            MediaType::AnimatedImage
        );
        assert_eq!(MediaType::from_path(Path::new("/a/b")), MediaType::Unknown);
    }
}
