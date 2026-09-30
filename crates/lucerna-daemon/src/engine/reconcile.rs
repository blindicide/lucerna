//! Making reality match the configuration: the only place that starts or stops renderers.

use std::collections::BTreeMap;
use std::time::Duration;

use lucerna_core::backend::{OutputId, OutputInfo};
use lucerna_core::library::file_exists;
use lucerna_core::plan::{Action, Actual, Desired, DesiredRenderer, LaunchKey, reconcile};
use lucerna_core::policy::{PauseReasons, PolicyInputs, evaluate};
use lucerna_core::renderer::{FailureReason, RendererSnapshot, RestartPolicy};
use lucerna_mpv::{LaunchSettings, RendererSupervisor, SupervisorConfig};
use tokio::time::Instant;

use super::{BackendStatus, Engine, Slot};

/// A renderer that would run but cannot, and why.
pub(crate) struct Blocked {
    pub output: OutputId,
    pub reason: FailureReason,
}

/// An upper bound for waiting on a renderer to stop: quit + TERM + KILL steps, with slack.
const STOP_WAIT: Duration = Duration::from_secs(10);

impl Engine {
    /// Pause reasons currently in force for `output`.
    pub(crate) fn pause_reasons(&self, output: &OutputId) -> PauseReasons {
        evaluate(
            output,
            &PolicyInputs {
                user_paused: self.user_paused,
                session_locked: self.session_locked,
                occluded: &self.occluded,
                settings: &self.loaded.config.general,
            },
        )
    }

    /// What should be running now, and what is wanted but blocked.
    pub(crate) fn compute_desired(&self) -> (Desired, Vec<Blocked>) {
        let mut desired = Desired::new();
        let mut blocked = Vec::new();
        if self.backend.is_none()
            || self.user_stopped
            || !matches!(self.backend_status, BackendStatus::Available { .. })
        {
            return (desired, blocked);
        }
        let config = &self.loaded.config;
        for output in &self.outputs {
            let Some(id) = config.effective_wallpaper(&output.id).0 else {
                continue;
            };
            let Some(wallpaper) = config.find_wallpaper(id) else {
                continue;
            };
            if !file_exists(&wallpaper.path) {
                blocked.push(Blocked {
                    output: output.id.clone(),
                    reason: FailureReason::MediaMissing,
                });
                continue;
            }
            if self.mpv.info.is_none() {
                blocked.push(Blocked {
                    output: output.id.clone(),
                    reason: FailureReason::MpvMissing,
                });
                continue;
            }
            desired.insert(
                output.id.clone(),
                DesiredRenderer {
                    wallpaper: id.clone(),
                    launch: LaunchKey {
                        media: wallpaper.path.clone(),
                        hwdec: config.general.hardware_decode,
                        fps: config.general.fps_limit,
                        audio: config.general.audio,
                    },
                    scaling: config.effective_scaling(&output.id),
                    paused: self.pause_reasons(&output.id).any(),
                    geometry: output.geometry,
                },
            );
        }
        (desired, blocked)
    }

    /// Bring renderers in line with configuration, outputs and policy.
    pub(crate) async fn reconcile(&mut self) {
        self.sync_availability();
        let (desired, _blocked) = self.compute_desired();
        let actual: Actual = self
            .slots
            .iter()
            .map(|(id, s)| (id.clone(), s.actual.clone()))
            .collect();
        self.surface_errors.retain(|id, _| desired.contains_key(id));
        for action in reconcile(&desired, &actual) {
            self.apply(action, &desired).await;
        }
        self.mark_dirty();
    }

    /// Keep the library's `available` flags honest; the file is rewritten only when one changes.
    pub(crate) fn sync_availability(&mut self) {
        if self.loaded.config.refresh_availability(file_exists) && !self.config_read_only() {
            let path = self.opts.paths.config_file();
            if let Err(err) = lucerna_core::config::save(&mut self.loaded, &path) {
                tracing::debug!(%err, "could not save wallpaper availability");
            }
        }
    }

    async fn apply(&mut self, action: Action, desired: &Desired) {
        match action {
            Action::Destroy(output) => self.destroy_slot(&output).await,
            Action::Replace(output) => {
                let surface = match self.slots.remove(&output) {
                    Some(slot) => {
                        stop_supervisor(slot.sup).await;
                        slot.surface
                    }
                    None => None,
                };
                if let Some(want) = desired.get(&output) {
                    self.create_slot(&output, want, surface).await;
                }
            }
            Action::Create(output) => {
                if let Some(want) = desired.get(&output) {
                    self.create_slot(&output, want, None).await;
                }
            }
            Action::Resize(output, geometry) => {
                if let Some(slot) = self.slots.get_mut(&output) {
                    slot.actual.geometry = geometry;
                    if let (Some(surface), Some(backend)) = (&slot.surface, self.backend.as_mut())
                        && let Err(err) = backend.resize_surface(surface.id, geometry)
                    {
                        tracing::warn!(%err, output = %output, "could not resize the wallpaper surface");
                    }
                }
            }
            Action::SetScaling(output, mode) => {
                if let Some(slot) = self.slots.get_mut(&output) {
                    slot.actual.scaling = mode;
                    slot.sup.set_scaling(mode);
                }
            }
            Action::SetPaused(output, paused) => {
                if let Some(slot) = self.slots.get_mut(&output) {
                    slot.actual.paused = paused;
                    if paused {
                        slot.sup.pause();
                        tracing::info!(output = %output, "renderer paused");
                    } else {
                        slot.sup.resume();
                        tracing::info!(output = %output, "renderer resumed");
                    }
                }
            }
        }
    }

    fn output_info(&self, output: &OutputId) -> Option<OutputInfo> {
        self.outputs.iter().find(|o| &o.id == output).cloned()
    }

    async fn create_slot(
        &mut self,
        output: &OutputId,
        want: &DesiredRenderer,
        reuse: Option<lucerna_core::backend::SurfaceHandle>,
    ) {
        let Some(info) = self.output_info(output) else {
            return;
        };
        let surface = match reuse {
            Some(surface) => surface,
            None => {
                let Some(backend) = self.backend.as_mut() else {
                    return;
                };
                match backend.create_surface(&info) {
                    Ok(surface) => surface,
                    Err(err) => {
                        tracing::warn!(%err, output = %output, "could not create a wallpaper surface");
                        self.surface_errors.insert(output.clone(), err.to_string());
                        return;
                    }
                }
            }
        };
        self.surface_errors.remove(output);
        let Some(mpv) = self.mpv.info.clone() else {
            return;
        };
        let Some(runtime_dir) = self.opts.paths.runtime_dir.clone() else {
            return;
        };
        let general = &self.loaded.config.renderer;
        let config = SupervisorConfig {
            output: output.clone(),
            mpv_path: mpv.path,
            runtime_dir,
            log_dir: Some(self.opts.paths.log_dir()),
            log_max_bytes: 512 * 1024,
            embed: Some(surface.embed),
            restart_policy: RestartPolicy::clamped(
                general.max_restarts,
                general.restart_window_secs,
            ),
            timings: self.opts.renderer_timings,
            vo_override: self.opts.vo_override.clone(),
            extra_env: self.opts.extra_env.clone(),
            registry: self.registry.clone(),
        };
        let launch = LaunchSettings {
            media: want.launch.media.clone(),
            scaling: want.scaling,
            hwdec: want.launch.hwdec,
            fps: want.launch.fps,
            audio: want.launch.audio,
        };
        let (fan_in, mut fan_out) = tokio::sync::mpsc::unbounded_channel();
        let sup = RendererSupervisor::spawn(config, launch, Some(fan_in));
        // Forward this supervisor's events into the engine's inbox.
        let engine_tx = self.tx.clone();
        tokio::spawn(async move {
            while let Some(event) = fan_out.recv().await {
                if engine_tx
                    .send(crate::messages::EngineMsg::Renderer(event))
                    .is_err()
                {
                    break;
                }
            }
        });
        tracing::info!(output = %output, wallpaper = %want.wallpaper, paused = want.paused, "starting renderer");
        sup.start(want.paused);
        self.slots.insert(
            output.clone(),
            Slot {
                sup,
                surface: Some(surface),
                actual: want.clone(),
                snapshot: RendererSnapshot::default(),
            },
        );
    }

    /// Stop one output's renderer and remove its surface.
    pub(crate) async fn destroy_slot(&mut self, output: &OutputId) {
        self.surface_errors.remove(output);
        if let Some(slot) = self.slots.remove(output) {
            stop_supervisor(slot.sup).await;
            if let (Some(surface), Some(backend)) = (slot.surface, self.backend.as_mut())
                && let Err(err) = backend.destroy_surface(surface.id)
            {
                tracing::warn!(%err, "could not destroy a wallpaper surface");
            }
        }
    }

    /// Stop every renderer concurrently and remove every surface.
    pub(crate) async fn stop_all_slots(&mut self) {
        let slots = std::mem::take(&mut self.slots);
        let mut stops = tokio::task::JoinSet::new();
        let mut surfaces = Vec::new();
        for (_, slot) in slots {
            surfaces.extend(slot.surface);
            stops.spawn(stop_supervisor(slot.sup));
        }
        while stops.join_next().await.is_some() {}
        if let Some(backend) = self.backend.as_mut() {
            for surface in surfaces {
                if let Err(err) = backend.destroy_surface(surface.id) {
                    tracing::warn!(%err, "could not destroy a wallpaper surface");
                }
            }
        }
        self.surface_errors.clear();
    }

    /// After a terminal failure, remove the surface so the normal desktop background shows.
    pub(crate) fn hide_surface(&mut self, output: &OutputId) {
        if let Some(slot) = self.slots.get_mut(output)
            && let Some(surface) = slot.surface.take()
            && let Some(backend) = self.backend.as_mut()
            && let Err(err) = backend.destroy_surface(surface.id)
        {
            tracing::warn!(%err, "could not hide the surface of a failed renderer");
        }
    }

    /// Drop every failed renderer so the next reconcile starts it afresh (`Start`, `Reload`).
    pub(crate) async fn clear_failed_slots(&mut self) {
        let failed: Vec<OutputId> = self
            .slots
            .iter()
            .filter(|(_, s)| s.snapshot.state == lucerna_core::renderer::RendererStateKind::Failed)
            .map(|(id, _)| id.clone())
            .collect();
        for output in failed {
            self.destroy_slot(&output).await;
        }
    }

    /// Every 60 s while a wallpaper is missing: has the file come back (an external drive)?
    pub(crate) fn arm_recheck(&mut self) {
        let (_, blocked) = self.compute_desired();
        let missing = blocked
            .iter()
            .any(|b| b.reason == FailureReason::MediaMissing);
        self.recheck_deadline = missing.then(|| Instant::now() + self.opts.recheck_interval);
    }

    pub(crate) async fn recheck_missing_media(&mut self) {
        let before: BTreeMap<_, _> = self
            .loaded
            .config
            .wallpapers
            .iter()
            .map(|w| (w.id.clone(), w.available))
            .collect();
        self.sync_availability();
        let changed = self
            .loaded
            .config
            .wallpapers
            .iter()
            .any(|w| before.get(&w.id) != Some(&w.available));
        if changed {
            tracing::info!("a wallpaper file appeared or disappeared");
        }
        // Reconcile whether or not a flag changed: a file may have returned for a slot that failed.
        self.clear_failed_slots_for_returned_media().await;
        self.reconcile().await;
        self.mark_dirty();
    }

    async fn clear_failed_slots_for_returned_media(&mut self) {
        let returned: Vec<OutputId> = self
            .slots
            .iter()
            .filter(|(_, s)| {
                s.snapshot.failure_code == "media-missing" && file_exists(&s.actual.launch.media)
            })
            .map(|(id, _)| id.clone())
            .collect();
        for output in returned {
            self.destroy_slot(&output).await;
        }
    }
}

async fn stop_supervisor(sup: RendererSupervisor) {
    if tokio::time::timeout(STOP_WAIT, sup.shutdown())
        .await
        .is_err()
    {
        tracing::warn!("a renderer did not stop in time");
    }
}
