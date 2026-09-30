//! Desktop lifecycle through the whole daemon: renderer crashes and recovery, the restart limit,
//! missing media coming back, mpv installed later, configuration problems (directive §30 v0.6.0,
//! §49, §51). In-process daemon on a private bus with a fake backend and a fake mpv.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::time::Duration;

use futures_util::StreamExt;
use lucerna_ipc::dto::StatusDto;
use lucerna_testkit::fixture::{Fixture, Setup, fake_mpv_path};
use lucerna_testkit::launches;

macro_rules! fixture {
    ($name:expr, $setup:expr) => {
        match Fixture::start_with($name, $setup).await {
            Some(f) => f,
            None => return,
        }
    };
}

fn playing(s: &StatusDto) -> bool {
    s.renderers.first().is_some_and(|r| r.state == "playing")
}

fn failed_with(s: &StatusDto, code: &str) -> bool {
    s.renderers
        .first()
        .is_some_and(|r| r.state == "failed" && r.failure_code == code)
}

/// A config with the given wallpaper assigned to all displays and a small restart budget.
fn config_for(media: &std::path::Path, extra: &str) -> String {
    format!(
        "schema_version = 1\n{extra}\n[all_displays]\nwallpaper = \"w1\"\n\n[[wallpapers]]\nid = \"w1\"\nname = \"Clip\"\npath = \"{}\"\nadded = 2026-09-30T10:00:00Z\n",
        media.display()
    )
}

// --------------------------------------------------------------------- crash and recovery

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_renderer_that_crashes_twice_is_restarted_and_the_failures_are_announced() {
    let env = lucerna_testkit::TestEnv::new("crashy-media");
    let media = env.media("flaky.mp4", "crash-first=2");
    let f = fixture!(
        "crash-recovery",
        Setup {
            config_toml: Some(config_for(&media, "")),
            ..Setup::default()
        }
    );
    let mut failures = f.proxy.receive_renderer_failed().await.unwrap();
    f.ready().await;

    f.wait_status("playing after two crashes", playing).await;
    assert_eq!(launches(&media).len(), 3, "two crashes, then a good run");

    let first = tokio::time::timeout(Duration::from_secs(5), failures.next())
        .await
        .expect("a RendererFailed signal")
        .expect("stream open");
    let args = first.args().unwrap();
    assert_eq!(args.display_id, "conn:HDMI-1");
    assert_eq!(args.code, "crashed");
    assert!(
        args.message.contains("crashed") && args.message.contains("Hardware decoding"),
        "{}",
        args.message
    );
    assert_eq!(
        f.fake.live_surfaces().len(),
        1,
        "the surface stays while retries are pending"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_crash_storm_stops_at_the_limit_hides_the_surface_and_start_recovers() {
    let env = lucerna_testkit::TestEnv::new("storm-media");
    let media = env.media("broken.mp4", "crash-after=50");
    let f = fixture!(
        "crash-storm",
        Setup {
            config_toml: Some(config_for(
                &media,
                "[renderer]\nmax_restarts = 1\nrestart_window_secs = 10\n"
            )),
            ..Setup::default()
        }
    );
    f.ready().await;

    let status = f
        .wait_status("restart limit", |s| failed_with(s, "restart-limit"))
        .await;
    let renderer = &status.renderers[0];
    assert!(
        renderer.failure_message.contains("stopped restarting"),
        "{}",
        renderer.failure_message
    );
    assert_eq!(status.playback, "failed");
    assert_eq!(
        launches(&media).len(),
        2,
        "the initial start plus exactly one restart"
    );
    assert!(
        f.fake.live_surfaces().is_empty(),
        "a failed renderer must not leave a black window over the desktop"
    );
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(
        launches(&media).len(),
        2,
        "no restart storm after giving up"
    );

    // The user fixes the file and asks to start again.
    std::fs::write(&media, "FAKE-MPV: play\n").unwrap();
    f.proxy.start().await.unwrap();
    f.wait_status("playing after Start", playing).await;
    assert_eq!(f.fake.live_surfaces().len(), 1);
    assert_eq!(
        f.status().await.renderers[0].restarts_in_window,
        0,
        "the budget was renewed"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_unplayable_file_fails_once_with_an_explanation_and_reload_retries_it() {
    let env = lucerna_testkit::TestEnv::new("unsupported-media");
    let media = env.media("odd.mp4", "unsupported");
    let f = fixture!(
        "unsupported",
        Setup {
            config_toml: Some(config_for(&media, "")),
            ..Setup::default()
        }
    );
    f.ready().await;
    let status = f
        .wait_status("media-unsupported", |s| failed_with(s, "media-unsupported"))
        .await;
    assert!(
        status.renderers[0]
            .failure_message
            .contains("could not play")
    );
    assert!(f.fake.live_surfaces().is_empty());
    assert_eq!(launches(&media).len(), 1);

    std::fs::write(&media, "FAKE-MPV: play\n").unwrap();
    f.proxy.reload().await.unwrap();
    f.wait_status("playing after Reload", playing).await;
}

// --------------------------------------------------------------------------- missing media

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_missing_wallpaper_comes_back_by_itself_when_the_file_returns() {
    let env = lucerna_testkit::TestEnv::new("drive-media");
    let media = env.media_dir.join("on-external-drive.mp4"); // not there yet
    let f = fixture!(
        "media-returns",
        Setup {
            config_toml: Some(config_for(&media, "")),
            recheck_interval: Duration::from_millis(200),
            ..Setup::default()
        }
    );
    f.ready().await;
    f.wait_status("media-missing", |s| failed_with(s, "media-missing"))
        .await;
    assert!(f.fake.live_surfaces().is_empty());
    assert!(
        launches(&media).is_empty(),
        "nothing is spawned for a missing file"
    );
    assert!(!f.wallpapers().await[0].available);

    // The drive is mounted: no user action is needed.
    std::fs::write(&media, "FAKE-MPV: play\n").unwrap();
    f.wait_status("playing without any action", playing).await;
    assert_eq!(launches(&media).len(), 1);
    assert!(f.wallpapers().await[0].available);
    assert!(
        f.config_text().contains("available = true"),
        "{}",
        f.config_text()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_file_that_vanishes_is_noticed_in_the_library_and_the_entry_is_kept() {
    let f = fixture!("media-vanishes", Setup::default());
    f.ready().await;
    let media = f.env.media("clip.mp4", "play");
    let id = f
        .proxy
        .add_wallpaper(media.to_str().unwrap(), "")
        .await
        .unwrap();
    assert!(f.wallpapers().await[0].available);

    std::fs::remove_file(&media).unwrap();
    let list = f.wallpapers().await;
    assert_eq!(list.len(), 1, "the entry stays");
    assert_eq!(list[0].id, id);
    assert!(!list[0].available);
}

// ------------------------------------------------------------------------------- mpv missing

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn mpv_installed_after_the_daemon_started_is_picked_up_by_reload() {
    let env = lucerna_testkit::TestEnv::new("late-mpv");
    let mpv_path = env.root.join("bin-mpv");
    let f = fixture!(
        "mpv-later",
        Setup {
            mpv_override: Some(mpv_path.clone().into_os_string()),
            ..Setup::default()
        }
    );
    let status = f.ready().await;
    assert!(!status.mpv_available);
    let media = f.env.media("clip.mp4", "play");
    let id = f
        .proxy
        .add_wallpaper(media.to_str().unwrap(), "")
        .await
        .unwrap();
    f.proxy.set_wallpaper(&id, "*").await.unwrap();
    let status = f
        .wait_status("mpv-missing", |s| failed_with(s, "mpv-missing"))
        .await;
    assert!(status.renderers[0].failure_message.contains("Install mpv"));
    assert!(launches(&media).is_empty());

    // The user installs mpv (here: a link to the fake) and reloads.
    std::os::unix::fs::symlink(fake_mpv_path(), &mpv_path).unwrap();
    f.proxy.reload().await.unwrap();
    let status = f.wait_status("playing", playing).await;
    assert!(status.mpv_available);
}

// ----------------------------------------------------------------------------- config problems

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn autostart_is_created_on_first_run_only_and_a_disabled_entry_stays_disabled() {
    // An existing configuration and no autostart entry means the user turned it off.
    let f = fixture!(
        "autostart-off",
        Setup {
            config_toml: Some("schema_version = 1\n".to_owned()),
            ..Setup::default()
        }
    );
    f.ready().await;
    assert!(
        !f.paths.autostart_file().exists(),
        "the daemon must not bring the entry back"
    );
    assert!(!f.settings().await.autostart);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_autostart_entry_disabled_from_the_desktop_settings_is_reported_as_disabled() {
    let f = fixture!(
        "hidden-entry",
        Setup {
            config_toml: Some("schema_version = 1\n".to_owned()),
            ..Setup::default()
        }
    );
    std::fs::create_dir_all(f.paths.autostart_file().parent().unwrap()).unwrap();
    std::fs::write(
        f.paths.autostart_file(),
        "[Desktop Entry]\nType=Application\nExec=lucernad\nHidden=true\n",
    )
    .unwrap();
    f.ready().await;
    assert!(
        !f.settings().await.autostart,
        "Startup Applications turned it off; Lucerna respects that"
    );
    assert!(
        f.paths.autostart_file().exists(),
        "and does not delete the user's own edit"
    );
}
