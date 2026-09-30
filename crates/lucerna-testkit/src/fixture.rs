//! An in-process daemon on a private bus with a fake backend and a fake mpv.

use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

use lucerna_core::backend::{OutputId, OutputInfo, Rect, Rotation};
use lucerna_core::paths::Paths;
use lucerna_core::renderer::Timings;
use lucerna_core::session::SessionEnv;
use lucerna_core::testing::FakeBackend;
use lucerna_daemon::{BackendChoice, BusChoice, DaemonOptions, Outcome, run};
use lucerna_ipc::LucernaProxy;
use lucerna_ipc::dto::{DisplayDto, SettingsDto, StatusDto, WallpaperDto};
use tokio::task::JoinHandle;

use crate::TestEnv;
use crate::testbus::TestBus;

/// The `fake-mpv` binary. Cargo builds it for every integration test of this package and exports
/// its path; the fallback looks next to the running test executable.
pub fn fake_mpv_path() -> OsString {
    if let Some(path) = std::env::var_os("CARGO_BIN_EXE_fake-mpv") {
        return path;
    }
    let exe = std::env::current_exe().expect("current_exe");
    let dir = exe
        .parent()
        .and_then(std::path::Path::parent)
        .expect("target dir");
    dir.join("fake-mpv").into_os_string()
}

/// A connected output for the fake backend.
pub fn output(connector: &str, x: i32, primary: bool) -> OutputInfo {
    OutputInfo {
        id: OutputId::new(format!("conn:{connector}")),
        connector: connector.to_owned(),
        geometry: Rect::new(x, 0, 1920, 1080),
        primary,
        rotation: Rotation::Normal,
        refresh_mhz: Some(60_000),
        edid: None,
    }
}

pub fn fast_timings() -> Timings {
    Timings {
        startup: Duration::from_millis(2000),
        ipc_connect: Duration::from_millis(1500),
        stop_step: Duration::from_millis(400),
    }
}

/// Test knobs applied on top of the defaults.
pub struct Setup {
    pub session: SessionEnv,
    pub mpv_override: Option<OsString>,
    pub backend: Option<BackendChoice>,
    pub recheck_interval: Duration,
    pub outputs: Vec<OutputInfo>,
    /// Text written to `config.toml` before the daemon starts, with `{ROOT}` replaced by the
    /// test directory.
    pub config_toml: Option<String>,
    /// Files created in the runtime directory before the daemon starts (name, content).
    pub runtime_files: Vec<(String, Vec<u8>)>,
}

impl Default for Setup {
    fn default() -> Self {
        Self {
            session: SessionEnv {
                xdg_session_type: Some("x11".to_owned()),
                display: Some(":99".to_owned()),
                xdg_current_desktop: Some("X-Cinnamon".to_owned()),
                desktop_session: Some("cinnamon".to_owned()),
                wayland_display: None,
            },
            mpv_override: Some(fake_mpv_path()),
            backend: None,
            recheck_interval: Duration::from_millis(300),
            outputs: vec![output("HDMI-1", 0, true)],
            config_toml: None,
            runtime_files: Vec::new(),
        }
    }
}

pub struct Fixture {
    pub bus: TestBus,
    pub env: TestEnv,
    pub paths: Paths,
    pub fake: FakeBackend,
    pub proxy: LucernaProxy<'static>,
    conn: zbus::Connection,
    task: Option<JoinHandle<Outcome>>,
}

impl Fixture {
    pub async fn start(name: &str) -> Option<Self> {
        Self::start_with(name, Setup::default()).await
    }

    pub async fn start_with(name: &str, setup: Setup) -> Option<Self> {
        let bus = TestBus::start()?;
        let env = TestEnv::new(name);
        let paths = Paths::under(&env.root);
        // The runtime base must exist (systemd creates it in real life); the daemon makes `lucerna`.
        std::fs::create_dir_all(paths.runtime_dir.as_ref()?.parent()?).ok()?;
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(
                paths.runtime_dir.as_ref()?.parent()?,
                std::fs::Permissions::from_mode(0o700),
            )
            .ok()?;
        }
        if !setup.runtime_files.is_empty() {
            let runtime = paths.runtime_dir.as_ref()?;
            std::fs::create_dir_all(runtime).ok()?;
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(runtime, std::fs::Permissions::from_mode(0o700)).ok()?;
            }
            for (name, content) in &setup.runtime_files {
                std::fs::write(runtime.join(name), content).ok()?;
            }
        }
        if let Some(toml) = &setup.config_toml {
            std::fs::create_dir_all(&paths.config_dir).ok()?;
            std::fs::write(
                paths.config_file(),
                toml.replace("{ROOT}", &env.root.to_string_lossy()),
            )
            .ok()?;
        }

        let fake = FakeBackend::new();
        fake.set_outputs(setup.outputs);
        let backend = setup
            .backend
            .unwrap_or_else(|| BackendChoice::Injected(Box::new(fake.clone())));

        let options = DaemonOptions {
            paths: paths.clone(),
            session: setup.session,
            bus: BusChoice::Address(bus.address.clone()),
            backend,
            mpv_override: setup.mpv_override,
            path_var: Some(OsString::from("/nonexistent-lucerna-test-path")),
            daemon_exe: PathBuf::from("/usr/bin/lucernad"),
            display_wait: Duration::from_millis(0),
            renderer_timings: fast_timings(),
            vo_override: None,
            extra_env: Vec::new(),
            recheck_interval: setup.recheck_interval,
            handle_signals: false,
            autostart_on_first_run: true,
        };
        let task = tokio::spawn(run(options));

        let conn = zbus::connection::Builder::address(bus.address.as_str())
            .ok()?
            .build()
            .await
            .ok()?;
        let proxy = LucernaProxy::new(&conn).await.ok()?;
        Some(Self {
            bus,
            env,
            paths,
            fake,
            proxy,
            conn,
            task: Some(task),
        })
    }

    pub fn connection(&self) -> &zbus::Connection {
        &self.conn
    }

    /// Wait until the daemon answers (its engine is initialised), then return the status.
    pub async fn ready(&self) -> StatusDto {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
        loop {
            if let Ok(dict) = self.proxy.get_status().await {
                return StatusDto::from_dict(&dict);
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "the daemon never became ready"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    pub async fn status(&self) -> StatusDto {
        StatusDto::from_dict(&self.proxy.get_status().await.expect("GetStatus"))
    }

    pub async fn displays(&self) -> Vec<DisplayDto> {
        self.proxy
            .get_displays()
            .await
            .expect("GetDisplays")
            .iter()
            .map(DisplayDto::from_dict)
            .collect()
    }

    pub async fn wallpapers(&self) -> Vec<WallpaperDto> {
        self.proxy
            .list_wallpapers()
            .await
            .expect("ListWallpapers")
            .iter()
            .map(WallpaperDto::from_dict)
            .collect()
    }

    pub async fn settings(&self) -> SettingsDto {
        SettingsDto::from_dict(&self.proxy.get_settings().await.expect("GetSettings"))
    }

    /// Poll the status until `predicate` holds.
    pub async fn wait_status(
        &self,
        what: &str,
        predicate: impl Fn(&StatusDto) -> bool,
    ) -> StatusDto {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
        loop {
            let status = self.status().await;
            if predicate(&status) {
                return status;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "timed out waiting for {what}; last status: {status:#?}"
            );
            tokio::time::sleep(Duration::from_millis(40)).await;
        }
    }

    /// Ask the daemon to quit and return how it ended.
    pub async fn quit(&mut self) -> Outcome {
        let _ = self.proxy.quit().await;
        self.join().await
    }

    pub async fn join(&mut self) -> Outcome {
        let task = self.task.take().expect("daemon already joined");
        tokio::time::timeout(Duration::from_secs(20), task)
            .await
            .expect("the daemon did not stop")
            .expect("the daemon task panicked")
    }

    pub fn config_text(&self) -> String {
        std::fs::read_to_string(self.paths.config_file()).unwrap_or_default()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}
