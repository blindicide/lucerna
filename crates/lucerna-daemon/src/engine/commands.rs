//! The D-Bus methods, one by one (docs/IMPLEMENTATION-PLAN.md §7.2).

use lucerna_core::autostart;
use lucerna_core::backend::OutputId;
use lucerna_core::config::{Config, ConfigError, StackingSetting, WallpaperId};
use lucerna_core::library::LibraryError;
use lucerna_core::timeutil::now_rfc3339;
use lucerna_core::types::{FpsLimit, HwDecode, ScalingMode};
use lucerna_ipc::LucernaError;
use lucerna_ipc::dto::SettingsPatch;
use lucerna_ipc::names::ALL_DISPLAYS;

use super::{BackendStatus, Engine};
use crate::messages::{Cmd, Reply, ReplyTx};

type Outcome = Result<Reply, LucernaError>;

fn config_error(err: &ConfigError) -> LucernaError {
    match err {
        ConfigError::ReadOnlyNewer(_) | ConfigError::ReadOnlyUnreadable(_) => {
            LucernaError::ConfigReadOnly(err.to_string())
        }
        ConfigError::Write { .. } => LucernaError::ConfigWrite(err.to_string()),
        ConfigError::Migration(_) => LucernaError::ConfigInvalid(err.to_string()),
    }
}

fn library_error(err: &LibraryError) -> LucernaError {
    match err {
        LibraryError::Relative(_) | LibraryError::NotUtf8(_) => {
            LucernaError::InvalidPath(err.to_string())
        }
        LibraryError::NotFound(_) => LucernaError::FileNotFound(err.to_string()),
        LibraryError::NotAFile(_) => LucernaError::NotAFile(err.to_string()),
        LibraryError::Unknown(_) => LucernaError::UnknownWallpaper(err.to_string()),
        LibraryError::Io { .. } => LucernaError::Internal(err.to_string()),
    }
}

impl Engine {
    pub(crate) async fn handle_command(&mut self, cmd: Cmd, reply: ReplyTx) {
        let outcome = self.execute(cmd).await;
        let quit = matches!(outcome, Ok(Reply::Unit)) && self.quitting_requested();
        let _ = reply.send(outcome);
        if quit {
            self.quitting = true;
        }
    }

    fn quitting_requested(&self) -> bool {
        self.quit_flag
    }

    async fn execute(&mut self, cmd: Cmd) -> Outcome {
        match cmd {
            Cmd::GetStatus => Ok(Reply::Dict(self.status_dto().to_dict())),
            Cmd::GetDisplays => Ok(Reply::Dicts(
                self.displays(false).iter().map(|d| d.to_dict()).collect(),
            )),
            Cmd::GetAssignments => Ok(Reply::Dicts(
                self.assignments().iter().map(|a| a.to_dict()).collect(),
            )),
            Cmd::ListWallpapers => {
                self.sync_availability();
                Ok(Reply::Dicts(
                    self.wallpapers().iter().map(|w| w.to_dict()).collect(),
                ))
            }
            Cmd::AddWallpaper { path, name } => self.add_wallpaper(&path, &name).await,
            Cmd::RemoveWallpaper { id } => self.remove_wallpaper(&id).await,
            Cmd::SetWallpaper {
                wallpaper_id,
                display_id,
            } => self.set_wallpaper(&wallpaper_id, &display_id).await,
            Cmd::ClearAssignment { display_id } => self.clear_assignment(&display_id).await,
            Cmd::SetScaling { display_id, mode } => self.set_scaling(&display_id, &mode).await,
            Cmd::GetSettings => Ok(Reply::Dict(self.settings_dto().to_dict())),
            Cmd::SetSettings { patch } => self.set_settings(&patch).await,
            Cmd::Pause => {
                self.user_paused = true;
                self.reconcile().await;
                Ok(Reply::Unit)
            }
            Cmd::Resume => {
                self.user_paused = false;
                self.reconcile().await;
                Ok(Reply::Unit)
            }
            Cmd::Stop => {
                tracing::info!("stopping all wallpapers (user request)");
                self.user_stopped = true;
                self.stop_all_slots().await;
                self.reconcile().await;
                Ok(Reply::Unit)
            }
            Cmd::Start => self.start().await,
            Cmd::Reload => self.reload().await,
            Cmd::Quit => {
                self.quit_flag = true;
                Ok(Reply::Unit)
            }
            Cmd::GetDiagnostics { redact } => Ok(Reply::Text(self.diagnostics_json(redact))),
        }
    }

    // ------------------------------------------------------------------------------ config

    /// Apply `change` to a copy of the configuration, save it, and adopt it only if saving worked.
    fn mutate<F>(&mut self, change: F) -> Result<(), LucernaError>
    where
        F: FnOnce(&mut Config) -> Result<(), LucernaError>,
    {
        if let Some(reason) = read_only_reason(&self.loaded.state) {
            return Err(LucernaError::ConfigReadOnly(reason));
        }
        let before = self.loaded.config.clone();
        if let Err(err) = change(&mut self.loaded.config) {
            self.loaded.config = before;
            return Err(err);
        }
        let path = self.opts.paths.config_file();
        if let Err(err) = lucerna_core::config::save(&mut self.loaded, &path) {
            self.loaded.config = before;
            return Err(config_error(&err));
        }
        Ok(())
    }

    fn known_display(&self, display: &str) -> Result<(), LucernaError> {
        let config = &self.loaded.config;
        let known = self.outputs.iter().any(|o| o.id.as_str() == display)
            || config.displays.keys().any(|id| id.as_str() == display);
        if known {
            Ok(())
        } else {
            Err(LucernaError::UnknownDisplay(format!(
                "No display matches '{display}'.\nRun `lucernactl monitors` to list displays."
            )))
        }
    }

    async fn add_wallpaper(&mut self, path: &str, name: &str) -> Outcome {
        let mut id = None;
        let stamp = now_rfc3339();
        let path = std::path::Path::new(path);
        self.mutate(|config| {
            id = Some(
                config
                    .add_wallpaper(path, name, &stamp)
                    .map_err(|e| library_error(&e))?,
            );
            Ok(())
        })?;
        self.emit_library_changed().await;
        self.mark_dirty();
        let id = id.ok_or_else(|| LucernaError::Internal("no id was produced".to_owned()))?;
        Ok(Reply::Text(id.to_string()))
    }

    async fn remove_wallpaper(&mut self, id: &str) -> Outcome {
        let wallpaper_id = parse_wallpaper_id(id)?;
        self.mutate(|config| {
            config
                .remove_wallpaper(&wallpaper_id)
                .map(|_| ())
                .map_err(|e| library_error(&e))
        })?;
        self.reconcile().await;
        self.emit_library_changed().await;
        Ok(Reply::Unit)
    }

    /// Resolve a `display_id` argument: `None` means all displays, `Some(id)` a specific one.
    fn target(&self, display: &str) -> Result<Option<OutputId>, LucernaError> {
        if display.is_empty() || display == ALL_DISPLAYS {
            return Ok(None);
        }
        self.known_display(display)?;
        Ok(Some(OutputId::new(display)))
    }

    /// The label to remember for a display, so it can be shown while unplugged.
    fn label_of(&self, id: &OutputId) -> Option<String> {
        self.outputs.iter().find(|o| &o.id == id).map(|o| o.label())
    }

    async fn set_wallpaper(&mut self, id: &str, display: &str) -> Outcome {
        let wallpaper_id = parse_wallpaper_id(id)?;
        if self.loaded.config.find_wallpaper(&wallpaper_id).is_none() {
            return Err(unknown_wallpaper(id));
        }
        let target = self.target(display)?;
        let label = target.as_ref().and_then(|t| self.label_of(t));
        self.mutate(|config| {
            match &target {
                None => config.all_displays.wallpaper = Some(wallpaper_id),
                Some(output) => {
                    let entry = config.displays.entry(output.clone()).or_default();
                    entry.wallpaper = Some(wallpaper_id);
                    if label.is_some() {
                        entry.last_seen = label;
                    }
                }
            }
            Ok(())
        })?;
        // Choosing a wallpaper is an explicit "play this".
        self.user_stopped = false;
        self.clear_failed_slots().await;
        self.reconcile().await;
        self.emit_displays_changed().await;
        Ok(Reply::Unit)
    }

    async fn clear_assignment(&mut self, display: &str) -> Outcome {
        let target = self.target(display)?;
        self.mutate(|config| {
            match &target {
                None => config.all_displays.wallpaper = None,
                Some(output) => {
                    if let Some(entry) = config.displays.get_mut(output) {
                        entry.wallpaper = None;
                        // An entry that overrides nothing any more is dropped, keeping the file tidy.
                        if entry.scaling.is_none() {
                            config.displays.remove(output);
                        }
                    }
                }
            }
            Ok(())
        })?;
        self.reconcile().await;
        self.emit_displays_changed().await;
        Ok(Reply::Unit)
    }

    /// `mode` is `fill`, `fit`, `stretch` or `center`; for one display also `inherit`, which
    /// removes the override so the display follows the all-displays scaling again.
    async fn set_scaling(&mut self, display: &str, mode: &str) -> Outcome {
        let target = self.target(display)?;
        let parsed: Option<ScalingMode> =
            if mode.trim().eq_ignore_ascii_case("inherit") && target.is_some() {
                None
            } else {
                Some(
                    mode.parse()
                        .map_err(|e: lucerna_core::types::ParseEnumError| {
                            LucernaError::InvalidArgument(format!("{e}."))
                        })?,
                )
            };
        let label = target.as_ref().and_then(|t| self.label_of(t));
        self.mutate(|config| {
            match (&target, parsed) {
                (None, Some(mode)) => config.all_displays.scaling = mode,
                (Some(output), _) => {
                    let entry = config.displays.entry(output.clone()).or_default();
                    entry.scaling = parsed;
                    if label.is_some() {
                        entry.last_seen = label;
                    }
                    if entry.wallpaper.is_none() && entry.scaling.is_none() {
                        config.displays.remove(output);
                    }
                }
                (None, None) => {}
            }
            Ok(())
        })?;
        self.reconcile().await;
        self.emit_displays_changed().await;
        Ok(Reply::Unit)
    }

    async fn set_settings(&mut self, patch: &SettingsPatch) -> Outcome {
        // Validate everything before changing anything (all or nothing).
        let hardware_decode = patch
            .hardware_decode
            .as_deref()
            .map(str::parse::<HwDecode>)
            .transpose()
            .map_err(|e| LucernaError::InvalidArgument(format!("{e}.")))?;
        let fps_limit = patch
            .fps_limit
            .as_deref()
            .map(str::parse::<FpsLimit>)
            .transpose()
            .map_err(|e| LucernaError::InvalidArgument(format!("{e}.")))?;
        let stacking = patch
            .stacking
            .as_deref()
            .map(str::parse::<StackingSetting>)
            .transpose()
            .map_err(|e| LucernaError::InvalidArgument(format!("{e}.")))?;
        if patch.max_restarts.is_some_and(|n| !(1..=10).contains(&n)) {
            return Err(LucernaError::InvalidArgument(
                "max_restarts must be between 1 and 10.".to_owned(),
            ));
        }
        if patch
            .restart_window_secs
            .is_some_and(|n| !(10..=3600).contains(&n))
        {
            return Err(LucernaError::InvalidArgument(
                "restart_window_secs must be between 10 and 3600.".to_owned(),
            ));
        }

        let previous = self.loaded.config.clone();
        self.mutate(|config| {
            if let Some(v) = patch.pause_on_fullscreen {
                config.general.pause_on_fullscreen = v;
            }
            if let Some(v) = patch.pause_on_lock {
                config.general.pause_on_lock = v;
            }
            if let Some(v) = patch.audio {
                config.general.audio = v;
            }
            if let Some(v) = hardware_decode {
                config.general.hardware_decode = v;
            }
            if let Some(v) = fps_limit {
                config.general.fps_limit = v;
            }
            if let Some(v) = stacking {
                config.x11.stacking = v;
            }
            if let Some(v) = patch.max_restarts {
                config.renderer.max_restarts = v;
            }
            if let Some(v) = patch.restart_window_secs {
                config.renderer.restart_window_secs = u64::from(v);
            }
            Ok(())
        })?;

        if let Some(enable) = patch.autostart {
            let file = self.opts.paths.autostart_file();
            let result = if enable {
                autostart::enable(&file, &self.opts.daemon_exe)
            } else {
                autostart::disable(&file)
            };
            if let Err(err) = result {
                // Roll the configuration back so the whole update is all-or-nothing.
                self.loaded.config = previous;
                let path = self.opts.paths.config_file();
                let _ = lucerna_core::config::save(&mut self.loaded, &path);
                return Err(LucernaError::AutostartWrite(format!(
                    "Lucerna could not {} autostart ({}): {err}\nCheck that the directory is writable.",
                    if enable { "enable" } else { "disable" },
                    file.display()
                )));
            }
        }

        self.reconcile().await;
        self.emit_settings_changed().await;
        Ok(Reply::Unit)
    }

    // ------------------------------------------------------------------------ lifecycle

    fn ensure_can_play(&self) -> Result<(), LucernaError> {
        if let BackendStatus::Unavailable(reason) = &self.backend_status {
            return Err(LucernaError::Unsupported(reason.message()));
        }
        if self.mpv.info.is_none() {
            return Err(LucernaError::MpvMissing(FailureReasonText::mpv_missing()));
        }
        Ok(())
    }

    async fn start(&mut self) -> Outcome {
        self.ensure_can_play()?;
        self.user_stopped = false;
        self.clear_failed_slots().await;
        self.reconcile().await;
        Ok(Reply::Unit)
    }

    /// Re-read configuration, re-probe mpv, re-select the backend, and start over.
    async fn reload(&mut self) -> Outcome {
        tracing::info!("reloading");
        self.stop_all_slots().await;
        if !self.backend_injected
            && let Some(mut backend) = self.backend.take()
        {
            let _ = backend.shutdown();
        }
        self.user_stopped = false;
        self.load_config_and_state();
        self.discover_mpv().await;
        self.init_backend().await;
        self.reconcile().await;
        self.arm_recheck();
        self.emit_settings_changed().await;
        self.emit_library_changed().await;
        self.emit_displays_changed().await;
        Ok(Reply::Unit)
    }
}

fn read_only_reason(state: &lucerna_core::config::ConfigState) -> Option<String> {
    use lucerna_core::config::ConfigState as S;
    match state {
        S::ReadOnlyNewerSchema { version } => {
            Some(ConfigError::ReadOnlyNewer(*version).to_string())
        }
        S::Unreadable { reason } => {
            Some(ConfigError::ReadOnlyUnreadable(reason.clone()).to_string())
        }
        S::Ok | S::DefaultsAfterCorruption { .. } => None,
    }
}

fn parse_wallpaper_id(id: &str) -> Result<WallpaperId, LucernaError> {
    WallpaperId::parse(id).ok_or_else(|| unknown_wallpaper(id))
}

fn unknown_wallpaper(id: &str) -> LucernaError {
    LucernaError::UnknownWallpaper(format!(
        "No wallpaper in the library has the id '{id}'.\nRun `lucernactl wallpapers` to list them."
    ))
}

/// The §4.7 message, shared with the renderer failure text.
struct FailureReasonText;

impl FailureReasonText {
    fn mpv_missing() -> String {
        lucerna_core::renderer::FailureReason::MpvMissing.user_message(&[])
    }
}
