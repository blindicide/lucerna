//! The banner across the top of the window: daemon state, problems and their actions.

use lucerna_ipc::dto::StatusDto;

use crate::strings;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BannerKind {
    Info,
    Warning,
    Error,
}

/// What the banner's button does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BannerAction {
    /// Launch `lucernad`.
    StartService,
    /// Ask the daemon to reload (after fixing a problem).
    Reload,
    /// Hide a transient error.
    Dismiss,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Banner {
    pub kind: BannerKind,
    pub text: String,
    pub action: Option<(BannerAction, String)>,
}

/// Connection state of the GUI to the daemon.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Link {
    /// No daemon owns the bus name (and we are not currently starting one).
    NotRunning,
    /// The GUI is starting the service.
    Starting,
    Connected,
}

/// Decide what to say. Highest priority first: a transient error, the daemon missing, an
/// unsupported session, missing mpv, a configuration problem, a failed renderer.
pub fn banner(
    link: Link,
    status: Option<&StatusDto>,
    transient_error: Option<&str>,
) -> Option<Banner> {
    if let Some(error) = transient_error {
        return Some(Banner {
            kind: BannerKind::Error,
            text: error.to_owned(),
            action: Some((BannerAction::Dismiss, strings::DISMISS.to_owned())),
        });
    }
    match link {
        Link::NotRunning => {
            return Some(Banner {
                kind: BannerKind::Warning,
                text: strings::SERVICE_NOT_RUNNING.to_owned(),
                action: Some((
                    BannerAction::StartService,
                    strings::START_SERVICE.to_owned(),
                )),
            });
        }
        Link::Starting => {
            return Some(Banner {
                kind: BannerKind::Info,
                text: strings::SERVICE_STARTING.to_owned(),
                action: None,
            });
        }
        Link::Connected => {}
    }
    let status = status?;
    if !status.supported {
        return Some(Banner {
            kind: BannerKind::Error,
            text: status.unsupported_message.clone(),
            action: Some((BannerAction::Reload, strings::RELOAD.to_owned())),
        });
    }
    if !status.mpv_available {
        return Some(Banner {
            kind: BannerKind::Error,
            text: strings::MPV_MISSING.to_owned(),
            action: Some((BannerAction::Reload, strings::RELOAD.to_owned())),
        });
    }
    if status.config_state != "ok" || !status.config_notice.is_empty() {
        let kind = if status.config_state == "ok" {
            BannerKind::Info
        } else {
            BannerKind::Warning
        };
        let text = if status.config_notice.is_empty() {
            strings::config_state(&status.config_state)
        } else {
            status.config_notice.clone()
        };
        return Some(Banner {
            kind,
            text,
            action: None,
        });
    }
    if let Some(failed) = status.renderers.iter().find(|r| r.state == "failed") {
        return Some(Banner {
            kind: BannerKind::Error,
            text: format!("{}: {}", failed.connector, failed.failure_message),
            action: Some((BannerAction::Reload, strings::RETRY.to_owned())),
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use lucerna_ipc::dto::RendererDto;

    fn healthy() -> StatusDto {
        StatusDto {
            supported: true,
            mpv_available: true,
            config_state: "ok".into(),
            ..StatusDto::default()
        }
    }

    #[test]
    fn a_healthy_daemon_shows_no_banner() {
        assert_eq!(banner(Link::Connected, Some(&healthy()), None), None);
        assert_eq!(
            banner(Link::Connected, None, None),
            None,
            "nothing known yet"
        );
    }

    #[test]
    fn a_missing_service_offers_to_start_it() {
        let b = banner(Link::NotRunning, None, None).unwrap();
        assert_eq!(b.text, strings::SERVICE_NOT_RUNNING);
        assert_eq!(b.action.unwrap().0, BannerAction::StartService);
        assert_eq!(banner(Link::Starting, None, None).unwrap().action, None);
    }

    #[test]
    fn transient_errors_win_over_everything_and_can_be_dismissed() {
        let b = banner(Link::NotRunning, None, Some("Could not add the file.")).unwrap();
        assert_eq!(b.kind, BannerKind::Error);
        assert_eq!(b.text, "Could not add the file.");
        assert_eq!(b.action.unwrap().0, BannerAction::Dismiss);
    }

    #[test]
    fn problems_are_reported_in_priority_order() {
        let mut s = healthy();
        s.renderers = vec![RendererDto {
            connector: "HDMI-1".into(),
            state: "failed".into(),
            failure_message: "The wallpaper file is missing.".into(),
            ..RendererDto::default()
        }];
        s.config_state = "defaults-after-corruption".into();
        s.config_notice = "The configuration file could not be parsed.".into();
        s.mpv_available = false;
        s.supported = false;
        s.unsupported_message = "This release supports X11 sessions only.".into();

        assert_eq!(
            banner(Link::Connected, Some(&s), None).unwrap().text,
            "This release supports X11 sessions only."
        );
        s.supported = true;
        assert_eq!(
            banner(Link::Connected, Some(&s), None).unwrap().text,
            strings::MPV_MISSING
        );
        s.mpv_available = true;
        let config = banner(Link::Connected, Some(&s), None).unwrap();
        assert_eq!(config.kind, BannerKind::Warning);
        assert!(config.text.contains("could not be parsed"));
        s.config_state = "ok".into();
        s.config_notice.clear();
        let failed = banner(Link::Connected, Some(&s), None).unwrap();
        assert_eq!(failed.text, "HDMI-1: The wallpaper file is missing.");
        assert_eq!(failed.action.unwrap().0, BannerAction::Reload);
    }

    #[test]
    fn a_read_only_configuration_is_explained_even_without_a_notice() {
        let mut s = healthy();
        s.config_state = "read-only-newer-schema".into();
        let b = banner(Link::Connected, Some(&s), None).unwrap();
        assert!(b.text.contains("newer version"), "{}", b.text);
    }
}
