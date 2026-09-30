//! Per-monitor assignments, hotplug, fullscreen and lock policy through the whole daemon
//! (directive §12, §13, §15). Backend events come from the fake backend; screen-lock state comes
//! from fake session services on private buses. Nothing here proves how it looks or behaves on a
//! real desktop: that stays IMPLEMENTED — MANUAL DESKTOP VALIDATION REQUIRED (LUC-T09, T12, T13).

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::time::Duration;

use lucerna_core::backend::{BackendEvent, Rect};
use lucerna_core::testing::FakeCall;
use lucerna_ipc::dict::DictBuilder;
use lucerna_ipc::dto::{AssignmentDto, RendererDto, StatusDto};
use lucerna_testkit::fixture::{Fixture, Setup, output};
use lucerna_testkit::providers::LockProvider;
use lucerna_testkit::{commands, eventually, launches};
use serde_json::json;

macro_rules! fixture {
    ($name:expr, $setup:expr) => {
        match Fixture::start_with($name, $setup).await {
            Some(f) => f,
            None => return,
        }
    };
}

fn two_monitors() -> Setup {
    Setup {
        outputs: vec![output("HDMI-1", 0, true), output("DP-1", 1920, false)],
        ..Setup::default()
    }
}

fn renderer<'a>(s: &'a StatusDto, connector: &str) -> Option<&'a RendererDto> {
    s.renderers.iter().find(|r| r.connector == connector)
}

fn playing(s: &StatusDto, connector: &str) -> bool {
    renderer(s, connector).is_some_and(|r| r.state == "playing")
}

async fn add(f: &Fixture, name: &str) -> (std::path::PathBuf, String) {
    let media = f.env.media(name, "play");
    let id = f
        .proxy
        .add_wallpaper(media.to_str().unwrap(), "")
        .await
        .unwrap();
    (media, id)
}

// ------------------------------------------------------------------------------ assignments

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn each_display_can_have_its_own_wallpaper_and_scaling() {
    let f = fixture!("per-display", two_monitors());
    f.ready().await;
    let (rain, rain_id) = add(&f, "rain.mp4").await;
    let (city, city_id) = add(&f, "city.mp4").await;

    // Mode 1 (§13): one wallpaper on every display.
    f.proxy.set_wallpaper(&rain_id, "*").await.unwrap();
    f.wait_status("both playing rain", |s| {
        playing(s, "HDMI-1") && playing(s, "DP-1")
    })
    .await;
    assert_eq!(f.fake.live_surfaces().len(), 2, "one surface per display");

    // Mode 2: an override for one display only.
    f.proxy.set_wallpaper(&city_id, "conn:DP-1").await.unwrap();
    let status = f
        .wait_status("DP-1 plays the city", |s| {
            renderer(s, "DP-1").is_some_and(|r| r.wallpaper_id == city_id && r.state == "playing")
        })
        .await;
    assert_eq!(renderer(&status, "HDMI-1").unwrap().wallpaper_id, rain_id);
    assert_eq!(launches(&city).len(), 1);
    assert_eq!(
        launches(&rain).len(),
        2,
        "rain ran on both displays before DP-1 was overridden"
    );

    let displays = f.displays().await;
    let hdmi = displays.iter().find(|d| d.connector == "HDMI-1").unwrap();
    let dp = displays.iter().find(|d| d.connector == "DP-1").unwrap();
    assert_eq!(
        (hdmi.wallpaper_source.as_str(), dp.wallpaper_source.as_str()),
        ("all", "display")
    );
    assert!(f.config_text().contains("[displays.\"conn:DP-1\"]"));

    // Scaling per display, applied live.
    f.proxy.set_scaling("conn:DP-1", "center").await.unwrap();
    eventually(Duration::from_secs(5), "scaling reaches mpv", || {
        commands(&city).contains(&json!(["set_property", "video-unscaled", "yes"]))
    })
    .await;
    let displays = f.displays().await;
    let dp = displays.iter().find(|d| d.connector == "DP-1").unwrap();
    let hdmi = displays.iter().find(|d| d.connector == "HDMI-1").unwrap();
    assert_eq!(
        (dp.scaling.as_str(), dp.scaling_source.as_str()),
        ("center", "display")
    );
    assert_eq!(
        (hdmi.scaling.as_str(), hdmi.scaling_source.as_str()),
        ("fill", "all")
    );
    assert_eq!(
        launches(&city).len(),
        1,
        "scaling never restarts a renderer"
    );

    // The all-displays scaling reaches displays that do not override it.
    f.proxy.set_scaling("*", "fit").await.unwrap();
    eventually(Duration::from_secs(5), "fit reaches HDMI-1", || {
        commands(&rain).contains(&json!(["set_property", "panscan", "0.0"]))
            && commands(&rain).contains(&json!(["set_property", "keepaspect", "yes"]))
    })
    .await;
    let dp = f
        .displays()
        .await
        .into_iter()
        .find(|d| d.connector == "DP-1")
        .unwrap();
    assert_eq!(dp.scaling, "center", "the override wins");

    // `inherit` removes the override; clearing the wallpaper returns the display to the shared one.
    f.proxy.set_scaling("conn:DP-1", "inherit").await.unwrap();
    f.proxy.clear_assignment("conn:DP-1").await.unwrap();
    f.wait_status("DP-1 back to rain", |s| {
        renderer(s, "DP-1").is_some_and(|r| r.wallpaper_id == rain_id && r.state == "playing")
    })
    .await;
    assert!(
        !f.config_text().contains("[displays."),
        "an override that overrides nothing is removed from the file:\n{}",
        f.config_text()
    );
    let assignments: Vec<_> = f
        .proxy
        .get_assignments()
        .await
        .unwrap()
        .iter()
        .map(AssignmentDto::from_dict)
        .collect();
    assert_eq!(assignments.len(), 1);
    assert_eq!(assignments[0].display_id, "*");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_absent_display_keeps_its_assignment_and_gets_it_back() {
    let f = fixture!("hotplug", two_monitors());
    f.ready().await;
    let (rain, rain_id) = add(&f, "rain.mp4").await;
    let (city, city_id) = add(&f, "city.mp4").await;
    f.proxy.set_wallpaper(&rain_id, "*").await.unwrap();
    f.proxy.set_wallpaper(&city_id, "conn:DP-1").await.unwrap();
    let status = f
        .wait_status("both playing", |s| {
            playing(s, "HDMI-1") && playing(s, "DP-1")
        })
        .await;
    let hdmi_pid = renderer(&status, "HDMI-1").unwrap().pid;
    let city_launches = launches(&city).len();

    // Unplug DP-1.
    f.fake.set_outputs(vec![output("HDMI-1", 0, true)]);
    f.fake.emit(BackendEvent::OutputsChanged);
    let status = f
        .wait_status("only HDMI-1 remains", |s| {
            s.renderers.len() == 1 && playing(s, "HDMI-1")
        })
        .await;
    assert_eq!(
        renderer(&status, "HDMI-1").unwrap().pid,
        hdmi_pid,
        "the other display is not disturbed"
    );
    assert_eq!(
        f.fake.live_surfaces().len(),
        1,
        "no surface for an absent display"
    );

    let displays = f.displays().await;
    let gone = displays.iter().find(|d| d.id == "conn:DP-1").unwrap();
    assert!(!gone.connected);
    assert!(
        gone.label.contains("DP-1") && gone.label.contains("(disconnected)"),
        "{}",
        gone.label
    );
    assert_eq!(gone.wallpaper_id, city_id, "the assignment is preserved");
    assert!(f.config_text().contains(&city_id));
    assert_eq!(
        launches(&city).len(),
        city_launches,
        "no renderer runs for it"
    );

    // Plug it back in: its wallpaper returns without any user action.
    f.fake
        .set_outputs(vec![output("HDMI-1", 0, true), output("DP-1", 1920, false)]);
    f.fake.emit(BackendEvent::OutputsChanged);
    let status = f
        .wait_status("DP-1 is back", |s| {
            renderer(s, "DP-1").is_some_and(|r| r.wallpaper_id == city_id && r.state == "playing")
        })
        .await;
    assert_eq!(f.fake.live_surfaces().len(), 2);
    assert_eq!(
        renderer(&status, "HDMI-1").unwrap().pid,
        hdmi_pid,
        "still undisturbed"
    );
    assert_eq!(launches(&city).len(), city_launches + 1);
    assert_eq!(
        launches(&rain).len(),
        2,
        "rain: once per display at first, never again"
    );
    assert!(f.displays().await.iter().all(|d| d.connected));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn display_identities_survive_a_different_enumeration_order() {
    let f = fixture!("order", two_monitors());
    f.ready().await;
    let (_media, id) = add(&f, "rain.mp4").await;
    f.proxy.set_wallpaper(&id, "conn:DP-1").await.unwrap();
    f.wait_status("DP-1 playing", |s| playing(s, "DP-1")).await;
    let dp_pid = renderer(&f.status().await, "DP-1").unwrap().pid;

    // The server now reports the monitors in the opposite order. Assignments follow identity, not
    // position in the list.
    f.fake
        .set_outputs(vec![output("DP-1", 1920, false), output("HDMI-1", 0, true)]);
    f.fake.emit(BackendEvent::OutputsChanged);
    tokio::time::sleep(Duration::from_millis(300)).await;
    let status = f.status().await;
    assert_eq!(
        renderer(&status, "DP-1").unwrap().pid,
        dp_pid,
        "DP-1 keeps its renderer"
    );
    assert!(
        renderer(&status, "HDMI-1").is_none(),
        "HDMI-1 has no wallpaper assigned"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_geometry_change_resizes_the_surface_without_restarting() {
    let f = fixture!("resize", Setup::default());
    f.ready().await;
    let (media, id) = add(&f, "rain.mp4").await;
    f.proxy.set_wallpaper(&id, "*").await.unwrap();
    f.wait_status("playing", |s| playing(s, "HDMI-1")).await;

    let mut changed = output("HDMI-1", 0, true);
    changed.geometry = Rect::new(0, 0, 2560, 1440);
    f.fake.set_outputs(vec![changed]);
    f.fake.emit(BackendEvent::OutputsChanged);
    eventually(Duration::from_secs(5), "the surface is resized", || {
        f.fake
            .calls()
            .iter()
            .any(|c| matches!(c, FakeCall::Resize(_, r) if *r == Rect::new(0, 0, 2560, 1440)))
    })
    .await;
    assert_eq!(
        launches(&media).len(),
        1,
        "mpv follows the surface; no restart"
    );
    assert_eq!(f.displays().await[0].width, 2560);
}

// ------------------------------------------------------------------------------- fullscreen

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fullscreen_pauses_only_the_occluded_monitor() {
    let f = fixture!("fullscreen", two_monitors());
    f.ready().await;
    let (_m, id) = add(&f, "rain.mp4").await;
    f.proxy.set_wallpaper(&id, "*").await.unwrap();
    f.wait_status("both playing", |s| {
        playing(s, "HDMI-1") && playing(s, "DP-1")
    })
    .await;

    // A fullscreen window on HDMI-1.
    f.fake.emit(BackendEvent::FullscreenChanged(vec![Rect::new(
        0, 0, 1920, 1080,
    )]));
    let status = f
        .wait_status("HDMI-1 paused", |s| {
            renderer(s, "HDMI-1").is_some_and(|r| r.state == "paused")
        })
        .await;
    assert_eq!(
        renderer(&status, "HDMI-1").unwrap().pause_reasons,
        ["fullscreen"]
    );
    assert!(playing(&status, "DP-1"), "the other monitor keeps playing");
    assert_eq!(status.playback, "playing");

    // It moves to DP-1: HDMI-1 resumes, DP-1 pauses.
    f.fake.emit(BackendEvent::FullscreenChanged(vec![Rect::new(
        1920, 0, 1920, 1080,
    )]));
    f.wait_status("switched", |s| {
        playing(s, "HDMI-1") && renderer(s, "DP-1").is_some_and(|r| r.state == "paused")
    })
    .await;

    // A window spanning both monitors pauses both.
    f.fake.emit(BackendEvent::FullscreenChanged(vec![Rect::new(
        0, 0, 3840, 1080,
    )]));
    let status = f
        .wait_status("both paused", |s| {
            s.renderers.iter().all(|r| r.state == "paused")
        })
        .await;
    assert_eq!(status.playback, "paused");

    // Back to nothing fullscreen.
    f.fake.emit(BackendEvent::FullscreenChanged(vec![]));
    f.wait_status("both playing again", |s| {
        playing(s, "HDMI-1") && playing(s, "DP-1")
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_fullscreen_setting_can_be_turned_off_and_a_partial_cover_does_not_pause() {
    let f = fixture!("fullscreen-setting", Setup::default());
    f.ready().await;
    let (_m, id) = add(&f, "rain.mp4").await;
    f.proxy.set_wallpaper(&id, "*").await.unwrap();
    f.wait_status("playing", |s| playing(s, "HDMI-1")).await;

    // A window covering less than 90% of the monitor is not "fullscreen" for this purpose.
    f.fake.emit(BackendEvent::FullscreenChanged(vec![Rect::new(
        0, 0, 1000, 700,
    )]));
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(playing(&f.status().await, "HDMI-1"));

    f.fake.emit(BackendEvent::FullscreenChanged(vec![Rect::new(
        0, 0, 1920, 1080,
    )]));
    f.wait_status("paused", |s| {
        renderer(s, "HDMI-1").is_some_and(|r| r.state == "paused")
    })
    .await;
    f.proxy
        .set_settings(
            DictBuilder::new()
                .bool("pause_on_fullscreen", false)
                .build(),
        )
        .await
        .unwrap();
    f.wait_status("resumed because the policy is off", |s| {
        playing(s, "HDMI-1")
    })
    .await;
}

// ----------------------------------------------------------------------------------- lock

async fn lock_scenario(kind: LockProvider, name: &str) {
    let Some(f) = Fixture::start_with(
        name,
        Setup {
            lock_provider: kind,
            ..two_monitors()
        },
    )
    .await
    else {
        return;
    };
    let status = f.ready().await;
    assert!(
        status.lock_detection,
        "{kind:?}: lock detection is available"
    );
    let (_m, id) = add(&f, "rain.mp4").await;
    f.proxy.set_wallpaper(&id, "*").await.unwrap();
    f.wait_status("both playing", |s| {
        playing(s, "HDMI-1") && playing(s, "DP-1")
    })
    .await;

    let lock = f.lock.as_ref().expect("provider");
    lock.set_locked(true).await;
    let status = f
        .wait_status("locked", |s| {
            s.session_locked && s.renderers.iter().all(|r| r.state == "paused")
        })
        .await;
    assert!(status.renderers.iter().all(|r| r.pause_reasons == ["lock"]));

    lock.set_locked(false).await;
    f.wait_status("unlocked", |s| {
        !s.session_locked && playing(s, "HDMI-1") && playing(s, "DP-1")
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_cinnamon_screensaver_pauses_every_renderer() {
    lock_scenario(LockProvider::Cinnamon, "lock-cinnamon").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_freedesktop_screensaver_is_the_second_choice() {
    lock_scenario(LockProvider::Freedesktop, "lock-fdo").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn logind_locked_hint_is_the_third_choice() {
    lock_scenario(LockProvider::Logind, "lock-logind").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn without_any_lock_service_detection_is_reported_unavailable() {
    let f = fixture!("lock-none", Setup::default());
    let status = f.ready().await;
    assert!(!status.lock_detection);
    assert!(!status.session_locked);
    let diag: serde_json::Value =
        serde_json::from_str(&f.proxy.get_diagnostics(false).await.unwrap()).unwrap();
    assert_eq!(diag["lock_detection"]["available"], false);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_session_that_starts_locked_starts_its_renderers_paused() {
    let f = fixture!(
        "lock-initial",
        Setup {
            lock_provider: LockProvider::Cinnamon,
            initially_locked: true,
            ..Setup::default()
        }
    );
    f.ready().await;
    let (media, id) = add(&f, "rain.mp4").await;
    f.proxy.set_wallpaper(&id, "*").await.unwrap();
    f.wait_status("paused from the start", |s| {
        renderer(s, "HDMI-1").is_some_and(|r| r.state == "paused")
    })
    .await;
    assert!(
        launches(&media)[0]["argv"]
            .to_string()
            .contains("--pause=yes"),
        "no frame of motion behind the lock screen"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pause_reasons_combine_and_the_lock_setting_can_be_turned_off() {
    let f = fixture!(
        "lock-combine",
        Setup {
            lock_provider: LockProvider::Cinnamon,
            ..Setup::default()
        }
    );
    f.ready().await;
    let (_m, id) = add(&f, "rain.mp4").await;
    f.proxy.set_wallpaper(&id, "*").await.unwrap();
    f.wait_status("playing", |s| playing(s, "HDMI-1")).await;

    f.proxy.pause().await.unwrap();
    f.f_lock().set_locked(true).await;
    let status = f
        .wait_status("paused for two reasons", |s| {
            renderer(s, "HDMI-1").is_some_and(|r| r.pause_reasons == ["user", "lock"])
        })
        .await;
    assert_eq!(status.playback, "paused");

    // Resume clears only the user's reason; the lock still holds.
    f.proxy.resume().await.unwrap();
    let status = f
        .wait_status("resume leaves the lock", |s| {
            renderer(s, "HDMI-1").is_some_and(|r| r.pause_reasons == ["lock"])
        })
        .await;
    assert_eq!(renderer(&status, "HDMI-1").unwrap().state, "paused");

    // With the policy off, locking no longer pauses.
    f.proxy
        .set_settings(DictBuilder::new().bool("pause_on_lock", false).build())
        .await
        .unwrap();
    f.wait_status("playing although locked", |s| playing(s, "HDMI-1"))
        .await;
}

trait LockHandle {
    fn f_lock(&self) -> &lucerna_testkit::providers::LockProviderHandle;
}

impl LockHandle for Fixture {
    fn f_lock(&self) -> &lucerna_testkit::providers::LockProviderHandle {
        self.lock.as_ref().expect("a lock provider")
    }
}

// ----------------------------------------------------------------------------------- restack

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_restack_fight_with_the_window_manager_is_rate_limited() {
    let f = fixture!("restack", Setup::default());
    f.ready().await;
    for _ in 0..40 {
        f.fake.emit(BackendEvent::StackingDisturbed);
    }
    tokio::time::sleep(Duration::from_millis(400)).await;
    let refreshes = f
        .fake
        .calls()
        .iter()
        .filter(|c| **c == FakeCall::Refresh)
        .count();
    assert!(
        (1..=5).contains(&refreshes),
        "at most five refreshes per ten seconds, got {refreshes}"
    );
    let diag: serde_json::Value =
        serde_json::from_str(&f.proxy.get_diagnostics(false).await.unwrap()).unwrap();
    assert!(
        diag["restack"]["fights"].as_u64().unwrap() > 0,
        "the fight is counted for doctor: {diag}"
    );
}
