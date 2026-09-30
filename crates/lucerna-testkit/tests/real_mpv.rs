//! Tests against the real mpv installed on the machine (directive §9, §10).
//!
//! Skipped with a visible message when mpv is not installed, unless `LUCERNA_REQUIRE_MPV=1`
//! (set in CI), in which case a missing mpv is a failure.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use lucerna_core::backend::OutputId;
use lucerna_core::mpv::{MpvInfo, RenderSpec, build_args, check_compat, discover_from_env};
use lucerna_core::renderer::{RendererStateKind, RestartPolicy, Timings};
use lucerna_core::runtime::socket_path;
use lucerna_core::types::{FpsLimit, HwDecode, ScalingMode};
use lucerna_mpv::{LaunchSettings, MpvIpc, PidRegistry, RendererSupervisor, SupervisorConfig};
use lucerna_testkit::{TestEnv, wait_for, wait_for_state};
use serde_json::json;

const WAIT: Duration = Duration::from_secs(20);

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/media")
        .join(name)
}

/// The installed mpv, or `None` (skip) when it is absent and not required.
fn real_mpv() -> Option<MpvInfo> {
    match discover_from_env() {
        Ok(info) => Some(info),
        Err(err) => {
            assert!(
                std::env::var_os("LUCERNA_REQUIRE_MPV").is_none(),
                "LUCERNA_REQUIRE_MPV is set but mpv is unusable: {err}"
            );
            eprintln!("SKIPPED (no mpv): {err}");
            None
        }
    }
}

fn supervisor(
    env: &TestEnv,
    mpv: &MpvInfo,
    media: &Path,
    settings: impl FnOnce(&mut LaunchSettings),
) -> RendererSupervisor {
    let mut launch = LaunchSettings {
        media: media.to_path_buf(),
        scaling: ScalingMode::Fill,
        hwdec: HwDecode::Auto,
        fps: FpsLimit::Native,
        audio: false,
    };
    settings(&mut launch);
    let config = SupervisorConfig {
        output: OutputId::new("conn:REAL-1"),
        mpv_path: mpv.path.clone(),
        runtime_dir: env.runtime_dir.clone(),
        log_dir: Some(env.log_dir.clone()),
        log_max_bytes: 512 * 1024,
        embed: None,
        restart_policy: RestartPolicy::default(),
        timings: Timings::default(),
        // No display on the server: render to the null video output.
        vo_override: Some("null".to_owned()),
        extra_env: Vec::new(),
        registry: Arc::new(PidRegistry::new(env.registry_path())),
    };
    RendererSupervisor::spawn(config, launch, None)
}

#[test]
fn discovery_reads_the_installed_version() {
    let Some(info) = real_mpv() else { return };
    assert!(info.path.is_absolute());
    assert!(
        info.version
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_digit()),
        "{info:?}"
    );
    assert!(info.version_line.starts_with("mpv "));
}

#[test]
fn options_supported() {
    let Some(info) = real_mpv() else { return };
    let report = check_compat(&info.path).expect("probe runs");
    assert!(
        report.compatible,
        "mpv {} rejected our options: {}",
        info.version, report.detail
    );
}

macro_rules! plays {
    ($name:ident, $file:literal) => {
        #[tokio::test]
        async fn $name() {
            let Some(mpv) = real_mpv() else { return };
            let env = TestEnv::new(stringify!($name));
            let sup = supervisor(&env, &mpv, &fixture($file), |_| {});
            sup.start(false);
            wait_for_state(&sup, RendererStateKind::Playing, WAIT).await;

            sup.pause();
            wait_for_state(&sup, RendererStateKind::Paused, WAIT).await;
            sup.resume();
            wait_for_state(&sup, RendererStateKind::Playing, WAIT).await;

            sup.stop();
            let snap = wait_for_state(&sup, RendererStateKind::Stopped, WAIT).await;
            assert!(
                snap.failure_code.is_empty(),
                "clean quit is not a failure: {snap:?}"
            );
            sup.shutdown().await;
        }
    };
}

plays!(plays_mp4, "sample.mp4");
plays!(plays_webm, "sample.webm");
plays!(plays_mkv, "sample.mkv");
plays!(plays_gif, "sample.gif");

#[tokio::test]
async fn corrupt_file_is_media_unsupported_and_is_not_retried() {
    let Some(mpv) = real_mpv() else { return };
    let env = TestEnv::new("corrupt");
    let sup = supervisor(&env, &mpv, &fixture("corrupt.mp4"), |_| {});
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
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert_eq!(
        sup.snapshot().state,
        RendererStateKind::Failed,
        "deterministic: no restart"
    );
    assert_eq!(sup.snapshot().restarts_in_window, 0);
    sup.shutdown().await;
}

#[tokio::test]
async fn fps_cap_hwdec_off_and_audio_options_are_accepted_by_mpv() {
    let Some(mpv) = real_mpv() else { return };
    let env = TestEnv::new("opts");
    let sup = supervisor(&env, &mpv, &fixture("sample.mp4"), |l| {
        l.fps = FpsLimit::Fps30;
        l.hwdec = HwDecode::Disabled;
        l.audio = true;
        l.scaling = ScalingMode::Center;
    });
    sup.start(false);
    wait_for_state(&sup, RendererStateKind::Playing, WAIT).await;
    sup.set_scaling(ScalingMode::Stretch);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        sup.snapshot().state,
        RendererStateKind::Playing,
        "live scaling change is accepted"
    );
    sup.shutdown().await;
}

#[tokio::test]
async fn starting_paused_reaches_paused_without_playing() {
    let Some(mpv) = real_mpv() else { return };
    let env = TestEnv::new("startpaused");
    let sup = supervisor(&env, &mpv, &fixture("sample.webm"), |_| {});
    sup.start(true);
    wait_for_state(&sup, RendererStateKind::Paused, WAIT).await;
    sup.resume();
    wait_for_state(&sup, RendererStateKind::Playing, WAIT).await;
    sup.shutdown().await;
}

/// The supervisor decides "already loaded" by asking for `time-pos` after connecting, because a
/// fast mpv may finish loading before we connect and its `file-loaded` event would be missed.
/// This pins down that assumption on the real mpv, in both the playing and paused start modes.
#[tokio::test]
async fn time_pos_answers_once_loaded_even_for_a_late_client() {
    let Some(mpv) = real_mpv() else { return };
    for paused in [false, true] {
        let env = TestEnv::new("timepos");
        let socket = socket_path(&env.runtime_dir, &OutputId::new("conn:X"), 1).unwrap();
        let spec = RenderSpec {
            embed: None,
            ipc_socket: socket.clone(),
            media: fixture("sample.mp4"),
            scaling: ScalingMode::Fill,
            hwdec: HwDecode::Auto,
            fps: FpsLimit::Native,
            audio: false,
            start_paused: paused,
            vo_override: Some("null".to_owned()),
        };
        let mut child = tokio::process::Command::new(&mpv.path)
            .args(build_args(&spec))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn mpv");

        // Connect long after the file has loaded.
        tokio::time::sleep(Duration::from_millis(1500)).await;
        let (ipc, _events) = MpvIpc::connect(
            &socket,
            Duration::from_millis(20),
            Duration::from_secs(5),
            || false,
        )
        .await
        .expect("connect");
        let pos = ipc.get_property("time-pos").await;
        assert!(
            pos.as_ref().is_ok_and(|v| !v.is_null()),
            "paused={paused}: time-pos was {pos:?}"
        );
        assert_eq!(ipc.get_property("pause").await.unwrap(), json!(paused));

        // A real round trip of the commands the supervisor sends.
        ipc.set_property("pause", !paused).await.expect("set pause");
        assert_eq!(ipc.get_property("pause").await.unwrap(), json!(!paused));
        for (name, value) in ScalingMode::Fit.mpv_properties() {
            ipc.set_property(name, value)
                .await
                .unwrap_or_else(|e| panic!("{name}: {e}"));
        }
        let _ = ipc.send(&[json!("quit")]);
        let status = tokio::time::timeout(Duration::from_secs(10), child.wait())
            .await
            .expect("mpv exits after quit")
            .unwrap();
        assert!(status.success(), "quit gives exit 0, got {status:?}");
    }
}
