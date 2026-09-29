//! The renderer lifecycle as pure data and pure functions (directive §9, §51).
//!
//! Nothing here starts a process or touches a clock: [`RendererMachine::handle`] takes the event
//! and the current time and returns [`Effect`]s for a driver to execute. That is what lets the
//! whole policy be tested without launching mpv.

mod failure;
mod machine;
mod restart;

pub use failure::{ExitInfo, FailureReason, LaunchError};
pub use machine::{
    Effect, Outcome, RendererEvent, RendererMachine, RendererSnapshot, RendererState,
    RendererStateKind, StopEscalation, Timings,
};
pub use restart::{RestartDecision, RestartPolicy, RestartTracker};
