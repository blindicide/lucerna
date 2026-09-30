//! Building the payloads clients see (status, displays, wallpapers, settings) and emitting signals.

use lucerna_core::autostart;
use lucerna_core::backend::OutputId;
use lucerna_core::config::CURRENT_SCHEMA;
use lucerna_core::renderer::{FailureReason, RendererSnapshot};
use lucerna_core::version::VERSION;
use lucerna_ipc::dto::{
    AssignmentDto, DisplayDto, RendererDto, SettingsDto, StatusDto, WallpaperDto,
};
use lucerna_ipc::names::{ALL_DISPLAYS, API_VERSION, OBJECT_PATH};
use zbus::object_server::SignalEmitter;

use super::{BackendStatus, Engine, Service};

impl Engine {
    fn renderer_dtos(&self) -> Vec<RendererDto> {
        let (_, blocked) = self.compute_desired();
        let config = &self.loaded.config;
        let mut out = Vec::new();
        for output in &self.outputs {
            let Some(wallpaper) = config.effective_wallpaper(&output.id).0 else {
                continue;
            };
            let base = RendererDto {
                display_id: output.id.as_str().to_owned(),
                connector: output.connector.clone(),
                wallpaper_id: wallpaper.as_str().to_owned(),
                scaling: config.effective_scaling(&output.id).as_str().to_owned(),
                pause_reasons: self
                    .pause_reasons(&output.id)
                    .names()
                    .into_iter()
                    .map(str::to_owned)
                    .collect(),
                ..RendererDto::default()
            };
            if let Some(slot) = self.slots.get(&output.id) {
                out.push(RendererDto {
                    state: slot.snapshot.state.as_str().to_owned(),
                    failure_code: slot.snapshot.failure_code.clone(),
                    failure_message: slot.snapshot.failure_message.clone(),
                    restarts_in_window: slot.snapshot.restarts_in_window,
                    pid: slot.snapshot.pid,
                    ..base
                });
            } else if let Some(err) = self.surface_errors.get(&output.id) {
                let reason = FailureReason::LaunchFailed(err.clone());
                out.push(failed(base, &reason));
            } else if let Some(b) = blocked.iter().find(|b| b.output == output.id) {
                out.push(failed(base, &b.reason));
            }
        }
        out
    }

    pub(crate) fn status_dto(&self) -> StatusDto {
        let (backend, supported, reason, message, fullscreen) = match &self.backend_status {
            BackendStatus::Available { kind, capabilities } => (
                kind.as_str().to_owned(),
                true,
                String::new(),
                String::new(),
                capabilities.fullscreen_detection,
            ),
            BackendStatus::Unavailable(reason) => (
                "none".to_owned(),
                false,
                reason.code().to_owned(),
                reason.message(),
                false,
            ),
        };
        let renderers = self.renderer_dtos();
        StatusDto {
            daemon_version: VERSION.to_owned(),
            api_version: API_VERSION,
            backend,
            session_type: self.session_kind.as_str().to_owned(),
            supported,
            unsupported_reason: reason,
            unsupported_message: message,
            playback: self.playback(&renderers, supported),
            user_paused: self.user_paused,
            user_stopped: self.user_stopped,
            session_locked: self.session_locked,
            lock_detection: self.lock_detection,
            fullscreen_detection: fullscreen,
            mpv_available: self.mpv.info.is_some(),
            mpv_version: self
                .mpv
                .info
                .as_ref()
                .map(|i| i.version.clone())
                .unwrap_or_default(),
            schema_version: CURRENT_SCHEMA,
            config_state: self.loaded.state.code().to_owned(),
            config_notice: self.loaded.notice.clone().unwrap_or_default(),
            config_warnings: self.loaded.warnings.clone(),
            renderers,
        }
    }

    /// One word for the whole daemon: `playing`, `paused`, `stopped`, `idle` or `failed`.
    fn playback(&self, renderers: &[RendererDto], supported: bool) -> String {
        if !supported {
            return "idle".to_owned();
        }
        if self.user_stopped {
            return "stopped".to_owned();
        }
        let any = |state: &str| renderers.iter().any(|r| r.state == state);
        let word = if any("playing") || any("starting") {
            "playing"
        } else if any("paused") {
            "paused"
        } else if any("failed") {
            "failed"
        } else {
            "idle"
        };
        word.to_owned()
    }

    pub(crate) fn displays(&self, include_serial: bool) -> Vec<DisplayDto> {
        let config = &self.loaded.config;
        let mut out: Vec<DisplayDto> = self
            .outputs
            .iter()
            .map(|o| {
                let (wallpaper, source) = config.effective_wallpaper(&o.id);
                DisplayDto {
                    id: o.id.as_str().to_owned(),
                    connector: o.connector.clone(),
                    label: o.label(),
                    connected: true,
                    primary: o.primary,
                    x: o.geometry.x,
                    y: o.geometry.y,
                    width: o.geometry.width,
                    height: o.geometry.height,
                    rotation: o.rotation.as_str().to_owned(),
                    edid_manufacturer: o
                        .edid
                        .as_ref()
                        .map(|e| e.manufacturer.clone())
                        .unwrap_or_default(),
                    edid_model: o
                        .edid
                        .as_ref()
                        .and_then(|e| e.model_name.clone())
                        .unwrap_or_default(),
                    edid_serial: if include_serial {
                        o.edid
                            .as_ref()
                            .and_then(|e| e.serial.clone())
                            .unwrap_or_default()
                    } else {
                        String::new()
                    },
                    wallpaper_id: wallpaper.map(|w| w.as_str().to_owned()).unwrap_or_default(),
                    wallpaper_source: source.as_str().to_owned(),
                    scaling: config.effective_scaling(&o.id).as_str().to_owned(),
                    scaling_source: scaling_source(config, &o.id),
                }
            })
            .collect();
        // Configured displays that are not connected right now keep their assignment (§12).
        for (id, cfg) in &config.displays {
            if self.outputs.iter().any(|o| &o.id == id) {
                continue;
            }
            let (wallpaper, source) = config.effective_wallpaper(id);
            let label = cfg
                .last_seen
                .clone()
                .unwrap_or_else(|| id.as_str().to_owned());
            out.push(DisplayDto {
                id: id.as_str().to_owned(),
                connector: label.split(" — ").next().unwrap_or_default().to_owned(),
                label: format!("{label} (disconnected)"),
                connected: false,
                wallpaper_id: wallpaper.map(|w| w.as_str().to_owned()).unwrap_or_default(),
                wallpaper_source: source.as_str().to_owned(),
                scaling: config.effective_scaling(id).as_str().to_owned(),
                scaling_source: scaling_source(config, id),
                ..DisplayDto::default()
            });
        }
        out
    }

    pub(crate) fn assignments(&self) -> Vec<AssignmentDto> {
        let config = &self.loaded.config;
        let mut out = vec![AssignmentDto {
            display_id: ALL_DISPLAYS.to_owned(),
            wallpaper_id: config
                .all_displays
                .wallpaper
                .as_ref()
                .map(|w| w.as_str().to_owned())
                .unwrap_or_default(),
            scaling: config.all_displays.scaling.as_str().to_owned(),
            last_seen: String::new(),
        }];
        for (id, cfg) in &config.displays {
            out.push(AssignmentDto {
                display_id: id.as_str().to_owned(),
                wallpaper_id: cfg
                    .wallpaper
                    .as_ref()
                    .map(|w| w.as_str().to_owned())
                    .unwrap_or_default(),
                scaling: config.effective_scaling(id).as_str().to_owned(),
                last_seen: cfg.last_seen.clone().unwrap_or_default(),
            });
        }
        out
    }

    pub(crate) fn wallpapers(&self) -> Vec<WallpaperDto> {
        self.loaded
            .config
            .wallpapers
            .iter()
            .map(|w| WallpaperDto {
                id: w.id.as_str().to_owned(),
                name: w.name.clone(),
                path: w.path.to_string_lossy().into_owned(),
                media_type: w.media_type.as_str().to_owned(),
                available: w.available,
                added: w.added.clone(),
                ..WallpaperDto::default()
            })
            .collect()
    }

    pub(crate) fn autostart_enabled(&self) -> bool {
        autostart::state(&self.opts.paths.autostart_file()).is_enabled()
    }

    pub(crate) fn settings_dto(&self) -> SettingsDto {
        let c = &self.loaded.config;
        SettingsDto {
            pause_on_fullscreen: c.general.pause_on_fullscreen,
            pause_on_lock: c.general.pause_on_lock,
            audio: c.general.audio,
            hardware_decode: c.general.hardware_decode.as_str().to_owned(),
            fps_limit: c.general.fps_limit.as_str().to_owned(),
            stacking: c.x11.stacking.as_str().to_owned(),
            max_restarts: c.renderer.max_restarts,
            restart_window_secs: u32::try_from(c.renderer.restart_window_secs).unwrap_or(u32::MAX),
            autostart: self.autostart_enabled(),
        }
    }

    fn emitter(&self) -> Option<SignalEmitter<'static>> {
        SignalEmitter::new(&self.conn, OBJECT_PATH).ok()
    }

    pub(crate) async fn emit_status(&self) {
        if let Some(e) = self.emitter()
            && let Err(err) = Service::status_changed(&e, self.status_dto().to_dict()).await
        {
            tracing::debug!(%err, "could not emit StatusChanged");
        }
    }

    pub(crate) async fn emit_displays_changed(&self) {
        if let Some(e) = self.emitter() {
            let _ = Service::displays_changed(&e).await;
        }
    }

    pub(crate) async fn emit_library_changed(&self) {
        if let Some(e) = self.emitter() {
            let _ = Service::library_changed(&e).await;
        }
    }

    pub(crate) async fn emit_settings_changed(&self) {
        if let Some(e) = self.emitter() {
            let _ = Service::settings_changed(&e).await;
        }
    }

    pub(crate) async fn emit_renderer_failed(
        &self,
        output: &OutputId,
        snapshot: &RendererSnapshot,
    ) {
        if let Some(e) = self.emitter() {
            let _ = Service::renderer_failed(
                &e,
                output.as_str(),
                &snapshot.failure_code,
                &snapshot.failure_message,
            )
            .await;
        }
    }
}

fn scaling_source(config: &lucerna_core::config::Config, id: &OutputId) -> String {
    if config.displays.get(id).is_some_and(|d| d.scaling.is_some()) {
        "display"
    } else {
        "all"
    }
    .to_owned()
}

fn failed(base: RendererDto, reason: &FailureReason) -> RendererDto {
    RendererDto {
        state: "failed".to_owned(),
        failure_code: reason.code().to_owned(),
        failure_message: reason.user_message(&[]),
        ..base
    }
}
