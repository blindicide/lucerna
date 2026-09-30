//! `lucernactl` against a running daemon over a private bus (directive §20, §35).

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::process::Stdio;

use lucerna_testkit::fixture::{Fixture, fake_mpv_path};
use lucerna_testkit::launches;
use lucerna_testkit::testbus::TestBus;
use tokio::process::Command;

const CTL: &str = env!("CARGO_BIN_EXE_lucernactl-under-test");

struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

/// Run `lucernactl` in a clean environment pointing at `bus` (the daemon's paths are the fixture's).
async fn ctl(bus: &str, root: &std::path::Path, args: &[&str]) -> Run {
    ctl_with(bus, root, args, &[]).await
}

async fn ctl_with(
    bus: &str,
    root: &std::path::Path,
    args: &[&str],
    extra_env: &[(&str, String)],
) -> Run {
    let out = Command::new(CTL)
        .args(args)
        .env_clear()
        .env("PATH", "/nonexistent-lucerna-test-path")
        .env("HOME", root.join("home"))
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("XDG_STATE_HOME", root.join("state"))
        .env("XDG_CACHE_HOME", root.join("cache"))
        .env("XDG_RUNTIME_DIR", root.join("runtime"))
        .env("DBUS_SESSION_BUS_ADDRESS", bus)
        .env("LUCERNA_MPV", fake_mpv_path())
        .env("XDG_SESSION_TYPE", "x11")
        .env("XDG_CURRENT_DESKTOP", "X-Cinnamon")
        .envs(extra_env.iter().map(|(k, v)| (*k, v.as_str())))
        .stdin(Stdio::null())
        .output()
        .await
        .expect("run lucernactl");
    Run {
        code: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

macro_rules! fixture {
    ($name:expr) => {
        match Fixture::start($name).await {
            Some(f) => f,
            None => return,
        }
    };
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn status_monitors_wallpapers_in_text_and_json() {
    let f = fixture!("cli-basic");
    f.ready().await;
    let bus = f.bus.address.clone();
    let root = f.env.root.clone();

    let status = ctl(&bus, &root, &["status"]).await;
    assert_eq!(status.code, 0, "{}", status.stderr);
    assert!(status.stdout.contains(&format!(
        "Lucerna daemon {} (API 1)",
        env!("CARGO_PKG_VERSION")
    )));
    assert!(
        status.stdout.contains("Backend:   cinnamon-x11")
            && status.stdout.contains("found (version 0.37.0)")
    );

    let json = ctl(&bus, &root, &["status", "--json"]).await;
    assert_eq!(json.code, 0);
    let parsed: serde_json::Value = serde_json::from_str(&json.stdout).expect("valid JSON");
    assert_eq!(parsed["backend"], "cinnamon-x11");
    assert_eq!(parsed["supported"], true);
    assert_eq!(parsed["api_version"], 1);
    assert!(parsed["renderers"].as_array().unwrap().is_empty());

    let monitors = ctl(&bus, &root, &["monitors"]).await;
    assert_eq!(monitors.code, 0);
    assert!(
        monitors.stdout.contains("HDMI-1 — 1920×1080 — Primary")
            && monitors.stdout.contains("id: conn:HDMI-1")
    );
    let displays: Vec<serde_json::Value> =
        serde_json::from_str(&ctl(&bus, &root, &["displays", "--json"]).await.stdout).unwrap();
    assert_eq!(displays.len(), 1);
    assert_eq!(displays[0]["id"], "conn:HDMI-1");
    assert_eq!(displays[0]["label"], "HDMI-1 — 1920×1080 — Primary");

    let empty = ctl(&bus, &root, &["wallpapers"]).await;
    assert_eq!(empty.code, 0);
    assert!(empty.stdout.contains("library is empty"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn play_pause_resume_stop_reload_drive_the_daemon() {
    let f = fixture!("cli-control");
    f.ready().await;
    let (bus, root) = (f.bus.address.clone(), f.env.root.clone());
    let media = f.env.media("clip.mp4", "play");

    let play = ctl(&bus, &root, &["play", media.to_str().unwrap()]).await;
    assert_eq!(play.code, 0, "{}", play.stderr);
    assert!(play.stdout.contains("Playing") && play.stdout.contains("all monitors"));
    f.wait_status("playing", |s| {
        s.renderers.first().is_some_and(|r| r.state == "playing")
    })
    .await;
    assert_eq!(launches(&media).len(), 1);

    let list = ctl(&bus, &root, &["wallpapers"]).await;
    assert!(
        list.stdout.contains("clip") && list.stdout.contains(media.to_str().unwrap()),
        "{}",
        list.stdout
    );
    let list_json: serde_json::Value =
        serde_json::from_str(&ctl(&bus, &root, &["wallpapers", "--json"]).await.stdout).unwrap();
    assert_eq!(list_json[0]["available"], true);

    assert_eq!(ctl(&bus, &root, &["pause"]).await.code, 0);
    let status = f
        .wait_status("paused", |s| {
            s.user_paused && s.renderers[0].state == "paused"
        })
        .await;
    assert_eq!(status.renderers[0].pause_reasons, ["user"]);
    let text = ctl(&bus, &root, &["status"]).await.stdout;
    assert!(text.contains("(paused: user)"), "{text}");

    assert_eq!(ctl(&bus, &root, &["resume"]).await.code, 0);
    f.wait_status("resumed", |s| {
        !s.user_paused && s.renderers[0].state == "playing"
    })
    .await;

    assert_eq!(ctl(&bus, &root, &["stop"]).await.code, 0);
    f.wait_status("stopped", |s| s.user_stopped).await;
    assert_eq!(ctl(&bus, &root, &["reload"]).await.code, 0);
    f.wait_status("playing after reload", |s| {
        !s.user_stopped && s.renderers.first().is_some_and(|r| r.state == "playing")
    })
    .await;

    // Playing a relative path works too (resolved against the current directory by the CLI).
    let relative = ctl(&bus, &root, &["play", "no-such-file.mp4"]).await;
    assert_eq!(relative.code, 4, "a missing file is a bad request");
    assert!(
        relative.stderr.contains("does not exist"),
        "{}",
        relative.stderr
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn bad_requests_and_unsupported_operations_have_their_exit_codes() {
    let f = fixture!("cli-errors");
    f.ready().await;
    let (bus, root) = (f.bus.address.clone(), f.env.root.clone());
    let media = f.env.media("clip.mp4", "play");

    // Unknown monitor: resolved client-side, exit 4, with the list of what exists.
    let bad = ctl(
        &bus,
        &root,
        &["play", media.to_str().unwrap(), "--monitor", "HDMI-9"],
    )
    .await;
    assert_eq!(bad.code, 4);
    assert!(
        bad.stderr.contains("No display matches 'HDMI-9'") && bad.stderr.contains("HDMI-1"),
        "{}",
        bad.stderr
    );

    // A connector name resolves to a display and the assignment is per display.
    let per_display = ctl(
        &bus,
        &root,
        &["play", media.to_str().unwrap(), "--monitor", "HDMI-1"],
    )
    .await;
    assert_eq!(per_display.code, 0, "{}", per_display.stderr);
    assert!(
        per_display.stdout.contains("on HDMI-1"),
        "{}",
        per_display.stdout
    );
    let assignments = f.proxy.get_assignments().await.unwrap();
    assert!(
        assignments
            .iter()
            .any(|a| { lucerna_ipc::dto::AssignmentDto::from_dict(a).display_id == "conn:HDMI-1" })
    );

    // Usage errors are clap's exit code 2.
    assert_eq!(ctl(&bus, &root, &["frobnicate"]).await.code, 2);
    assert_eq!(ctl(&bus, &root, &["play"]).await.code, 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stop_daemon_asks_the_service_to_quit() {
    let mut f = fixture!("cli-quit");
    f.ready().await;
    let run = ctl(
        &f.bus.address.clone(),
        &f.env.root.clone(),
        &["stop", "--daemon"],
    )
    .await;
    assert_eq!(run.code, 0, "{}", run.stderr);
    assert!(run.stdout.contains("quit"));
    assert_eq!(f.join().await, lucerna_daemon::Outcome::Clean);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn without_a_daemon_every_command_says_so_with_exit_3() {
    let Some(bus) = TestBus::start() else { return };
    let root = lucerna_testkit::TestEnv::new("cli-nodaemon");
    for args in [
        &["status"][..],
        &["monitors"],
        &["wallpapers"],
        &["pause"],
        &["resume"],
        &["stop"],
        &["reload"],
        &["stop", "--daemon"],
    ] {
        let run = ctl(&bus.address, &root.root, args).await;
        assert_eq!(run.code, 3, "{args:?}: {}", run.stderr);
        assert!(
            run.stderr.contains("The Lucerna daemon is not running."),
            "{args:?}: {}",
            run.stderr
        );
        assert!(run.stderr.contains("lucernad"), "it says how to start it");
        assert!(!run.stderr.contains("panicked"));
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn doctor_with_a_daemon_includes_its_diagnostics() {
    let f = fixture!("cli-doctor");
    f.ready().await;
    let (bus, root) = (f.bus.address.clone(), f.env.root.clone());

    let json = ctl(&bus, &root, &["doctor", "--json"]).await;
    assert_eq!(json.code, 0, "{}", json.stderr);
    let report: serde_json::Value = serde_json::from_str(&json.stdout).unwrap();
    assert_eq!(report["report_version"], 1);
    assert_eq!(report["lucerna_version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(report["daemon"]["reachable"], true);
    let d = &report["daemon"]["diagnostics"];
    assert_eq!(d["backend"]["kind"], "cinnamon-x11");
    assert_eq!(d["mpv"]["found"], true);
    assert_eq!(d["mpv"]["version"], "0.37.0");
    assert_eq!(d["status"]["supported"], true);
    assert_eq!(d["config"]["state"], "ok");
    assert_eq!(report["mpv"]["found"], true);
    assert_eq!(report["mpv"]["options_compatible"], true);
    assert_eq!(report["environment"]["XDG_SESSION_TYPE"], "x11");
    assert!(
        report["environment"].get("HOME").is_none(),
        "the environment is an allow-list, never dumped whole"
    );
    assert!(
        report
            .get("x11_probe")
            .is_none_or(serde_json::Value::is_null),
        "no probe when the daemon answers"
    );

    // Directive §21: every item of the diagnostic list has a home in the report.
    assert_eq!(d["daemon_version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(report["config"]["current_schema_version"], 1);
    assert!(d["status"]["schema_version"].is_number(), "schema version");
    for key in [
        "XDG_SESSION_TYPE",
        "XDG_CURRENT_DESKTOP",
        "DESKTOP_SESSION",
        "DISPLAY",
    ] {
        assert!(d["session"].get(key).is_some(), "session.{key}");
    }
    assert!(
        d["backend"]["available"].is_boolean(),
        "X connection status"
    );
    assert!(d["backend_diagnostics"].is_object(), "X window information");
    assert!(d["displays"][0]["id"].is_string(), "monitor identity");
    assert!(
        d["displays"][0]["connector"].is_string(),
        "detected outputs"
    );
    assert!(
        d["displays"][0].get("edid_model").is_some(),
        "EDID identity"
    );
    assert!(d["status"]["renderers"].is_array(), "renderer states");
    assert!(d["recent_failures"].is_array(), "recent renderer failures");
    for key in ["config_dir", "state_dir", "cache_dir", "runtime_dir"] {
        assert!(d["paths"][key].is_string(), "paths.{key}");
    }
    assert!(report["paths"]["config_file"].is_string());
    assert_eq!(report["autostart"]["state"], "enabled");

    let text = ctl(&bus, &root, &["doctor"]).await;
    assert_eq!(text.code, 0);
    assert!(
        text.stdout.contains("✓ Daemon: running") && text.stdout.contains("✓ mpv:"),
        "{}",
        text.stdout
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn doctor_without_a_daemon_still_produces_a_report() {
    let Some(bus) = TestBus::start() else { return };
    let env = lucerna_testkit::TestEnv::new("cli-doctor-alone");

    let run = ctl(&bus.address, &env.root, &["doctor", "--json"]).await;
    assert_eq!(
        run.code, 0,
        "doctor exits 0 whenever a report is produced: {}",
        run.stderr
    );
    let report: serde_json::Value = serde_json::from_str(&run.stdout).unwrap();
    assert_eq!(report["daemon"]["reachable"], false);
    assert!(
        report["daemon"]["error"]
            .as_str()
            .unwrap()
            .contains("not running")
    );
    assert_eq!(
        report["x11_probe"]["connected"], false,
        "no DISPLAY: the probe explains why instead of failing"
    );
    assert!(
        report["x11_probe"]["error"]
            .as_str()
            .unwrap()
            .contains("could not connect")
    );

    let plain = ctl(&bus.address, &env.root, &["doctor"]).await;
    assert_eq!(plain.code, 0);
    assert!(
        plain.stdout.contains("! Daemon: not reachable"),
        "{}",
        plain.stdout
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn doctor_redact_hides_the_home_directory_and_user_name() {
    let Some(bus) = TestBus::start() else { return };
    let env = lucerna_testkit::TestEnv::new("cli-doctor-redact");
    let home = env.root.join("alice-home");
    std::fs::create_dir_all(&home).unwrap();
    let vars = [
        ("HOME", home.to_string_lossy().into_owned()),
        (
            "XDG_CONFIG_HOME",
            home.join(".config").to_string_lossy().into_owned(),
        ),
        ("USER", "alice".to_owned()),
    ];

    let raw = ctl_with(&bus.address, &env.root, &["doctor", "--json"], &vars).await;
    assert!(
        raw.stdout.contains(home.to_str().unwrap()),
        "without --redact the real path is shown"
    );

    let redacted = ctl_with(
        &bus.address,
        &env.root,
        &["doctor", "--json", "--redact"],
        &vars,
    )
    .await;
    assert_eq!(redacted.code, 0, "{}", redacted.stderr);
    assert!(
        !redacted.stdout.contains(home.to_str().unwrap()),
        "{}",
        redacted.stdout
    );
    let report: serde_json::Value = serde_json::from_str(&redacted.stdout).unwrap();
    assert_eq!(report["config"]["file"], "~/.config/lucerna/config.toml");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_corrupt_configuration_is_explained_by_status_and_doctor() {
    let Some(f) = Fixture::start_with(
        "cli-corrupt",
        lucerna_testkit::fixture::Setup {
            config_toml: Some("this is [not valid".to_owned()),
            ..Default::default()
        },
    )
    .await
    else {
        return;
    };
    f.ready().await;
    let (bus, root) = (f.bus.address.clone(), f.env.root.clone());

    let status = ctl(&bus, &root, &["status"]).await;
    assert_eq!(status.code, 0, "{}", status.stderr);
    assert!(
        status
            .stdout
            .contains("Config:    defaults-after-corruption"),
        "{}",
        status.stdout
    );
    assert!(
        status.stdout.contains("could not be parsed")
            && status.stdout.contains("config.toml.corrupt-"),
        "the notice names what was done with the file: {}",
        status.stdout
    );

    let json: serde_json::Value =
        serde_json::from_str(&ctl(&bus, &root, &["status", "--json"]).await.stdout).unwrap();
    assert_eq!(json["config_state"], "defaults-after-corruption");

    let doctor: serde_json::Value =
        serde_json::from_str(&ctl(&bus, &root, &["doctor", "--json"]).await.stdout).unwrap();
    assert_eq!(
        doctor["daemon"]["diagnostics"]["config"]["state"],
        "defaults-after-corruption"
    );
    assert!(
        doctor["daemon"]["diagnostics"]["config"]["notice"]
            .as_str()
            .unwrap()
            .contains("kept as"),
        "{doctor}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_missing_mpv_is_explained_by_status() {
    let Some(f) = Fixture::start_with(
        "cli-no-mpv",
        lucerna_testkit::fixture::Setup {
            mpv_override: Some("/nonexistent/mpv".into()),
            ..Default::default()
        },
    )
    .await
    else {
        return;
    };
    f.ready().await;
    let (bus, root) = (f.bus.address.clone(), f.env.root.clone());
    let status = ctl(&bus, &root, &["status"]).await;
    assert!(
        status.stdout.contains("mpv:       NOT FOUND"),
        "{}",
        status.stdout
    );

    // `play` adds the file and assigns it; the renderer then reports the actionable message.
    let media = f.env.media("clip.mp4", "play");
    assert_eq!(
        ctl(&bus, &root, &["play", media.to_str().unwrap()])
            .await
            .code,
        0
    );
    f.wait_status("mpv-missing", |s| {
        s.renderers
            .first()
            .is_some_and(|r| r.failure_code == "mpv-missing")
    })
    .await;
    let status = ctl(&bus, &root, &["status"]).await;
    assert!(
        status.stdout.contains("Lucerna could not start mpv."),
        "{}",
        status.stdout
    );
    assert!(status.stdout.contains("Install mpv"), "{}", status.stdout);
}
