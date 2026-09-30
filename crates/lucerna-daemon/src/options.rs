//! Everything the daemon needs from the outside world, injected so tests can substitute a fake
//! backend, a fake mpv and a private bus without touching the process environment.

use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

use lucerna_core::backend::WallpaperBackend;
use lucerna_core::paths::Paths;
use lucerna_core::renderer::Timings;
use lucerna_core::session::SessionEnv;

/// Which D-Bus to talk to.
#[derive(Clone, Debug)]
pub enum BusChoice {
    /// The user's session bus (`DBUS_SESSION_BUS_ADDRESS`, or the per-user socket).
    Session,
    /// An explicit address, for tests with a private `dbus-daemon`.
    Address(String),
}

/// Which system bus to use for the logind lock probe.
#[derive(Clone, Debug)]
pub enum SystemBusChoice {
    /// The real system bus.
    System,
    /// An explicit address (tests with a fake logind).
    Address(String),
    /// Do not probe logind.
    Disabled,
}

/// Where the wallpaper backend comes from.
pub enum BackendChoice {
    /// Pick from the session: X11 (Cinnamon or generic) or none.
    Auto,
    /// Use exactly this backend (tests).
    Injected(Box<dyn WallpaperBackend>),
}

pub struct DaemonOptions {
    pub paths: Paths,
    pub session: SessionEnv,
    pub bus: BusChoice,
    pub system_bus: SystemBusChoice,
    pub backend: BackendChoice,
    /// `LUCERNA_MPV`.
    pub mpv_override: Option<OsString>,
    /// `PATH`.
    pub path_var: Option<OsString>,
    /// The daemon's own executable, written into the autostart entry.
    pub daemon_exe: PathBuf,
    /// How long to wait for `DISPLAY` and the X server at login (§22).
    pub display_wait: Duration,
    pub renderer_timings: Timings,
    /// Tests only: `--vo=null`.
    pub vo_override: Option<String>,
    /// Tests only: extra environment for mpv.
    pub extra_env: Vec<(String, String)>,
    /// Interval of the missing-media recheck (60 s in production).
    pub recheck_interval: Duration,
    /// Handle SIGTERM/SIGINT/SIGHUP. Tests running the daemon in-process turn this off.
    pub handle_signals: bool,
    /// Write the autostart entry on first run (D4). Tests running in-process may turn it off.
    pub autostart_on_first_run: bool,
}

impl DaemonOptions {
    /// Production defaults from the process environment.
    pub fn from_env() -> Self {
        Self {
            paths: Paths::from_env(),
            session: SessionEnv::from_env(),
            bus: BusChoice::Session,
            system_bus: SystemBusChoice::System,
            backend: BackendChoice::Auto,
            mpv_override: std::env::var_os("LUCERNA_MPV"),
            path_var: std::env::var_os("PATH"),
            daemon_exe: std::env::current_exe().unwrap_or_else(|_| PathBuf::from("lucernad")),
            display_wait: std::env::var("LUCERNA_DISPLAY_WAIT_MS")
                .ok()
                .and_then(|v| v.parse::<u64>().ok())
                .map_or(Duration::from_secs(10), Duration::from_millis),
            renderer_timings: Timings::default(),
            vo_override: None,
            extra_env: Vec::new(),
            recheck_interval: Duration::from_secs(60),
            handle_signals: true,
            autostart_on_first_run: true,
        }
    }
}
