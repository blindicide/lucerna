//! The D-Bus interface object: a thin translator between method calls and engine messages.
//! It owns no state; the engine is the only owner of configuration, backend and renderers.

use lucerna_ipc::LucernaError;
use lucerna_ipc::dict::Dict;
use lucerna_ipc::dto::SettingsPatch;
use lucerna_ipc::names::API_VERSION;
use tokio::sync::{mpsc, oneshot};
use zbus::object_server::SignalEmitter;

use crate::messages::{Cmd, EngineMsg, Reply};

pub struct LucernaService {
    pub tx: mpsc::UnboundedSender<EngineMsg>,
}

impl LucernaService {
    async fn call(&self, cmd: Cmd) -> Result<Reply, LucernaError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(EngineMsg::Command { cmd, reply })
            .map_err(|_| {
                LucernaError::Internal("The Lucerna daemon is shutting down.".to_owned())
            })?;
        rx.await.map_err(|_| {
            LucernaError::Internal("The Lucerna daemon is shutting down.".to_owned())
        })?
    }

    async fn unit(&self, cmd: Cmd) -> Result<(), LucernaError> {
        self.call(cmd).await.map(|_| ())
    }
}

fn wrong_reply() -> LucernaError {
    LucernaError::Internal("internal error: unexpected reply type".to_owned())
}

#[zbus::interface(name = "org.lucerna.Lucerna1")]
impl LucernaService {
    async fn get_status(&self) -> Result<Dict, LucernaError> {
        match self.call(Cmd::GetStatus).await? {
            Reply::Dict(d) => Ok(d),
            _ => Err(wrong_reply()),
        }
    }

    async fn get_displays(&self) -> Result<Vec<Dict>, LucernaError> {
        match self.call(Cmd::GetDisplays).await? {
            Reply::Dicts(d) => Ok(d),
            _ => Err(wrong_reply()),
        }
    }

    async fn get_assignments(&self) -> Result<Vec<Dict>, LucernaError> {
        match self.call(Cmd::GetAssignments).await? {
            Reply::Dicts(d) => Ok(d),
            _ => Err(wrong_reply()),
        }
    }

    async fn list_wallpapers(&self) -> Result<Vec<Dict>, LucernaError> {
        match self.call(Cmd::ListWallpapers).await? {
            Reply::Dicts(d) => Ok(d),
            _ => Err(wrong_reply()),
        }
    }

    async fn add_wallpaper(&self, path: String, name: String) -> Result<String, LucernaError> {
        match self.call(Cmd::AddWallpaper { path, name }).await? {
            Reply::Text(id) => Ok(id),
            _ => Err(wrong_reply()),
        }
    }

    async fn remove_wallpaper(&self, wallpaper_id: String) -> Result<(), LucernaError> {
        self.unit(Cmd::RemoveWallpaper { id: wallpaper_id }).await
    }

    async fn set_wallpaper(
        &self,
        wallpaper_id: String,
        display_id: String,
    ) -> Result<(), LucernaError> {
        self.unit(Cmd::SetWallpaper {
            wallpaper_id,
            display_id,
        })
        .await
    }

    async fn clear_assignment(&self, display_id: String) -> Result<(), LucernaError> {
        self.unit(Cmd::ClearAssignment { display_id }).await
    }

    async fn set_scaling(&self, display_id: String, mode: String) -> Result<(), LucernaError> {
        self.unit(Cmd::SetScaling { display_id, mode }).await
    }

    async fn get_settings(&self) -> Result<Dict, LucernaError> {
        match self.call(Cmd::GetSettings).await? {
            Reply::Dict(d) => Ok(d),
            _ => Err(wrong_reply()),
        }
    }

    async fn set_settings(&self, changes: Dict) -> Result<(), LucernaError> {
        let patch = SettingsPatch::from_dict(&changes).map_err(LucernaError::InvalidArgument)?;
        self.unit(Cmd::SetSettings { patch }).await
    }

    async fn pause(&self) -> Result<(), LucernaError> {
        self.unit(Cmd::Pause).await
    }

    async fn resume(&self) -> Result<(), LucernaError> {
        self.unit(Cmd::Resume).await
    }

    async fn stop(&self) -> Result<(), LucernaError> {
        self.unit(Cmd::Stop).await
    }

    async fn start(&self) -> Result<(), LucernaError> {
        self.unit(Cmd::Start).await
    }

    async fn reload(&self) -> Result<(), LucernaError> {
        self.unit(Cmd::Reload).await
    }

    async fn quit(&self) -> Result<(), LucernaError> {
        self.unit(Cmd::Quit).await
    }

    async fn get_diagnostics(&self, redact: bool) -> Result<String, LucernaError> {
        match self.call(Cmd::GetDiagnostics { redact }).await? {
            Reply::Text(text) => Ok(text),
            _ => Err(wrong_reply()),
        }
    }

    #[zbus(signal)]
    pub async fn status_changed(emitter: &SignalEmitter<'_>, status: Dict) -> zbus::Result<()>;

    #[zbus(signal)]
    pub async fn displays_changed(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;

    #[zbus(signal)]
    pub async fn library_changed(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;

    #[zbus(signal)]
    pub async fn settings_changed(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;

    #[zbus(signal)]
    pub async fn renderer_failed(
        emitter: &SignalEmitter<'_>,
        display_id: &str,
        code: &str,
        message: &str,
    ) -> zbus::Result<()>;

    #[zbus(property)]
    fn version(&self) -> String {
        lucerna_core::version::VERSION.to_owned()
    }

    #[zbus(property)]
    fn api_version(&self) -> u32 {
        API_VERSION
    }
}
