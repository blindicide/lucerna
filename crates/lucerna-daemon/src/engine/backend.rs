//! mpv discovery, backend selection and backend events.

use std::time::{Duration, Instant as StdInstant};

use lucerna_core::backend::{BackendError, BackendEvent, WallpaperBackend};
use lucerna_core::config::StackingSetting;
use lucerna_core::geometry::occluded_outputs;
use lucerna_core::mpv::{check_compat, discover};
use lucerna_core::session::{SessionEnv, SessionKind, UnsupportedReason, classify};
use lucerna_x11::{StackingMode, X11Options};

use super::{BackendStatus, Engine, Exit, MpvState};
use crate::messages::EngineMsg;

/// At most this many `refresh()` calls per window before the fight-guard slows them down.
const RESTACK_BURST: usize = 5;
const RESTACK_WINDOW: Duration = Duration::from_secs(10);

impl Engine {
    /// Locate mpv and check that it accepts our options (§4.7).
    pub(crate) async fn discover_mpv(&mut self) {
        let mpv_override = self.opts.mpv_override.clone();
        let path_var = self.opts.path_var.clone();
        let result = tokio::task::spawn_blocking(move || {
            discover(mpv_override.as_deref(), path_var.as_deref()).map(|info| {
                let compat = check_compat(&info.path).ok();
                (info, compat)
            })
        })
        .await;
        self.mpv = match result {
            Ok(Ok((info, compat))) => {
                tracing::info!(path = %info.path.display(), version = %info.version, "mpv found");
                if let Some(report) = compat.as_ref().filter(|c| !c.compatible) {
                    tracing::warn!(detail = %report.detail, "the installed mpv rejected Lucerna's options");
                }
                MpvState {
                    info: Some(info),
                    compat,
                    error: None,
                }
            }
            Ok(Err(err)) => {
                tracing::warn!(%err, "mpv is unavailable");
                MpvState {
                    info: None,
                    compat: None,
                    error: Some(err.to_string()),
                }
            }
            Err(err) => MpvState {
                info: None,
                compat: None,
                error: Some(format!("mpv discovery failed: {err}")),
            },
        };
    }

    fn event_sink(&self) -> lucerna_core::backend::EventSink {
        let tx = self.tx.clone();
        Box::new(move |event| {
            let _ = tx.send(EngineMsg::Backend(event));
        })
    }

    fn x11_stacking(&self) -> StackingMode {
        match self.loaded.config.x11.stacking {
            StackingSetting::Auto => StackingMode::Auto,
            StackingSetting::OverrideRedirect => StackingMode::OverrideRedirect,
            StackingSetting::DesktopWindow => StackingMode::DesktopWindow,
        }
    }

    /// Choose and connect the backend (§5.1). Unsupported sessions get no backend, no mpv and no
    /// retry loop; the daemon keeps serving D-Bus so the GUI, CLI and `doctor` can explain why.
    pub(crate) async fn init_backend(&mut self) {
        self.outputs.clear();
        self.occluded.clear();
        self.session_kind = classify(&self.opts.session);

        if self.opts.paths.runtime_dir.is_none() {
            self.backend_status =
                BackendStatus::Unavailable(UnsupportedReason::RuntimeDirUnavailable);
            return;
        }

        if !self.backend_injected {
            self.backend = None;
            match self.connect_auto().await {
                Ok(backend) => self.backend = Some(backend),
                Err(reason) => {
                    tracing::warn!(
                        reason = reason.code(),
                        "no wallpaper backend: {}",
                        reason.message().replace('\n', " ")
                    );
                    self.backend_status = BackendStatus::Unavailable(reason);
                    return;
                }
            }
        }

        let sink = self.event_sink();
        let Some(backend) = self.backend.as_mut() else {
            return;
        };
        let probed = backend.probe().and_then(|probe| {
            backend.subscribe(sink)?;
            Ok(probe)
        });
        match probed {
            Ok(probe) => {
                tracing::info!(backend = probe.kind.as_str(), "backend selected");
                self.backend_status = BackendStatus::Available {
                    kind: probe.kind,
                    capabilities: probe.capabilities,
                };
                self.refresh_outputs();
            }
            Err(err) => {
                self.backend = None;
                self.backend_status = BackendStatus::Unavailable(unsupported_from(&err, "unknown"));
            }
        }
    }

    /// Wait for `DISPLAY` and the X server (login race, §22), then connect.
    async fn connect_auto(&mut self) -> Result<Box<dyn WallpaperBackend>, UnsupportedReason> {
        if self.session_kind == SessionKind::Wayland {
            return Err(UnsupportedReason::Wayland);
        }
        let wait = self.opts.display_wait;
        let deadline = StdInstant::now() + wait;
        let mut session = self.opts.session.clone();
        while session.display.is_none() && StdInstant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(200)).await;
            session = SessionEnv {
                display: std::env::var("DISPLAY").ok().filter(|d| !d.is_empty()),
                ..session
            };
        }
        let Some(display) = session.display.clone() else {
            return Err(UnsupportedReason::NoDisplay);
        };
        self.opts.session = session.clone();
        self.session_kind = classify(&session);

        let options = X11Options {
            display: Some(display.clone()),
            current_desktop: session.xdg_current_desktop.clone(),
            stacking: self.x11_stacking(),
        };
        let result = tokio::task::spawn_blocking(move || {
            loop {
                match lucerna_x11::connect(&options) {
                    Ok(backend) => return Ok(backend),
                    Err(BackendError::MissingFeature(what)) => {
                        return Err(UnsupportedReason::MissingFeature(what.to_owned()));
                    }
                    Err(err) => {
                        if StdInstant::now() >= deadline {
                            return Err(unsupported_from(&err, &display));
                        }
                        std::thread::sleep(Duration::from_millis(500));
                    }
                }
            }
        })
        .await;
        match result {
            Ok(Ok(backend)) => Ok(Box::new(backend)),
            Ok(Err(reason)) => Err(reason),
            Err(err) => Err(UnsupportedReason::X11ConnectFailed {
                display: String::new(),
                reason: err.to_string(),
            }),
        }
    }

    /// Re-read the outputs from the backend.
    pub(crate) fn refresh_outputs(&mut self) {
        let Some(backend) = self.backend.as_mut() else {
            self.outputs.clear();
            return;
        };
        match backend.enumerate_outputs() {
            Ok(outputs) => {
                for o in &outputs {
                    tracing::info!(id = %o.id, connector = %o.connector, geometry = ?o.geometry, "display detected");
                }
                self.outputs = outputs;
                self.remember_display_labels();
            }
            Err(err) => tracing::warn!(%err, "could not enumerate displays"),
        }
    }

    pub(crate) async fn on_backend_event(&mut self, event: BackendEvent) -> Option<Exit> {
        match event {
            BackendEvent::OutputsChanged => {
                tracing::info!("display configuration changed");
                self.refresh_outputs();
                self.reconcile().await;
                self.emit_displays_changed().await;
                self.mark_dirty();
            }
            BackendEvent::FullscreenChanged(rects) => {
                let outputs: Vec<_> = self
                    .outputs
                    .iter()
                    .map(|o| (o.id.clone(), o.geometry))
                    .collect();
                let occluded = occluded_outputs(&outputs, &rects);
                if occluded != self.occluded {
                    self.occluded = occluded;
                    self.reconcile().await;
                    self.mark_dirty();
                }
            }
            BackendEvent::StackingDisturbed => self.restack(),
            BackendEvent::ConnectionLost(why) => return Some(Exit::ConnectionLost(why)),
            // `BackendEvent` is non-exhaustive: ignore events this build does not know.
            _ => {}
        }
        None
    }

    /// Re-assert stacking, but never fight the window manager: at most 5 calls per 10 s, then one
    /// per 10 s with a single warning (§5.2).
    fn restack(&mut self) {
        let now = tokio::time::Instant::now();
        while self
            .restack_times
            .front()
            .is_some_and(|t| now.duration_since(*t) > RESTACK_WINDOW)
        {
            self.restack_times.pop_front();
        }
        if self.restack_times.len() >= RESTACK_BURST {
            if self.restack_fights == 0 {
                tracing::warn!("another client keeps restacking the desktop layer; slowing down");
            }
            self.restack_fights += 1;
            return;
        }
        self.restack_times.push_back(now);
        if let Some(backend) = self.backend.as_mut()
            && let Err(err) = backend.refresh()
        {
            tracing::warn!(%err, "could not re-assert the wallpaper stacking");
        }
    }

    /// Remember a human-readable label for every configured display that is present, so an absent
    /// display can still be shown meaningfully later.
    fn remember_display_labels(&mut self) {
        let mut changed = false;
        for output in &self.outputs {
            if let Some(display) = self.loaded.config.displays.get_mut(&output.id) {
                let label = output.label();
                if display.last_seen.as_deref() != Some(label.as_str()) {
                    display.last_seen = Some(label);
                    changed = true;
                }
            }
        }
        if changed && !self.config_read_only() {
            let path = self.opts.paths.config_file();
            if let Err(err) = lucerna_core::config::save(&mut self.loaded, &path) {
                tracing::debug!(%err, "could not save display labels");
            }
        }
    }
}

fn unsupported_from(err: &BackendError, display: &str) -> UnsupportedReason {
    match err {
        BackendError::MissingFeature(what) => UnsupportedReason::MissingFeature((*what).to_owned()),
        BackendError::Connect { display, reason } => UnsupportedReason::X11ConnectFailed {
            display: display.clone(),
            reason: reason.clone(),
        },
        other => UnsupportedReason::X11ConnectFailed {
            display: display.to_owned(),
            reason: other.to_string(),
        },
    }
}
