//! The GUI's connection to the daemon: a thin async wrapper over the D-Bus proxy.
//!
//! Runtime-agnostic (plain futures over zbus), so it runs on the glib main context in the app and
//! on any executor in tests. It owns no wallpaper logic: the GUI never runs a renderer (§5).

use std::path::{Path, PathBuf};
use std::pin::Pin;

use futures_util::stream::{self, Stream, StreamExt};
use lucerna_ipc::dto::{DisplayDto, SettingsDto, SettingsPatch, StatusDto, WallpaperDto};
use lucerna_ipc::error::is_daemon_absent;
use lucerna_ipc::{LucernaError, LucernaProxy};

/// Why a call failed, already worded for the user.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkError {
    /// Nobody owns the daemon's bus name.
    NotRunning,
    /// The daemon (or the bus) said no; the message explains and suggests a fix.
    Failed(String),
}

impl LinkError {
    pub fn message(&self) -> String {
        match self {
            Self::NotRunning => crate::strings::SERVICE_NOT_RUNNING.to_owned(),
            Self::Failed(message) => message.clone(),
        }
    }
}

impl From<zbus::Error> for LinkError {
    fn from(err: zbus::Error) -> Self {
        if is_daemon_absent(&err) {
            Self::NotRunning
        } else {
            Self::Failed(LucernaError::from(err).message())
        }
    }
}

/// Something the daemon announced.
#[derive(Clone, Debug)]
pub enum Event {
    Status(Box<StatusDto>),
    DisplaysChanged,
    LibraryChanged,
    SettingsChanged,
}

pub type EventStream = Pin<Box<dyn Stream<Item = Event>>>;

pub struct DaemonLink {
    proxy: LucernaProxy<'static>,
}

type Result<T> = std::result::Result<T, LinkError>;

impl DaemonLink {
    /// Connect to the session bus, or to `address` (tests).
    pub async fn connect(address: Option<&str>) -> Result<Self> {
        let conn = match address {
            Some(address) => zbus::connection::Builder::address(address)?.build().await?,
            None => zbus::Connection::session().await?,
        };
        Ok(Self {
            proxy: LucernaProxy::new(&conn).await?,
        })
    }

    pub async fn status(&self) -> Result<StatusDto> {
        Ok(StatusDto::from_dict(&self.proxy.get_status().await?))
    }

    pub async fn displays(&self) -> Result<Vec<DisplayDto>> {
        Ok(self
            .proxy
            .get_displays()
            .await?
            .iter()
            .map(DisplayDto::from_dict)
            .collect())
    }

    pub async fn wallpapers(&self) -> Result<Vec<WallpaperDto>> {
        Ok(self
            .proxy
            .list_wallpapers()
            .await?
            .iter()
            .map(WallpaperDto::from_dict)
            .collect())
    }

    pub async fn settings(&self) -> Result<SettingsDto> {
        Ok(SettingsDto::from_dict(&self.proxy.get_settings().await?))
    }

    /// Add `path` to the library and return its id.
    pub async fn add_wallpaper(&self, path: &Path) -> Result<String> {
        Ok(self
            .proxy
            .add_wallpaper(&path.to_string_lossy(), "")
            .await?)
    }

    pub async fn remove_wallpaper(&self, id: &str) -> Result<()> {
        Ok(self.proxy.remove_wallpaper(id).await?)
    }

    /// Play `id` on `display` (a display id, or `*` for all displays).
    pub async fn set_wallpaper(&self, id: &str, display: &str) -> Result<()> {
        Ok(self.proxy.set_wallpaper(id, display).await?)
    }

    /// Remove the assignment of `display` (`*` for the all-displays one).
    pub async fn clear_wallpaper(&self, display: &str) -> Result<()> {
        Ok(self.proxy.clear_assignment(display).await?)
    }

    /// Set the scaling of `display`; `inherit` (one display only) follows the all-displays mode.
    pub async fn set_scaling(&self, display: &str, mode: &str) -> Result<()> {
        Ok(self.proxy.set_scaling(display, mode).await?)
    }

    pub async fn apply_settings(&self, patch: &SettingsPatch) -> Result<()> {
        Ok(self.proxy.set_settings(patch.to_dict()).await?)
    }

    pub async fn pause(&self) -> Result<()> {
        Ok(self.proxy.pause().await?)
    }

    pub async fn resume(&self) -> Result<()> {
        Ok(self.proxy.resume().await?)
    }

    pub async fn stop(&self) -> Result<()> {
        Ok(self.proxy.stop().await?)
    }

    pub async fn reload(&self) -> Result<()> {
        Ok(self.proxy.reload().await?)
    }

    pub async fn quit(&self) -> Result<()> {
        Ok(self.proxy.quit().await?)
    }

    /// One stream of everything the daemon announces.
    pub async fn events(&self) -> Result<EventStream> {
        let status = self
            .proxy
            .receive_status_changed()
            .await?
            .filter_map(|signal| async move {
                let args = signal.args().ok()?;
                Some(Event::Status(Box::new(StatusDto::from_dict(&args.status))))
            })
            .boxed_local();
        let displays = self
            .proxy
            .receive_displays_changed()
            .await?
            .map(|_| Event::DisplaysChanged)
            .boxed_local();
        let library = self
            .proxy
            .receive_library_changed()
            .await?
            .map(|_| Event::LibraryChanged)
            .boxed_local();
        let settings = self
            .proxy
            .receive_settings_changed()
            .await?
            .map(|_| Event::SettingsChanged)
            .boxed_local();
        Ok(Box::pin(stream::select_all(vec![
            status, displays, library, settings,
        ])))
    }
}

/// Locate `lucernad`: next to this executable, else on `PATH`.
pub fn find_daemon() -> Option<PathBuf> {
    let beside = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|d| d.join("lucernad")));
    if let Some(path) = beside.filter(|p| p.is_file()) {
        return Some(path);
    }
    std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .map(|dir| dir.join("lucernad"))
            .find(|candidate| candidate.is_file())
    })
}

/// Launch the daemon so that it outlives the GUI: its own process group, no inherited stdio.
pub fn spawn_daemon() -> std::io::Result<()> {
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};
    let path = find_daemon().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::NotFound, "lucernad was not found")
    })?;
    Command::new(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .map(|_| ())
}
