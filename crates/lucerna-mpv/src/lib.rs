//! mpv process supervision and JSON IPC (directive §9).
//!
//! This crate executes the decisions made by the pure state machine in `lucerna-core`: it
//! spawns mpv without a shell, talks to it over its private IPC socket, captures its output into
//! bounded logs, and keeps the child processes accounted for so that stale ones can be recovered
//! after a daemon crash.

mod capture;
mod ipc;
mod proc;
mod registry;
mod supervisor;

pub use ipc::{IpcError, MpvEvent, MpvIpc};
pub use registry::{PidRecord, PidRegistry, RecoveryOptions, RecoveryReport, recover_stale};
pub use supervisor::{LaunchSettings, RendererSupervisor, SupervisorConfig, SupervisorEvent};
