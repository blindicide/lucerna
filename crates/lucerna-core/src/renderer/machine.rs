//! The renderer state machine (directive §9): pure, deterministic, driven by events.
//!
//! Each renderer is in exactly one [`RendererState`]. Events that carry a `generation` belong to
//! one particular process; events from a previous process are answered with
//! [`Outcome::Ignored`] and can never disturb the current one.

use std::time::{Duration, Instant};

use super::failure::{ExitInfo, FailureReason, LaunchError};
use super::restart::{RestartDecision, RestartPolicy, RestartTracker};

/// Timeouts used by the machine and its driver.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Timings {
    /// From spawn to `file-loaded`.
    pub startup: Duration,
    /// How long to keep retrying to connect to mpv's IPC socket.
    pub ipc_connect: Duration,
    /// How long to wait after each stop escalation step (quit, SIGTERM, SIGKILL).
    pub stop_step: Duration,
}

impl Default for Timings {
    fn default() -> Self {
        Self {
            startup: Duration::from_secs(10),
            ipc_connect: Duration::from_secs(5),
            stop_step: Duration::from_secs(2),
        }
    }
}

/// Which signal the stop sequence has reached.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopEscalation {
    QuitSent,
    TermSent,
    KillSent,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RendererState {
    Stopped,
    Starting {
        generation: u64,
        pause_on_ready: bool,
    },
    Playing {
        generation: u64,
    },
    Paused {
        generation: u64,
    },
    Stopping {
        generation: u64,
        escalation: StopEscalation,
    },
    Failed {
        reason: FailureReason,
        retry_at: Option<Instant>,
        pause_on_ready: bool,
    },
}

/// The state without payload, for display and D-Bus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RendererStateKind {
    Stopped,
    Starting,
    Playing,
    Paused,
    Stopping,
    Failed,
}

impl RendererStateKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stopped => "stopped",
            Self::Starting => "starting",
            Self::Playing => "playing",
            Self::Paused => "paused",
            Self::Stopping => "stopping",
            Self::Failed => "failed",
        }
    }
}

impl RendererState {
    pub fn kind(&self) -> RendererStateKind {
        match self {
            Self::Stopped => RendererStateKind::Stopped,
            Self::Starting { .. } => RendererStateKind::Starting,
            Self::Playing { .. } => RendererStateKind::Playing,
            Self::Paused { .. } => RendererStateKind::Paused,
            Self::Stopping { .. } => RendererStateKind::Stopping,
            Self::Failed { .. } => RendererStateKind::Failed,
        }
    }

    /// Generation of the process this state refers to, if any.
    pub fn generation(&self) -> Option<u64> {
        match self {
            Self::Starting { generation, .. }
            | Self::Playing { generation }
            | Self::Paused { generation }
            | Self::Stopping { generation, .. } => Some(*generation),
            Self::Stopped | Self::Failed { .. } => None,
        }
    }
}

/// What has happened (or what somebody wants).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RendererEvent {
    /// Intent: start playing. `paused` starts mpv paused so no frame of motion is shown.
    Start {
        paused: bool,
    },
    Pause,
    Resume,
    Stop,
    /// IPC connected and the file is loaded.
    Ready {
        generation: u64,
    },
    /// The process was reaped.
    Exited {
        generation: u64,
        exit: ExitInfo,
    },
    /// mpv reported that it could not play the file.
    MediaError {
        generation: u64,
        message: String,
    },
    LaunchError {
        generation: u64,
        error: LaunchError,
    },
    IpcLost {
        generation: u64,
        message: String,
    },
    StartupTimedOut {
        generation: u64,
    },
    StopTimedOut {
        generation: u64,
    },
    RetryDue,
}

/// What a driver must do as a consequence of an event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Effect {
    Spawn {
        generation: u64,
        start_paused: bool,
    },
    SetPause(bool),
    SendQuit,
    SendTerm,
    SendKill,
    ArmStartupTimer {
        generation: u64,
        after: Duration,
    },
    ArmStopTimer {
        generation: u64,
        after: Duration,
    },
    ArmRetryTimer {
        after: Duration,
    },
    CancelTimers,
    /// The observable state changed; publish it (coalesced by the driver).
    Notify,
    LogFailure(FailureReason),
}

/// How the machine treated an event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The event changed something.
    Applied,
    /// The intent was already satisfied; nothing to do.
    Normalized,
    /// The event belongs to a previous process (or cannot apply in this state) and was dropped.
    Ignored,
    /// The event is not allowed right now; the caller may retry later.
    Rejected(&'static str),
}

/// Observable renderer state, as published to the daemon and over D-Bus.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RendererSnapshot {
    pub state: RendererStateKind,
    pub generation: u64,
    /// Stable failure code, empty unless [`RendererStateKind::Failed`].
    pub failure_code: String,
    /// User-facing failure message, empty unless failed.
    pub failure_message: String,
    /// Restart attempts counted in the current window.
    pub restarts_in_window: u32,
    /// Process id, 0 if none.
    pub pid: u32,
}

impl Default for RendererSnapshot {
    fn default() -> Self {
        Self {
            state: RendererStateKind::Stopped,
            generation: 0,
            failure_code: String::new(),
            failure_message: String::new(),
            restarts_in_window: 0,
            pid: 0,
        }
    }
}

#[derive(Clone, Debug)]
pub struct RendererMachine {
    state: RendererState,
    next_generation: u64,
    restarts: RestartTracker,
    timings: Timings,
}

impl RendererMachine {
    pub fn new(policy: RestartPolicy, timings: Timings) -> Self {
        Self {
            state: RendererState::Stopped,
            next_generation: 1,
            restarts: RestartTracker::new(policy),
            timings,
        }
    }

    pub fn state(&self) -> &RendererState {
        &self.state
    }

    pub fn timings(&self) -> &Timings {
        &self.timings
    }

    pub fn restarts(&self) -> &RestartTracker {
        &self.restarts
    }

    pub fn failure(&self) -> Option<&FailureReason> {
        match &self.state {
            RendererState::Failed { reason, .. } => Some(reason),
            _ => None,
        }
    }

    #[cfg(test)]
    fn with_state(state: RendererState) -> Self {
        let mut machine = Self::new(RestartPolicy::default(), Timings::default());
        machine.next_generation = 2;
        machine.state = state;
        machine
    }

    /// Apply one event at time `now`.
    pub fn handle(&mut self, event: RendererEvent, now: Instant) -> (Outcome, Vec<Effect>) {
        use RendererEvent as E;
        match event {
            E::Start { paused } => self.start(paused),
            E::Pause => self.set_paused(true),
            E::Resume => self.set_paused(false),
            E::Stop => self.stop(),
            E::Ready { generation } => self.ready(generation),
            E::Exited { generation, exit } => self.exited(generation, exit, now),
            E::MediaError {
                generation,
                message,
            } => self.media_error(generation, message),
            E::LaunchError { generation, error } => self.launch_error(generation, error),
            E::IpcLost {
                generation,
                message,
            } => self.connection_problem(generation, FailureReason::IpcFailed(message), now),
            E::StartupTimedOut { generation } => {
                self.connection_problem(generation, FailureReason::StartupTimeout, now)
            }
            E::StopTimedOut { generation } => self.stop_timed_out(generation),
            E::RetryDue => self.retry_due(),
        }
    }

    fn begin_start(&mut self, paused: bool, mut effects: Vec<Effect>) -> (Outcome, Vec<Effect>) {
        let generation = self.next_generation;
        self.next_generation += 1;
        self.state = RendererState::Starting {
            generation,
            pause_on_ready: paused,
        };
        effects.push(Effect::Spawn {
            generation,
            start_paused: paused,
        });
        effects.push(Effect::ArmStartupTimer {
            generation,
            after: self.timings.startup,
        });
        effects.push(Effect::Notify);
        (Outcome::Applied, effects)
    }

    fn start(&mut self, paused: bool) -> (Outcome, Vec<Effect>) {
        match &self.state {
            RendererState::Stopped => self.begin_start(paused, vec![]),
            RendererState::Starting { .. }
            | RendererState::Playing { .. }
            | RendererState::Paused { .. } => (Outcome::Normalized, vec![]),
            RendererState::Stopping { .. } => (Outcome::Rejected("stopping"), vec![]),
            RendererState::Failed { .. } => {
                // An explicit start is a user action: the restart budget is renewed.
                self.restarts.reset();
                self.begin_start(paused, vec![Effect::CancelTimers])
            }
        }
    }

    fn set_paused(&mut self, pause: bool) -> (Outcome, Vec<Effect>) {
        match &mut self.state {
            RendererState::Starting { pause_on_ready, .. }
            | RendererState::Failed { pause_on_ready, .. } => {
                if *pause_on_ready == pause {
                    (Outcome::Normalized, vec![])
                } else {
                    *pause_on_ready = pause;
                    (Outcome::Applied, vec![])
                }
            }
            RendererState::Playing { generation } if pause => {
                self.state = RendererState::Paused {
                    generation: *generation,
                };
                (
                    Outcome::Applied,
                    vec![Effect::SetPause(true), Effect::Notify],
                )
            }
            RendererState::Paused { generation } if !pause => {
                self.state = RendererState::Playing {
                    generation: *generation,
                };
                (
                    Outcome::Applied,
                    vec![Effect::SetPause(false), Effect::Notify],
                )
            }
            _ => (Outcome::Normalized, vec![]),
        }
    }

    fn stop(&mut self) -> (Outcome, Vec<Effect>) {
        match &self.state {
            RendererState::Stopped | RendererState::Stopping { .. } => {
                (Outcome::Normalized, vec![])
            }
            RendererState::Starting { generation, .. }
            | RendererState::Playing { generation }
            | RendererState::Paused { generation } => {
                let generation = *generation;
                self.state = RendererState::Stopping {
                    generation,
                    escalation: StopEscalation::QuitSent,
                };
                (
                    Outcome::Applied,
                    vec![
                        Effect::CancelTimers,
                        Effect::SendQuit,
                        Effect::ArmStopTimer {
                            generation,
                            after: self.timings.stop_step,
                        },
                        Effect::Notify,
                    ],
                )
            }
            RendererState::Failed { .. } => {
                self.state = RendererState::Stopped;
                (Outcome::Applied, vec![Effect::CancelTimers, Effect::Notify])
            }
        }
    }

    fn ready(&mut self, g: u64) -> (Outcome, Vec<Effect>) {
        match &self.state {
            RendererState::Starting {
                generation,
                pause_on_ready,
            } if *generation == g => {
                let mut effects = vec![Effect::CancelTimers];
                if *pause_on_ready {
                    self.state = RendererState::Paused { generation: g };
                    effects.push(Effect::SetPause(true));
                } else {
                    self.state = RendererState::Playing { generation: g };
                }
                effects.push(Effect::Notify);
                (Outcome::Applied, effects)
            }
            RendererState::Playing { generation }
            | RendererState::Paused { generation }
            | RendererState::Stopping { generation, .. }
                if *generation == g =>
            {
                (Outcome::Normalized, vec![])
            }
            _ => (Outcome::Ignored, vec![]),
        }
    }

    fn exited(&mut self, g: u64, exit: ExitInfo, now: Instant) -> (Outcome, Vec<Effect>) {
        match &self.state {
            RendererState::Starting { generation, .. }
            | RendererState::Playing { generation }
            | RendererState::Paused { generation }
                if *generation == g =>
            {
                match exit.code {
                    // mpv exit codes: 1 = initialisation error, 2 = file could not be played.
                    // Both are deterministic, so retrying would only repeat the failure.
                    Some(1) => self.fail_terminal(
                        FailureReason::LaunchFailed(
                            "mpv exited with status 1 (initialisation error)".to_owned(),
                        ),
                        vec![Effect::CancelTimers],
                    ),
                    Some(2) => self.fail_terminal(
                        FailureReason::MediaUnsupported(
                            "mpv exited with status 2 (the file could not be played)".to_owned(),
                        ),
                        vec![Effect::CancelTimers],
                    ),
                    Some(0) => self.crash(FailureReason::UnexpectedExit(exit), now, vec![]),
                    _ => self.crash(FailureReason::Crashed(exit), now, vec![]),
                }
            }
            RendererState::Stopping { generation, .. } if *generation == g => {
                self.state = RendererState::Stopped;
                (Outcome::Applied, vec![Effect::CancelTimers, Effect::Notify])
            }
            // Reaping a process we already gave up on: bookkeeping only.
            RendererState::Failed { .. } => (Outcome::Normalized, vec![]),
            _ => (Outcome::Ignored, vec![]),
        }
    }

    fn media_error(&mut self, g: u64, message: String) -> (Outcome, Vec<Effect>) {
        match &self.state {
            RendererState::Starting { generation, .. }
            | RendererState::Playing { generation }
            | RendererState::Paused { generation }
                if *generation == g =>
            {
                self.fail_terminal(
                    FailureReason::MediaUnsupported(message),
                    vec![Effect::CancelTimers, Effect::SendKill],
                )
            }
            RendererState::Stopping { generation, .. } if *generation == g => {
                (Outcome::Normalized, vec![])
            }
            _ => (Outcome::Ignored, vec![]),
        }
    }

    fn launch_error(&mut self, g: u64, error: LaunchError) -> (Outcome, Vec<Effect>) {
        match &self.state {
            RendererState::Starting { generation, .. } if *generation == g => {
                let reason = match error {
                    LaunchError::NotFound => FailureReason::MpvMissing,
                    LaunchError::MediaMissing => FailureReason::MediaMissing,
                    LaunchError::Other(why) => FailureReason::LaunchFailed(why),
                };
                self.fail_terminal(reason, vec![Effect::CancelTimers])
            }
            RendererState::Stopping { generation, .. } if *generation == g => {
                (Outcome::Normalized, vec![])
            }
            _ => (Outcome::Ignored, vec![]),
        }
    }

    /// `IpcLost` and `StartupTimedOut`: the process may still be alive, so kill it, then apply
    /// the bounded restart rule.
    fn connection_problem(
        &mut self,
        g: u64,
        reason: FailureReason,
        now: Instant,
    ) -> (Outcome, Vec<Effect>) {
        let startup_timeout = matches!(reason, FailureReason::StartupTimeout);
        match &self.state {
            // A startup timeout only means something while starting. If a late timer fires after
            // the renderer became ready it must not kill a healthy process.
            RendererState::Playing { generation } | RendererState::Paused { generation }
                if *generation == g && startup_timeout =>
            {
                (Outcome::Ignored, vec![])
            }
            RendererState::Starting { generation, .. }
            | RendererState::Playing { generation }
            | RendererState::Paused { generation }
                if *generation == g =>
            {
                self.crash(reason, now, vec![Effect::SendKill])
            }
            // Losing the socket while stopping is expected.
            RendererState::Stopping { generation, .. } if *generation == g => {
                (Outcome::Normalized, vec![])
            }
            _ => (Outcome::Ignored, vec![]),
        }
    }

    fn stop_timed_out(&mut self, g: u64) -> (Outcome, Vec<Effect>) {
        match &mut self.state {
            RendererState::Stopping {
                generation,
                escalation,
            } if *generation == g => {
                let arm = Effect::ArmStopTimer {
                    generation: g,
                    after: self.timings.stop_step,
                };
                let signal = match escalation {
                    StopEscalation::QuitSent => {
                        *escalation = StopEscalation::TermSent;
                        Effect::SendTerm
                    }
                    StopEscalation::TermSent => {
                        *escalation = StopEscalation::KillSent;
                        Effect::SendKill
                    }
                    // Nothing stronger exists; keep insisting.
                    StopEscalation::KillSent => Effect::SendKill,
                };
                (Outcome::Applied, vec![signal, arm])
            }
            _ => (Outcome::Ignored, vec![]),
        }
    }

    fn retry_due(&mut self) -> (Outcome, Vec<Effect>) {
        match &self.state {
            RendererState::Failed {
                retry_at: Some(_),
                pause_on_ready,
                ..
            } => {
                let paused = *pause_on_ready;
                self.begin_start(paused, vec![])
            }
            _ => (Outcome::Normalized, vec![]),
        }
    }

    /// Failed with no automatic retry (the failure is deterministic).
    fn fail_terminal(
        &mut self,
        reason: FailureReason,
        mut effects: Vec<Effect>,
    ) -> (Outcome, Vec<Effect>) {
        let pause_on_ready = self.wants_pause();
        self.state = RendererState::Failed {
            reason: reason.clone(),
            retry_at: None,
            pause_on_ready,
        };
        effects.push(Effect::LogFailure(reason));
        effects.push(Effect::Notify);
        (Outcome::Applied, effects)
    }

    /// The crash rule: fail, then either schedule a bounded retry or give up.
    fn crash(
        &mut self,
        reason: FailureReason,
        now: Instant,
        pre: Vec<Effect>,
    ) -> (Outcome, Vec<Effect>) {
        let pause_on_ready = self.wants_pause();
        let mut effects = vec![Effect::CancelTimers];
        effects.extend(pre);
        match self.restarts.on_failure(now) {
            RestartDecision::RetryAfter(after) => {
                self.state = RendererState::Failed {
                    reason: reason.clone(),
                    retry_at: Some(now + after),
                    pause_on_ready,
                };
                effects.push(Effect::LogFailure(reason));
                effects.push(Effect::ArmRetryTimer { after });
            }
            RestartDecision::GiveUp => {
                let limited = FailureReason::RestartLimit(Box::new(reason));
                self.state = RendererState::Failed {
                    reason: limited.clone(),
                    retry_at: None,
                    pause_on_ready,
                };
                effects.push(Effect::LogFailure(limited));
            }
        }
        effects.push(Effect::Notify);
        (Outcome::Applied, effects)
    }

    /// Whether a restart should come back paused.
    fn wants_pause(&self) -> bool {
        match &self.state {
            RendererState::Starting { pause_on_ready, .. }
            | RendererState::Failed { pause_on_ready, .. } => *pause_on_ready,
            RendererState::Paused { .. } => true,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use RendererEvent as E;

    fn machine() -> RendererMachine {
        RendererMachine::new(RestartPolicy::default(), Timings::default())
    }

    fn crash_event(g: u64) -> E {
        E::Exited {
            generation: g,
            exit: ExitInfo::signal(6),
        }
    }

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    #[test]
    fn happy_path_start_ready_pause_resume_stop() {
        let t0 = Instant::now();
        let mut m = machine();

        let (outcome, effects) = m.handle(E::Start { paused: false }, t0);
        assert_eq!(outcome, Outcome::Applied);
        assert_eq!(
            effects,
            vec![
                Effect::Spawn {
                    generation: 1,
                    start_paused: false
                },
                Effect::ArmStartupTimer {
                    generation: 1,
                    after: secs(10)
                },
                Effect::Notify,
            ]
        );

        let (_, effects) = m.handle(E::Ready { generation: 1 }, t0);
        assert_eq!(m.state(), &RendererState::Playing { generation: 1 });
        assert_eq!(effects, vec![Effect::CancelTimers, Effect::Notify]);

        let (_, effects) = m.handle(E::Pause, t0);
        assert_eq!(m.state(), &RendererState::Paused { generation: 1 });
        assert_eq!(effects, vec![Effect::SetPause(true), Effect::Notify]);

        let (_, effects) = m.handle(E::Resume, t0);
        assert_eq!(m.state(), &RendererState::Playing { generation: 1 });
        assert_eq!(effects, vec![Effect::SetPause(false), Effect::Notify]);

        let (_, effects) = m.handle(E::Stop, t0);
        assert_eq!(
            effects,
            vec![
                Effect::CancelTimers,
                Effect::SendQuit,
                Effect::ArmStopTimer {
                    generation: 1,
                    after: secs(2)
                },
                Effect::Notify,
            ]
        );
        let (_, effects) = m.handle(
            E::Exited {
                generation: 1,
                exit: ExitInfo::code(0),
            },
            t0,
        );
        assert_eq!(m.state(), &RendererState::Stopped);
        assert_eq!(effects, vec![Effect::CancelTimers, Effect::Notify]);
    }

    #[test]
    fn starting_paused_spawns_paused_and_stays_paused_on_ready() {
        let t0 = Instant::now();
        let mut m = machine();
        let (_, effects) = m.handle(E::Start { paused: true }, t0);
        assert!(effects.contains(&Effect::Spawn {
            generation: 1,
            start_paused: true
        }));
        let (_, effects) = m.handle(E::Ready { generation: 1 }, t0);
        assert_eq!(m.state(), &RendererState::Paused { generation: 1 });
        assert!(effects.contains(&Effect::SetPause(true)));
    }

    #[test]
    fn pause_requested_while_starting_applies_on_ready() {
        let t0 = Instant::now();
        let mut m = machine();
        m.handle(E::Start { paused: false }, t0);
        assert_eq!(m.handle(E::Pause, t0).0, Outcome::Applied);
        assert_eq!(m.handle(E::Pause, t0).0, Outcome::Normalized);
        m.handle(E::Ready { generation: 1 }, t0);
        assert_eq!(m.state(), &RendererState::Paused { generation: 1 });

        // ...and a resume while starting cancels it again.
        let mut m = machine();
        m.handle(E::Start { paused: false }, t0);
        m.handle(E::Pause, t0);
        m.handle(E::Resume, t0);
        m.handle(E::Ready { generation: 1 }, t0);
        assert_eq!(m.state(), &RendererState::Playing { generation: 1 });
    }

    #[test]
    fn crash_schedules_a_retry_and_retry_due_spawns_a_new_generation() {
        let t0 = Instant::now();
        let mut m = machine();
        m.handle(E::Start { paused: false }, t0);
        m.handle(E::Ready { generation: 1 }, t0);

        let (outcome, effects) = m.handle(crash_event(1), t0);
        assert_eq!(outcome, Outcome::Applied);
        let reason = FailureReason::Crashed(ExitInfo::signal(6));
        assert_eq!(
            effects,
            vec![
                Effect::CancelTimers,
                Effect::LogFailure(reason.clone()),
                Effect::ArmRetryTimer { after: secs(1) },
                Effect::Notify,
            ]
        );
        assert_eq!(
            m.state(),
            &RendererState::Failed {
                reason,
                retry_at: Some(t0 + secs(1)),
                pause_on_ready: false
            }
        );

        let (_, effects) = m.handle(E::RetryDue, t0 + secs(1));
        assert_eq!(
            m.state(),
            &RendererState::Starting {
                generation: 2,
                pause_on_ready: false
            }
        );
        assert!(effects.contains(&Effect::Spawn {
            generation: 2,
            start_paused: false
        }));
    }

    #[test]
    fn restart_storm_ends_in_restart_limit_after_three_retries() {
        let t0 = Instant::now();
        let mut m = machine();
        m.handle(E::Start { paused: false }, t0);
        let mut spawns = 1;
        for (i, generation) in (1..=4u64).enumerate() {
            let now = t0 + secs(i as u64);
            m.handle(E::Ready { generation }, now);
            m.handle(crash_event(generation), now);
            if matches!(
                m.state(),
                RendererState::Failed {
                    retry_at: Some(_),
                    ..
                }
            ) {
                m.handle(E::RetryDue, now);
                spawns += 1;
            }
        }
        assert_eq!(
            spawns, 4,
            "initial start plus exactly three automatic restarts"
        );
        match m.state() {
            RendererState::Failed {
                reason,
                retry_at: None,
                ..
            } => {
                assert_eq!(reason.code(), "restart-limit");
                assert_eq!(
                    reason.root_cause(),
                    &FailureReason::Crashed(ExitInfo::signal(6))
                );
            }
            other => panic!("expected terminal failure, got {other:?}"),
        }
        // No further automatic restart.
        assert_eq!(m.handle(E::RetryDue, t0 + secs(100)).0, Outcome::Normalized);
    }

    #[test]
    fn explicit_start_after_giving_up_renews_the_budget() {
        let t0 = Instant::now();
        let mut m = machine();
        m.handle(E::Start { paused: false }, t0);
        for generation in 1..=4u64 {
            m.handle(crash_event(generation), t0);
            m.handle(E::RetryDue, t0);
        }
        assert!(matches!(
            m.state(),
            RendererState::Failed { retry_at: None, .. }
        ));
        let (outcome, effects) = m.handle(E::Start { paused: false }, t0);
        assert_eq!(outcome, Outcome::Applied);
        assert_eq!(effects[0], Effect::CancelTimers);
        assert_eq!(m.restarts().count_in_window(t0), 0);
        m.handle(E::Ready { generation: 5 }, t0);
        m.handle(crash_event(5), t0);
        assert!(matches!(
            m.state(),
            RendererState::Failed {
                retry_at: Some(_),
                ..
            }
        ));
    }

    #[test]
    fn exit_codes_are_classified() {
        let t0 = Instant::now();
        let cases = [
            (ExitInfo::code(1), "launch-failed", false),
            (ExitInfo::code(2), "media-unsupported", false),
            (ExitInfo::code(0), "unexpected-exit", true),
            (ExitInfo::code(3), "crashed", true),
            (ExitInfo::signal(11), "crashed", true),
        ];
        for (exit, code, retries) in cases {
            let mut m = machine();
            m.handle(E::Start { paused: false }, t0);
            m.handle(E::Ready { generation: 1 }, t0);
            m.handle(
                E::Exited {
                    generation: 1,
                    exit,
                },
                t0,
            );
            match m.state() {
                RendererState::Failed {
                    reason, retry_at, ..
                } => {
                    assert_eq!(reason.code(), code, "{exit}");
                    assert_eq!(retry_at.is_some(), retries, "{exit}");
                }
                other => panic!("{exit}: {other:?}"),
            }
        }
    }

    #[test]
    fn media_error_is_terminal_and_kills_the_process() {
        let t0 = Instant::now();
        let mut m = machine();
        m.handle(E::Start { paused: false }, t0);
        let (_, effects) = m.handle(
            E::MediaError {
                generation: 1,
                message: "unrecognized file format".into(),
            },
            t0,
        );
        assert!(effects.contains(&Effect::SendKill));
        assert!(
            !effects
                .iter()
                .any(|e| matches!(e, Effect::ArmRetryTimer { .. }))
        );
        assert_eq!(
            m.failure().map(FailureReason::code),
            Some("media-unsupported")
        );
        // The reaped process afterwards is only bookkeeping.
        assert_eq!(
            m.handle(
                E::Exited {
                    generation: 1,
                    exit: ExitInfo::code(2)
                },
                t0
            )
            .0,
            Outcome::Normalized
        );
        assert_eq!(
            m.failure().map(FailureReason::code),
            Some("media-unsupported")
        );
    }

    #[test]
    fn launch_errors_map_to_specific_reasons() {
        let t0 = Instant::now();
        for (error, code) in [
            (LaunchError::NotFound, "mpv-missing"),
            (LaunchError::MediaMissing, "media-missing"),
            (LaunchError::Other("boom".into()), "launch-failed"),
        ] {
            let mut m = machine();
            m.handle(E::Start { paused: false }, t0);
            let (_, effects) = m.handle(
                E::LaunchError {
                    generation: 1,
                    error,
                },
                t0,
            );
            assert_eq!(m.failure().map(FailureReason::code), Some(code));
            assert!(
                !effects.contains(&Effect::SendKill),
                "no process exists yet"
            );
        }
    }

    #[test]
    fn stop_escalates_quit_term_kill() {
        let t0 = Instant::now();
        let mut m = machine();
        m.handle(E::Start { paused: false }, t0);
        m.handle(E::Ready { generation: 1 }, t0);
        m.handle(E::Stop, t0);
        let (_, e) = m.handle(E::StopTimedOut { generation: 1 }, t0);
        assert_eq!(e[0], Effect::SendTerm);
        let (_, e) = m.handle(E::StopTimedOut { generation: 1 }, t0);
        assert_eq!(e[0], Effect::SendKill);
        assert_eq!(
            m.state(),
            &RendererState::Stopping {
                generation: 1,
                escalation: StopEscalation::KillSent
            }
        );
        // A stale stop timer does nothing.
        assert_eq!(
            m.handle(E::StopTimedOut { generation: 7 }, t0).0,
            Outcome::Ignored
        );
    }

    #[test]
    fn late_startup_timeout_cannot_kill_a_healthy_renderer() {
        let t0 = Instant::now();
        let mut m = machine();
        m.handle(E::Start { paused: false }, t0);
        m.handle(E::Ready { generation: 1 }, t0);
        let (outcome, effects) = m.handle(E::StartupTimedOut { generation: 1 }, t0);
        assert_eq!(outcome, Outcome::Ignored);
        assert!(effects.is_empty());
        assert_eq!(m.state(), &RendererState::Playing { generation: 1 });
    }

    #[test]
    fn startup_timeout_and_ipc_loss_kill_then_apply_the_restart_rule() {
        let t0 = Instant::now();
        let mut m = machine();
        m.handle(E::Start { paused: false }, t0);
        let (_, effects) = m.handle(E::StartupTimedOut { generation: 1 }, t0);
        assert_eq!(effects[..2], [Effect::CancelTimers, Effect::SendKill]);
        assert_eq!(
            m.failure().map(FailureReason::code),
            Some("startup-timeout")
        );

        let mut m = machine();
        m.handle(E::Start { paused: false }, t0);
        m.handle(E::Ready { generation: 1 }, t0);
        m.handle(
            E::IpcLost {
                generation: 1,
                message: "eof".into(),
            },
            t0,
        );
        assert_eq!(m.failure().map(FailureReason::code), Some("ipc-failed"));
        assert!(matches!(
            m.state(),
            RendererState::Failed {
                retry_at: Some(_),
                ..
            }
        ));
    }

    #[test]
    fn a_paused_renderer_that_crashes_restarts_paused() {
        let t0 = Instant::now();
        let mut m = machine();
        m.handle(E::Start { paused: false }, t0);
        m.handle(E::Ready { generation: 1 }, t0);
        m.handle(E::Pause, t0);
        m.handle(crash_event(1), t0);
        let (_, effects) = m.handle(E::RetryDue, t0 + secs(1));
        assert!(effects.contains(&Effect::Spawn {
            generation: 2,
            start_paused: true
        }));
    }

    #[test]
    fn stale_generations_are_ignored_everywhere() {
        let t0 = Instant::now();
        let mut m = machine();
        m.handle(E::Start { paused: false }, t0);
        m.handle(E::Ready { generation: 1 }, t0);
        for event in [
            E::Ready { generation: 0 },
            E::Exited {
                generation: 0,
                exit: ExitInfo::signal(9),
            },
            E::MediaError {
                generation: 0,
                message: "x".into(),
            },
            E::IpcLost {
                generation: 0,
                message: "x".into(),
            },
            E::StartupTimedOut { generation: 0 },
        ] {
            let (outcome, effects) = m.handle(event, t0);
            assert_eq!(outcome, Outcome::Ignored);
            assert!(effects.is_empty());
        }
        assert_eq!(m.state(), &RendererState::Playing { generation: 1 });
    }

    #[test]
    fn start_while_stopping_is_rejected_not_queued() {
        let t0 = Instant::now();
        let mut m = machine();
        m.handle(E::Start { paused: false }, t0);
        m.handle(E::Stop, t0);
        let (outcome, effects) = m.handle(E::Start { paused: false }, t0);
        assert_eq!(outcome, Outcome::Rejected("stopping"));
        assert!(effects.is_empty());
    }

    #[test]
    fn stop_from_failed_cancels_the_pending_retry() {
        let t0 = Instant::now();
        let mut m = machine();
        m.handle(E::Start { paused: false }, t0);
        m.handle(crash_event(1), t0);
        let (_, effects) = m.handle(E::Stop, t0);
        assert_eq!(m.state(), &RendererState::Stopped);
        assert_eq!(effects, vec![Effect::CancelTimers, Effect::Notify]);
    }

    /// Every state × event cell of the transition table.
    ///
    /// Cell format: `<outcome>><resulting state>` where the outcome is A(pplied), N(ormalized),
    /// I(gnored) or R(ejected). Generation-carrying events use generation 1 unless the column
    /// says `99` (a stale generation).
    #[test]
    fn transition_table() {
        let columns = [
            "start",
            "pause",
            "resume",
            "stop",
            "ready",
            "ready99",
            "exit",
            "exit99",
            "mediaerr",
            "launcherr",
            "ipclost",
            "startto",
            "stopto",
            "retry",
        ];
        let rows: [(&str, [&str; 14]); 7] = [
            (
                "stopped",
                [
                    "A>starting",
                    "N>stopped",
                    "N>stopped",
                    "N>stopped",
                    "I>stopped",
                    "I>stopped",
                    "I>stopped",
                    "I>stopped",
                    "I>stopped",
                    "I>stopped",
                    "I>stopped",
                    "I>stopped",
                    "I>stopped",
                    "N>stopped",
                ],
            ),
            (
                "starting",
                [
                    "N>starting",
                    "A>starting",
                    "N>starting",
                    "A>stopping",
                    "A>playing",
                    "I>starting",
                    "A>failed",
                    "I>starting",
                    "A>failed",
                    "A>failed",
                    "A>failed",
                    "A>failed",
                    "I>starting",
                    "N>starting",
                ],
            ),
            (
                "playing",
                [
                    "N>playing",
                    "A>paused",
                    "N>playing",
                    "A>stopping",
                    "N>playing",
                    "I>playing",
                    "A>failed",
                    "I>playing",
                    "A>failed",
                    "I>playing",
                    "A>failed",
                    "I>playing",
                    "I>playing",
                    "N>playing",
                ],
            ),
            (
                "paused",
                [
                    "N>paused",
                    "N>paused",
                    "A>playing",
                    "A>stopping",
                    "N>paused",
                    "I>paused",
                    "A>failed",
                    "I>paused",
                    "A>failed",
                    "I>paused",
                    "A>failed",
                    "I>paused",
                    "I>paused",
                    "N>paused",
                ],
            ),
            (
                "stopping",
                [
                    "R>stopping",
                    "N>stopping",
                    "N>stopping",
                    "N>stopping",
                    "N>stopping",
                    "I>stopping",
                    "A>stopped",
                    "I>stopping",
                    "N>stopping",
                    "N>stopping",
                    "N>stopping",
                    "N>stopping",
                    "A>stopping",
                    "N>stopping",
                ],
            ),
            (
                "failed-retry",
                [
                    "A>starting",
                    "A>failed",
                    "N>failed",
                    "A>stopped",
                    "I>failed",
                    "I>failed",
                    "N>failed",
                    "N>failed",
                    "I>failed",
                    "I>failed",
                    "I>failed",
                    "I>failed",
                    "I>failed",
                    "A>starting",
                ],
            ),
            (
                "failed-final",
                [
                    "A>starting",
                    "A>failed",
                    "N>failed",
                    "A>stopped",
                    "I>failed",
                    "I>failed",
                    "N>failed",
                    "N>failed",
                    "I>failed",
                    "I>failed",
                    "I>failed",
                    "I>failed",
                    "I>failed",
                    "N>failed",
                ],
            ),
        ];

        let t0 = Instant::now();
        let fixture = |label: &str| -> RendererState {
            match label {
                "stopped" => RendererState::Stopped,
                "starting" => RendererState::Starting {
                    generation: 1,
                    pause_on_ready: false,
                },
                "playing" => RendererState::Playing { generation: 1 },
                "paused" => RendererState::Paused { generation: 1 },
                "stopping" => RendererState::Stopping {
                    generation: 1,
                    escalation: StopEscalation::QuitSent,
                },
                "failed-retry" => RendererState::Failed {
                    reason: FailureReason::Crashed(ExitInfo::signal(6)),
                    retry_at: Some(t0),
                    pause_on_ready: false,
                },
                "failed-final" => RendererState::Failed {
                    reason: FailureReason::MediaMissing,
                    retry_at: None,
                    pause_on_ready: false,
                },
                other => panic!("unknown fixture {other}"),
            }
        };
        let event = |label: &str| -> RendererEvent {
            match label {
                "start" => E::Start { paused: false },
                "pause" => E::Pause,
                "resume" => E::Resume,
                "stop" => E::Stop,
                "ready" => E::Ready { generation: 1 },
                "ready99" => E::Ready { generation: 99 },
                "exit" => E::Exited {
                    generation: 1,
                    exit: ExitInfo::signal(6),
                },
                "exit99" => E::Exited {
                    generation: 99,
                    exit: ExitInfo::signal(6),
                },
                "mediaerr" => E::MediaError {
                    generation: 1,
                    message: "bad".into(),
                },
                "launcherr" => E::LaunchError {
                    generation: 1,
                    error: LaunchError::NotFound,
                },
                "ipclost" => E::IpcLost {
                    generation: 1,
                    message: "eof".into(),
                },
                "startto" => E::StartupTimedOut { generation: 1 },
                "stopto" => E::StopTimedOut { generation: 1 },
                "retry" => E::RetryDue,
                other => panic!("unknown event {other}"),
            }
        };

        for (state_label, cells) in rows {
            for (column, cell) in columns.iter().zip(cells) {
                let mut m = RendererMachine::with_state(fixture(state_label));
                let (outcome, _) = m.handle(event(column), t0);
                let (want_outcome, want_kind) = cell.split_once('>').expect("cell format");
                let got_outcome = match outcome {
                    Outcome::Applied => "A",
                    Outcome::Normalized => "N",
                    Outcome::Ignored => "I",
                    Outcome::Rejected(_) => "R",
                };
                assert_eq!(
                    (got_outcome, m.state().kind().as_str()),
                    (want_outcome, want_kind),
                    "state={state_label} event={column}"
                );
            }
        }
    }
}
