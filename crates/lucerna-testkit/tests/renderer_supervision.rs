//! Renderer supervision against the fake mpv (directive §35 "integration tests").

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use lucerna_core::backend::OutputId;
use lucerna_core::mpv::{RenderSpec, build_args};
use lucerna_core::renderer::{RendererStateKind, RestartPolicy, Timings};
use lucerna_core::runtime::socket_path;
use lucerna_core::types::{FpsLimit, HwDecode, ScalingMode};
use lucerna_mpv::{
    LaunchSettings, PidRegistry, RecoveryOptions, RendererSupervisor, SupervisorConfig,
    recover_stale,
};
use lucerna_testkit::{
    TestEnv, commands, dir_size, eventually, launches, pid_alive, wait_for, wait_for_state,
};
use serde_json::json;

const FAKE_MPV: &str = env!("CARGO_BIN_EXE_fake-mpv");
const WAIT: Duration = Duration::from_secs(10);

fn fast_timings() -> Timings {
    Timings {
        startup: Duration::from_millis(1500),
        ipc_connect: Duration::from_millis(1000),
        stop_step: Duration::from_millis(300),
    }
}

fn policy(max: u32, window_ms: u64) -> RestartPolicy {
    RestartPolicy {
        max_restarts: max,
        window: Duration::from_millis(window_ms),
        backoff: [Duration::from_millis(60); 3],
    }
}

fn output() -> OutputId {
    OutputId::new("conn:TEST-1")
}

fn config(env: &TestEnv) -> SupervisorConfig {
    SupervisorConfig {
        output: output(),
        mpv_path: PathBuf::from(FAKE_MPV),
        runtime_dir: env.runtime_dir.clone(),
        log_dir: Some(env.log_dir.clone()),
        log_max_bytes: 512 * 1024,
        embed: None,
        restart_policy: policy(3, 60_000),
        timings: fast_timings(),
        vo_override: None,
        extra_env: Vec::new(),
        registry: Arc::new(PidRegistry::new(env.registry_path())),
    }
}

fn settings(media: &Path) -> LaunchSettings {
    LaunchSettings {
        media: media.to_path_buf(),
        scaling: ScalingMode::Fill,
        hwdec: HwDecode::Auto,
        fps: FpsLimit::Native,
        audio: false,
    }
}

fn spawn(env: &TestEnv, media: &Path) -> RendererSupervisor {
    RendererSupervisor::spawn(config(env), settings(media), None)
}

#[tokio::test]
async fn play_pause_resume_stop_round_trip() {
    let env = TestEnv::new("round");
    let media = env.media("clip.mp4", "play");
    let sup = spawn(&env, &media);

    sup.start(false);
    let snap = wait_for_state(&sup, RendererStateKind::Playing, WAIT).await;
    assert_ne!(snap.pid, 0);
    let pid = u64::from(snap.pid);
    assert!(pid_alive(pid));

    sup.pause();
    wait_for_state(&sup, RendererStateKind::Paused, WAIT).await;
    eventually(WAIT, "pause command at mpv", || {
        commands(&media).contains(&json!(["set_property", "pause", true]))
    })
    .await;

    sup.resume();
    wait_for_state(&sup, RendererStateKind::Playing, WAIT).await;
    eventually(WAIT, "resume command at mpv", || {
        commands(&media).contains(&json!(["set_property", "pause", false]))
    })
    .await;

    sup.stop();
    wait_for_state(&sup, RendererStateKind::Stopped, WAIT).await;
    assert!(
        commands(&media).contains(&json!(["quit"])),
        "polite quit came first"
    );
    eventually(WAIT, "process to disappear", || !pid_alive(pid)).await;
    assert_eq!(launches(&media).len(), 1);
    sup.shutdown().await;
}

#[tokio::test]
async fn argv_seen_by_mpv_matches_the_pure_builder() {
    let env = TestEnv::new("argv");
    let media = env.media("clip.mkv", "play");
    let sup = spawn(&env, &media);
    sup.start(true);
    wait_for_state(&sup, RendererStateKind::Paused, WAIT).await;

    let spec = RenderSpec {
        embed: None,
        ipc_socket: socket_path(&env.runtime_dir, &output(), 1).unwrap(),
        media: media.clone(),
        scaling: ScalingMode::Fill,
        hwdec: HwDecode::Auto,
        fps: FpsLimit::Native,
        audio: false,
        start_paused: true,
        vo_override: None,
    };
    let expected: Vec<String> = build_args(&spec)
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let launched = launches(&media);
    let actual: Vec<String> = launched[0]["argv"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_owned())
        .collect();
    assert_eq!(actual, expected);
    assert!(actual.contains(&"--pause=yes".to_owned()));
    sup.shutdown().await;
}

#[tokio::test]
async fn a_file_named_like_a_shell_command_is_just_a_filename() {
    let env = TestEnv::new("evil");
    let victim = env.root.join("victim.txt");
    std::fs::write(&victim, "still here").unwrap();
    let media = env.media("$(rm -f victim.txt) `id` ; && echo hi.mp4", "play");
    let sup = spawn(&env, &media);
    sup.start(false);
    wait_for_state(&sup, RendererStateKind::Playing, WAIT).await;
    let argv = launches(&media)[0]["argv"].clone();
    assert_eq!(
        argv.as_array().unwrap().last().unwrap(),
        media.to_str().unwrap()
    );
    assert!(victim.exists(), "nothing was executed");
    sup.shutdown().await;
}

#[tokio::test]
async fn live_scaling_change_sends_three_properties_without_restart() {
    let env = TestEnv::new("scale");
    let media = env.media("clip.webm", "play");
    let sup = spawn(&env, &media);
    sup.start(false);
    wait_for_state(&sup, RendererStateKind::Playing, WAIT).await;
    sup.set_scaling(ScalingMode::Center);
    eventually(WAIT, "scaling properties", || {
        let cmds = commands(&media);
        cmds.contains(&json!(["set_property", "keepaspect", "yes"]))
            && cmds.contains(&json!(["set_property", "panscan", "0.0"]))
            && cmds.contains(&json!(["set_property", "video-unscaled", "yes"]))
    })
    .await;
    assert_eq!(launches(&media).len(), 1, "scaling is live: no restart");
    sup.shutdown().await;
}

#[tokio::test]
async fn restart_storm_is_bounded() {
    let env = TestEnv::new("storm");
    let media = env.media("crashy.mp4", "crash-after=100");
    let mut cfg = config(&env);
    cfg.restart_policy = policy(3, 5_000);
    cfg.timings = Timings {
        startup: Duration::from_secs(5),
        ..fast_timings()
    };
    let sup = RendererSupervisor::spawn(cfg, settings(&media), None);
    sup.start(false);

    let snap = wait_for(&sup, Duration::from_secs(20), "restart limit", |s| {
        s.state == RendererStateKind::Failed && s.failure_code == "restart-limit"
    })
    .await;
    assert!(
        snap.failure_message.contains("stopped restarting"),
        "{}",
        snap.failure_message
    );
    assert!(
        snap.failure_message.contains("crashed"),
        "{}",
        snap.failure_message
    );
    assert_eq!(
        launches(&media).len(),
        4,
        "initial start plus exactly three restarts"
    );

    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(
        launches(&media).len(),
        4,
        "no further spawns after giving up"
    );
    assert_eq!(sup.snapshot().state, RendererStateKind::Failed);

    // An explicit start renews the budget.
    sup.start(false);
    wait_for(&sup, WAIT, "a new attempt", |s| {
        s.state == RendererStateKind::Starting
    })
    .await;
    sup.shutdown().await;
}

#[tokio::test]
async fn renderer_recovers_after_two_crashes() {
    let env = TestEnv::new("recover");
    let media = env.media("flaky.mp4", "crash-first=2");
    let sup = spawn(&env, &media);
    sup.start(false);
    wait_for(
        &sup,
        Duration::from_secs(20),
        "playing after recovery",
        |s| s.state == RendererStateKind::Playing,
    )
    .await;
    assert_eq!(launches(&media).len(), 3);
    sup.shutdown().await;
}

#[tokio::test]
async fn unsupported_media_fails_once_without_retrying() {
    let env = TestEnv::new("unsup");
    let media = env.media("broken.mp4", "unsupported");
    let sup = spawn(&env, &media);
    sup.start(false);
    let snap = wait_for(&sup, WAIT, "media-unsupported", |s| {
        s.state == RendererStateKind::Failed && s.failure_code == "media-unsupported"
    })
    .await;
    assert!(
        snap.failure_message.contains("could not play"),
        "{}",
        snap.failure_message
    );
    tokio::time::sleep(Duration::from_millis(700)).await;
    assert_eq!(
        launches(&media).len(),
        1,
        "deterministic failures are not retried"
    );
    sup.shutdown().await;
}

#[tokio::test]
async fn missing_media_never_spawns_a_process() {
    let env = TestEnv::new("nomedia");
    let media = env.media_dir.join("gone.mp4"); // never created
    let sup = spawn(&env, &media);
    sup.start(false);
    let snap = wait_for(&sup, WAIT, "media-missing", |s| {
        s.failure_code == "media-missing"
    })
    .await;
    assert!(
        snap.failure_message.contains("missing"),
        "{}",
        snap.failure_message
    );
    assert!(launches(&media).is_empty());
    sup.shutdown().await;
}

#[tokio::test]
async fn missing_mpv_is_reported_with_the_actionable_message_and_no_storm() {
    let env = TestEnv::new("nompv");
    let media = env.media("clip.mp4", "play");
    let mut cfg = config(&env);
    cfg.mpv_path = env.root.join("no-such-mpv");
    let sup = RendererSupervisor::spawn(cfg, settings(&media), None);
    sup.start(false);
    let snap = wait_for(&sup, WAIT, "mpv-missing", |s| {
        s.failure_code == "mpv-missing"
    })
    .await;
    assert!(
        snap.failure_message
            .contains("Lucerna could not start mpv.")
    );
    assert!(snap.failure_message.contains("Install mpv"));
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(
        sup.snapshot().failure_code,
        "mpv-missing",
        "terminal: no retry loop"
    );
    sup.shutdown().await;
}

#[tokio::test]
async fn hung_and_socketless_renderers_time_out_and_are_killed() {
    for (directive, name) in [("hang-before-load", "hang"), ("no-ipc", "noipc")] {
        let env = TestEnv::new(name);
        let media = env.media("clip.mp4", directive);
        let mut cfg = config(&env);
        cfg.restart_policy = policy(1, 60_000);
        let sup = RendererSupervisor::spawn(cfg, settings(&media), None);
        sup.start(false);
        let snap = wait_for(&sup, Duration::from_secs(20), "restart limit", |s| {
            s.failure_code == "restart-limit"
        })
        .await;
        assert!(
            snap.failure_message.contains("did not finish loading"),
            "{directive}: {}",
            snap.failure_message
        );
        assert_eq!(launches(&media).len(), 2, "{directive}");
        for launch in launches(&media) {
            let pid = launch["pid"].as_u64().unwrap();
            eventually(WAIT, "hung process to be killed", || !pid_alive(pid)).await;
        }
        sup.shutdown().await;
    }
}

#[tokio::test]
async fn losing_the_control_socket_kills_and_restarts_the_renderer() {
    let env = TestEnv::new("ipcdrop");
    let media = env.media("clip.mp4", "ipc-drop-after=300");
    let mut cfg = config(&env);
    cfg.restart_policy = policy(1, 60_000);
    let sup = RendererSupervisor::spawn(cfg, settings(&media), None);
    sup.start(false);
    let snap = wait_for(&sup, Duration::from_secs(20), "restart limit", |s| {
        s.failure_code == "restart-limit"
    })
    .await;
    assert!(
        snap.failure_message.contains("lost contact"),
        "{}",
        snap.failure_message
    );
    for launch in launches(&media) {
        let pid = launch["pid"].as_u64().unwrap();
        eventually(WAIT, "orphaned renderer to be killed", || !pid_alive(pid)).await;
    }
    sup.shutdown().await;
}

#[tokio::test]
async fn stop_escalates_from_quit_to_term_to_kill() {
    for (directive, name) in [
        ("ignore-quit", "term"),
        ("ignore-quit; ignore-term", "kill"),
    ] {
        let env = TestEnv::new(name);
        let media = env.media("clip.mp4", directive);
        let sup = spawn(&env, &media);
        sup.start(false);
        let snap = wait_for_state(&sup, RendererStateKind::Playing, WAIT).await;
        let pid = u64::from(snap.pid);
        sup.stop();
        wait_for_state(&sup, RendererStateKind::Stopped, WAIT).await;
        assert!(commands(&media).contains(&json!(["quit"])));
        eventually(WAIT, "process gone", || !pid_alive(pid)).await;
        sup.shutdown().await;
    }
}

#[tokio::test]
async fn stderr_flood_stays_within_the_log_bound_and_does_not_disturb_playback() {
    let env = TestEnv::new("flood");
    let media = env.media("clip.mp4", "stderr-flood=5000000");
    let sup = spawn(&env, &media);
    sup.start(false);
    wait_for_state(&sup, RendererStateKind::Playing, Duration::from_secs(20)).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    let files: Vec<_> = std::fs::read_dir(&env.log_dir).unwrap().flatten().collect();
    assert!(
        files.len() <= 2,
        "one log plus one rotated file, got {}",
        files.len()
    );
    assert!(
        dir_size(&env.log_dir) <= 1024 * 1024,
        "log dir is {} bytes",
        dir_size(&env.log_dir)
    );
    sup.shutdown().await;
}

#[tokio::test]
async fn stale_socket_files_are_replaced_at_start() {
    let env = TestEnv::new("stale");
    let media = env.media("clip.mp4", "play");
    let socket = socket_path(&env.runtime_dir, &output(), 1).unwrap();
    // First a plain file where the socket will go...
    std::fs::write(&socket, b"leftover").unwrap();
    let sup = spawn(&env, &media);
    sup.start(false);
    wait_for_state(&sup, RendererStateKind::Playing, WAIT).await;
    sup.shutdown().await;

    // ...then a dead socket (bound and abandoned).
    let media2 = env.media("clip2.mp4", "play");
    let socket2 = socket_path(&env.runtime_dir, &output(), 1).unwrap();
    drop(std::os::unix::net::UnixListener::bind(&socket2).unwrap());
    assert!(socket2.exists());
    let sup = spawn(&env, &media2);
    sup.start(false);
    wait_for_state(&sup, RendererStateKind::Playing, WAIT).await;
    sup.shutdown().await;
}

#[tokio::test]
async fn shutdown_removes_the_socket_and_the_registry_entry() {
    let env = TestEnv::new("cleanup");
    let media = env.media("clip.mp4", "play");
    let cfg = config(&env);
    let registry = Arc::clone(&cfg.registry);
    let sup = RendererSupervisor::spawn(cfg, settings(&media), None);
    sup.start(false);
    let snap = wait_for_state(&sup, RendererStateKind::Playing, WAIT).await;
    assert_eq!(registry.snapshot().len(), 1);
    assert_eq!(registry.snapshot()[0].pid, snap.pid);
    sup.shutdown().await;
    assert!(registry.snapshot().is_empty());
    let leftovers: Vec<_> = std::fs::read_dir(&env.runtime_dir)
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().ends_with(".sock"))
        .collect();
    assert!(leftovers.is_empty(), "sockets left: {leftovers:?}");
    assert!(!pid_alive(u64::from(snap.pid)));
}

/// Spawn a fake-mpv exactly as the supervisor would, and register it, without a supervisor.
fn launch_orphan(
    env: &TestEnv,
    media: &Path,
    output: &str,
    register: bool,
) -> (std::process::Child, PathBuf) {
    let out = OutputId::new(output);
    let socket = socket_path(&env.runtime_dir, &out, 1).unwrap();
    let spec = RenderSpec {
        embed: None,
        ipc_socket: socket.clone(),
        media: media.to_path_buf(),
        scaling: ScalingMode::Fill,
        hwdec: HwDecode::Auto,
        fps: FpsLimit::Native,
        audio: false,
        start_paused: false,
        vo_override: None,
    };
    let child = std::process::Command::new(FAKE_MPV)
        .args(build_args(&spec))
        .spawn()
        .unwrap();
    if register {
        let registry = PidRegistry::new(env.registry_path());
        registry.register(lucerna_mpv::PidRecord {
            pid: child.id(),
            starttime: std::fs::read_to_string(format!("/proc/{}/stat", child.id()))
                .unwrap()
                .rsplit_once(')')
                .unwrap()
                .1
                .split_whitespace()
                .nth(19)
                .unwrap()
                .parse()
                .unwrap(),
            socket: socket.to_string_lossy().into_owned(),
            output_id: output.to_owned(),
            generation: 1,
        });
    }
    (child, socket)
}

#[tokio::test]
async fn stale_process_recovery_kills_ours_and_spares_unrelated_renderers() {
    let env = TestEnv::new("recovery");
    let ours_media = env.media("ours.mp4", "play");
    let theirs_media = env.media("theirs.mp4", "play");

    let (mut ours, _) = launch_orphan(&env, &ours_media, "conn:OURS", true);
    // An mpv the user started themselves: same binary name, but not in the registry.
    let (mut theirs, _) = launch_orphan(&env, &theirs_media, "conn:THEIRS", false);
    let ours_pid = u64::from(ours.id());
    let theirs_pid = u64::from(theirs.id());
    eventually(WAIT, "both to be running", || {
        !launches(&ours_media).is_empty() && !launches(&theirs_media).is_empty()
    })
    .await;

    let registry_path = env.registry_path();
    let report = tokio::task::spawn_blocking(move || {
        recover_stale(
            &registry_path,
            &RecoveryOptions {
                exe_name: "fake-mpv".to_owned(),
                grace: Duration::from_secs(2),
            },
        )
    })
    .await
    .unwrap();

    assert_eq!(report.terminated, vec![ours.id()]);
    // Reap our child so it is not a zombie, then check both.
    let status = ours.wait().unwrap();
    assert!(!status.success(), "terminated by signal");
    assert!(!pid_alive(ours_pid));
    assert!(
        pid_alive(theirs_pid),
        "the unrelated renderer must survive recovery"
    );
    assert_eq!(std::fs::read_to_string(env.registry_path()).unwrap(), "[]");

    theirs.kill().unwrap();
    theirs.wait().unwrap();
}
