//! The blocking D-Bus client and its error mapping (docs/IMPLEMENTATION-PLAN.md §7.5).

use lucerna_ipc::dto::DisplayDto;
use lucerna_ipc::error::{describe_transport_error, exit, is_daemon_absent};
use lucerna_ipc::{LucernaError, LucernaProxyBlocking, exit_code_for};

/// An error ready to print: a user-facing message and the exit code that goes with it.
#[derive(Debug)]
pub struct CliError {
    pub code: u8,
    pub message: String,
}

impl CliError {
    pub fn new(code: u8, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl From<zbus::Error> for CliError {
    fn from(err: zbus::Error) -> Self {
        let code = exit_code_for(&err);
        let message = if is_daemon_absent(&err) {
            describe_transport_error(&err)
        } else {
            LucernaError::from(err).message()
        };
        Self { code, message }
    }
}

pub struct Client {
    pub proxy: LucernaProxyBlocking<'static>,
}

impl Client {
    /// Connect to the session bus and build a proxy. Does not check that the daemon exists.
    pub fn connect() -> Result<Self, CliError> {
        let conn = zbus::blocking::Connection::session().map_err(|err| {
            CliError::new(
                exit::DAEMON_NOT_RUNNING,
                format!(
                    "Lucerna could not reach the session D-Bus ({err}).\n\
                     Run lucernactl from inside a desktop session."
                ),
            )
        })?;
        let proxy = LucernaProxyBlocking::new(&conn).map_err(CliError::from)?;
        Ok(Self { proxy })
    }
}

/// Resolve `--monitor` to a stable display id: an exact id, or a connector name that matches
/// exactly one connected display.
pub fn resolve_monitor(displays: &[DisplayDto], wanted: &str) -> Result<String, CliError> {
    if let Some(d) = displays.iter().find(|d| d.id == wanted) {
        return Ok(d.id.clone());
    }
    let by_connector: Vec<&DisplayDto> = displays
        .iter()
        .filter(|d| d.connected && d.connector == wanted)
        .collect();
    match by_connector.as_slice() {
        [one] => Ok(one.id.clone()),
        [] => {
            let available: Vec<String> = displays
                .iter()
                .filter(|d| d.connected)
                .map(|d| format!("{} ({})", d.connector, d.id))
                .collect();
            Err(CliError::new(
                exit::BAD_REQUEST,
                format!(
                    "No display matches '{wanted}'.\nRun `lucernactl monitors` to list displays. Connected: {}.",
                    if available.is_empty() {
                        "none".to_owned()
                    } else {
                        available.join(", ")
                    }
                ),
            ))
        }
        _ => Err(CliError::new(
            exit::BAD_REQUEST,
            format!(
                "'{wanted}' matches several displays.\nUse the full id shown by `lucernactl monitors`."
            ),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn display(id: &str, connector: &str, connected: bool) -> DisplayDto {
        DisplayDto {
            id: id.into(),
            connector: connector.into(),
            connected,
            ..DisplayDto::default()
        }
    }

    #[test]
    fn a_monitor_resolves_by_id_or_by_a_unique_connector() {
        let displays = vec![
            display("edid:A-1", "HDMI-1", true),
            display("edid:B-2", "DP-1", true),
        ];
        assert_eq!(resolve_monitor(&displays, "edid:B-2").unwrap(), "edid:B-2");
        assert_eq!(resolve_monitor(&displays, "HDMI-1").unwrap(), "edid:A-1");
    }

    #[test]
    fn unknown_and_ambiguous_monitors_are_bad_requests_with_help() {
        let displays = vec![
            display("a", "DP-1", true),
            display("b", "DP-1", true),
            display("c", "HDMI-1", false),
        ];
        let unknown = resolve_monitor(&displays, "HDMI-9").unwrap_err();
        assert_eq!(unknown.code, exit::BAD_REQUEST);
        assert!(
            unknown.message.contains("No display matches 'HDMI-9'")
                && unknown.message.contains("lucernactl monitors")
        );
        let ambiguous = resolve_monitor(&displays, "DP-1").unwrap_err();
        assert!(ambiguous.message.contains("several displays"));
        // A disconnected display cannot be selected by connector name.
        assert!(resolve_monitor(&displays, "HDMI-1").is_err());
        // ...but its id still works, so assignments to absent displays can be managed.
        assert_eq!(resolve_monitor(&displays, "c").unwrap(), "c");
    }

    #[test]
    fn daemon_absence_maps_to_exit_3_with_the_actionable_message() {
        let err = zbus::Error::MethodError(
            zbus::names::OwnedErrorName::try_from("org.freedesktop.DBus.Error.ServiceUnknown")
                .unwrap(),
            None,
            zbus::message::Message::method_call("/", "X")
                .unwrap()
                .build(&())
                .unwrap(),
        );
        let cli = CliError::from(err);
        assert_eq!(cli.code, exit::DAEMON_NOT_RUNNING);
        assert!(cli.message.contains("not running") && cli.message.contains("lucernad"));
    }

    #[test]
    fn remote_errors_keep_their_message_and_get_their_exit_code() {
        let err = zbus::Error::MethodError(
            zbus::names::OwnedErrorName::try_from("org.lucerna.Lucerna1.Error.MpvMissing").unwrap(),
            Some("Lucerna could not start mpv.".into()),
            zbus::message::Message::method_call("/", "X")
                .unwrap()
                .build(&())
                .unwrap(),
        );
        let cli = CliError::from(err);
        assert_eq!(cli.code, exit::MPV_MISSING);
        assert_eq!(cli.message, "Lucerna could not start mpv.");
    }
}
