//! The renderer supervisor: executes the effects of `lucerna_core::renderer::RendererMachine`.
//!
//! One supervisor task (an actor) exists per output. It owns the machine, at most one mpv
//! process, that process's IPC connection and the timers. Every decision (what to do after a
//! crash, when to give up, how to stop) is made by the pure machine in `lucerna-core`; this
//! module only performs the I/O.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use lucerna_core::backend::{EmbedTarget, OutputId};
use lucerna_core::bounded_log::BoundedLog;
use lucerna_core::mpv::{RenderSpec, build_args};
use lucerna_core::renderer::{
    Effect, ExitInfo, FailureReason, LaunchError, Outcome, RendererEvent, RendererMachine,
    RendererSnapshot, RendererState, RendererStateKind, RestartPolicy, Timings,
};
use lucerna_core::runtime;
use lucerna_core::types::{FpsLimit, HwDecode, ScalingMode};
use serde_json::json;
use tokio::process::Command;
use tokio::sync::{mpsc, oneshot, watch};
use tokio::time::{Instant, sleep_until};

use crate::capture::Capture;
use crate::ipc::{MpvEvent, MpvIpc};
use crate::proc::{self, Sig};
use crate::registry::{PidRecord, PidRegistry};

/// Fixed for the lifetime of one supervisor.
pub struct SupervisorConfig {
    pub output: OutputId,
    pub mpv_path: PathBuf,
    /// Verified private runtime directory (`$XDG_RUNTIME_DIR/lucerna`).
    pub runtime_dir: PathBuf,
    /// Where `renderer-<slot>.log` goes; `None` disables file logging.
    pub log_dir: Option<PathBuf>,
    pub log_max_bytes: u64,
    /// Surface to draw into; `None` for tests that render to `--vo=null`.
    pub embed: Option<EmbedTarget>,
    pub restart_policy: RestartPolicy,
    pub timings: Timings,
    /// Test builds only.
    pub vo_override: Option<String>,
    pub registry: Arc<PidRegistry>,
}

/// What to play and how. Changing anything except `scaling` requires a new supervisor.
#[derive(Clone, Debug)]
pub struct LaunchSettings {
    pub media: PathBuf,
    pub scaling: ScalingMode,
    pub hwdec: HwDecode,
    pub fps: FpsLimit,
    pub audio: bool,
}

/// Sent to the owner whenever the observable state changes.
#[derive(Clone, Debug)]
pub struct SupervisorEvent {
    pub output: OutputId,
    pub snapshot: RendererSnapshot,
}

enum Command_ {
    Start { paused: bool },
    Pause,
    Resume,
    Stop,
    SetScaling(ScalingMode),
    Shutdown(oneshot::Sender<()>),
}

enum Internal {
    ChildExited { generation: u64, exit: ExitInfo },
    IpcConnected { generation: u64, ipc: Arc<MpvIpc> },
    IpcConnectFailed { generation: u64, message: String },
    IpcEvent { generation: u64, event: MpvEvent },
    IpcClosed { generation: u64 },
}

enum Msg {
    Command(Command_),
    Internal(Internal),
}

/// Handle to one renderer. Cheap to use from any task; dropping it stops the renderer.
pub struct RendererSupervisor {
    tx: mpsc::UnboundedSender<Msg>,
    snapshot: watch::Receiver<RendererSnapshot>,
    task: tokio::task::JoinHandle<()>,
}

impl RendererSupervisor {
    pub fn spawn(
        config: SupervisorConfig,
        launch: LaunchSettings,
        notify: Option<mpsc::UnboundedSender<SupervisorEvent>>,
    ) -> Self {
        let (tx, inbox) = mpsc::unbounded_channel();
        let (snap_tx, snap_rx) = watch::channel(RendererSnapshot::default());

        let log = config.log_dir.as_ref().and_then(|dir| {
            let path = dir.join(format!("renderer-{}.log", config.output.slot()));
            match BoundedLog::open(path, config.log_max_bytes) {
                Ok(log) => Some(log),
                Err(err) => {
                    tracing::warn!(%err, "renderer log unavailable; continuing without a log file");
                    None
                }
            }
        });

        let actor = Actor {
            machine: RendererMachine::new(config.restart_policy, config.timings),
            capture: Arc::new(Capture::new(log)),
            cfg: config,
            launch,
            self_tx: tx.clone(),
            inbox,
            snap_tx,
            notify,
            timers: Timers::default(),
            child: None,
            ipc: None,
            queue: VecDeque::new(),
            shutdown: None,
        };
        let task = tokio::spawn(actor.run());
        Self {
            tx,
            snapshot: snap_rx,
            task,
        }
    }

    fn send(&self, command: Command_) {
        let _ = self.tx.send(Msg::Command(command));
    }

    /// Start playing; `paused` starts mpv paused so no motion is ever shown.
    pub fn start(&self, paused: bool) {
        self.send(Command_::Start { paused });
    }

    pub fn pause(&self) {
        self.send(Command_::Pause);
    }

    pub fn resume(&self) {
        self.send(Command_::Resume);
    }

    pub fn stop(&self) {
        self.send(Command_::Stop);
    }

    /// Change the scaling mode live (no restart).
    pub fn set_scaling(&self, mode: ScalingMode) {
        self.send(Command_::SetScaling(mode));
    }

    pub fn snapshot(&self) -> RendererSnapshot {
        self.snapshot.borrow().clone()
    }

    pub fn subscribe(&self) -> watch::Receiver<RendererSnapshot> {
        self.snapshot.clone()
    }

    /// Stop the renderer (with the full quit → SIGTERM → SIGKILL escalation) and end the task.
    pub async fn shutdown(self) {
        let (done_tx, done_rx) = oneshot::channel();
        self.send(Command_::Shutdown(done_tx));
        let _ = done_rx.await;
        let _ = self.task.await;
    }
}

#[derive(Default)]
struct Timers {
    startup: Option<(Instant, u64)>,
    stop: Option<(Instant, u64)>,
    retry: Option<Instant>,
    /// A closed control socket is only reported as `IpcLost` if the process does not exit within
    /// a short grace period: a crash closes the socket *and* ends the process, and the exit is the
    /// truer cause.
    ipc_lost: Option<(Instant, u64)>,
}

/// How long a closed control socket waits for the process exit before counting as an IPC failure.
const IPC_LOST_GRACE: Duration = Duration::from_millis(300);

impl Timers {
    fn earliest(&self) -> Option<Instant> {
        [
            self.startup.map(|t| t.0),
            self.stop.map(|t| t.0),
            self.retry,
            self.ipc_lost.map(|t| t.0),
        ]
        .into_iter()
        .flatten()
        .min()
    }
}

struct ChildCtl {
    generation: u64,
    pid: u32,
    signals: mpsc::UnboundedSender<Sig>,
}

struct Actor {
    cfg: SupervisorConfig,
    launch: LaunchSettings,
    machine: RendererMachine,
    capture: Arc<Capture>,
    self_tx: mpsc::UnboundedSender<Msg>,
    inbox: mpsc::UnboundedReceiver<Msg>,
    snap_tx: watch::Sender<RendererSnapshot>,
    notify: Option<mpsc::UnboundedSender<SupervisorEvent>>,
    timers: Timers,
    child: Option<ChildCtl>,
    ipc: Option<Arc<MpvIpc>>,
    queue: VecDeque<RendererEvent>,
    shutdown: Option<oneshot::Sender<()>>,
}

impl Actor {
    async fn run(mut self) {
        loop {
            let deadline = self.timers.earliest();
            tokio::select! {
                msg = self.inbox.recv() => {
                    match msg {
                        Some(msg) => self.on_message(msg),
                        None => break, // handle dropped
                    }
                }
                () = async {
                    match deadline {
                        Some(at) => sleep_until(at).await,
                        None => std::future::pending().await,
                    }
                } => self.on_timer(),
            }
            self.pump();
            if self.shutdown_complete() {
                break;
            }
        }
        // Owner gone or shutdown finished: never leave a child behind.
        if let Some(child) = self.child.take() {
            let _ = child.signals.send(Sig::Kill);
        }
        if let Some(done) = self.shutdown.take() {
            let _ = done.send(());
        }
    }

    fn shutdown_complete(&self) -> bool {
        self.shutdown.is_some()
            && self.child.is_none()
            && matches!(
                self.machine.state(),
                RendererState::Stopped | RendererState::Failed { .. }
            )
    }

    fn on_message(&mut self, msg: Msg) {
        match msg {
            Msg::Command(command) => self.on_command(command),
            Msg::Internal(internal) => self.on_internal(internal),
        }
    }

    fn on_command(&mut self, command: Command_) {
        match command {
            Command_::Start { paused } => self.queue.push_back(RendererEvent::Start { paused }),
            Command_::Pause => self.queue.push_back(RendererEvent::Pause),
            Command_::Resume => self.queue.push_back(RendererEvent::Resume),
            Command_::Stop => self.queue.push_back(RendererEvent::Stop),
            Command_::SetScaling(mode) => self.set_scaling(mode),
            Command_::Shutdown(done) => {
                self.shutdown = Some(done);
                self.queue.push_back(RendererEvent::Stop);
            }
        }
    }

    fn on_internal(&mut self, internal: Internal) {
        match internal {
            Internal::ChildExited { generation, exit } => {
                if self
                    .child
                    .as_ref()
                    .is_some_and(|c| c.generation == generation)
                {
                    if let Some(child) = self.child.take() {
                        self.cfg.registry.unregister(child.pid);
                    }
                    self.ipc = None;
                }
                self.remove_socket(generation);
                self.queue
                    .push_back(RendererEvent::Exited { generation, exit });
            }
            Internal::IpcConnected { generation, ipc } => {
                if self
                    .child
                    .as_ref()
                    .is_some_and(|c| c.generation == generation)
                {
                    self.ipc = Some(ipc);
                }
            }
            Internal::IpcConnectFailed {
                generation,
                message,
            } => {
                tracing::debug!(output = %self.cfg.output, %message, "mpv control socket never appeared");
                self.queue
                    .push_back(RendererEvent::StartupTimedOut { generation });
            }
            Internal::IpcEvent { generation, event } => match event {
                MpvEvent::FileLoaded => self.queue.push_back(RendererEvent::Ready { generation }),
                MpvEvent::EndFile { reason, file_error } if reason.as_deref() == Some("error") => {
                    let message = file_error.unwrap_or_else(|| "unknown playback error".to_owned());
                    self.queue.push_back(RendererEvent::MediaError {
                        generation,
                        message,
                    });
                }
                _ => {}
            },
            Internal::IpcClosed { generation } => {
                if self
                    .child
                    .as_ref()
                    .is_some_and(|c| c.generation == generation)
                {
                    self.timers.ipc_lost = Some((Instant::now() + IPC_LOST_GRACE, generation));
                }
            }
        }
    }

    fn on_timer(&mut self) {
        let now = Instant::now();
        if let Some((at, generation)) = self.timers.startup
            && at <= now
        {
            self.timers.startup = None;
            self.queue
                .push_back(RendererEvent::StartupTimedOut { generation });
        }
        if let Some((at, generation)) = self.timers.stop
            && at <= now
        {
            self.timers.stop = None;
            self.queue
                .push_back(RendererEvent::StopTimedOut { generation });
        }
        if let Some(at) = self.timers.retry
            && at <= now
        {
            self.timers.retry = None;
            self.queue.push_back(RendererEvent::RetryDue);
        }
        if let Some((at, generation)) = self.timers.ipc_lost
            && at <= now
        {
            self.timers.ipc_lost = None;
            self.queue.push_back(RendererEvent::IpcLost {
                generation,
                message: "the control connection closed".to_owned(),
            });
        }
    }

    /// Feed queued events through the machine until nothing is left, executing effects.
    fn pump(&mut self) {
        while let Some(event) = self.queue.pop_front() {
            let (outcome, effects) = self
                .machine
                .handle(event.clone(), std::time::Instant::now());
            match outcome {
                Outcome::Ignored => {
                    tracing::debug!(output = %self.cfg.output, ?event, "ignored stale renderer event");
                }
                Outcome::Rejected(why) => {
                    tracing::debug!(output = %self.cfg.output, ?event, why, "renderer event rejected");
                }
                Outcome::Applied | Outcome::Normalized => {}
            }
            for effect in effects {
                self.execute(effect);
            }
            // A rejected Start (while stopping) is retried once the process is gone, if wanted.
        }
    }

    fn execute(&mut self, effect: Effect) {
        match effect {
            Effect::Spawn {
                generation,
                start_paused,
            } => self.spawn_child(generation, start_paused),
            Effect::SetPause(pause) => {
                if let Some(ipc) = &self.ipc
                    && let Err(err) =
                        ipc.send(&[json!("set_property"), json!("pause"), json!(pause)])
                {
                    tracing::debug!(%err, "could not send pause to mpv");
                }
            }
            Effect::SendQuit => match &self.ipc {
                Some(ipc) => {
                    if ipc.send(&[json!("quit")]).is_err() {
                        self.signal(Sig::Term);
                    }
                }
                // Never connected: there is nobody to ask politely.
                None => self.signal(Sig::Term),
            },
            Effect::SendTerm => self.signal(Sig::Term),
            Effect::SendKill => self.signal(Sig::Kill),
            Effect::ArmStartupTimer { generation, after } => {
                self.timers.startup = Some((Instant::now() + after, generation));
            }
            Effect::ArmStopTimer { generation, after } => {
                self.timers.stop = Some((Instant::now() + after, generation));
            }
            Effect::ArmRetryTimer { after } => self.timers.retry = Some(Instant::now() + after),
            Effect::CancelTimers => self.timers = Timers::default(),
            Effect::Notify => self.publish(),
            Effect::LogFailure(reason) => self.log_failure(&reason),
        }
    }

    fn signal(&self, sig: Sig) {
        if let Some(child) = &self.child {
            let _ = child.signals.send(sig);
        }
    }

    fn log_failure(&self, reason: &FailureReason) {
        let tail = self.capture.tail(5);
        tracing::warn!(
            output = %self.cfg.output,
            code = reason.code(),
            reason = %reason,
            mpv_stderr = %tail.join(" | "),
            "renderer failed"
        );
    }

    fn publish(&self) {
        let (failure_code, failure_message) = match self.machine.failure() {
            Some(reason) => (
                reason.code().to_owned(),
                reason.user_message(&self.capture.tail(5)),
            ),
            None => (String::new(), String::new()),
        };
        let generation = self
            .machine
            .state()
            .generation()
            .or(self.child.as_ref().map(|c| c.generation));
        let snapshot = RendererSnapshot {
            state: self.machine.state().kind(),
            generation: generation.unwrap_or(0),
            failure_code,
            failure_message,
            restarts_in_window: self
                .machine
                .restarts()
                .count_in_window(std::time::Instant::now()),
            pid: self.child.as_ref().map_or(0, |c| c.pid),
        };
        self.snap_tx.send_replace(snapshot.clone());
        if let Some(notify) = &self.notify {
            let _ = notify.send(SupervisorEvent {
                output: self.cfg.output.clone(),
                snapshot,
            });
        }
    }

    fn set_scaling(&mut self, mode: ScalingMode) {
        self.launch.scaling = mode;
        let live = matches!(
            self.machine.state().kind(),
            RendererStateKind::Playing | RendererStateKind::Paused
        );
        if let (true, Some(ipc)) = (live, &self.ipc) {
            for (name, value) in mode.mpv_properties() {
                if let Err(err) = ipc.send(&[json!("set_property"), json!(name), json!(value)]) {
                    tracing::debug!(%err, "could not send scaling to mpv");
                }
            }
        }
    }

    fn remove_socket(&self, generation: u64) {
        if let Ok(path) = runtime::socket_path(&self.cfg.runtime_dir, &self.cfg.output, generation)
        {
            let _ = std::fs::remove_file(path);
        }
    }

    fn spawn_child(&mut self, generation: u64, start_paused: bool) {
        let launch_error = |actor: &mut Actor, error: LaunchError| {
            actor
                .queue
                .push_back(RendererEvent::LaunchError { generation, error });
        };

        // The file must exist now; a returning drive is handled by the owner.
        if !std::fs::metadata(&self.launch.media).is_ok_and(|m| m.is_file()) {
            launch_error(self, LaunchError::MediaMissing);
            return;
        }
        let socket = match runtime::socket_path(&self.cfg.runtime_dir, &self.cfg.output, generation)
        {
            Ok(path) => path,
            Err(err) => {
                launch_error(self, LaunchError::Other(err.to_string()));
                return;
            }
        };
        match std::fs::remove_file(&socket) {
            Ok(()) => tracing::debug!(socket = %socket.display(), "removed stale renderer socket"),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => {
                launch_error(
                    self,
                    LaunchError::Other(format!("cannot clear {}: {err}", socket.display())),
                );
                return;
            }
        }

        let spec = RenderSpec {
            embed: self.cfg.embed,
            ipc_socket: socket.clone(),
            media: self.launch.media.clone(),
            scaling: self.launch.scaling,
            hwdec: self.launch.hwdec,
            fps: self.launch.fps,
            audio: self.launch.audio,
            start_paused,
            vo_override: self.cfg.vo_override.clone(),
        };

        let mut command = Command::new(&self.cfg.mpv_path);
        command
            .args(build_args(&spec))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(false);
        die_with_parent(&mut command);

        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                launch_error(self, LaunchError::NotFound);
                return;
            }
            Err(err) => {
                launch_error(
                    self,
                    LaunchError::Other(format!(
                        "could not execute {}: {err}",
                        self.cfg.mpv_path.display()
                    )),
                );
                return;
            }
        };
        let Some(pid) = child.id() else {
            launch_error(
                self,
                LaunchError::Other("mpv exited immediately".to_owned()),
            );
            return;
        };
        tracing::info!(output = %self.cfg.output, pid, generation, "renderer launched");

        self.cfg.registry.register(PidRecord {
            pid,
            starttime: proc::start_time(pid).unwrap_or(0),
            socket: socket.to_string_lossy().into_owned(),
            output_id: self.cfg.output.as_str().to_owned(),
            generation,
        });

        if let Some(stdout) = child.stdout.take() {
            let capture = Arc::clone(&self.capture);
            tokio::spawn(async move { capture.pump(stdout, "stdout").await });
        }
        if let Some(stderr) = child.stderr.take() {
            let capture = Arc::clone(&self.capture);
            tokio::spawn(async move { capture.pump(stderr, "stderr").await });
        }

        // The waiter owns the Child. Signals are delivered from inside it, so a signal can never
        // reach a pid that has already been reaped and reused.
        let (sig_tx, mut sig_rx) = mpsc::unbounded_channel::<Sig>();
        let gone = Arc::new(AtomicBool::new(false));
        let gone_for_waiter = Arc::clone(&gone);
        let tx = self.self_tx.clone();
        tokio::spawn(async move {
            let status = loop {
                tokio::select! {
                    status = child.wait() => break status,
                    Some(sig) = sig_rx.recv() => { let _ = proc::send_signal(pid, sig); }
                }
            };
            gone_for_waiter.store(true, Ordering::SeqCst);
            let exit = match status {
                Ok(status) => exit_info(&status),
                Err(_) => ExitInfo::default(),
            };
            let _ = tx.send(Msg::Internal(Internal::ChildExited { generation, exit }));
        });

        self.child = Some(ChildCtl {
            generation,
            pid,
            signals: sig_tx,
        });
        self.ipc = None;

        let tx = self.self_tx.clone();
        let timings = *self.machine.timings();
        tokio::spawn(ipc_session(generation, socket, timings, gone, tx));
    }
}

/// Connect to a renderer's control socket, report readiness and forward its events.
async fn ipc_session(
    generation: u64,
    socket: PathBuf,
    timings: Timings,
    gone: Arc<AtomicBool>,
    tx: mpsc::UnboundedSender<Msg>,
) {
    let connected = MpvIpc::connect(
        &socket,
        Duration::from_millis(20),
        timings.ipc_connect,
        || gone.load(Ordering::SeqCst),
    )
    .await;
    let (ipc, mut events) = match connected {
        Ok(pair) => pair,
        Err(err) => {
            if !gone.load(Ordering::SeqCst) {
                let _ = tx.send(Msg::Internal(Internal::IpcConnectFailed {
                    generation,
                    message: err.to_string(),
                }));
            }
            return;
        }
    };
    let ipc = Arc::new(ipc);
    let _ = tx.send(Msg::Internal(Internal::IpcConnected {
        generation,
        ipc: Arc::clone(&ipc),
    }));

    // mpv may have finished loading before we connected, so `file-loaded` would be missed.
    // `time-pos` is only available once playback has been initialised, which happens before the
    // event is sent: if it answers, the file is already loaded; if not, the event is still to come.
    if ipc
        .get_property("time-pos")
        .await
        .is_ok_and(|v| !v.is_null())
    {
        let _ = tx.send(Msg::Internal(Internal::IpcEvent {
            generation,
            event: MpvEvent::FileLoaded,
        }));
    }

    while let Some(event) = events.recv().await {
        let _ = tx.send(Msg::Internal(Internal::IpcEvent { generation, event }));
    }
    let _ = tx.send(Msg::Internal(Internal::IpcClosed { generation }));
}

fn exit_info(status: &std::process::ExitStatus) -> ExitInfo {
    use std::os::unix::process::ExitStatusExt;
    ExitInfo {
        code: status.code(),
        signal: status.signal(),
    }
}

/// Ask the kernel to send SIGTERM to mpv if the thread that spawned it dies (defence in depth
/// for §52). The daemon spawns from a long-lived runtime thread, so the usual `PDEATHSIG`
/// caveat (it is tied to the *thread*, not the process) does not bite.
#[allow(unsafe_code)]
fn die_with_parent(command: &mut Command) {
    // SAFETY: the closure runs in the forked child before exec and performs exactly one
    // `prctl(PR_SET_PDEATHSIG)` syscall through rustix. It allocates nothing and takes no locks,
    // so it is async-signal-safe.
    unsafe {
        command.pre_exec(|| {
            rustix::process::set_parent_process_death_signal(Some(rustix::process::Signal::TERM))
                .map_err(std::io::Error::from)
        });
    }
}
