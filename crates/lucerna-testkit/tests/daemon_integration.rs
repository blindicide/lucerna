//! The whole daemon in-process: private bus, fake backend, fake mpv (directive §35).

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::ffi::OsString;
use std::time::Duration;

use lucerna_core::session::SessionEnv;
use lucerna_daemon::{BackendChoice, Outcome};
use lucerna_ipc::LucernaError;
use lucerna_ipc::dict::DictBuilder;
use lucerna_testkit::fixture::{Fixture, Setup, output};
use lucerna_testkit::{commands, eventually, launches};
use serde_json::json;

macro_rules! fixture {
    ($name:expr) => {
        match Fixture::start($name).await {
            Some(f) => f,
            None => return,
        }
    };
    ($name:expr, $setup:expr) => {
        match Fixture::start_with($name, $setup).await {
            Some(f) => f,
            None => return,
        }
    };
}

fn lerr(err: zbus::Error) -> LucernaError {
    LucernaError::from(err)
}

const PLAY: &str = "play";

async fn play_media(f: &Fixture, name: &str, directives: &str) -> (std::path::PathBuf, String) {
    let media = f.env.media(name, directives);
    let id = f
        .proxy
        .add_wallpaper(media.to_str().unwrap(), "")
        .await
        .expect("AddWallpaper");
    (media, id)
}

// ---------------------------------------------------------------------------- startup, status

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn startup_reports_a_healthy_status_and_quit_cleans_up() {
    let mut f = fixture!("status");
    let status = f.ready().await;
    assert!(status.supported && status.unsupported_reason.is_empty());
    assert_eq!(status.backend, "cinnamon-x11");
    assert_eq!(status.session_type, "x11");
    assert_eq!(status.daemon_version, env!("CARGO_PKG_VERSION"));
    assert_eq!(status.api_version, 1);
    assert!(status.mpv_available);
    assert_eq!(status.mpv_version, "0.37.0");
    assert_eq!(status.config_state, "ok");
    assert_eq!(status.schema_version, 1);
    assert_eq!(status.playback, "idle");
    assert!(status.fullscreen_detection);

    // The interface's properties.
    let version: String = f.proxy.version().await.unwrap();
    assert_eq!(version, env!("CARGO_PKG_VERSION"));
    assert_eq!(f.proxy.api_version().await.unwrap(), 1);

    // First run: the autostart entry is created (D4, §22).
    assert!(f.paths.autostart_file().exists());
    assert!(f.settings().await.autostart);

    let runtime = f.paths.runtime_dir.clone().unwrap();
    assert!(runtime.join("daemon.lock").exists());
    assert_eq!(f.quit().await, Outcome::Clean);

    assert!(
        std::fs::read_dir(&runtime).unwrap().flatten().all(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            !name.ends_with(".sock") && name != "renderers.json"
        }),
        "no sockets or registry are left behind"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_displays_are_reported_with_readable_labels() {
    let f = fixture!(
        "displays",
        Setup {
            outputs: vec![output("HDMI-1", 0, false), output("eDP-1", 1920, true)],
            ..Setup::default()
        }
    );
    f.ready().await;
    let displays = f.displays().await;
    assert_eq!(displays.len(), 2);
    let edp = displays.iter().find(|d| d.connector == "eDP-1").unwrap();
    assert_eq!(edp.label, "eDP-1 — 1920×1080 — Primary");
    assert_eq!(edp.id, "conn:eDP-1");
    assert!(edp.connected && edp.primary);
    assert_eq!(
        displays
            .iter()
            .find(|d| d.connector == "HDMI-1")
            .unwrap()
            .label,
        "HDMI-1 — 1920×1080"
    );
    assert!(
        edp.edid_serial.is_empty(),
        "serials are not exposed over the bus by default"
    );
}

// ------------------------------------------------------------------- single-instance enforcement

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_second_daemon_on_the_same_bus_exits_cleanly() {
    use lucerna_core::paths::Paths;
    use lucerna_daemon::{BusChoice, DaemonOptions};

    let mut first = fixture!("dup-bus");
    first.ready().await;

    // A second daemon for the same session bus but with its own runtime directory: the bus name
    // is what stops it.
    let other = lucerna_testkit::TestEnv::new("dup-bus-2");
    let paths = Paths::under(&other.root);
    std::fs::create_dir_all(paths.runtime_dir.as_ref().unwrap().parent().unwrap()).unwrap();
    std::fs::set_permissions(
        paths.runtime_dir.as_ref().unwrap().parent().unwrap(),
        std::os::unix::fs::PermissionsExt::from_mode(0o700),
    )
    .unwrap();
    let options = DaemonOptions {
        paths,
        session: SessionEnv::default(),
        bus: BusChoice::Address(first.bus.address.clone()),
        backend: BackendChoice::Injected(Box::new(lucerna_core::testing::FakeBackend::new())),
        mpv_override: None,
        path_var: None,
        daemon_exe: "/usr/bin/lucernad".into(),
        display_wait: Duration::ZERO,
        renderer_timings: lucerna_core::renderer::Timings::default(),
        vo_override: None,
        extra_env: Vec::new(),
        recheck_interval: Duration::from_secs(60),
        handle_signals: false,
        autostart_on_first_run: false,
    };
    assert_eq!(
        lucerna_daemon::run(options).await,
        Outcome::AlreadyRunning { pid: None }
    );

    // The first daemon is unaffected.
    assert!(first.status().await.supported);
    assert_eq!(first.quit().await, Outcome::Clean);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_second_daemon_with_the_same_runtime_directory_is_stopped_by_the_lock() {
    use lucerna_daemon::{BusChoice, DaemonOptions};

    let mut first = fixture!("dup-lock");
    first.ready().await;
    let second_bus = lucerna_testkit::testbus::TestBus::start().expect("second bus");
    let options = DaemonOptions {
        paths: first.paths.clone(), // same runtime directory
        session: SessionEnv::default(),
        bus: BusChoice::Address(second_bus.address.clone()),
        backend: BackendChoice::Injected(Box::new(lucerna_core::testing::FakeBackend::new())),
        mpv_override: None,
        path_var: None,
        daemon_exe: "/usr/bin/lucernad".into(),
        display_wait: Duration::ZERO,
        renderer_timings: lucerna_core::renderer::Timings::default(),
        vo_override: None,
        extra_env: Vec::new(),
        recheck_interval: Duration::from_secs(60),
        handle_signals: false,
        autostart_on_first_run: false,
    };
    assert_eq!(
        lucerna_daemon::run(options).await,
        Outcome::AlreadyRunning {
            pid: Some(std::process::id())
        },
        "the lock file names the daemon that holds it"
    );
    assert!(first.status().await.supported);
    first.quit().await;
}

// ------------------------------------------------------------------------------- the library

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn adding_and_removing_wallpapers_never_touches_the_media() {
    let f = fixture!("library");
    f.ready().await;
    let media = f.env.media("Rain Loop.mp4", PLAY);

    let id = f
        .proxy
        .add_wallpaper(media.to_str().unwrap(), "")
        .await
        .unwrap();
    let again = f
        .proxy
        .add_wallpaper(media.to_str().unwrap(), "Renamed")
        .await
        .unwrap();
    assert_eq!(id, again, "adding the same file twice is idempotent");
    let list = f.wallpapers().await;
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].name, "Rain Loop");
    assert_eq!(list[0].media_type, "video");
    assert!(list[0].available);
    assert!(f.config_text().contains("[[wallpapers]]"));

    f.proxy.remove_wallpaper(&id).await.unwrap();
    assert!(f.wallpapers().await.is_empty());
    assert!(
        media.exists(),
        "removing a library entry must never delete the user's file"
    );
    assert!(!f.config_text().contains("[[wallpapers]]"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn bad_requests_get_specific_errors_and_actionable_messages() {
    let f = fixture!("errors");
    f.ready().await;

    let e = lerr(
        f.proxy
            .add_wallpaper("relative/path.mp4", "")
            .await
            .unwrap_err(),
    );
    assert!(
        matches!(e, LucernaError::InvalidPath(ref m) if m.contains("not absolute")),
        "{e:?}"
    );

    let missing = f.env.media_dir.join("nope.mp4");
    let e = lerr(
        f.proxy
            .add_wallpaper(missing.to_str().unwrap(), "")
            .await
            .unwrap_err(),
    );
    assert!(
        matches!(e, LucernaError::FileNotFound(ref m) if m.contains("does not exist")),
        "{e:?}"
    );

    let e = lerr(
        f.proxy
            .add_wallpaper(f.env.media_dir.to_str().unwrap(), "")
            .await
            .unwrap_err(),
    );
    assert!(matches!(e, LucernaError::NotAFile(_)), "{e:?}");

    let e = lerr(f.proxy.set_wallpaper("no-such-id", "*").await.unwrap_err());
    assert!(
        matches!(e, LucernaError::UnknownWallpaper(ref m) if m.contains("lucernactl wallpapers")),
        "{e:?}"
    );
    let e = lerr(f.proxy.remove_wallpaper("no-such-id").await.unwrap_err());
    assert!(matches!(e, LucernaError::UnknownWallpaper(_)));

    let (_, id) = play_media(&f, "ok.mp4", PLAY).await;
    let e = lerr(f.proxy.set_wallpaper(&id, "conn:HDMI-1").await.unwrap_err());
    assert!(
        matches!(e, LucernaError::InvalidArgument(ref m) if m.contains("not available in this version")),
        "{e:?}"
    );
    let e = lerr(f.proxy.set_scaling("*", "zoom").await.unwrap_err());
    assert!(
        matches!(e, LucernaError::InvalidArgument(ref m) if m.contains("fill, fit, stretch, center")),
        "{e:?}"
    );
    let e = lerr(f.proxy.set_scaling("conn:NOPE-9", "fit").await.unwrap_err());
    assert!(
        matches!(e, LucernaError::UnknownDisplay(ref m) if m.contains("lucernactl monitors")),
        "{e:?}"
    );
}

// ---------------------------------------------------------------------------- playback control

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn play_pause_resume_stop_start() {
    let f = fixture!("playback");
    f.ready().await;
    let (media, id) = play_media(&f, "clip.mp4", PLAY).await;
    f.proxy.set_wallpaper(&id, "*").await.unwrap();

    let status = f
        .wait_status("playing", |s| {
            s.renderers.first().is_some_and(|r| r.state == "playing")
        })
        .await;
    assert_eq!(status.playback, "playing");
    assert_eq!(status.renderers[0].display_id, "conn:HDMI-1");
    assert_eq!(status.renderers[0].wallpaper_id, id);
    assert_eq!(status.renderers[0].scaling, "fill");
    assert_ne!(status.renderers[0].pid, 0);
    assert_eq!(f.fake.live_surfaces().len(), 1);
    assert_eq!(launches(&media).len(), 1);
    let argv = launches(&media)[0]["argv"].clone();
    assert!(
        argv.to_string().contains("--wid="),
        "the renderer is embedded in the surface: {argv}"
    );

    f.proxy.pause().await.unwrap();
    let status = f
        .wait_status("paused", |s| s.renderers[0].state == "paused")
        .await;
    assert!(status.user_paused);
    assert_eq!(status.renderers[0].pause_reasons, ["user"]);
    assert_eq!(status.playback, "paused");
    eventually(Duration::from_secs(5), "pause reaches mpv", || {
        commands(&media).contains(&json!(["set_property", "pause", true]))
    })
    .await;

    f.proxy.resume().await.unwrap();
    let status = f
        .wait_status("resumed", |s| s.renderers[0].state == "playing")
        .await;
    assert!(!status.user_paused && status.renderers[0].pause_reasons.is_empty());

    f.proxy.stop().await.unwrap();
    let status = f
        .wait_status("stopped", |s| s.user_stopped && s.renderers.is_empty())
        .await;
    assert_eq!(status.playback, "stopped");
    assert!(
        f.fake.live_surfaces().is_empty(),
        "stopping removes the wallpaper windows"
    );

    f.proxy.start().await.unwrap();
    f.wait_status("playing again", |s| {
        s.renderers.first().is_some_and(|r| r.state == "playing")
    })
    .await;
    assert_eq!(launches(&media).len(), 2);
    assert_eq!(f.fake.live_surfaces().len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_pause_before_playing_starts_the_renderer_paused() {
    let f = fixture!("pause-first");
    f.ready().await;
    f.proxy.pause().await.unwrap();
    let (media, id) = play_media(&f, "clip.mp4", PLAY).await;
    f.proxy.set_wallpaper(&id, "*").await.unwrap();
    f.wait_status("paused", |s| {
        s.renderers.first().is_some_and(|r| r.state == "paused")
    })
    .await;
    assert!(
        launches(&media)[0]["argv"]
            .to_string()
            .contains("--pause=yes"),
        "no frame of motion is shown"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn choosing_a_wallpaper_after_stop_plays_it() {
    let f = fixture!("stop-then-set");
    f.ready().await;
    let (_, id) = play_media(&f, "a.mp4", PLAY).await;
    f.proxy.set_wallpaper(&id, "*").await.unwrap();
    f.wait_status("playing", |s| {
        s.renderers.first().is_some_and(|r| r.state == "playing")
    })
    .await;
    f.proxy.stop().await.unwrap();
    f.wait_status("stopped", |s| s.user_stopped).await;
    let (_, id2) = play_media(&f, "b.mp4", PLAY).await;
    f.proxy.set_wallpaper(&id2, "*").await.unwrap();
    let status = f
        .wait_status("playing b", |s| {
            s.renderers.first().is_some_and(|r| r.state == "playing")
        })
        .await;
    assert!(!status.user_stopped);
    assert_eq!(status.renderers[0].wallpaper_id, id2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn scaling_changes_are_live_and_persist() {
    let f = fixture!("scaling");
    f.ready().await;
    let (media, id) = play_media(&f, "clip.mp4", PLAY).await;
    f.proxy.set_wallpaper(&id, "*").await.unwrap();
    f.wait_status("playing", |s| {
        s.renderers.first().is_some_and(|r| r.state == "playing")
    })
    .await;

    f.proxy.set_scaling("*", "center").await.unwrap();
    eventually(Duration::from_secs(5), "scaling reaches mpv", || {
        commands(&media).contains(&json!(["set_property", "video-unscaled", "yes"]))
    })
    .await;
    assert_eq!(launches(&media).len(), 1, "no restart for a scaling change");
    assert_eq!(f.status().await.renderers[0].scaling, "center");
    assert!(f.config_text().contains("scaling = \"center\""));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn launch_time_settings_restart_the_renderer() {
    let f = fixture!("restart-on-audio");
    f.ready().await;
    let (media, id) = play_media(&f, "clip.mp4", PLAY).await;
    f.proxy.set_wallpaper(&id, "*").await.unwrap();
    f.wait_status("playing", |s| {
        s.renderers.first().is_some_and(|r| r.state == "playing")
    })
    .await;
    assert!(
        launches(&media)[0]["argv"]
            .to_string()
            .contains("--mute=yes"),
        "audio is off by default"
    );

    f.proxy
        .set_settings(DictBuilder::new().bool("audio", true).build())
        .await
        .unwrap();
    eventually(Duration::from_secs(10), "second launch", || {
        launches(&media).len() == 2
    })
    .await;
    let second = launches(&media)[1]["argv"].to_string();
    assert!(
        second.contains("--aid=auto") && second.contains("--volume=100"),
        "{second}"
    );
    f.wait_status("playing again", |s| {
        s.renderers.first().is_some_and(|r| r.state == "playing")
    })
    .await;
    assert_eq!(f.fake.live_surfaces().len(), 1, "the surface is reused");
}

// ------------------------------------------------------------------------------------ settings

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn settings_round_trip_validate_and_persist() {
    let f = fixture!("settings");
    f.ready().await;
    let defaults = f.settings().await;
    assert!(defaults.pause_on_fullscreen && defaults.pause_on_lock && !defaults.audio);
    assert_eq!(
        (
            defaults.hardware_decode.as_str(),
            defaults.fps_limit.as_str(),
            defaults.stacking.as_str()
        ),
        ("auto", "native", "auto")
    );
    assert_eq!(
        (defaults.max_restarts, defaults.restart_window_secs),
        (3, 60)
    );

    f.proxy
        .set_settings(
            DictBuilder::new()
                .bool("pause_on_fullscreen", false)
                .str("hardware_decode", "disabled")
                .str("fps_limit", "30")
                .str("stacking", "desktop-window")
                .u32("max_restarts", 5)
                .build(),
        )
        .await
        .unwrap();
    let now = f.settings().await;
    assert!(
        !now.pause_on_fullscreen && now.pause_on_lock,
        "untouched settings keep their value"
    );
    assert_eq!(
        (
            now.hardware_decode.as_str(),
            now.fps_limit.as_str(),
            now.stacking.as_str()
        ),
        ("disabled", "30", "desktop-window")
    );
    assert_eq!(now.max_restarts, 5);
    let text = f.config_text();
    for expected in [
        "pause_on_fullscreen = false",
        "hardware_decode = \"disabled\"",
        "fps_limit = \"30\"",
        "stacking = \"desktop-window\"",
        "max_restarts = 5",
    ] {
        assert!(text.contains(expected), "{expected} missing from:\n{text}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn invalid_settings_are_rejected_as_a_whole() {
    let f = fixture!("settings-invalid");
    f.ready().await;
    let before = f.settings().await;

    // A typo in a key is an error, never silently ignored, and nothing is applied.
    let e = f
        .proxy
        .set_settings(
            DictBuilder::new()
                .bool("audio", true)
                .bool("pause_on_fullscren", true)
                .build(),
        )
        .await
        .unwrap_err();
    assert!(
        matches!(lerr(e), LucernaError::InvalidArgument(ref m) if m.contains("pause_on_fullscren"))
    );
    // A bad value for one key stops the whole update.
    let e = f
        .proxy
        .set_settings(
            DictBuilder::new()
                .bool("audio", true)
                .str("fps_limit", "144")
                .build(),
        )
        .await
        .unwrap_err();
    assert!(
        matches!(lerr(e), LucernaError::InvalidArgument(ref m) if m.contains("native, 60, 30, 15"))
    );
    let e = f
        .proxy
        .set_settings(DictBuilder::new().u32("max_restarts", 99).build())
        .await
        .unwrap_err();
    assert!(matches!(lerr(e), LucernaError::InvalidArgument(_)));
    let e = f
        .proxy
        .set_settings(DictBuilder::new().str("audio", "yes").build())
        .await
        .unwrap_err();
    assert!(matches!(lerr(e), LucernaError::InvalidArgument(ref m) if m.contains("wrong type")));
    assert_eq!(f.settings().await, before, "all or nothing");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn autostart_is_a_real_switch() {
    let f = fixture!("autostart");
    f.ready().await;
    let file = f.paths.autostart_file();
    assert!(file.exists() && f.settings().await.autostart);
    let entry = std::fs::read_to_string(&file).unwrap();
    assert!(
        entry.contains("Exec=/usr/bin/lucernad") && entry.contains("TryExec=/usr/bin/lucernad")
    );

    f.proxy
        .set_settings(DictBuilder::new().bool("autostart", false).build())
        .await
        .unwrap();
    assert!(
        !file.exists(),
        "disabling autostart removes the only startup mechanism"
    );
    assert!(!f.settings().await.autostart);
    assert!(
        !f.config_text().contains("autostart"),
        "the state lives in the file, not in config.toml (D4)"
    );

    f.proxy
        .set_settings(DictBuilder::new().bool("autostart", true).build())
        .await
        .unwrap();
    assert!(file.exists() && f.settings().await.autostart);
}

// ------------------------------------------------------------------- configuration robustness

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_corrupt_config_is_moved_aside_and_the_daemon_carries_on() {
    let f = fixture!(
        "corrupt",
        Setup {
            config_toml: Some("this is [not valid toml".to_owned()),
            ..Setup::default()
        }
    );
    let status = f.ready().await;
    assert_eq!(status.config_state, "defaults-after-corruption");
    assert!(
        status.config_notice.contains("could not be parsed")
            && status.config_notice.contains("config.toml.corrupt-")
    );
    let backups: Vec<_> = std::fs::read_dir(&f.paths.config_dir)
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().contains(".corrupt-"))
        .collect();
    assert_eq!(backups.len(), 1, "kept, never deleted");
    assert_eq!(
        std::fs::read_to_string(backups[0].path()).unwrap(),
        "this is [not valid toml"
    );

    // The daemon is fully usable afterwards.
    let (_, id) = play_media(&f, "clip.mp4", PLAY).await;
    f.proxy.set_wallpaper(&id, "*").await.unwrap();
    f.wait_status("playing", |s| {
        s.renderers.first().is_some_and(|r| r.state == "playing")
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_newer_config_schema_is_read_only_but_still_renders() {
    let f = fixture!("future-schema");
    // Put a media file where the config expects it, then start.
    drop(f);
    let env = lucerna_testkit::TestEnv::new("future-media");
    let media = env.media("clip.mp4", PLAY);
    let toml = format!(
        "schema_version = 9\nbrand_new_thing = true\n[general]\nfps_limit = \"144\"\n\n[all_displays]\nwallpaper = \"w1\"\n\n[[wallpapers]]\nid = \"w1\"\nname = \"Clip\"\npath = \"{}\"\nadded = 2026-09-30T10:00:00Z\n",
        media.display()
    );
    let f = fixture!(
        "future-schema2",
        Setup {
            config_toml: Some(toml.clone()),
            ..Setup::default()
        }
    );
    let status = f.ready().await;
    assert_eq!(status.config_state, "read-only-newer-schema");
    assert!(status.config_notice.contains("newer Lucerna"));
    f.wait_status("playing from the newer config", |s| {
        s.renderers.first().is_some_and(|r| r.state == "playing")
    })
    .await;

    // Runtime state (pause) is not configuration, so it still works.
    f.proxy.pause().await.unwrap();
    let e = lerr(
        f.proxy
            .set_settings(DictBuilder::new().bool("audio", true).build())
            .await
            .unwrap_err(),
    );
    assert!(
        matches!(e, LucernaError::ConfigReadOnly(ref m) if m.contains("newer Lucerna (schema 9)")),
        "{e:?}"
    );
    let e = lerr(
        f.proxy
            .add_wallpaper(media.to_str().unwrap(), "")
            .await
            .unwrap_err(),
    );
    assert!(matches!(e, LucernaError::ConfigReadOnly(_)));
    assert_eq!(
        f.config_text(),
        toml.replace("{ROOT}", ""),
        "the file is byte-for-byte untouched"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn unknown_config_keys_survive_a_daemon_save() {
    let toml = "schema_version = 1\n# my notes\nfuture_key = \"keep\"\n\n[general]\nsparkles = 3\n";
    let f = fixture!(
        "unknown-keys",
        Setup {
            config_toml: Some(toml.to_owned()),
            ..Setup::default()
        }
    );
    f.ready().await;
    f.proxy
        .set_settings(DictBuilder::new().bool("audio", true).build())
        .await
        .unwrap();
    let text = f.config_text();
    for keep in [
        "# my notes",
        "future_key = \"keep\"",
        "sparkles = 3",
        "audio = true",
    ] {
        assert!(text.contains(keep), "{keep} missing from:\n{text}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn reload_applies_an_external_edit() {
    let f = fixture!("reload");
    f.ready().await;
    let (media, id) = play_media(&f, "clip.mp4", PLAY).await;
    f.proxy.set_wallpaper(&id, "*").await.unwrap();
    f.wait_status("playing", |s| {
        s.renderers.first().is_some_and(|r| r.state == "playing")
    })
    .await;

    // The user edits config.toml by hand.
    let edited = f.config_text().replace(
        "[all_displays]",
        "[general]\nfps_limit = \"15\"\n\n[all_displays]",
    );
    std::fs::write(f.paths.config_file(), edited).unwrap();
    assert_eq!(
        f.settings().await.fps_limit,
        "native",
        "not applied until reload"
    );

    f.proxy.reload().await.unwrap();
    assert_eq!(f.settings().await.fps_limit, "15");
    f.wait_status("playing after reload", |s| {
        s.renderers.first().is_some_and(|r| r.state == "playing")
    })
    .await;
    assert!(
        launches(&media).last().unwrap()["argv"]
            .to_string()
            .contains("--vf=fps=15")
    );
}

// ----------------------------------------------------------------- missing media and missing mpv

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_missing_wallpaper_file_never_spawns_and_never_shows_a_surface() {
    let toml = "schema_version = 1\n[all_displays]\nwallpaper = \"gone\"\n\n[[wallpapers]]\nid = \"gone\"\nname = \"Gone\"\npath = \"{ROOT}/media/absent.mp4\"\nadded = 2026-09-30T10:00:00Z\n";
    let f = fixture!(
        "missing-media",
        Setup {
            config_toml: Some(toml.to_owned()),
            ..Setup::default()
        }
    );
    f.ready().await;
    let status = f
        .wait_status("media-missing", |s| {
            s.renderers
                .first()
                .is_some_and(|r| r.failure_code == "media-missing")
        })
        .await;
    assert_eq!(status.renderers[0].state, "failed");
    assert!(status.renderers[0].failure_message.contains("missing"));
    assert_eq!(status.playback, "failed");
    assert!(
        f.fake.live_surfaces().is_empty(),
        "nothing covers the normal desktop background"
    );
    let list = f.wallpapers().await;
    assert!(
        !list[0].available,
        "the library remembers the file is missing but keeps the entry"
    );
    assert!(f.config_text().contains("available = false"));
    // The daemon is fine.
    assert!(f.status().await.supported);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_missing_mpv_is_reported_with_the_actionable_message() {
    let f = fixture!(
        "no-mpv",
        Setup {
            mpv_override: Some(OsString::from("/nonexistent/mpv")),
            ..Setup::default()
        }
    );
    let status = f.ready().await;
    assert!(!status.mpv_available);
    let (_, id) = play_media(&f, "clip.mp4", PLAY).await;
    f.proxy.set_wallpaper(&id, "*").await.unwrap();
    let status = f
        .wait_status("mpv-missing", |s| {
            s.renderers
                .first()
                .is_some_and(|r| r.failure_code == "mpv-missing")
        })
        .await;
    assert!(
        status.renderers[0]
            .failure_message
            .contains("Lucerna could not start mpv.")
    );
    assert!(f.fake.live_surfaces().is_empty());

    let e = lerr(f.proxy.start().await.unwrap_err());
    assert!(
        matches!(e, LucernaError::MpvMissing(ref m) if m.contains("Install mpv")),
        "{e:?}"
    );
    assert_eq!(e.exit_code(), 7);
}

// ------------------------------------------------------------------------ unsupported sessions

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_wayland_session_is_reported_clearly_and_starts_nothing() {
    let session = SessionEnv {
        xdg_session_type: Some("wayland".to_owned()),
        wayland_display: Some("wayland-0".to_owned()),
        display: Some(":0".to_owned()),
        ..SessionEnv::default()
    };
    let f = fixture!(
        "wayland",
        Setup {
            session,
            backend: Some(BackendChoice::Auto),
            ..Setup::default()
        }
    );
    let status = f.ready().await;
    assert!(!status.supported);
    assert_eq!(status.backend, "none");
    assert_eq!(status.session_type, "wayland");
    assert_eq!(status.unsupported_reason, "wayland");
    assert!(
        status
            .unsupported_message
            .contains("supports X11 sessions only")
            && status.unsupported_message.contains("Wayland")
    );
    assert_eq!(status.playback, "idle");
    assert!(f.displays().await.is_empty());

    // Everything still works as a control plane, but nothing is played and nothing is spawned.
    let (media, id) = play_media(&f, "clip.mp4", PLAY).await;
    f.proxy.set_wallpaper(&id, "*").await.unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(f.status().await.renderers.is_empty());
    assert!(
        launches(&media).is_empty(),
        "no mpv is started in an unsupported session"
    );
    let e = lerr(f.proxy.start().await.unwrap_err());
    assert!(
        matches!(e, LucernaError::Unsupported(ref m) if m.contains("Wayland")),
        "{e:?}"
    );
    assert_eq!(e.exit_code(), 5);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn no_display_is_reported_as_such() {
    let session = SessionEnv {
        xdg_session_type: None,
        display: None,
        ..SessionEnv::default()
    };
    let f = fixture!(
        "nodisplay",
        Setup {
            session,
            backend: Some(BackendChoice::Auto),
            ..Setup::default()
        }
    );
    let status = f.ready().await;
    assert!(!status.supported);
    assert_eq!(status.unsupported_reason, "no-display");
    assert!(status.unsupported_message.contains("DISPLAY is not set"));
}

// -------------------------------------------------------------------------- stale runtime files

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stale_sockets_and_registry_from_a_previous_run_are_cleaned_at_startup() {
    let f = fixture!("stale-start", Setup {
        runtime_files: vec![
            ("mpv-deadbeef-3.sock".to_owned(), b"stale".to_vec()),
            ("notes.txt".to_owned(), b"not ours to touch".to_vec()),
            // A registry entry whose process does not exist: recovery must skip it, not crash.
            (
                "renderers.json".to_owned(),
                br#"[{"pid": 4294967290, "starttime": 1, "socket": "/x.sock", "output_id": "conn:A", "generation": 1}]"#.to_vec(),
            ),
        ],
        ..Setup::default()
    });
    f.ready().await;
    let runtime = f.paths.runtime_dir.clone().unwrap();
    assert!(
        !runtime.join("mpv-deadbeef-3.sock").exists(),
        "stale sockets are removed"
    );
    assert!(
        runtime.join("notes.txt").exists(),
        "unrelated files are left alone"
    );
    assert_eq!(
        std::fs::read_to_string(runtime.join("renderers.json")).unwrap(),
        "[]",
        "the registry is reset"
    );
}
