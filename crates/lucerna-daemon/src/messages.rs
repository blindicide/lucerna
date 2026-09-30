//! Messages between the D-Bus service, backend threads, renderer supervisors and the engine.

use lucerna_core::backend::BackendEvent;
use lucerna_ipc::LucernaError;
use lucerna_ipc::dict::Dict;
use lucerna_ipc::dto::SettingsPatch;
use lucerna_mpv::SupervisorEvent;
use tokio::sync::oneshot;

/// A request from a D-Bus client.
#[derive(Debug)]
pub enum Cmd {
    GetStatus,
    GetDisplays,
    GetAssignments,
    ListWallpapers,
    AddWallpaper {
        path: String,
        name: String,
    },
    RemoveWallpaper {
        id: String,
    },
    SetWallpaper {
        wallpaper_id: String,
        display_id: String,
    },
    ClearAssignment {
        display_id: String,
    },
    SetScaling {
        display_id: String,
        mode: String,
    },
    GetSettings,
    SetSettings {
        patch: SettingsPatch,
    },
    Pause,
    Resume,
    Stop,
    Start,
    Reload,
    Quit,
    GetDiagnostics {
        redact: bool,
    },
}

#[derive(Debug)]
pub enum Reply {
    Unit,
    Dict(Dict),
    Dicts(Vec<Dict>),
    Text(String),
}

pub type ReplyTx = oneshot::Sender<Result<Reply, LucernaError>>;

pub enum EngineMsg {
    Command {
        cmd: Cmd,
        reply: ReplyTx,
    },
    Backend(BackendEvent),
    Renderer(SupervisorEvent),
    /// The screen locked (`true`) or unlocked (`false`). Sent by the lock monitor.
    Lock(bool),
    /// A termination signal arrived.
    Signal(&'static str),
}
