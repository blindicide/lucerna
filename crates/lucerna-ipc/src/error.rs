//! `org.lucerna.Lucerna1.Error.*` (docs/IMPLEMENTATION-PLAN.md §7.5).

use zbus::DBusError;

/// Every error carries a complete user-facing message (§49): what happened, why, what to do.
#[derive(Debug, DBusError)]
#[zbus(prefix = "org.lucerna.Lucerna1.Error")]
pub enum LucernaError {
    #[zbus(error)]
    ZBus(zbus::Error),
    UnknownWallpaper(String),
    UnknownDisplay(String),
    InvalidArgument(String),
    InvalidPath(String),
    FileNotFound(String),
    NotAFile(String),
    ConfigReadOnly(String),
    ConfigWrite(String),
    ConfigInvalid(String),
    AutostartWrite(String),
    Unsupported(String),
    BackendUnavailable(String),
    MpvMissing(String),
    Internal(String),
}

/// `lucernactl` exit codes.
pub mod exit {
    pub const OK: u8 = 0;
    pub const INTERNAL: u8 = 1;
    /// Command-line usage error (from clap).
    pub const USAGE: u8 = 2;
    pub const DAEMON_NOT_RUNNING: u8 = 3;
    pub const BAD_REQUEST: u8 = 4;
    pub const UNSUPPORTED: u8 = 5;
    pub const CONFIG: u8 = 6;
    pub const MPV_MISSING: u8 = 7;
}

impl LucernaError {
    pub fn exit_code(&self) -> u8 {
        match self {
            Self::UnknownWallpaper(_)
            | Self::UnknownDisplay(_)
            | Self::InvalidArgument(_)
            | Self::InvalidPath(_)
            | Self::FileNotFound(_)
            | Self::NotAFile(_) => exit::BAD_REQUEST,
            Self::ConfigReadOnly(_)
            | Self::ConfigWrite(_)
            | Self::ConfigInvalid(_)
            | Self::AutostartWrite(_) => exit::CONFIG,
            Self::Unsupported(_) | Self::BackendUnavailable(_) => exit::UNSUPPORTED,
            Self::MpvMissing(_) => exit::MPV_MISSING,
            Self::Internal(_) => exit::INTERNAL,
            Self::ZBus(err) => exit_code_for(err),
        }
    }

    /// The user-facing message.
    pub fn message(&self) -> String {
        match self {
            Self::ZBus(err) => describe_transport_error(err),
            Self::UnknownWallpaper(m)
            | Self::UnknownDisplay(m)
            | Self::InvalidArgument(m)
            | Self::InvalidPath(m)
            | Self::FileNotFound(m)
            | Self::NotAFile(m)
            | Self::ConfigReadOnly(m)
            | Self::ConfigWrite(m)
            | Self::ConfigInvalid(m)
            | Self::AutostartWrite(m)
            | Self::Unsupported(m)
            | Self::BackendUnavailable(m)
            | Self::MpvMissing(m)
            | Self::Internal(m) => m.clone(),
        }
    }
}

/// True if the call failed because nobody owns the bus name (the daemon is not running).
pub fn is_daemon_absent(err: &zbus::Error) -> bool {
    match err {
        zbus::Error::MethodError(name, _, _) => {
            matches!(
                name.as_str(),
                "org.freedesktop.DBus.Error.ServiceUnknown"
                    | "org.freedesktop.DBus.Error.NameHasNoOwner"
            )
        }
        zbus::Error::NameTaken => false,
        zbus::Error::FDO(fdo) => matches!(
            **fdo,
            zbus::fdo::Error::ServiceUnknown(_) | zbus::fdo::Error::NameHasNoOwner(_)
        ),
        _ => false,
    }
}

/// Map any client-side D-Bus error to a `lucernactl` exit code.
pub fn exit_code_for(err: &zbus::Error) -> u8 {
    if is_daemon_absent(err) {
        return exit::DAEMON_NOT_RUNNING;
    }
    match LucernaError::from(clone_error(err)) {
        LucernaError::ZBus(_) => exit::INTERNAL,
        other => other.exit_code(),
    }
}

fn clone_error(err: &zbus::Error) -> zbus::Error {
    match err {
        zbus::Error::MethodError(name, detail, msg) => {
            zbus::Error::MethodError(name.clone(), detail.clone(), msg.clone())
        }
        other => zbus::Error::Failure(other.to_string()),
    }
}

/// A message for failures of the transport itself.
pub fn describe_transport_error(err: &zbus::Error) -> String {
    if is_daemon_absent(err) {
        "The Lucerna daemon is not running.\nStart it with `lucernad &` or open Lucerna.".to_owned()
    } else {
        format!("Unexpected daemon error: {err}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_codes_follow_the_table() {
        let cases: [(LucernaError, u8); 14] = [
            (LucernaError::UnknownWallpaper("x".into()), 4),
            (LucernaError::UnknownDisplay("x".into()), 4),
            (LucernaError::InvalidArgument("x".into()), 4),
            (LucernaError::InvalidPath("x".into()), 4),
            (LucernaError::FileNotFound("x".into()), 4),
            (LucernaError::NotAFile("x".into()), 4),
            (LucernaError::ConfigReadOnly("x".into()), 6),
            (LucernaError::ConfigWrite("x".into()), 6),
            (LucernaError::ConfigInvalid("x".into()), 6),
            (LucernaError::AutostartWrite("x".into()), 6),
            (LucernaError::Unsupported("x".into()), 5),
            (LucernaError::BackendUnavailable("x".into()), 5),
            (LucernaError::MpvMissing("x".into()), 7),
            (LucernaError::Internal("x".into()), 1),
        ];
        for (err, code) in cases {
            assert_eq!(err.exit_code(), code, "{err:?}");
        }
    }

    #[test]
    fn a_missing_service_is_exit_3() {
        let err = zbus::Error::MethodError(
            zbus::names::OwnedErrorName::try_from("org.freedesktop.DBus.Error.ServiceUnknown")
                .unwrap(),
            None,
            zbus::message::Message::method_call("/", "X")
                .unwrap()
                .build(&())
                .unwrap(),
        );
        assert!(is_daemon_absent(&err));
        assert_eq!(exit_code_for(&err), exit::DAEMON_NOT_RUNNING);
        assert!(describe_transport_error(&err).contains("not running"));
    }

    #[test]
    fn remote_lucerna_errors_map_through_their_names() {
        let name =
            zbus::names::OwnedErrorName::try_from("org.lucerna.Lucerna1.Error.UnknownDisplay")
                .unwrap();
        let msg = zbus::message::Message::method_call("/", "X")
            .unwrap()
            .build(&())
            .unwrap();
        let err = zbus::Error::MethodError(name, Some("No display matches 'HDMI-9'.".into()), msg);
        assert_eq!(exit_code_for(&err), exit::BAD_REQUEST);
        let lucerna = LucernaError::from(clone_error(&err));
        assert!(
            matches!(lucerna, LucernaError::UnknownDisplay(ref m) if m.contains("HDMI-9")),
            "{lucerna:?}"
        );
    }

    #[test]
    fn unrecognised_transport_errors_are_internal() {
        assert_eq!(
            exit_code_for(&zbus::Error::Failure("boom".into())),
            exit::INTERNAL
        );
    }
}
