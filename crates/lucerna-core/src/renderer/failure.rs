//! Why a renderer is in the `Failed` state, with user-facing wording (directive §49).

use std::fmt;

/// How a child process ended.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ExitInfo {
    /// Exit status, if the process exited normally.
    pub code: Option<i32>,
    /// Terminating signal, if it was killed.
    pub signal: Option<i32>,
}

impl ExitInfo {
    pub fn code(code: i32) -> Self {
        Self {
            code: Some(code),
            signal: None,
        }
    }

    pub fn signal(signal: i32) -> Self {
        Self {
            code: None,
            signal: Some(signal),
        }
    }
}

impl fmt::Display for ExitInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.code, self.signal) {
            (Some(code), _) => write!(f, "exit status {code}"),
            (None, Some(sig)) => write!(f, "signal {sig}"),
            (None, None) => f.write_str("unknown exit"),
        }
    }
}

/// Why a start attempt failed before or while spawning.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LaunchError {
    /// The mpv executable does not exist.
    NotFound,
    /// The wallpaper file is absent or is not a regular file.
    MediaMissing,
    Other(String),
}

/// Each variant has a stable kebab-case code used over D-Bus and in `doctor`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FailureReason {
    MpvMissing,
    LaunchFailed(String),
    MediaMissing,
    MediaUnsupported(String),
    Crashed(ExitInfo),
    UnexpectedExit(ExitInfo),
    StartupTimeout,
    IpcFailed(String),
    /// The bounded restart policy is exhausted; wraps the last cause.
    RestartLimit(Box<FailureReason>),
}

impl FailureReason {
    pub fn code(&self) -> &'static str {
        match self {
            Self::MpvMissing => "mpv-missing",
            Self::LaunchFailed(_) => "launch-failed",
            Self::MediaMissing => "media-missing",
            Self::MediaUnsupported(_) => "media-unsupported",
            Self::Crashed(_) => "crashed",
            Self::UnexpectedExit(_) => "unexpected-exit",
            Self::StartupTimeout => "startup-timeout",
            Self::IpcFailed(_) => "ipc-failed",
            Self::RestartLimit(_) => "restart-limit",
        }
    }

    /// The innermost cause (unwraps [`Self::RestartLimit`]).
    pub fn root_cause(&self) -> &FailureReason {
        match self {
            Self::RestartLimit(inner) => inner.root_cause(),
            other => other,
        }
    }

    /// A user-facing message: what happened, why, and what to do (§49).
    ///
    /// `stderr_tail` are the last lines mpv printed, appended for diagnosis when useful.
    pub fn user_message(&self, stderr_tail: &[String]) -> String {
        let tail = if stderr_tail.is_empty() {
            String::new()
        } else {
            format!("\nmpv said: {}", stderr_tail.join(" | "))
        };
        match self {
            Self::MpvMissing => "Lucerna could not start mpv.\n\
                Executable \"mpv\" was not found in PATH.\n\
                Install mpv (for example: sudo apt install mpv) and choose Reload."
                .to_owned(),
            Self::LaunchFailed(why) => format!(
                "Lucerna could not start mpv.\n{why}\n\
                 Run `lucernactl doctor` for details, then choose Reload.{tail}"
            ),
            Self::MediaMissing => "The wallpaper file is missing.\n\
                It may have been moved, deleted or be on a drive that is not connected.\n\
                Restore the file or pick another wallpaper; Lucerna will resume automatically \
                when the file returns."
                .to_owned(),
            Self::MediaUnsupported(why) => format!(
                "mpv could not play this wallpaper file.\n{why}\n\
                 Check that the file is a valid video or animated image, or pick another \
                 wallpaper.{tail}"
            ),
            Self::Crashed(exit) => format!(
                "The mpv renderer crashed ({exit}).\n\
                 Lucerna restarts it a limited number of times; if it keeps crashing, try \
                 setting Hardware decoding to Disabled.{tail}"
            ),
            Self::UnexpectedExit(exit) => format!(
                "The mpv renderer stopped on its own ({exit}) although the wallpaper should \
                 loop forever.\nLucerna restarts it a limited number of times.{tail}"
            ),
            Self::StartupTimeout => format!(
                "mpv did not finish loading the wallpaper in time.\n\
                 The file may be on a slow drive or too heavy to open; try another file or \
                 disable hardware decoding.{tail}"
            ),
            Self::IpcFailed(why) => format!(
                "Lucerna lost contact with the mpv renderer ({why}).\n\
                 Lucerna restarts it a limited number of times.{tail}"
            ),
            Self::RestartLimit(inner) => format!(
                "{}\nLucerna stopped restarting this renderer after repeated failures.\n\
                 Fix the cause, then choose Reload (or run `lucernactl reload`) to try again.",
                inner.user_message(stderr_tail)
            ),
        }
    }
}

impl fmt::Display for FailureReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RestartLimit(inner) => write!(f, "restart-limit after {inner}"),
            Self::LaunchFailed(why) | Self::MediaUnsupported(why) | Self::IpcFailed(why) => {
                write!(f, "{}: {why}", self.code())
            }
            Self::Crashed(exit) | Self::UnexpectedExit(exit) => {
                write!(f, "{} ({exit})", self.code())
            }
            other => f.write_str(other.code()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_stable_kebab_case() {
        let cases = [
            (FailureReason::MpvMissing, "mpv-missing"),
            (FailureReason::LaunchFailed("x".into()), "launch-failed"),
            (FailureReason::MediaMissing, "media-missing"),
            (
                FailureReason::MediaUnsupported("x".into()),
                "media-unsupported",
            ),
            (FailureReason::Crashed(ExitInfo::signal(6)), "crashed"),
            (
                FailureReason::UnexpectedExit(ExitInfo::code(0)),
                "unexpected-exit",
            ),
            (FailureReason::StartupTimeout, "startup-timeout"),
            (FailureReason::IpcFailed("x".into()), "ipc-failed"),
            (
                FailureReason::RestartLimit(Box::new(FailureReason::StartupTimeout)),
                "restart-limit",
            ),
        ];
        for (reason, code) in cases {
            assert_eq!(reason.code(), code);
        }
    }

    #[test]
    fn root_cause_unwraps_restart_limit() {
        let r = FailureReason::RestartLimit(Box::new(FailureReason::Crashed(ExitInfo::signal(6))));
        assert_eq!(r.root_cause(), &FailureReason::Crashed(ExitInfo::signal(6)));
    }

    #[test]
    fn mpv_missing_message_matches_the_spec_wording() {
        let text = FailureReason::MpvMissing.user_message(&[]);
        assert!(text.starts_with("Lucerna could not start mpv."));
        assert!(text.contains("Executable \"mpv\" was not found in PATH."));
        assert!(text.contains("Install mpv"));
    }

    #[test]
    fn every_message_says_what_to_do_and_includes_the_stderr_tail() {
        let tail = vec!["boom".to_owned()];
        for reason in [
            FailureReason::LaunchFailed("x".into()),
            FailureReason::MediaUnsupported("x".into()),
            FailureReason::Crashed(ExitInfo::signal(11)),
            FailureReason::UnexpectedExit(ExitInfo::code(0)),
            FailureReason::StartupTimeout,
            FailureReason::IpcFailed("closed".into()),
        ] {
            let text = reason.user_message(&tail);
            assert!(text.contains("boom"), "{reason}: {text}");
            assert!(text.lines().count() >= 2, "{reason}");
        }
        let limit =
            FailureReason::RestartLimit(Box::new(FailureReason::Crashed(ExitInfo::signal(6))));
        assert!(limit.user_message(&[]).contains("Reload"));
    }
}
