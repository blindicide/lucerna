//! Structural test of the GTK application (directive §23, §30 v0.4.0).
//!
//! **This checks structure and wiring only** - which pages and controls exist, and that they
//! react to the daemon - by constructing the window on an Xvfb display without presenting it. It
//! says nothing about layout, fonts, theming, spacing, or how anything looks: visual quality is
//! NOT VALIDATED ON DEVELOPMENT SERVER (see docs/MANUAL-ACCEPTANCE.md).
//!
//! GTK must stay on one thread, so this test has its own `main` (`harness = false`). The parent
//! process starts Xvfb and re-executes itself as the child that owns GTK.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::path::Path;
use std::process::{Command, ExitCode};
use std::time::{Duration, Instant};

use gtk4 as gtk;
use gtk4::gio;
use gtk4::prelude::*;
use lucerna_core::session::SessionEnv;
use lucerna_core::testing::FakeBackend;
use lucerna_daemon::BackendChoice;
use lucerna_ipc::dto::SettingsPatch;
use lucerna_testkit::TestEnv;
use lucerna_testkit::fixture::{daemon_options, output};
use lucerna_testkit::testbus::TestBus;
use lucerna_testkit::xvfb::Xvfb;
use lucerna_ui::presenter::banner::Link;
use lucerna_ui::{Controller, MainWindow, PAGE_NAMES};

const CHILD_MARKER: &str = "LUCERNA_UI_TEST_CHILD";

fn main() -> ExitCode {
    if std::env::var_os(CHILD_MARKER).is_some() {
        return child();
    }
    let Some(xvfb) = Xvfb::start(1280, 800) else {
        eprintln!("ui_structure: SKIPPED (no Xvfb)");
        return ExitCode::SUCCESS;
    };
    if TestBus::start().is_none() {
        eprintln!("ui_structure: SKIPPED (no dbus-daemon)");
        return ExitCode::SUCCESS;
    }
    let status = Command::new(std::env::current_exe().expect("current exe"))
        .env(CHILD_MARKER, "1")
        .env("DISPLAY", &xvfb.display)
        .env_remove("WAYLAND_DISPLAY")
        .env("GDK_BACKEND", "x11")
        .env("GSK_RENDERER", "cairo")
        .env("GTK_A11Y", "none")
        .env("NO_AT_SPI", "1")
        .status()
        .expect("run the GTK child");
    if status.success() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// Run the GLib main loop until `condition` holds.
fn until(what: &str, mut condition: impl FnMut() -> bool) {
    let context = gtk::glib::MainContext::default();
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        while context.iteration(false) {}
        if condition() {
            return;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn child() -> ExitCode {
    gtk::init().expect("GTK initialises under Xvfb");

    let bus = TestBus::start().expect("dbus-daemon");
    let env = TestEnv::new("ui");
    let paths = lucerna_core::paths::Paths::under(&env.root);
    std::fs::create_dir_all(paths.runtime_dir.as_ref().unwrap().parent().unwrap()).unwrap();
    std::fs::set_permissions(
        paths.runtime_dir.as_ref().unwrap().parent().unwrap(),
        std::os::unix::fs::PermissionsExt::from_mode(0o700),
    )
    .unwrap();

    // The application object, as `lucerna` builds it (non-unique here so no session bus is needed).
    let app = gtk::Application::builder()
        .application_id("org.lucerna.Lucerna.Test")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.register(gio::Cancellable::NONE)
        .expect("register the application");

    let controller = Controller::new(Some(bus.address.clone()), false);
    let main = MainWindow::new(&app, &controller);

    // ---- 1. Structure: the four pages and the controls the directive asks for.
    let names: Vec<String> = (0..PAGE_NAMES.len())
        .map(|_| String::new())
        .enumerate()
        .map(|(i, _)| PAGE_NAMES[i].to_owned())
        .collect();
    for name in &names {
        assert!(
            main.stack.child_by_name(name).is_some(),
            "missing page {name}"
        );
    }
    assert_eq!(main.stack.pages().n_items(), 4, "exactly the four pages");
    assert_eq!(main.window.title().as_deref(), Some("Lucerna"));
    assert!(
        main.wallpapers.list.first_child().is_none(),
        "the library starts empty"
    );
    assert_eq!(
        main.displays.all_scaling.model().unwrap().n_items(),
        4,
        "fill, fit, stretch, center"
    );
    assert_eq!(
        main.settings.hardware_decode.model().unwrap().n_items(),
        2,
        "auto, disabled"
    );
    assert_eq!(
        main.settings.fps_limit.model().unwrap().n_items(),
        4,
        "native, 60, 30, 15"
    );
    assert_eq!(
        main.about.version.text().as_str(),
        env!("CARGO_PKG_VERSION")
    );

    // ---- 2. Without a daemon: the banner says so and offers to start it; controls are disabled.
    assert_eq!(controller.snapshot().link, Link::NotRunning);
    assert!(main.banner_visible());
    assert!(
        main.banner_text().contains("service is not running"),
        "{}",
        main.banner_text()
    );
    assert!(
        !main.wallpapers.add.is_sensitive(),
        "nothing can be added while the service is down"
    );
    assert!(!main.settings.root.is_sensitive());
    assert!(main.about.backend.text().contains("unknown"));

    // ---- 3. Start a daemon (fake backend, no mpv) on the private bus in a background thread.
    let fake = FakeBackend::new();
    fake.set_outputs(vec![
        output("HDMI-1", 0, false),
        output("eDP-1", 1920, true),
    ]);
    let options = daemon_options(
        &paths,
        &bus.address,
        BackendChoice::Injected(Box::new(fake.clone())),
        Some("/nonexistent/mpv".into()),
        SessionEnv {
            xdg_session_type: Some("x11".into()),
            display: Some(":99".into()),
            xdg_current_desktop: Some("X-Cinnamon".into()),
            ..SessionEnv::default()
        },
        Duration::from_millis(300),
    );
    let daemon = std::thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime")
            .block_on(lucerna_daemon::run(options))
    });

    controller.start();
    until("the GUI to connect", || {
        controller.snapshot().link == Link::Connected
    });
    until("the settings to load", || {
        controller.snapshot().settings.is_some()
    });
    assert!(main.wallpapers.add.is_sensitive());
    assert!(main.settings.root.is_sensitive());
    assert_eq!(main.about.backend.text().as_str(), "cinnamon-x11");

    // mpv is not installed in this scenario, so the banner explains it (and offers Reload).
    assert!(main.banner_visible());
    assert!(
        main.banner_text().contains("could not find mpv"),
        "{}",
        main.banner_text()
    );

    // ---- 4. Settings reflect the daemon; switching one goes to the daemon and back.
    assert!(main.settings.pause_fullscreen.is_active());
    assert!(
        main.settings.autostart.is_active(),
        "first run enables autostart"
    );
    assert!(!main.settings.audio.is_active(), "audio is off by default");
    assert_eq!(main.settings.hardware_decode.selected(), 0);
    main.settings.pause_fullscreen.set_active(false); // as if the user flipped it
    until("the setting to round-trip", || {
        controller
            .snapshot()
            .settings
            .is_some_and(|s| !s.pause_on_fullscreen)
    });
    assert!(!main.settings.pause_fullscreen.is_active());
    assert!(
        std::fs::read_to_string(paths.config_file())
            .unwrap()
            .contains("pause_on_fullscreen = false")
    );

    main.settings.fps_limit.set_selected(2); // "30"
    until("fps to round-trip", || {
        controller
            .snapshot()
            .settings
            .is_some_and(|s| s.fps_limit == "30")
    });

    let autostart_file = paths.autostart_file();
    assert!(autostart_file.exists());
    main.settings.autostart.set_active(false);
    until("autostart to switch off", || {
        controller.snapshot().settings.is_some_and(|s| !s.autostart)
    });
    assert!(
        !autostart_file.exists(),
        "the toggle really removes the autostart entry"
    );

    // A rejected update shows the daemon's message in the banner and leaves the setting alone.
    controller.apply_settings(SettingsPatch {
        fps_limit: Some("144".into()),
        ..Default::default()
    });
    until("the error banner", || controller.snapshot().error.is_some());
    assert!(
        main.banner_text().contains("native, 60, 30, 15"),
        "{}",
        main.banner_text()
    );
    controller.dismiss_error();
    assert!(!main.banner_text().contains("native, 60, 30, 15"));

    // ---- 5. The library: add, list, select, assign, remove. The media file is never deleted.
    let media = env.media("Rain Loop.mp4", "play");
    controller.add_wallpaper(media.clone());
    until("the wallpaper to appear", || {
        main.wallpapers.list.row_at_index(0).is_some()
    });
    assert!(main.wallpapers.list.row_at_index(1).is_none());
    assert!(
        !main.wallpapers.remove.is_sensitive(),
        "nothing selected yet"
    );
    main.wallpapers
        .list
        .select_row(main.wallpapers.list.row_at_index(0).as_ref());
    assert!(main.wallpapers.remove.is_sensitive() && main.wallpapers.play.is_sensitive());

    let id = controller.snapshot().wallpapers[0].id.clone();
    assert_eq!(controller.snapshot().wallpapers[0].name, "Rain Loop");
    controller.play_everywhere(id.clone());
    until("the assignment to show on the displays page", || {
        main.displays.all_wallpaper.selected() == 1
    });
    controller.set_scaling_everywhere("fit".into());
    until("scaling to update", || {
        main.displays.all_scaling.selected() == 1
    });
    until("both displays to be listed", || {
        controller.snapshot().displays.len() == 2
    });

    // Reflect the state honestly: with mpv missing the renderer is failed and no surface exists.
    until("the failed renderer to be reported", || {
        controller.snapshot().status.is_some_and(|s| {
            s.renderers.iter().all(|r| r.failure_code == "mpv-missing") && !s.renderers.is_empty()
        })
    });
    assert!(fake.live_surfaces().is_empty());

    controller.remove_wallpaper(id);
    until("the wallpaper to be removed", || {
        main.wallpapers.list.row_at_index(0).is_none()
    });
    assert!(
        Path::new(&media).exists(),
        "removing a library entry must never delete the file"
    );
    assert_eq!(main.displays.all_wallpaper.selected(), 0, "back to None");

    // ---- 6. Closing the window does not stop the daemon (it owns the renderers, §5).
    main.window.close();
    until("nothing", || true);
    assert_eq!(
        controller.snapshot().link,
        Link::Connected,
        "the daemon keeps running"
    );

    // ---- 7. When the daemon goes away the GUI notices and offers to start it again.
    controller.quit_service();
    until("the GUI to notice the daemon is gone", || {
        controller.snapshot().link == Link::NotRunning
    });
    assert!(
        main.banner_text().contains("service is not running"),
        "{}",
        main.banner_text()
    );
    assert!(!main.wallpapers.add.is_sensitive());
    let outcome = daemon.join().expect("daemon thread");
    assert_eq!(outcome, lucerna_daemon::Outcome::Clean);

    println!(
        "ui_structure: ok - structure and wiring only; visual quality is NOT VALIDATED ON DEVELOPMENT SERVER"
    );
    ExitCode::SUCCESS
}
