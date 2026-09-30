//! The real `lucernad` binary as a process: single instance, signals, cleanup (directive §52).
//!
//! Uses Xvfb for the X11 backend, so window cleanup is checked at the protocol level (the windows
//! are gone from the root's children). That says nothing about how a desktop would look.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use lucerna_ipc::LucernaProxy;
use lucerna_ipc::dto::StatusDto;
use lucerna_testkit::fixture::fake_mpv_path;
use lucerna_testkit::testbus::TestBus;
use lucerna_testkit::xvfb::Xvfb;
use lucerna_testkit::{TestEnv, launches, pid_alive};
use tokio::process::{Child, Command};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::ConnectionExt as _;
use x11rb::rust_connection::RustConnection;

const DAEMON: &str = env!("CARGO_BIN_EXE_lucernad-under-test");

struct World {
    env: TestEnv,
    bus: TestBus,
    xvfb: Option<Xvfb>,
    runtime: PathBuf,
}

impl World {
    fn new(name: &str, with_xvfb: bool) -> Option<Self> {
        let bus = TestBus::start()?;
        let xvfb = if with_xvfb {
            Some(Xvfb::start(1920, 1080)?)
        } else {
            None
        };
        let env = TestEnv::new(name);
        let runtime = env.root.join("run");
        std::fs::create_dir_all(&runtime).unwrap();
        std::fs::set_permissions(
            &runtime,
            std::os::unix::fs::PermissionsExt::from_mode(0o700),
        )
        .unwrap();
        Some(Self {
            env,
            bus,
            xvfb,
            runtime,
        })
    }

    /// A clean environment: nothing leaks in from the developer's real session.
    fn command(&self, session_type: &str) -> Command {
        let mut cmd = Command::new(DAEMON);
        cmd.env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", self.env.root.join("home"))
            .env("XDG_CONFIG_HOME", self.env.root.join("config"))
            .env("XDG_STATE_HOME", self.env.root.join("state"))
            .env("XDG_CACHE_HOME", self.env.root.join("cache"))
            .env("XDG_RUNTIME_DIR", &self.runtime)
            .env("DBUS_SESSION_BUS_ADDRESS", &self.bus.address)
            .env("XDG_SESSION_TYPE", session_type)
            .env("XDG_CURRENT_DESKTOP", "X-Cinnamon")
            .env("LUCERNA_MPV", fake_mpv_path())
            .env("LUCERNA_LOG", "lucerna=debug")
            // A bare Xvfb has no window manager; do not sit out the 10 s login wait in every test.
            .env("LUCERNA_DISPLAY_WAIT_MS", "300")
            .env("RUST_BACKTRACE", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        if let Some(xvfb) = &self.xvfb {
            cmd.env("DISPLAY", &xvfb.display);
        }
        cmd
    }

    async fn proxy(&self) -> (zbus::Connection, LucernaProxy<'static>) {
        let conn = zbus::connection::Builder::address(self.bus.address.as_str())
            .unwrap()
            .build()
            .await
            .unwrap();
        let proxy = LucernaProxy::new(&conn).await.unwrap();
        (conn, proxy)
    }

    /// Root children carrying our `_LUCERNA_WALLPAPER` marker.
    fn wallpaper_windows(&self) -> Vec<u32> {
        let (x, screen) =
            RustConnection::connect(Some(&self.xvfb.as_ref().unwrap().display)).unwrap();
        let root = x.setup().roots[screen].root;
        let atom = x
            .intern_atom(false, b"_LUCERNA_WALLPAPER")
            .unwrap()
            .reply()
            .unwrap()
            .atom;
        x.query_tree(root)
            .unwrap()
            .reply()
            .unwrap()
            .children
            .into_iter()
            .filter(|w| {
                x.get_property(
                    false,
                    *w,
                    atom,
                    x11rb::protocol::xproto::AtomEnum::ANY,
                    0,
                    8,
                )
                .unwrap()
                .reply()
                .is_ok_and(|r| !r.value.is_empty())
            })
            .collect()
    }

    fn x_children(&self) -> Vec<u32> {
        let (x, screen) =
            RustConnection::connect(Some(&self.xvfb.as_ref().unwrap().display)).unwrap();
        let root = x.setup().roots[screen].root;
        x.query_tree(root).unwrap().reply().unwrap().children
    }
}

async fn wait_ready(proxy: &LucernaProxy<'_>) -> StatusDto {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    loop {
        if let Ok(dict) = proxy.get_status().await {
            return StatusDto::from_dict(&dict);
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the daemon never became ready"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn wait_playing(proxy: &LucernaProxy<'_>) -> StatusDto {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    loop {
        let status = StatusDto::from_dict(&proxy.get_status().await.unwrap());
        if status
            .renderers
            .first()
            .is_some_and(|r| r.state == "playing")
        {
            return status;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "never playing: {status:#?}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn exit_code(child: &mut Child) -> i32 {
    tokio::time::timeout(Duration::from_secs(20), child.wait())
        .await
        .expect("the daemon did not exit")
        .unwrap()
        .code()
        .unwrap_or(-1)
}

fn signal(child: &Child, sig: rustix::process::Signal) {
    let pid = rustix::process::Pid::from_raw(i32::try_from(child.id().unwrap()).unwrap()).unwrap();
    rustix::process::kill_process(pid, sig).unwrap();
}

async fn start_playing(world: &World, proxy: &LucernaProxy<'_>) -> (PathBuf, u64) {
    start_playing_with(world, proxy, "play").await
}

async fn start_playing_with(
    world: &World,
    proxy: &LucernaProxy<'_>,
    directives: &str,
) -> (PathBuf, u64) {
    let media = world.env.media("clip.mp4", directives);
    let id = proxy
        .add_wallpaper(media.to_str().unwrap(), "")
        .await
        .unwrap();
    proxy.set_wallpaper(&id, "*").await.unwrap();
    let status = wait_playing(proxy).await;
    assert_eq!(status.backend, "cinnamon-x11");
    let pid = launches(&media)[0]["pid"].as_u64().unwrap();
    assert!(pid_alive(pid));
    (media, pid)
}

fn assert_runtime_clean(runtime: &Path) {
    let leftovers: Vec<String> = std::fs::read_dir(runtime.join("lucerna"))
        .map(|d| {
            d.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| n.ends_with(".sock") || n == "renderers.json")
                .collect()
        })
        .unwrap_or_default();
    assert!(
        leftovers.is_empty(),
        "left in the runtime directory: {leftovers:?}"
    );
}

async fn name_is_free(proxy: &LucernaProxy<'_>) -> bool {
    proxy.get_status().await.is_err()
}

macro_rules! world {
    ($name:expr, $xvfb:expr) => {
        match World::new($name, $xvfb) {
            Some(w) => w,
            None => return,
        }
    };
}

// -------------------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sigterm_cleans_up_everything() {
    let world = world!("sigterm", true);
    let mut daemon = world.command("x11").spawn().unwrap();
    let (_conn, proxy) = world.proxy().await;
    wait_ready(&proxy).await;
    let (_media, mpv_pid) = start_playing(&world, &proxy).await;
    assert_eq!(
        world.wallpaper_windows().len(),
        1,
        "one wallpaper window on the X server"
    );

    signal(&daemon, rustix::process::Signal::TERM);
    assert_eq!(
        exit_code(&mut daemon).await,
        0,
        "a signal is a normal way to stop"
    );

    assert!(!pid_alive(mpv_pid), "the renderer child was stopped");
    assert!(
        world.x_children().is_empty(),
        "the wallpaper windows were removed"
    );
    assert_runtime_clean(&world.runtime);
    assert!(name_is_free(&proxy).await, "the bus name was released");
    // The lock was released too: a new daemon can start straight away.
    let mut again = world.command("x11").spawn().unwrap();
    wait_ready(&proxy).await;
    signal(&again, rustix::process::Signal::TERM);
    assert_eq!(exit_code(&mut again).await, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn quit_cleans_up_everything() {
    let world = world!("quit", true);
    let mut daemon = world.command("x11").spawn().unwrap();
    let (_conn, proxy) = world.proxy().await;
    wait_ready(&proxy).await;
    let (_media, mpv_pid) = start_playing(&world, &proxy).await;

    proxy.quit().await.unwrap();
    assert_eq!(exit_code(&mut daemon).await, 0);
    assert!(!pid_alive(mpv_pid));
    assert!(world.x_children().is_empty());
    assert_runtime_clean(&world.runtime);
    assert!(name_is_free(&proxy).await);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn duplicate_daemon_exits_cleanly_and_the_first_is_unaffected() {
    let world = world!("duplicate", true);
    let mut first = world.command("x11").spawn().unwrap();
    let (_conn, proxy) = world.proxy().await;
    wait_ready(&proxy).await;
    let first_pid = first.id().unwrap();

    // Same environment, same session: the second one must step aside without a panic.
    let second = world.command("x11").output().await.unwrap();
    assert_eq!(second.status.code(), Some(0));
    let out = String::from_utf8_lossy(&second.stdout);
    assert!(
        out.contains(&format!(
            "already running for this session (PID {first_pid}). Nothing to do."
        )),
        "stdout: {out}"
    );
    assert!(!String::from_utf8_lossy(&second.stderr).contains("panicked"));

    // The first daemon still works.
    assert!(wait_ready(&proxy).await.supported);
    signal(&first, rustix::process::Signal::TERM);
    assert_eq!(exit_code(&mut first).await, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_display_going_away_shuts_the_daemon_down_cleanly() {
    let mut world = world!("logout", true);
    let mut daemon = world.command("x11").spawn().unwrap();
    let (_conn, proxy) = world.proxy().await;
    wait_ready(&proxy).await;
    let (_media, mpv_pid) = start_playing(&world, &proxy).await;

    // Logout: the X server disappears.
    world.xvfb.as_mut().unwrap().kill();
    assert_eq!(exit_code(&mut daemon).await, 0);
    assert!(!pid_alive(mpv_pid));
    assert_runtime_clean(&world.runtime);
    assert!(name_is_free(&proxy).await);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_wayland_session_is_explained_not_crashed_on() {
    let world = world!("wayland", false);
    let mut cmd = world.command("wayland");
    cmd.env("WAYLAND_DISPLAY", "wayland-0").env("DISPLAY", ":0");
    let mut daemon = cmd.spawn().unwrap();
    let (_conn, proxy) = world.proxy().await;
    let status = wait_ready(&proxy).await;
    assert!(!status.supported);
    assert_eq!(status.unsupported_reason, "wayland");
    assert!(
        status
            .unsupported_message
            .contains("supports X11 sessions only")
    );

    signal(&daemon, rustix::process::Signal::TERM);
    assert_eq!(exit_code(&mut daemon).await, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn no_display_is_reported_after_the_login_wait() {
    let world = world!("nodisplay", false);
    let mut daemon = world
        .command("x11")
        .env("LUCERNA_DISPLAY_WAIT_MS", "300")
        .spawn()
        .unwrap();
    let (_conn, proxy) = world.proxy().await;
    let status = wait_ready(&proxy).await;
    assert!(!status.supported);
    assert_eq!(status.unsupported_reason, "no-display");
    signal(&daemon, rustix::process::Signal::TERM);
    assert_eq!(exit_code(&mut daemon).await, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn no_session_bus_gives_a_readable_error_not_a_panic() {
    let world = world!("nobus", false);
    let out = world
        .command("x11")
        .env(
            "DBUS_SESSION_BUS_ADDRESS",
            "unix:path=/nonexistent/lucerna-bus",
        )
        .output()
        .await
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("could not connect to the session D-Bus"),
        "{err}"
    );
    assert!(
        err.contains("dbus-run-session"),
        "it says what to do: {err}"
    );
    assert!(!err.contains("panicked"));
}

// ------------------------------------------------------------------------ logout and crashes

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sighup_ends_the_session_cleanly() {
    // Logout delivers SIGHUP to the session's processes.
    let world = world!("sighup", true);
    let mut daemon = world.command("x11").spawn().unwrap();
    let (_conn, proxy) = world.proxy().await;
    wait_ready(&proxy).await;
    let (_media, mpv_pid) = start_playing(&world, &proxy).await;

    signal(&daemon, rustix::process::Signal::HUP);
    assert_eq!(exit_code(&mut daemon).await, 0);
    assert!(!pid_alive(mpv_pid));
    assert!(world.x_children().is_empty());
    assert_runtime_clean(&world.runtime);
    assert!(name_is_free(&proxy).await);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn when_the_daemon_is_killed_the_kernel_stops_its_renderer() {
    // Defence in depth: PR_SET_PDEATHSIG makes mpv follow its parent even without cleanup code.
    let world = world!("sigkill-pdeath", true);
    let mut daemon = world.command("x11").spawn().unwrap();
    let (_conn, proxy) = world.proxy().await;
    wait_ready(&proxy).await;
    let (_media, mpv_pid) = start_playing(&world, &proxy).await;

    signal(&daemon, rustix::process::Signal::KILL);
    let _ = daemon.wait().await;
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while pid_alive(mpv_pid) && std::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(
        !pid_alive(mpv_pid),
        "the renderer must not outlive a killed daemon"
    );
    // X destroys the windows of a client that vanished.
    assert!(world.wallpaper_windows().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_renderer_that_survives_a_killed_daemon_is_recovered_by_the_next_one() {
    let world = world!("sigkill-recover", true);
    let mut first = world.command("x11").spawn().unwrap();
    let (_conn, proxy) = world.proxy().await;
    wait_ready(&proxy).await;
    // This fake mpv ignores SIGTERM, so it survives the death of its parent.
    let (media, old_pid) = start_playing_with(&world, &proxy, "ignore-term; ignore-quit").await;

    signal(&first, rustix::process::Signal::KILL);
    let _ = first.wait().await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(
        pid_alive(old_pid),
        "the orphan survived (it ignores SIGTERM)"
    );
    let registry = std::fs::read_to_string(world.runtime.join("lucerna/renderers.json")).unwrap();
    assert!(
        registry.contains(&old_pid.to_string()),
        "the registry names the orphan: {registry}"
    );

    // The next daemon finds it by pid, start time, executable and socket, and removes it.
    let mut second = world.command("x11").spawn().unwrap();
    wait_ready(&proxy).await;
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    while pid_alive(old_pid) && std::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(
        !pid_alive(old_pid),
        "stale renderer terminated by the next daemon"
    );

    // The persisted assignment is played again by a fresh renderer.
    let status = wait_playing(&proxy).await;
    assert_ne!(u64::from(status.renderers[0].pid), old_pid);
    assert!(pid_alive(u64::from(status.renderers[0].pid)));
    assert!(launches(&media).len() >= 2);
    signal(&second, rustix::process::Signal::TERM);
    assert_eq!(exit_code(&mut second).await, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unrelated_mpv_is_never_touched_by_recovery() {
    let world = world!("unrelated-mpv", true);
    // A "user's mpv" (the fake, launched by hand with its own socket) and a stale registry that
    // names a different pid entirely.
    let media = world.env.media("mine.mp4", "play");
    let socket = world.env.root.join("mine.sock");
    let mut theirs = std::process::Command::new(fake_mpv_path())
        .arg(format!("--input-ipc-server={}", socket.display()))
        .arg("--")
        .arg(&media)
        .spawn()
        .unwrap();
    let their_pid = u64::from(theirs.id());
    std::fs::create_dir_all(world.runtime.join("lucerna")).unwrap();
    std::fs::set_permissions(
        world.runtime.join("lucerna"),
        std::os::unix::fs::PermissionsExt::from_mode(0o700),
    )
    .unwrap();
    std::fs::write(
        world.runtime.join("lucerna/renderers.json"),
        format!(r#"[{{"pid": {their_pid}, "starttime": 1, "socket": "{}", "output_id": "conn:X", "generation": 1}}]"#, socket.display()),
    )
    .unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;

    let mut daemon = world.command("x11").spawn().unwrap();
    let (_conn, proxy) = world.proxy().await;
    wait_ready(&proxy).await;
    assert!(
        pid_alive(their_pid),
        "a process whose start time does not match the record is left alone"
    );
    signal(&daemon, rustix::process::Signal::TERM);
    assert_eq!(exit_code(&mut daemon).await, 0);
    theirs.kill().unwrap();
    theirs.wait().unwrap();
}

// -------------------------------------------------------------------------------- logging

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_daemon_keeps_a_concise_bounded_log_of_the_events_that_matter() {
    let world = world!("logfile", true);
    let mut daemon = world.command("x11").spawn().unwrap();
    let (_conn, proxy) = world.proxy().await;
    wait_ready(&proxy).await;
    let (_media, _pid) = start_playing(&world, &proxy).await;
    proxy.pause().await.unwrap();
    proxy.resume().await.unwrap();

    let log_path = world.env.root.join("state/lucerna/logs/lucernad.log");
    let before = std::fs::read_to_string(&log_path).unwrap();
    // Nothing is logged per poll: hundreds of status calls add nothing.
    for _ in 0..300 {
        proxy.get_status().await.unwrap();
    }
    let after = std::fs::read_to_string(&log_path).unwrap();
    assert_eq!(
        before.lines().count(),
        after.lines().count(),
        "status polling must not log"
    );

    for event in [
        "lucernad starting",
        "backend selected",
        "display detected",
        "wallpaper assigned",
        "starting renderer",
        "renderer launched",
        "renderer paused",
        "renderer resumed",
    ] {
        assert!(after.contains(event), "the log lacks {event:?}:\n{after}");
    }
    assert!(
        after.lines().count() < 60,
        "concise: {} lines",
        after.lines().count()
    );

    signal(&daemon, rustix::process::Signal::TERM);
    assert_eq!(exit_code(&mut daemon).await, 0);
    let final_log = std::fs::read_to_string(&log_path).unwrap();
    assert!(
        final_log.contains("shutting down")
            && final_log.contains("renderer exited")
            && final_log.contains("daemon stopped"),
        "{final_log}"
    );
    assert!(std::fs::metadata(&log_path).unwrap().len() <= 512 * 1024);
}

// ------------------------------------------------------------------ the login race (window manager)

fn fake_window_manager(world: &World) {
    let (x, screen) = RustConnection::connect(Some(&world.xvfb.as_ref().unwrap().display)).unwrap();
    let root = x.setup().roots[screen].root;
    let intern = |name: &str| {
        x.intern_atom(false, name.as_bytes())
            .unwrap()
            .reply()
            .unwrap()
            .atom
    };
    let window = x.generate_id().unwrap();
    x.create_window(
        0,
        window,
        root,
        0,
        0,
        1,
        1,
        0,
        x11rb::protocol::xproto::WindowClass::INPUT_OUTPUT,
        0,
        &x11rb::protocol::xproto::CreateWindowAux::new(),
    )
    .unwrap();
    let check = intern("_NET_SUPPORTING_WM_CHECK");
    use x11rb::wrapper::ConnectionExt as _;
    x.change_property32(
        x11rb::protocol::xproto::PropMode::REPLACE,
        root,
        check,
        x11rb::protocol::xproto::AtomEnum::WINDOW,
        &[window],
    )
    .unwrap();
    x.change_property8(
        x11rb::protocol::xproto::PropMode::REPLACE,
        window,
        intern("_NET_WM_NAME"),
        intern("UTF8_STRING"),
        b"Muffin",
    )
    .unwrap();
    x.flush().unwrap();
    // Keep the connection (and so the window) alive for a while.
    std::mem::forget(x);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_daemon_waits_for_the_window_manager_before_creating_surfaces() {
    let world = world!("wm-wait", true);
    let mut daemon = world
        .command("x11")
        .env("LUCERNA_DISPLAY_WAIT_MS", "8000")
        .spawn()
        .unwrap();
    let (_conn, proxy) = world.proxy().await;

    // The daemon serves D-Bus immediately but is still waiting to pick its backend's surfaces up:
    // give it a moment, then the window manager "starts".
    tokio::time::sleep(Duration::from_millis(800)).await;
    let started = std::time::Instant::now();
    fake_window_manager(&world);
    let status = wait_ready(&proxy).await;
    assert!(
        started.elapsed() < Duration::from_secs(6),
        "it did not sit out the whole wait once the window manager appeared"
    );
    assert!(status.supported);
    assert_eq!(status.backend, "cinnamon-x11", "Muffin was recognised");

    signal(&daemon, rustix::process::Signal::TERM);
    assert_eq!(exit_code(&mut daemon).await, 0);
    let log =
        std::fs::read_to_string(world.env.root.join("state/lucerna/logs/lucernad.log")).unwrap();
    assert!(log.contains("waiting for the window manager"), "{log}");
    assert!(!log.contains("no window manager appeared"), "{log}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn without_a_window_manager_the_wait_is_bounded() {
    let world = world!("wm-timeout", true);
    let mut daemon = world
        .command("x11")
        .env("LUCERNA_DISPLAY_WAIT_MS", "700")
        .spawn()
        .unwrap();
    let (_conn, proxy) = world.proxy().await;
    let status = wait_ready(&proxy).await;
    assert!(status.supported, "a bare X server still works");
    signal(&daemon, rustix::process::Signal::TERM);
    assert_eq!(exit_code(&mut daemon).await, 0);
    let log =
        std::fs::read_to_string(world.env.root.join("state/lucerna/logs/lucernad.log")).unwrap();
    assert!(log.contains("no window manager appeared"), "{log}");
}
