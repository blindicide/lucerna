//! X11 **protocol** tests under Xvfb (directive §35).
//!
//! These check window properties, event routing, RandR data and process lifecycle. They are NOT
//! visual tests: Xvfb has no window manager, no compositor and no Cinnamon, and nothing here says
//! anything about how a wallpaper looks or stacks on a real desktop. Every desktop-appearance
//! behaviour stays `IMPLEMENTED — MANUAL DESKTOP VALIDATION REQUIRED`.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use lucerna_core::backend::{BackendEvent, EmbedTarget, OutputInfo, Rect, WallpaperBackend};
use lucerna_core::renderer::{RendererStateKind, RestartPolicy, Timings};
use lucerna_core::types::{FpsLimit, HwDecode, ScalingMode};
use lucerna_mpv::{LaunchSettings, PidRegistry, RendererSupervisor, SupervisorConfig};
use lucerna_testkit::xvfb::Xvfb;
use lucerna_testkit::{TestEnv, wait_for_state};
use lucerna_x11::{StackingMode, X11Backend, X11Options, probe_display};
use x11rb::COPY_FROM_PARENT;
use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::randr::{self, ConnectionExt as _, MonitorInfo};
use x11rb::protocol::shape::{ConnectionExt as _, SK};
use x11rb::protocol::xproto::{
    AtomEnum, ConfigureWindowAux, ConnectionExt as _, CreateWindowAux, EventMask, MapState,
    PropMode, StackMode, Window, WindowClass,
};
use x11rb::protocol::xtest::ConnectionExt as _;
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

const EVENT_WAIT: Duration = Duration::from_secs(5);

// ---------------------------------------------------------------- harness

fn options(xvfb: &Xvfb, stacking: StackingMode) -> X11Options {
    X11Options {
        display: Some(xvfb.display.clone()),
        current_desktop: Some("X-Cinnamon".to_owned()),
        stacking,
    }
}

fn backend(xvfb: &Xvfb) -> X11Backend {
    X11Backend::connect(&options(xvfb, StackingMode::Auto)).expect("connect backend")
}

/// A second connection playing the roles of window manager and user in the tests.
struct Client {
    x: RustConnection,
    root: Window,
}

impl Client {
    fn new(xvfb: &Xvfb) -> Self {
        let (x, screen) = RustConnection::connect(Some(&xvfb.display)).expect("client connect");
        let root = x.setup().roots[screen].root;
        Self { x, root }
    }

    fn atom(&self, name: &str) -> u32 {
        self.x
            .intern_atom(false, name.as_bytes())
            .unwrap()
            .reply()
            .unwrap()
            .atom
    }

    fn u32s(&self, window: Window, property: &str) -> Vec<u32> {
        let atom = self.atom(property);
        self.x
            .get_property(false, window, atom, AtomEnum::ANY, 0, 64)
            .unwrap()
            .reply()
            .unwrap()
            .value32()
            .map(Iterator::collect)
            .unwrap_or_default()
    }

    fn text(&self, window: Window, property: &str) -> String {
        let atom = self.atom(property);
        let reply = self
            .x
            .get_property(false, window, atom, AtomEnum::ANY, 0, 256)
            .unwrap()
            .reply()
            .unwrap();
        String::from_utf8_lossy(&reply.value).into_owned()
    }

    fn set_atoms(&self, window: Window, property: &str, values: &[&str]) {
        let atoms: Vec<u32> = values.iter().map(|v| self.atom(v)).collect();
        self.x
            .change_property32(
                PropMode::REPLACE,
                window,
                self.atom(property),
                AtomEnum::ATOM,
                &atoms,
            )
            .unwrap();
        self.x.flush().unwrap();
    }

    fn set_cardinal(&self, window: Window, property: &str, values: &[u32]) {
        self.x
            .change_property32(
                PropMode::REPLACE,
                window,
                self.atom(property),
                AtomEnum::CARDINAL,
                values,
            )
            .unwrap();
        self.x.flush().unwrap();
    }

    fn set_windows(&self, window: Window, property: &str, values: &[Window]) {
        self.x
            .change_property32(
                PropMode::REPLACE,
                window,
                self.atom(property),
                AtomEnum::WINDOW,
                values,
            )
            .unwrap();
        self.x.flush().unwrap();
    }

    /// A plain mapped window.
    fn window(&self, x: i16, y: i16, w: u16, h: u16, events: EventMask) -> Window {
        let id = self.x.generate_id().unwrap();
        self.x
            .create_window(
                COPY_FROM_PARENT as u8,
                id,
                self.root,
                x,
                y,
                w,
                h,
                0,
                WindowClass::INPUT_OUTPUT,
                COPY_FROM_PARENT,
                &CreateWindowAux::new().event_mask(events),
            )
            .unwrap();
        self.x.map_window(id).unwrap();
        self.x.flush().unwrap();
        id
    }

    fn children(&self) -> Vec<Window> {
        self.x
            .query_tree(self.root)
            .unwrap()
            .reply()
            .unwrap()
            .children
    }

    fn sync(&self) {
        self.x.get_input_focus().unwrap().reply().unwrap();
    }

    fn window_exists(&self, window: Window) -> bool {
        self.x
            .get_window_attributes(window)
            .unwrap()
            .reply()
            .is_ok()
    }
}

fn sink() -> (lucerna_core::backend::EventSink, Receiver<BackendEvent>) {
    let (tx, rx) = mpsc::channel();
    let tx = std::sync::Mutex::new(tx);
    let sink: lucerna_core::backend::EventSink = Box::new(move |event| {
        let _ = tx.lock().unwrap().send(event);
    });
    (sink, rx)
}

fn expect_fullscreen(rx: &Receiver<BackendEvent>, want: &[Rect]) {
    let deadline = Instant::now() + EVENT_WAIT;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(remaining) {
            Ok(BackendEvent::FullscreenChanged(got)) => {
                assert_eq!(got, want);
                return;
            }
            Ok(_) => {}
            Err(_) => panic!("timed out waiting for FullscreenChanged({want:?})"),
        }
    }
}

fn first_output(backend: &mut X11Backend) -> OutputInfo {
    backend.enumerate_outputs().unwrap().remove(0)
}

macro_rules! xvfb {
    ($w:expr, $h:expr) => {
        match Xvfb::start($w, $h) {
            Some(server) => server,
            None => return,
        }
    };
}

// ---------------------------------------------------------------- 1-3: connection and outputs

#[test]
fn connect_and_probe() {
    let xvfb = xvfb!(1920, 1080);
    let mut backend = backend(&xvfb);
    let probe = backend.probe().unwrap();

    assert_eq!(
        probe.kind.as_str(),
        "cinnamon-x11",
        "XDG_CURRENT_DESKTOP says Cinnamon"
    );
    assert!(probe.capabilities.input_passthrough, "SHAPE present");
    assert!(probe.capabilities.hotplug_events, "RandR >= 1.2");
    assert!(
        !probe.capabilities.fullscreen_detection,
        "Xvfb has no window manager"
    );
    assert!(
        !probe.capabilities.stable_monitor_identity,
        "Xvfb exposes no EDID"
    );
    let randr = probe.facts["randr"]["version"].as_str().unwrap();
    assert!(randr.starts_with("1."), "{randr}");
    assert_eq!(probe.facts["shape"], true);
    assert_eq!(probe.facts["compositor"]["running"], false);
    assert!(probe.facts["window_manager"].is_null());
    assert!(probe.facts["server"]["vendor"].as_str().unwrap().len() > 2);
    assert!(probe.facts["nemo"]["desktop_window"].is_null());

    // Without a Cinnamon hint and without Muffin the generic mode is chosen.
    let generic = X11Backend::connect(&X11Options {
        display: Some(xvfb.display.clone()),
        current_desktop: None,
        stacking: StackingMode::Auto,
    })
    .unwrap();
    assert_eq!(generic.kind().as_str(), "x11-ewmh");
}

#[test]
fn connecting_to_a_missing_display_explains_itself() {
    let err = X11Backend::connect(&X11Options {
        display: Some(":199".to_owned()),
        ..X11Options::default()
    })
    .err()
    .expect("no server there");
    let text = err.to_string();
    assert!(
        text.contains("could not connect to the X11 display"),
        "{text}"
    );
    assert!(text.contains(":199"), "{text}");
}

#[test]
fn enumerate_single_screen() {
    let xvfb = xvfb!(1920, 1080);
    let mut backend = backend(&xvfb);
    let outputs = backend.enumerate_outputs().unwrap();
    assert_eq!(outputs.len(), 1);
    let o = &outputs[0];
    assert_eq!(o.geometry, Rect::new(0, 0, 1920, 1080));
    assert!(
        o.id.as_str().starts_with("conn:"),
        "no EDID under Xvfb: {}",
        o.id
    );
    assert!(o.edid.is_none());
    assert_eq!(
        o.label(),
        format!(
            "{} — 1920×1080{}",
            o.connector,
            if o.primary { " — Primary" } else { "" }
        )
    );
}

#[test]
fn enumerate_virtual_monitors() {
    let xvfb = xvfb!(3200, 1080);
    let client = Client::new(&xvfb);
    let resources = client
        .x
        .randr_get_screen_resources_current(client.root)
        .unwrap()
        .reply()
        .unwrap();
    let output = resources.outputs[0];
    let name = |s: &str| client.atom(s);
    // Monitor A claims the real output (which removes the automatic whole-screen monitor); B is
    // a pure virtual monitor without an output.
    client
        .x
        .randr_set_monitor(
            client.root,
            MonitorInfo {
                name: name("A"),
                primary: true,
                automatic: false,
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
                width_in_millimeters: 500,
                height_in_millimeters: 300,
                outputs: vec![output],
            },
        )
        .unwrap()
        .check()
        .unwrap();
    client
        .x
        .randr_set_monitor(
            client.root,
            MonitorInfo {
                name: name("B"),
                primary: false,
                automatic: false,
                x: 1920,
                y: 0,
                width: 1280,
                height: 1024,
                width_in_millimeters: 300,
                height_in_millimeters: 200,
                outputs: vec![],
            },
        )
        .unwrap()
        .check()
        .unwrap();

    let mut backend = backend(&xvfb);
    let outputs = backend.enumerate_outputs().unwrap();
    assert_eq!(outputs.len(), 2, "{outputs:?}");
    let by_geometry = |g: Rect| {
        outputs
            .iter()
            .find(|o| o.geometry == g)
            .unwrap_or_else(|| panic!("no output at {g:?}: {outputs:?}"))
    };
    let a = by_geometry(Rect::new(0, 0, 1920, 1080));
    let b = by_geometry(Rect::new(1920, 0, 1280, 1024));
    assert!(a.primary && !b.primary);
    assert_eq!(
        b.connector, "B",
        "a virtual monitor is named by its own atom"
    );
    assert_eq!(b.id.as_str(), "conn:B");
    assert_ne!(a.id, b.id);
    // Stable across calls and independent of enumeration order.
    let again = backend.enumerate_outputs().unwrap();
    assert_eq!(
        outputs.iter().map(|o| &o.id).collect::<Vec<_>>(),
        again.iter().map(|o| &o.id).collect::<Vec<_>>()
    );
}

// ---------------------------------------------------------------- 4-7: the surface

#[test]
fn surface_properties() {
    let xvfb = xvfb!(1920, 1080);
    let client = Client::new(&xvfb);
    let mut backend = backend(&xvfb);
    let output = first_output(&mut backend);
    let surface = backend.create_surface(&output).unwrap();
    let EmbedTarget::X11Window(window) = surface.embed else {
        panic!("not an X11 target")
    };
    client.sync();

    let a = |n: &str| client.atom(n);
    assert_eq!(
        client.u32s(window, "_NET_WM_WINDOW_TYPE"),
        [a("_NET_WM_WINDOW_TYPE_DESKTOP")]
    );
    let state = client.u32s(window, "_NET_WM_STATE");
    for wanted in [
        "_NET_WM_STATE_BELOW",
        "_NET_WM_STATE_SKIP_TASKBAR",
        "_NET_WM_STATE_SKIP_PAGER",
        "_NET_WM_STATE_STICKY",
    ] {
        assert!(state.contains(&a(wanted)), "missing {wanted}: {state:?}");
    }
    assert_eq!(client.u32s(window, "_NET_WM_DESKTOP"), [0xFFFF_FFFF]);
    assert_eq!(client.u32s(window, "_NET_WM_BYPASS_COMPOSITOR"), [2]);

    let motif = client.u32s(window, "_MOTIF_WM_HINTS");
    assert_eq!(motif.len(), 5);
    assert_eq!(motif[0] & 2, 2, "decorations flag is set");
    assert_eq!(motif[2], 0, "decorations = none");

    // WM_HINTS: flags word, then `input`. Bit 0 (InputHint) must be set and input False.
    let hints = client.u32s(window, "WM_HINTS");
    assert_eq!(hints[0] & 1, 1, "InputHint flag");
    assert_eq!(hints[1], 0, "input = False: never takes keyboard focus");

    assert_eq!(
        client.text(window, "WM_CLASS"),
        "lucerna-wallpaper\0Lucerna\0"
    );
    assert_eq!(
        client.text(window, "_NET_WM_NAME"),
        format!("Lucerna wallpaper ({})", output.connector)
    );
    assert_eq!(client.u32s(window, "_NET_WM_PID"), [std::process::id()]);
    assert_eq!(
        client.text(window, "_LUCERNA_WALLPAPER"),
        output.id.as_str()
    );

    let attrs = client
        .x
        .get_window_attributes(window)
        .unwrap()
        .reply()
        .unwrap();
    assert!(
        attrs.override_redirect,
        "override-redirect is the default stacking strategy"
    );
    assert_eq!(attrs.map_state, MapState::VIEWABLE);

    // The input shape is an empty region: nothing in this window can receive pointer input.
    let input_rects = client
        .x
        .shape_get_rectangles(window, SK::INPUT)
        .unwrap()
        .reply()
        .unwrap();
    assert!(
        input_rects.rectangles.is_empty(),
        "{:?}",
        input_rects.rectangles
    );

    let g = client.x.get_geometry(window).unwrap().reply().unwrap();
    assert_eq!((g.x, g.y, g.width, g.height), (0, 0, 1920, 1080));
}

#[test]
fn desktop_window_mode_leaves_the_surface_managed() {
    let xvfb = xvfb!(1920, 1080);
    let client = Client::new(&xvfb);
    let mut backend = X11Backend::connect(&options(&xvfb, StackingMode::DesktopWindow)).unwrap();
    let output = first_output(&mut backend);
    let surface = backend.create_surface(&output).unwrap();
    let EmbedTarget::X11Window(window) = surface.embed else {
        panic!()
    };
    client.sync();
    let attrs = client
        .x
        .get_window_attributes(window)
        .unwrap()
        .reply()
        .unwrap();
    assert!(
        !attrs.override_redirect,
        "managed by the window manager in this mode"
    );
    assert_eq!(
        client.u32s(window, "_NET_WM_WINDOW_TYPE"),
        [client.atom("_NET_WM_WINDOW_TYPE_DESKTOP")]
    );
    // Refresh is a no-op in managed mode: the window manager owns the stacking.
    backend.refresh().unwrap();
}

/// Route a fake pointer click through XTEST at `(x, y)` in root coordinates.
fn click(client: &Client, x: i16, y: i16) {
    let fake = |kind: u8, detail: u8| {
        client
            .x
            .xtest_fake_input(kind, detail, 0, client.root, x, y, 0)
            .unwrap();
    };
    fake(6, 0); // MotionNotify to the target position
    fake(4, 1); // ButtonPress, button 1
    fake(5, 1); // ButtonRelease
    client.x.flush().unwrap();
    client.sync();
}

fn saw_button_press(client: &Client, window: Window) -> bool {
    let deadline = Instant::now() + Duration::from_millis(600);
    while Instant::now() < deadline {
        if let Some(Event::ButtonPress(e)) = client.x.poll_for_event().unwrap()
            && e.event == window
        {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

#[test]
fn input_passthrough_protocol() {
    // A statement about X event routing, not about Cinnamon: a window with an empty input shape
    // is invisible to the pointer, so a click falls through to the window underneath it.
    let xvfb = xvfb!(1000, 800);
    let client = Client::new(&xvfb);
    let lower = client.window(100, 100, 400, 400, EventMask::BUTTON_PRESS);

    let mut backend = backend(&xvfb);
    let output = first_output(&mut backend);
    let surface = backend.create_surface(&output).unwrap();
    let EmbedTarget::X11Window(ours) = surface.embed else {
        panic!()
    };
    // Our surface is created at the bottom; put the click target beneath it.
    client
        .x
        .configure_window(
            lower,
            &ConfigureWindowAux::new().stack_mode(StackMode::BELOW),
        )
        .unwrap();
    client.sync();
    let stack = client.children();
    assert!(
        stack.iter().position(|w| *w == lower) < stack.iter().position(|w| *w == ours),
        "test setup: the target must be below our surface"
    );

    click(&client, 200, 200);
    assert!(
        saw_button_press(&client, lower),
        "the click must reach the window under our surface"
    );

    // Negative control: an ordinary, un-shaped window in the same place *does* swallow the click,
    // which is what makes the assertion above meaningful.
    backend.destroy_surface(surface.id).unwrap();
    let blocker = client.window(0, 0, 1000, 800, EventMask::NO_EVENT);
    client
        .x
        .configure_window(
            blocker,
            &ConfigureWindowAux::new()
                .sibling(lower)
                .stack_mode(StackMode::ABOVE),
        )
        .unwrap();
    client.sync();
    click(&client, 200, 200);
    assert!(
        !saw_button_press(&client, lower),
        "an ordinary window in front must block the click"
    );
}

#[test]
fn resize() {
    let xvfb = xvfb!(3200, 1080);
    let client = Client::new(&xvfb);
    let mut backend = backend(&xvfb);
    let output = first_output(&mut backend);
    let surface = backend.create_surface(&output).unwrap();
    let EmbedTarget::X11Window(window) = surface.embed else {
        panic!()
    };
    backend
        .resize_surface(surface.id, Rect::new(1920, 10, 1280, 1024))
        .unwrap();
    client.sync();
    let g = client.x.get_geometry(window).unwrap().reply().unwrap();
    assert_eq!((g.x, g.y, g.width, g.height), (1920, 10, 1280, 1024));
    assert!(matches!(
        backend.resize_surface(lucerna_core::backend::SurfaceId(999), Rect::new(0, 0, 1, 1)),
        Err(lucerna_core::backend::BackendError::UnknownSurface(_))
    ));
}

#[test]
fn lower_and_restack() {
    let xvfb = xvfb!(1000, 800);
    let client = Client::new(&xvfb);
    let mut backend = backend(&xvfb);
    let output = first_output(&mut backend);
    let surface = backend.create_surface(&output).unwrap();
    let EmbedTarget::X11Window(ours) = surface.embed else {
        panic!()
    };

    // An unrelated window appears...
    let other = client.window(10, 10, 100, 100, EventMask::NO_EVENT);
    client.sync();
    assert_eq!(
        client.children(),
        [ours, other],
        "created at the bottom, newcomers go on top"
    );

    // ...and somebody raises our surface above it.
    client
        .x
        .configure_window(
            ours,
            &ConfigureWindowAux::new().stack_mode(StackMode::ABOVE),
        )
        .unwrap();
    client.sync();
    assert_eq!(client.children(), [other, ours]);

    backend.refresh().unwrap();
    client.sync();
    assert_eq!(
        client.children()[0],
        ours,
        "refresh() puts the surface back at the bottom"
    );

    // Refresh with nothing to fix must not disturb the stack.
    let before = client.children();
    backend.refresh().unwrap();
    client.sync();
    assert_eq!(client.children(), before);
}

#[test]
fn stacking_disturbance_is_reported_when_something_is_sent_below_us() {
    let xvfb = xvfb!(1000, 800);
    let client = Client::new(&xvfb);
    let mut backend = backend(&xvfb);
    let output = first_output(&mut backend);
    backend.create_surface(&output).unwrap();
    let (sink, rx) = sink();
    backend.subscribe(sink).unwrap();

    let intruder = client.window(10, 10, 100, 100, EventMask::NO_EVENT);
    client
        .x
        .configure_window(
            intruder,
            &ConfigureWindowAux::new().stack_mode(StackMode::BELOW),
        )
        .unwrap();
    client.sync();

    let deadline = Instant::now() + EVENT_WAIT;
    loop {
        match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(BackendEvent::StackingDisturbed) => break,
            Ok(_) => {}
            Err(_) => panic!("no StackingDisturbed event"),
        }
    }
}

// ---------------------------------------------------------------- 8: Nemo simulation

#[test]
fn nemo_simulation_detected() {
    let xvfb = xvfb!(1920, 1080);
    let client = Client::new(&xvfb);
    let mut backend = backend(&xvfb);
    let output = first_output(&mut backend);
    backend.create_surface(&output).unwrap();

    // Play Nemo's desktop window: class nemo-desktop, type DESKTOP, listed as a client. (Xvfb has
    // only 24-bit visuals, so the depth-32 ARGB window a real Nemo may use cannot be simulated;
    // `diagnostics()` reports the depth so a real session can be checked.)
    let nemo = client.window(0, 0, 1920, 1080, EventMask::NO_EVENT);
    client
        .x
        .change_property8(
            PropMode::REPLACE,
            nemo,
            AtomEnum::WM_CLASS,
            AtomEnum::STRING,
            b"nemo-desktop\0Nemo-desktop\0",
        )
        .unwrap();
    client.set_atoms(
        nemo,
        "_NET_WM_WINDOW_TYPE",
        &["_NET_WM_WINDOW_TYPE_DESKTOP"],
    );
    client.set_windows(client.root, "_NET_CLIENT_LIST", &[nemo]);
    client.sync();

    let diagnostics = backend.diagnostics();
    let found = &diagnostics["nemo_desktop_window"];
    assert_eq!(found["xid"], nemo, "{diagnostics}");
    assert_eq!(found["depth"], 24);
    assert_eq!(found["window_type"][0], "_NET_WM_WINDOW_TYPE_DESKTOP");
    // The X connection, server and RandR facts §21 asks for are part of the daemon's report too.
    let connection = &diagnostics["connection"];
    assert!(connection["server"]["vendor"].is_string(), "{diagnostics}");
    assert_eq!(connection["randr"]["present"], true);
    assert!(connection["randr"]["version"].is_string());
    assert!(connection["display"].is_string());
    let surface = &diagnostics["surfaces"][0];
    assert_eq!(
        surface["below_nemo"], true,
        "our surface sits below the simulated Nemo window: {diagnostics}"
    );
    assert!(surface["stack_index"].as_u64().unwrap() < found["stack_index"].as_u64().unwrap());
    assert_eq!(surface["override_redirect"], true);
    assert_eq!(surface["map_state"], "viewable");

    // The probe sees it too, and the read-only doctor probe agrees.
    let probe = backend.probe().unwrap();
    assert_eq!(probe.facts["nemo"]["desktop_window"]["xid"], nemo);
    let report = probe_display(Some(&xvfb.display));
    assert!(report.connected);
    assert_eq!(report.facts["nemo"]["desktop_window"]["xid"], nemo);
    assert_eq!(
        report.lucerna_windows.len(),
        1,
        "the doctor probe finds our surface"
    );
    assert_eq!(report.outputs.len(), 1);
}

// ---------------------------------------------------------------- 9: fullscreen

#[test]
fn fullscreen_detection() {
    let xvfb = xvfb!(1920, 1080);
    let wm = Client::new(&xvfb); // plays the window manager
    let mut backend = backend(&xvfb);

    // Advertise the EWMH features a real window manager would.
    wm.set_atoms(
        wm.root,
        "_NET_SUPPORTED",
        &[
            "_NET_CLIENT_LIST",
            "_NET_WM_STATE_FULLSCREEN",
            "_NET_WM_STATE",
        ],
    );
    wm.set_cardinal(wm.root, "_NET_CURRENT_DESKTOP", &[0]);
    wm.sync();
    assert!(backend.probe().unwrap().capabilities.fullscreen_detection);

    let (sink, rx) = sink();
    backend.subscribe(sink).unwrap();

    let full = Rect::new(0, 0, 1920, 1080);
    let app = wm.window(0, 0, 1920, 1080, EventMask::NO_EVENT);
    wm.set_cardinal(app, "_NET_WM_DESKTOP", &[0]);
    wm.set_atoms(app, "_NET_WM_STATE", &["_NET_WM_STATE_FULLSCREEN"]);
    wm.set_windows(wm.root, "_NET_CLIENT_LIST", &[app]);
    expect_fullscreen(&rx, &[full]);

    // Maximised is not fullscreen (§15).
    wm.set_atoms(
        app,
        "_NET_WM_STATE",
        &[
            "_NET_WM_STATE_MAXIMIZED_VERT",
            "_NET_WM_STATE_MAXIMIZED_HORZ",
        ],
    );
    expect_fullscreen(&rx, &[]);

    wm.set_atoms(app, "_NET_WM_STATE", &["_NET_WM_STATE_FULLSCREEN"]);
    expect_fullscreen(&rx, &[full]);

    // Minimised/hidden fullscreen windows do not occlude anything.
    wm.set_atoms(
        app,
        "_NET_WM_STATE",
        &["_NET_WM_STATE_FULLSCREEN", "_NET_WM_STATE_HIDDEN"],
    );
    expect_fullscreen(&rx, &[]);

    wm.set_atoms(app, "_NET_WM_STATE", &["_NET_WM_STATE_FULLSCREEN"]);
    expect_fullscreen(&rx, &[full]);

    // On another workspace: not visible now.
    wm.set_cardinal(app, "_NET_WM_DESKTOP", &[1]);
    expect_fullscreen(&rx, &[]);

    // On all workspaces: visible again.
    wm.set_cardinal(app, "_NET_WM_DESKTOP", &[0xFFFF_FFFF]);
    expect_fullscreen(&rx, &[full]);

    // Pinned to workspace 0 it is still visible (no change, so no event); switching the current
    // workspace away then hides it.
    wm.set_cardinal(app, "_NET_WM_DESKTOP", &[0]);
    wm.set_cardinal(wm.root, "_NET_CURRENT_DESKTOP", &[2]);
    expect_fullscreen(&rx, &[]);

    // The client going away is noticed too.
    wm.set_cardinal(wm.root, "_NET_CURRENT_DESKTOP", &[0]);
    expect_fullscreen(&rx, &[full]);
    wm.x.destroy_window(app).unwrap();
    wm.set_windows(wm.root, "_NET_CLIENT_LIST", &[]);
    expect_fullscreen(&rx, &[]);
}

#[test]
fn a_fullscreen_window_that_already_exists_is_reported_on_subscribe() {
    let xvfb = xvfb!(1920, 1080);
    let wm = Client::new(&xvfb);
    wm.set_cardinal(wm.root, "_NET_CURRENT_DESKTOP", &[0]);
    let app = wm.window(0, 0, 1280, 1024, EventMask::NO_EVENT);
    wm.set_atoms(app, "_NET_WM_STATE", &["_NET_WM_STATE_FULLSCREEN"]);
    wm.set_windows(wm.root, "_NET_CLIENT_LIST", &[app]);
    wm.sync();

    let mut backend = backend(&xvfb);
    let (sink, rx) = sink();
    backend.subscribe(sink).unwrap();
    expect_fullscreen(&rx, &[Rect::new(0, 0, 1280, 1024)]);
}

// ---------------------------------------------------------------- 10: hotplug

fn count_outputs_changed(rx: &Receiver<BackendEvent>, window: Duration) -> usize {
    let deadline = Instant::now() + window;
    let mut count = 0;
    while let Ok(event) = rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
        if matches!(event, BackendEvent::OutputsChanged) {
            count += 1;
        }
    }
    count
}

/// Register a `width`x`height` mode on the (only) output and return its id.
fn add_mode(client: &Client, output: u32, width: u16, height: u16, name: &str) -> u32 {
    let info = randr::ModeInfo {
        id: 0,
        width,
        height,
        dot_clock: 118_250_000,
        hsync_start: width + 96,
        hsync_end: width + 256,
        htotal: width + 512,
        hskew: 0,
        vsync_start: height + 3,
        vsync_end: height + 8,
        vtotal: height + 34,
        name_len: u16::try_from(name.len()).unwrap(),
        mode_flags: randr::ModeFlag::HSYNC_NEGATIVE | randr::ModeFlag::VSYNC_POSITIVE,
    };
    let mode = client
        .x
        .randr_create_mode(client.root, info, name.as_bytes())
        .unwrap()
        .reply()
        .unwrap()
        .mode;
    client
        .x
        .randr_add_output_mode(output, mode)
        .unwrap()
        .check()
        .unwrap();
    mode
}

fn set_mode(client: &Client, crtc: u32, output: u32, mode: u32) {
    let stamp = client
        .x
        .randr_get_screen_resources_current(client.root)
        .unwrap()
        .reply()
        .unwrap()
        .config_timestamp;
    let reply = client
        .x
        .randr_set_crtc_config(
            crtc,
            x11rb::CURRENT_TIME,
            stamp,
            0,
            0,
            mode,
            randr::Rotation::ROTATE0,
            &[output],
        )
        .unwrap()
        .reply()
        .unwrap();
    assert_eq!(reply.status, randr::SetConfig::SUCCESS);
}

#[test]
fn hotplug_event() {
    // A mode change on the CRTC is a real RandR event, the same kind a physical hotplug or a
    // resolution change produces. (`SetMonitor` on Xvfb notifies nobody, so virtual monitors are
    // only used for enumeration, above.)
    let xvfb = xvfb!(3200, 1080);
    let client = Client::new(&xvfb);
    let resources = client
        .x
        .randr_get_screen_resources_current(client.root)
        .unwrap()
        .reply()
        .unwrap();
    let (crtc, output) = (resources.crtcs[0], resources.outputs[0]);
    let mode_a = add_mode(&client, output, 1600, 900, "1600x900_t");
    let mode_b = add_mode(&client, output, 1280, 720, "1280x720_t");

    let mut backend = backend(&xvfb);
    let (sink, rx) = sink();
    backend.subscribe(sink).unwrap();
    assert_eq!(
        backend.enumerate_outputs().unwrap()[0].geometry,
        Rect::new(0, 0, 3200, 1080)
    );

    // A burst of changes produces exactly one debounced event.
    set_mode(&client, crtc, output, mode_a);
    set_mode(&client, crtc, output, mode_b);
    assert_eq!(
        count_outputs_changed(&rx, Duration::from_millis(1800)),
        1,
        "one debounced OutputsChanged for the burst"
    );
    assert_eq!(
        backend.enumerate_outputs().unwrap()[0].geometry,
        Rect::new(0, 0, 1280, 720)
    );

    set_mode(&client, crtc, output, mode_a);
    assert_eq!(count_outputs_changed(&rx, Duration::from_millis(1800)), 1);
    assert_eq!(
        backend.enumerate_outputs().unwrap()[0].geometry,
        Rect::new(0, 0, 1600, 900)
    );

    // Nothing happens, nothing is reported.
    assert_eq!(count_outputs_changed(&rx, Duration::from_millis(800)), 0);
}

// ---------------------------------------------------------------- 11-12: lifecycle

#[test]
fn destroy_and_shutdown() {
    let xvfb = xvfb!(1920, 1080);
    let client = Client::new(&xvfb);
    let mut backend = backend(&xvfb);
    let output = first_output(&mut backend);

    let first = backend.create_surface(&output).unwrap();
    let second = backend.create_surface(&output).unwrap();
    let (EmbedTarget::X11Window(w1), EmbedTarget::X11Window(w2)) = (first.embed, second.embed)
    else {
        panic!()
    };
    assert_ne!(first.id, second.id, "surface ids are never reused");

    backend.destroy_surface(first.id).unwrap();
    client.sync();
    assert!(
        !client.window_exists(w1),
        "destroyed window is gone (BadWindow)"
    );
    assert!(client.window_exists(w2));
    backend.destroy_surface(first.id).unwrap(); // idempotent

    backend.shutdown().unwrap();
    client.sync();
    assert!(!client.window_exists(w2), "shutdown destroys every surface");
    backend.shutdown().unwrap(); // idempotent
    assert!(
        client.children().is_empty(),
        "nothing of ours is left on the root"
    );
}

#[test]
fn dropping_the_backend_cleans_up_like_shutdown() {
    let xvfb = xvfb!(1920, 1080);
    let client = Client::new(&xvfb);
    let window = {
        let mut backend = backend(&xvfb);
        let output = first_output(&mut backend);
        let surface = backend.create_surface(&output).unwrap();
        let EmbedTarget::X11Window(w) = surface.embed else {
            panic!()
        };
        w
    };
    client.sync();
    assert!(!client.window_exists(window));
}

#[test]
fn connection_lost_and_reconnect() {
    let mut xvfb = xvfb!(1920, 1080);
    let mut backend = backend(&xvfb);
    let (sink, rx) = sink();
    backend.subscribe(sink).unwrap();

    xvfb.kill();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(BackendEvent::ConnectionLost(reason)) => {
                assert!(!reason.is_empty());
                break;
            }
            Ok(_) => {}
            Err(_) => panic!("ConnectionLost was never reported"),
        }
    }
    // Every operation now fails cleanly, with no panic and no hang.
    assert!(backend.enumerate_outputs().is_err());
    backend.shutdown().unwrap();
    drop(backend);

    // A new server, a new backend: everything works again.
    let xvfb2 = xvfb!(1024, 768);
    let mut fresh = X11Backend::connect(&options(&xvfb2, StackingMode::Auto)).unwrap();
    let output = first_output(&mut fresh);
    assert_eq!(output.geometry, Rect::new(0, 0, 1024, 768));
    fresh.create_surface(&output).unwrap();
}

#[test]
fn the_doctor_probe_reports_a_missing_server_without_panicking() {
    let report = probe_display(Some(":198"));
    assert!(!report.connected);
    assert!(
        report
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("could not connect")
    );
    assert!(report.outputs.is_empty());
}

// ---------------------------------------------------------------- 13: mpv embedding

#[tokio::test]
async fn mpv_embeds_into_surface() {
    // Protocol evidence that mpv attached itself to our surface window (a child window appears).
    // It says nothing about what is drawn: `--vo=x11` is used only to avoid needing GL here.
    let Some(mpv) = lucerna_core::mpv::discover_from_env().ok() else {
        assert!(
            std::env::var_os("LUCERNA_REQUIRE_MPV").is_none(),
            "LUCERNA_REQUIRE_MPV is set but mpv is unusable"
        );
        eprintln!("SKIPPED (no mpv)");
        return;
    };
    let Some(xvfb) = Xvfb::start(1920, 1080) else {
        return;
    };
    let client = Client::new(&xvfb);
    let mut backend = backend(&xvfb);
    let output = first_output(&mut backend);
    let surface = backend.create_surface(&output).unwrap();
    let EmbedTarget::X11Window(window) = surface.embed else {
        panic!()
    };

    let env = TestEnv::new("embed");
    let media: PathBuf =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/media/sample.webm");
    let config = SupervisorConfig {
        output: output.id.clone(),
        mpv_path: mpv.path,
        runtime_dir: env.runtime_dir.clone(),
        log_dir: Some(env.log_dir.clone()),
        log_max_bytes: 512 * 1024,
        embed: Some(surface.embed),
        restart_policy: RestartPolicy::default(),
        timings: Timings::default(),
        vo_override: Some("x11".to_owned()),
        extra_env: vec![("DISPLAY".to_owned(), xvfb.display.clone())],
        registry: Arc::new(PidRegistry::new(env.registry_path())),
    };
    let launch = LaunchSettings {
        media,
        scaling: ScalingMode::Fill,
        hwdec: HwDecode::Auto,
        fps: FpsLimit::Native,
        audio: false,
    };
    let supervisor = RendererSupervisor::spawn(config, launch, None);
    supervisor.start(false);
    wait_for_state(
        &supervisor,
        RendererStateKind::Playing,
        Duration::from_secs(20),
    )
    .await;

    let tree = client.x.query_tree(window).unwrap().reply().unwrap();
    assert!(
        !tree.children.is_empty(),
        "mpv creates a child window inside the wid it was given"
    );
    supervisor.shutdown().await;
}
