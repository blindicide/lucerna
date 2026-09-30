//! The engine: the single owner of configuration, backend, renderers and policy inputs.
//!
//! It processes one [`EngineMsg`] at a time on a current-thread runtime, so there are no locks
//! and no lost updates between the GUI, the CLI and the daemon's own events. The zbus interface
//! only translates calls into messages; renderer supervisors and the backend thread only send
//! messages.

mod backend;
mod commands;
mod diagnostics;
mod reconcile;
mod status;

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use lucerna_core::autostart;
use lucerna_core::backend::{
    BackendCapabilities, BackendKind, OutputId, OutputInfo, SurfaceHandle, WallpaperBackend,
};
use lucerna_core::config::LoadedConfig;
use lucerna_core::mpv::{CompatReport, MpvInfo};
use lucerna_core::paths::Paths;
use lucerna_core::plan::DesiredRenderer;
use lucerna_core::renderer::{RendererSnapshot, Timings};
use lucerna_core::session::{SessionEnv, SessionKind, UnsupportedReason};
use lucerna_core::state::{FailureRecord, StateFile};
use lucerna_core::timeutil::{now_compact, now_rfc3339};
use lucerna_mpv::{PidRegistry, RendererSupervisor, SupervisorEvent};
use tokio::sync::mpsc;
use tokio::time::Instant;
use zbus::Connection;

use crate::messages::EngineMsg;
use crate::options::BackendChoice;
use crate::service::LucernaService;

/// Coalescing window for `StatusChanged` (docs/IMPLEMENTATION-PLAN.md §7.3).
const STATUS_COALESCE: Duration = Duration::from_millis(100);

/// What the engine needs to be told at construction.
pub struct EngineOpts {
    pub paths: Paths,
    pub session: SessionEnv,
    pub daemon_exe: PathBuf,
    pub renderer_timings: Timings,
    pub vo_override: Option<String>,
    pub extra_env: Vec<(String, String)>,
    pub recheck_interval: Duration,
    pub display_wait: Duration,
    pub mpv_override: Option<OsString>,
    pub path_var: Option<OsString>,
    pub autostart_on_first_run: bool,
}

/// The state of mpv discovery.
#[derive(Default)]
pub(crate) struct MpvState {
    pub info: Option<MpvInfo>,
    pub compat: Option<CompatReport>,
    pub error: Option<String>,
}

/// The state of the wallpaper backend.
pub(crate) enum BackendStatus {
    Available {
        kind: BackendKind,
        capabilities: BackendCapabilities,
    },
    Unavailable(UnsupportedReason),
}

/// A running (or failed) renderer for one output.
pub(crate) struct Slot {
    pub sup: RendererSupervisor,
    /// `None` once a terminal failure removed the surface, so the normal desktop background shows.
    pub surface: Option<SurfaceHandle>,
    /// What the renderer was started with, for reconciliation.
    pub actual: DesiredRenderer,
    pub snapshot: RendererSnapshot,
}

pub struct Engine {
    pub(crate) opts: EngineOpts,
    pub(crate) conn: Connection,
    pub(crate) tx: mpsc::UnboundedSender<EngineMsg>,
    rx: mpsc::UnboundedReceiver<EngineMsg>,

    pub(crate) loaded: LoadedConfig,
    pub(crate) state: StateFile,
    pub(crate) mpv: MpvState,
    pub(crate) session_kind: SessionKind,
    pub(crate) backend_status: BackendStatus,
    pub(crate) backend: Option<Box<dyn WallpaperBackend>>,
    /// True if the backend was injected and must be kept across reloads.
    pub(crate) backend_injected: bool,
    pub(crate) outputs: Vec<OutputInfo>,
    pub(crate) slots: BTreeMap<OutputId, Slot>,
    /// Surface creation failures, shown as renderer failures.
    pub(crate) surface_errors: BTreeMap<OutputId, String>,
    pub(crate) registry: Arc<PidRegistry>,

    pub(crate) user_paused: bool,
    pub(crate) user_stopped: bool,
    pub(crate) session_locked: bool,
    pub(crate) occluded: BTreeSet<OutputId>,
    pub(crate) lock_detection: bool,
    /// Recent `refresh()` calls, to stop a restack fight with the window manager (§5.2).
    pub(crate) restack_times: std::collections::VecDeque<Instant>,
    pub(crate) restack_fights: u32,

    status_deadline: Option<Instant>,
    recheck_deadline: Option<Instant>,
    pub(crate) quit_flag: bool,
    pub(crate) quitting: bool,
}

/// Why the engine loop ended.
pub enum Exit {
    Quit,
    Signal(&'static str),
    ConnectionLost(String),
    ChannelClosed,
}

impl Engine {
    pub fn new(
        opts: EngineOpts,
        conn: Connection,
        tx: mpsc::UnboundedSender<EngineMsg>,
        rx: mpsc::UnboundedReceiver<EngineMsg>,
        backend: BackendChoice,
    ) -> Self {
        let registry_path = opts
            .paths
            .registry_file()
            .unwrap_or_else(|| std::env::temp_dir().join("lucerna-renderers-unavailable.json"));
        let (backend, backend_injected) = match backend {
            BackendChoice::Auto => (None, false),
            BackendChoice::Injected(b) => (Some(b), true),
        };
        Self {
            opts,
            conn,
            tx,
            rx,
            loaded: LoadedConfig::defaults(),
            state: StateFile::default(),
            mpv: MpvState::default(),
            session_kind: SessionKind::Unknown,
            backend_status: BackendStatus::Unavailable(UnsupportedReason::NoDisplay),
            backend,
            backend_injected,
            outputs: Vec::new(),
            slots: BTreeMap::new(),
            surface_errors: BTreeMap::new(),
            registry: Arc::new(PidRegistry::new(registry_path)),
            user_paused: false,
            user_stopped: false,
            session_locked: false,
            occluded: BTreeSet::new(),
            lock_detection: false,
            restack_times: std::collections::VecDeque::new(),
            restack_fights: 0,
            status_deadline: None,
            recheck_deadline: None,
            quit_flag: false,
            quitting: false,
        }
    }

    /// Load configuration and state, find mpv, choose the backend, and start wallpapers.
    pub async fn initialise(&mut self) {
        self.load_config_and_state();
        self.first_run_autostart();
        self.discover_mpv().await;
        self.init_backend().await;
        self.reconcile().await;
        self.arm_recheck();
    }

    pub(crate) fn load_config_and_state(&mut self) {
        let path = self.opts.paths.config_file();
        self.loaded = lucerna_core::config::load(&path, &now_compact());
        self.state = StateFile::load(&self.opts.paths.state_file());
        self.state.config_notice.clone_from(&self.loaded.notice);
        if let Some(notice) = &self.loaded.notice {
            tracing::warn!(%notice, "configuration notice");
        }
        for warning in &self.loaded.warnings {
            tracing::warn!(%warning, "configuration warning");
        }
    }

    /// The first time a configuration is created the autostart entry is enabled (D4, §22).
    fn first_run_autostart(&mut self) {
        if !self.loaded.first_run || !self.opts.autostart_on_first_run {
            return;
        }
        let file = self.opts.paths.autostart_file();
        if autostart::state(&file) == autostart::AutostartState::Absent
            && let Err(err) = autostart::enable(&file, &self.opts.daemon_exe)
        {
            tracing::warn!(%err, "could not enable autostart on first run");
        }
    }

    pub(crate) fn config_read_only(&self) -> bool {
        self.loaded.state.is_read_only()
    }

    pub(crate) fn save_state_file(&self) {
        if let Err(err) = self.state.save(&self.opts.paths.state_file()) {
            tracing::warn!(%err, "could not save state.json");
        }
    }

    pub(crate) fn mark_dirty(&mut self) {
        if self.status_deadline.is_none() {
            self.status_deadline = Some(Instant::now() + STATUS_COALESCE);
        }
    }

    /// Run until asked to stop, then shut everything down cleanly (§52).
    pub async fn run(mut self) {
        let exit = loop {
            let deadline = [self.status_deadline, self.recheck_deadline]
                .into_iter()
                .flatten()
                .min();
            tokio::select! {
                msg = self.rx.recv() => {
                    let Some(msg) = msg else { break Exit::ChannelClosed };
                    if let Some(exit) = self.handle(msg).await {
                        break exit;
                    }
                }
                () = async {
                    match deadline {
                        Some(at) => tokio::time::sleep_until(at).await,
                        None => std::future::pending().await,
                    }
                } => self.on_timer().await,
            }
            if self.quitting {
                break Exit::Quit;
            }
        };
        match &exit {
            Exit::Quit => tracing::info!("shutting down: requested"),
            Exit::Signal(name) => tracing::info!(signal = name, "shutting down: signal"),
            Exit::ConnectionLost(why) => {
                tracing::info!(%why, "shutting down: display connection lost")
            }
            Exit::ChannelClosed => tracing::info!("shutting down: engine channel closed"),
        }
        self.shutdown().await;
    }

    async fn handle(&mut self, msg: EngineMsg) -> Option<Exit> {
        match msg {
            EngineMsg::Command { cmd, reply } => {
                self.handle_command(cmd, reply).await;
                None
            }
            EngineMsg::Backend(event) => self.on_backend_event(event).await,
            EngineMsg::Renderer(event) => {
                self.on_renderer_event(event).await;
                None
            }
            EngineMsg::Lock(locked) => {
                self.on_lock_changed(locked).await;
                None
            }
            EngineMsg::Signal(name) => Some(Exit::Signal(name)),
        }
    }

    async fn on_timer(&mut self) {
        let now = Instant::now();
        if self.status_deadline.is_some_and(|d| d <= now) {
            self.status_deadline = None;
            self.emit_status().await;
        }
        if self.recheck_deadline.is_some_and(|d| d <= now) {
            self.recheck_deadline = None;
            self.recheck_missing_media().await;
            self.arm_recheck();
        }
    }

    /// Record a renderer state change; on a new failure, remember it and tell clients.
    pub(crate) async fn on_renderer_event(&mut self, event: SupervisorEvent) {
        let Some(slot) = self.slots.get_mut(&event.output) else {
            return;
        };
        let previous = slot.snapshot.clone();
        slot.snapshot = event.snapshot.clone();
        let failed_now = event.snapshot.state == lucerna_core::renderer::RendererStateKind::Failed;
        let new_failure = failed_now
            && (previous.state != lucerna_core::renderer::RendererStateKind::Failed
                || previous.failure_code != event.snapshot.failure_code);
        if new_failure {
            self.state.record_failure(FailureRecord {
                time: now_rfc3339(),
                output: event.output.as_str().to_owned(),
                code: event.snapshot.failure_code.clone(),
                message: event.snapshot.failure_message.clone(),
            });
            self.save_state_file();
            self.emit_renderer_failed(&event.output, &event.snapshot)
                .await;
            if is_terminal_failure(&event.snapshot.failure_code) {
                self.hide_surface(&event.output);
            }
        }
        self.mark_dirty();
    }

    pub(crate) async fn on_lock_changed(&mut self, locked: bool) {
        if self.session_locked != locked {
            tracing::info!(locked, "screen lock state changed");
            self.session_locked = locked;
            self.reconcile().await;
            self.mark_dirty();
        }
    }

    /// Stop everything in the order §52 requires.
    async fn shutdown(mut self) {
        // Stop accepting requests and answer everything already queued. Clients that call while
        // we shut down must get an error immediately: otherwise their handlers would wait for
        // replies that are only dropped when this function returns, and closing the bus
        // connection (which waits for handlers) would never finish.
        self.rx.close();
        while let Ok(msg) = self.rx.try_recv() {
            if let EngineMsg::Command { reply, .. } = msg {
                let _ = reply.send(Err(lucerna_ipc::LucernaError::Internal(
                    "The Lucerna daemon is shutting down.".to_owned(),
                )));
            }
        }
        // 1. Renderers first (in parallel, each bounded by its own escalation).
        self.stop_all_slots().await;
        // 2. Remove the wallpaper windows.
        if let Some(backend) = self.backend.as_mut()
            && let Err(err) = backend.shutdown()
        {
            tracing::warn!(%err, "backend shutdown reported an error");
        }
        // 3. Sockets and the registry.
        if let Some(dir) = &self.opts.paths.runtime_dir {
            let _ = lucerna_core::runtime::remove_stale_sockets(dir);
        }
        if let Some(path) = self.opts.paths.registry_file() {
            let _ = std::fs::remove_file(path);
        }
        // 4. Flush state.
        self.save_state_file();
        // 5. Let any pending reply reach its caller, then release the bus name.
        tokio::time::sleep(Duration::from_millis(50)).await;
        self.conn.graceful_shutdown().await;
        tracing::info!("daemon stopped");
    }
}

/// Failure codes that no automatic retry will fix.
pub(crate) fn is_terminal_failure(code: &str) -> bool {
    matches!(
        code,
        "mpv-missing" | "launch-failed" | "media-missing" | "media-unsupported" | "restart-limit"
    )
}

/// The interface object registered on the bus, for signal emission.
pub(crate) type Service = LucernaService;
