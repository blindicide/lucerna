//! What kind of desktop session are we in? (directive §49, §50)

use std::ffi::OsString;

/// The environment variables that decide it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SessionEnv {
    pub xdg_session_type: Option<String>,
    pub wayland_display: Option<String>,
    pub display: Option<String>,
    pub xdg_current_desktop: Option<String>,
    pub desktop_session: Option<String>,
    /// `XDG_SESSION_ID`, used to find this session in logind.
    pub xdg_session_id: Option<String>,
}

fn non_empty(value: Option<OsString>) -> Option<String> {
    value
        .map(|v| v.to_string_lossy().into_owned())
        .filter(|v| !v.is_empty())
}

impl SessionEnv {
    pub fn from_env() -> Self {
        Self::from_lookup(|key| std::env::var_os(key))
    }

    pub fn from_lookup(get: impl Fn(&str) -> Option<OsString>) -> Self {
        Self {
            xdg_session_type: non_empty(get("XDG_SESSION_TYPE")),
            wayland_display: non_empty(get("WAYLAND_DISPLAY")),
            display: non_empty(get("DISPLAY")),
            xdg_current_desktop: non_empty(get("XDG_CURRENT_DESKTOP")),
            desktop_session: non_empty(get("DESKTOP_SESSION")),
            xdg_session_id: non_empty(get("XDG_SESSION_ID")),
        }
    }
}

/// The kind of session, as far as the environment can tell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionKind {
    X11,
    Wayland,
    /// No graphical display variables at all (a TTY, ssh, a container).
    Tty,
    Unknown,
}

impl SessionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::X11 => "x11",
            Self::Wayland => "wayland",
            Self::Tty => "tty",
            Self::Unknown => "unknown",
        }
    }
}

/// Why wallpaper playback is not possible in this session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnsupportedReason {
    Wayland,
    NoDisplay,
    X11ConnectFailed { display: String, reason: String },
    MissingFeature(String),
    RuntimeDirUnavailable,
}

impl UnsupportedReason {
    /// Stable code used in `GetStatus.unsupported_reason`.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Wayland => "wayland",
            Self::NoDisplay => "no-display",
            Self::X11ConnectFailed { .. } => "x11-connect-failed",
            Self::MissingFeature(_) => "missing-feature",
            Self::RuntimeDirUnavailable => "runtime-dir-unavailable",
        }
    }

    /// A message that says what happened, why, and what to do (§49).
    pub fn message(&self) -> String {
        match self {
            Self::Wayland => "Lucerna could not start wallpaper playback.\n\
                This release supports X11 sessions only; your current session appears to be Wayland.\n\
                Log in with a \"Cinnamon\" (X11) session."
                .to_owned(),
            Self::NoDisplay => "Lucerna could not connect to a graphical display.\n\
                DISPLAY is not set, so there is no X11 session to draw the wallpaper on.\n\
                Start Lucerna from inside a desktop session."
                .to_owned(),
            Self::X11ConnectFailed { display, reason } => format!(
                "Lucerna could not connect to the X11 display \"{display}\".\n{reason}\n\
                 This release currently supports X11 sessions. Check that DISPLAY is correct and the X server is running."
            ),
            Self::MissingFeature(what) => format!(
                "The X server lacks a feature Lucerna needs: {what}.\nUse a session with a modern X server (RandR 1.2 or newer)."
            ),
            Self::RuntimeDirUnavailable => "Lucerna could not find a private runtime directory.\n\
                XDG_RUNTIME_DIR is not set and /run/user/<uid> is not usable.\n\
                Log in through a normal desktop session."
                .to_owned(),
        }
    }
}

/// Classify the session from the environment. Wayland wins even when `DISPLAY` is set, because
/// XWayland would only give a window that is not the desktop (§50).
pub fn classify(env: &SessionEnv) -> SessionKind {
    match env
        .xdg_session_type
        .as_deref()
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("wayland") => return SessionKind::Wayland,
        Some("x11") => return SessionKind::X11,
        Some("tty") if env.display.is_none() && env.wayland_display.is_none() => {
            return SessionKind::Tty;
        }
        _ => {}
    }
    if env.xdg_session_type.is_none() && env.wayland_display.is_some() {
        return SessionKind::Wayland;
    }
    if env.display.is_some() {
        return SessionKind::X11;
    }
    if env.xdg_session_type.is_none() && env.wayland_display.is_none() {
        return SessionKind::Tty;
    }
    SessionKind::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(session: Option<&str>, wayland: Option<&str>, display: Option<&str>) -> SessionEnv {
        SessionEnv {
            xdg_session_type: session.map(str::to_owned),
            wayland_display: wayland.map(str::to_owned),
            display: display.map(str::to_owned),
            ..Default::default()
        }
    }

    #[test]
    fn classification_table() {
        let cases = [
            (env(Some("x11"), None, Some(":0")), SessionKind::X11),
            (env(Some("X11"), None, None), SessionKind::X11),
            (
                env(Some("wayland"), Some("wayland-0"), Some(":0")),
                SessionKind::Wayland,
            ),
            (env(Some("wayland"), None, Some(":1")), SessionKind::Wayland),
            (
                env(None, Some("wayland-0"), Some(":0")),
                SessionKind::Wayland,
            ),
            (env(None, None, Some(":0")), SessionKind::X11),
            (env(None, None, None), SessionKind::Tty),
            (env(Some("tty"), None, None), SessionKind::Tty),
            (env(Some("mir"), None, None), SessionKind::Unknown),
        ];
        for (input, want) in cases {
            assert_eq!(classify(&input), want, "{input:?}");
        }
    }

    #[test]
    fn lookup_ignores_empty_values() {
        let e = SessionEnv::from_lookup(|k| match k {
            "DISPLAY" => Some(OsString::from("")),
            "XDG_SESSION_TYPE" => Some(OsString::from("x11")),
            _ => None,
        });
        assert_eq!(e.display, None);
        assert_eq!(e.xdg_session_type.as_deref(), Some("x11"));
    }

    #[test]
    fn wayland_message_matches_the_spec_and_suggests_a_fix() {
        let text = UnsupportedReason::Wayland.message();
        assert!(text.contains("supports X11 sessions only"));
        assert!(text.contains("appears to be Wayland"));
        assert!(text.contains("Log in with a \"Cinnamon\" (X11) session"));
        assert_eq!(UnsupportedReason::Wayland.code(), "wayland");
    }

    #[test]
    fn every_reason_has_a_stable_code_and_a_helpful_message() {
        let reasons = [
            (UnsupportedReason::NoDisplay, "no-display"),
            (
                UnsupportedReason::X11ConnectFailed {
                    display: ":0".into(),
                    reason: "refused".into(),
                },
                "x11-connect-failed",
            ),
            (
                UnsupportedReason::MissingFeature("RandR".into()),
                "missing-feature",
            ),
            (
                UnsupportedReason::RuntimeDirUnavailable,
                "runtime-dir-unavailable",
            ),
        ];
        for (reason, code) in reasons {
            assert_eq!(reason.code(), code);
            assert!(reason.message().lines().count() >= 2, "{code}");
        }
    }
}
