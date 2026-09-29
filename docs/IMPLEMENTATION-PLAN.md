# Lucerna — v1 Implementation Plan

| | |
| --- | --- |
| Status | Phase A deliverable. Drives Phase B. |
| Authority | `INSTRUCTION-LUCERNA.md` (the directive) wins over this plan wherever the two disagree. Sections are cited as §N. |
| Scope | `v0.0.1` → `v1.0.0-rc.1`. `v1.0.0` is **not** tagged by the coding agent (§30: it needs manual desktop acceptance first). |
| Environment | Ubuntu 24.04-based headless server; rustc 1.98.1; GTK 4.14.5; mpv 0.37.0; Xvfb; Docker. `DISPLAY` is never set. |

This plan is meant to be carried out as written. When it names a value (a timeout, a
path, a flag), use that value. When it leaves something open, take the simplest option
that fits the directive (§57) and write down what you chose in the relevant `docs/` file.

---

## 0. Decisions that deviate from, or refine, the directive

Each of these is allowed by the directive's "may vary / may be refined" clauses. They are
listed first so the operator can overrule any of them before Phase B starts.

| # | Decision | Directive hook | Rationale |
| --- | --- | --- | --- |
| D1 | Eight crates instead of five: adds `lucerna-mpv`, `lucerna-ipc`, `lucerna-testkit`. | §6 "names may vary" | Lets the crate graph itself enforce the §48 boundaries (see §1). |
| D2 | **The daemon is the only process that writes `config.toml`.** The GUI and CLI change configuration only through D-Bus. | §18, §19 | One writer means no lost updates between GUI, CLI and daemon. |
| D3 | The GUI starts `lucernad` itself if the daemon isn't running. No D-Bus activation file is installed. | §5, §22 | Activation would start the daemon on any `lucernactl status` call, possibly without `DISPLAY`, and would be a second startup mechanism (§22). |
| D4 | **Autostart state is the per-user XDG autostart file itself.** There is no `general.autostart` key in `config.toml`. | §18 example, §22 | One source of truth. If a user disables the entry from Cinnamon's *Startup Applications*, Lucerna reports it as disabled instead of fighting the user. |
| D5 | Don't use `--really-quiet` or `--no-terminal`. Use `--quiet --msg-level=all=warn --no-input-terminal` instead. | §9 "exact options must be tested" | `--no-terminal` suppresses all of mpv's stderr, which would leave nothing to put in the diagnostic logs §9 requires. |
| D6 | Extra mpv flags `--stop-screensaver=no`, `--osc=no`, `--ytdl=no`, `--resume-playback=no`, `--input-vo-keyboard=no`, `--input-cursor=no`, `--x11-bypass-compositor=never`, among others. | §9, §16, §26 | Without `--stop-screensaver=no` the wallpaper would stop the screen from locking or blanking. The rest block network access, stray input handling and inherited state. |
| D7 | Default stacking strategy is an **override-redirect window kept at the bottom of the stack**. A managed `_NET_WM_WINDOW_TYPE_DESKTOP` strategy is available through `[x11] stacking`. | §8, §14 | Neither strategy can be checked headlessly. A config switch lets the operator try the other one on the Mint machine without a rebuild. |
| D8 | Target is **Linux Mint 22.x (Ubuntu 24.04 base) and newer, with GTK ≥ 4.10**. Mint 21.x (GTK 4.6) is not supported. | §4, §23 | `gtk::FileDialog` needs 4.10, and `FileChooserDialog` is deprecated. The `.deb` is built on `ubuntu:24.04` so its dependencies match Mint 22. |
| D9 | Packages are built with the rustup toolchain pinned in `rust-toolchain.toml` inside native `ubuntu:24.04` / `fedora:<N>` containers, not with the distro's `rustc`. | §36 | Every build uses the same compiler, and Ubuntu 24.04's stock rustc is too old. The package is still compiled and linked natively in each distro container. |
| D10 | RPM `Release` has no `%{?dist}` suffix (`1`, or `0.rcN`). | §30, §41 | Matches the required artifact names `lucerna-1.0.0-1.x86_64.rpm` and `lucerna-1.0.0-0.rc1.x86_64.rpm` exactly. |
| D11 | The Debian source format is `3.0 (native)`, so there is no Debian revision. | §30, §41 | Matches `lucerna_1.0.0_amd64.deb` and `lucerna_1.0.0~rc1_amd64.deb` exactly. |
| D12 | Wallpaper paths must be valid UTF-8. Non-UTF-8 paths are rejected with a readable error. | §10, §26 | TOML and D-Bus strings are UTF-8. Documented as a known limitation. |
| D13 | Fullscreen detection uses EWMH only (`_NET_CLIENT_LIST` + `_NET_WM_STATE_FULLSCREEN`). Override-redirect fullscreen windows, which some old games use, are not detected. | §15 | Documented limitation. Per-monitor pausing is still implemented. |
| D14 | `lucernactl stop --daemon` (asks the daemon to quit) is the only CLI addition beyond §20. | §5, §20 | The GUI needs a daemon stop control anyway (§5), and the CLI should be able to do the same. |
| D15 | `LICENSE` is MIT with copyright holder "The Lucerna authors". | §43 | The repository has no license yet. **The operator should confirm the holder string.** |
| D16 | The agent tags up to `v1.0.0-rc.1` and then stops. | §30, Phase B brief | `v1.0.0` needs real desktop results. |

---

## 1. Workspace and crate boundaries

### 1.1 Repository layout

```text
lucerna/
├── Cargo.toml                 # virtual workspace; [workspace.package] version = canonical version (§29)
├── Cargo.lock                 # committed (§4)
├── rust-toolchain.toml        # channel = "1.98.1", components = ["rustfmt", "clippy"]
├── rustfmt.toml               # edition = "2024", max_width = 100
├── clippy.toml                # allow-unwrap-in-tests = true, allow-expect-in-tests = true
├── deny.toml                  # cargo-deny: advisories, licenses, bans, sources
├── README.md  CHANGELOG.md  LICENSE
├── crates/
│   ├── lucerna-core/          # pure logic + domain types + backend trait (no GTK/X11/D-Bus/tokio)
│   ├── lucerna-mpv/           # mpv process supervision + JSON IPC (tokio)
│   ├── lucerna-x11/           # WallpaperBackend impl for X11 / Cinnamon (x11rb)
│   ├── lucerna-ipc/           # D-Bus contract: names, DTOs, errors, client proxy (zbus)
│   ├── lucerna-daemon/        # lib + bin `lucernad`
│   ├── lucerna-cli/           # lib + bin `lucernactl`
│   ├── lucerna-ui/            # lib + bin `lucerna` (GTK 4)
│   └── lucerna-testkit/       # publish=false: fake-mpv, private D-Bus, Xvfb harness, integration + protocol suites
├── assets/
│   ├── icons/hicolor/scalable/apps/org.lucerna.Lucerna.svg
│   └── desktop/
│       ├── org.lucerna.Lucerna.desktop
│       ├── org.lucerna.Lucerna.metainfo.xml
│       └── org.lucerna.Lucerna.Daemon.autostart.desktop.in   # template embedded by lucernad
├── packaging/
│   ├── debian/                # control, rules, copyright, source/format, lucerna.docs (changelog is generated)
│   └── rpm/lucerna.spec       # Version/Release come from --define (no hardcoded version)
├── docs/                      # ARCHITECTURE, BUILDING, CONFIGURATION, IPC, PACKAGING, MANUAL-ACCEPTANCE,
│                              # TROUBLESHOOTING, X11-CINNAMON-NOTES, TEST-MATRIX, IMPLEMENTATION-PLAN
├── tests/
│   ├── fixtures/media/        # sample.mp4 sample.webm sample.mkv sample.gif corrupt.mp4 + README (provenance, CC0)
│   └── scripts/               # shell tests for scripts/ (version mapping, changelog extraction, tag check)
├── scripts/                   # version.sh, build-deb.sh, build-rpm.sh, make-source-archive.sh,
│                              # smoke-test-package.sh, changelog-section.sh, check-tag.sh, gen-fixtures.sh
└── .github/workflows/         # ci.yml, packages.yml, release.yml   (probe.yml is deleted in v0.0.1)
```

All crates set `version.workspace = true`, `edition.workspace = true`,
`license.workspace = true`, `publish.workspace = true` (`publish = false`),
`rust-version.workspace = true` and `lints.workspace = true`. Internal path dependencies
don't declare a version, because `publish = false` makes one unnecessary. That leaves the
root `Cargo.toml` as the only place a version number appears.

### 1.2 What belongs in each crate

| Crate | Owns | Must not contain |
| --- | --- | --- |
| **lucerna-core** | Domain types (`OutputInfo`, `OutputId`, `Rect`, `Rotation`, `WallpaperId`, `ScalingMode`, `HwDecode`, `FpsLimit`, `MediaType`). **The `WallpaperBackend` trait** and `BackendEvent`. EDID parsing and stable monitor identity. Config schema v1, lenient parsing, migration framework and atomic writes. The library model. XDG path resolution and runtime directory checks. **The renderer state machine and restart policy**, as pure functions. **The pause-policy evaluator**, pure. **The reconciliation planner**, pure. The mpv argument builder, pure. mpv discovery (a `PATH` search plus `mpv --version` / `--list-options`, sync `std::process`). The `BoundedLog` rotating writer. The session-environment classifier. The doctor report model and redaction. Logging initialisation. Autostart-file read/write. `testing` module behind the `test-support` feature (`FakeBackend`, `ManualClock`). | Any GTK, X11, D-Bus or async runtime. Any long-running process management. |
| **lucerna-mpv** | `MpvProcess` (spawn without a shell, stdio capture, signals). `MpvIpc` (a JSON-lines client over a Unix socket, with `request_id` correlation and an event stream). `RendererSupervisor`, which drives core's state machine by executing its `Effect`s. `PidRegistry` (`renderers.json`) and stale-process recovery. | GTK, X11, D-Bus. Policy decisions: it asks core what to do next. |
| **lucerna-x11** | `X11Backend: WallpaperBackend`, used for both `cinnamon-x11` and the generic `x11-ewmh` mode. Connection handling, RandR enumeration, EDID fetch, surface windows, EWMH/ICCCM/Motif hints, the empty input shape, restacking, the event thread (RandR, stacking, fullscreen), Cinnamon/Nemo/compositor probes, and read-only probe functions for `doctor`. | Daemon policy, mpv, D-Bus, GTK. Nothing here decides *whether* to pause. It only reports facts. |
| **lucerna-ipc** | Bus name, object path and interface name constants. DTOs (`StatusDto`, `DisplayDto`, `AssignmentDto`, `WallpaperDto`, `SettingsDto`) with `a{sv}` conversions. The `LucernaError` D-Bus error enum. The `#[zbus::proxy]` client trait, async plus the generated blocking proxy. | Server logic, GTK, X11, mpv. |
| **lucerna-daemon** | `lucernad`: the startup sequence, single-instance enforcement, backend selection, the `Engine` actor, the D-Bus server implementation (`#[zbus::interface]`), the lock monitor (screensaver D-Bus), the config store, signal handling and clean shutdown. | GTK. **No direct `x11rb` import.** The daemon talks to X11 only through `dyn WallpaperBackend`. |
| **lucerna-cli** | `lucernactl`: clap parsing, blocking D-Bus client, text and JSON output, exit codes, and `doctor`, which combines a local probe with the daemon's report. | GTK, tokio, mpv supervision. |
| **lucerna-ui** | `lucerna`: clap parsing *before* GTK init, the display check, `gtk::Application`, four pages, the async D-Bus client on the glib main context, and daemon spawning. `presenter/` holds gtk-free view models and string formatting. `strings.rs` holds every user-visible string. | X11, mpv, `lucerna-x11`, `lucerna-mpv`. |
| **lucerna-testkit** | `bin fake-mpv`; `bin lucernad-under-test` and `bin lucernactl-under-test`, one-line wrappers around `lucerna_daemon::cli_main()` and `lucerna_cli::cli_main()`; lib `TestBus` (private `dbus-daemon`), `Xvfb` harness and helpers; `tests/` holding the integration, X11 protocol, real-mpv and architecture suites. | Anything that ships. It is never packaged. |

The shipped binaries' `main.rs` files are at most 30 lines each:

```rust
fn main() -> std::process::ExitCode { lucerna_daemon::cli_main() }
```

All real code lives in the library, so the testkit wrappers exercise exactly the same code.

### 1.3 Dependency direction (normal `[dependencies]` only)

```text
                 lucerna-core            (leaf: no internal deps)
               ↗      ↑      ↑      ↖
     lucerna-mpv  lucerna-x11  lucerna-ipc
          ↑    ↖       ↑    ↗      ↑     ↖
          └── lucerna-daemon ──────┘      lucerna-ui  (core, ipc)
                       lucerna-cli (core, ipc, x11[probe])

     lucerna-testkit → everything (tests only; never a dependency of anything)
```

Allowed internal edges. This is the complete list. Every other edge is forbidden.

| From | May depend on |
| --- | --- |
| lucerna-core | — |
| lucerna-mpv | core |
| lucerna-x11 | core |
| lucerna-ipc | core |
| lucerna-daemon | core, mpv, x11, ipc |
| lucerna-cli | core, ipc, x11 |
| lucerna-ui | core, ipc |
| lucerna-testkit | any |

Forbidden external crates, checked on direct dependencies:

| Crate | Forbidden |
| --- | --- |
| core | `gtk4`, `glib`, `gio`, `x11rb`, `zbus`, `tokio`, `rustix` |
| mpv | `gtk4`, `x11rb`, `zbus` |
| x11 | `gtk4`, `zbus`, `tokio` |
| ipc | `gtk4`, `x11rb`, `tokio` |
| daemon | `gtk4`, `glib`, `x11rb` |
| cli | `gtk4`, `tokio` |
| ui | `x11rb`, `tokio`, `rustix` |

In the workspace root, `zbus` is declared with default features only. **The `tokio`
feature of zbus must never be enabled.** Cargo unifies features across the workspace, and
turning it on would force the GTK client to host a tokio runtime (see §7.6).

Everything points toward core and the only internal edges are the ones in the table, so
the graph is a DAG by construction.

### 1.4 How §48 is enforced by the graph, not by intentions

| §48 rule | Mechanism |
| --- | --- |
| Core logic testable without creating GTK widgets | `lucerna-core` can't name GTK: it isn't in the dependency graph. Its unit tests build and run with no display. The GUI's own logic lives in `lucerna-ui/src/presenter/`, which contains no `gtk` symbols. |
| Renderer policy testable without launching mpv | The state machine, restart policy, pause policy and reconciliation planner are pure functions in core that take `now: Instant` as a parameter. They return `Effect` / `Action` values and never perform I/O. `lucerna-mpv` only interprets those effects. |
| X11 backend separable from daemon policy | The daemon can't import `x11rb`; it holds `Box<dyn WallpaperBackend>`. `lucerna-x11` can't reach the daemon because the edge doesn't exist. Daemon integration tests run against `lucerna_core::testing::FakeBackend`, with no X server at all. |
| GTK must not directly manipulate X11 windows or mpv subprocesses | `lucerna-ui` may depend only on core and ipc. It can't link `lucerna-x11` or `lucerna-mpv`. |

**Enforcement test.** `crates/lucerna-testkit/tests/architecture.rs` runs as part of
`cargo test --workspace`, so it is one of the §47 gates. It:

1. Reads every `crates/*/Cargo.toml`, using `toml` as a dev-dependency and locating files
   through `CARGO_MANIFEST_DIR`.
2. Asserts that each crate's `[dependencies]` internal edges are a subset of the allow
   table above, and that none of its forbidden external crates are listed. Only
   `[dev-dependencies]` are exempt.
3. Asserts that the root `[workspace.dependencies] zbus` entry does not enable `tokio`.
4. Scans `crates/lucerna-ui/src/presenter/**/*.rs` and `crates/lucerna-core/src/**/*.rs`
   and fails on any `gtk`, `x11rb`, `zbus` or `tokio` path token.
5. Asserts that every shipped `main.rs` is at most 30 lines.

Adding a forbidden edge therefore breaks CI with a message naming the rule.

### 1.5 Workspace-wide conventions

- **Lints** (`[workspace.lints]`):
  - `rust.unsafe_code = "deny"`
  - `clippy.unwrap_used = "warn"`, `clippy.expect_used = "warn"`, `clippy.panic = "warn"`,
    `clippy.todo = "warn"`, `clippy.dbg_macro = "warn"`

  CI runs clippy with `-D warnings`, which turns all of these into errors. `clippy.toml`
  allows unwrap and expect in tests. A genuine invariant violation may use
  `#[allow(clippy::expect_used)]` only with a `// INVARIANT:` comment on the same item.
- **One unsafe site** is allowed: the `pre_exec` closure in `lucerna-mpv` that sets
  `PR_SET_PDEATHSIG` via `rustix`. It carries `#[allow(unsafe_code)]` and a `// SAFETY:`
  comment, and calls only async-signal-safe functions.
- **Errors.** Each library defines typed errors with `thiserror` and keeps the underlying
  error in `#[source]`. Binaries use `anyhow` only at the top level to add context.
  User-facing messages follow §49: *what happened*, then *why*, then *what to do*. They
  live in core's `messages` module or the UI's `strings.rs`, never inline.
- **Never use a shell.** Processes are started only with `std::process::Command` or
  `tokio::process::Command`, with arguments passed individually. Grep-style CI checks in
  `architecture.rs` fail on `"sh"`, `"-c"` or `"bash"` string literals in non-test sources.
- **Logging.** `tracing` everywhere. `lucerna_core::logging::init(component)` builds an
  `EnvFilter` from `LUCERNA_LOG`, then `RUST_LOG`, then `lucerna=info`. Filter targets
  match by string prefix, so `RUST_LOG=lucerna=debug` covers every `lucerna_*` crate.
  Nothing logs per frame or per poll. The §25 event list is logged at `info`, failures at
  `warn` or `error`.
- **Clock injection.** Anything that depends on time takes `now: Instant` (pure code) or
  a `Clock` trait object (drivers), so tests are deterministic.
- **Dependencies.** Use the directive's list (§4) plus `toml_edit` (to preserve unknown
  keys and comments, §53), `rustix` (signals, `prctl`, `flock`) and `serde_json`. Nothing
  else without a written justification in `docs/ARCHITECTURE.md`. There is no `which`,
  `chrono`, `nix` or `assert_cmd`: the standard library covers those needs, and dates
  are formatted with a small civil-from-days function in core that has unit tests.

---

## 2. The backend abstraction (§7)

### 2.1 Trait (lives in `lucerna-core::backend`)

```rust
/// Stable identity of a physical display. Derived from EDID where available (see §5.3).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct OutputId(String);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rect { pub x: i32, pub y: i32, pub width: u32, pub height: u32 }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Rotation { Normal, Left, Inverted, Right }

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EdidIdentity { pub manufacturer: String, pub product_code: u16,
                          pub serial: Option<String>, pub model_name: Option<String> }

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutputInfo {
    pub id: OutputId,
    pub connector: String,          // "HDMI-1", "eDP-1"
    pub geometry: Rect,             // root-window coordinates, post-rotation/transform
    pub primary: bool,
    pub rotation: Rotation,
    pub refresh_mhz: Option<u32>,
    pub edid: Option<EdidIdentity>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SurfaceId(u64);          // backend-local, never reused within a process

/// What a renderer needs in order to draw into a surface. Non-exhaustive so that a future
/// Wayland backend can add a variant (e.g. a libmpv render-API target) without changing callers.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EmbedTarget { X11Window(u32) }

#[derive(Clone, Debug)]
pub struct SurfaceHandle { pub id: SurfaceId, pub output: OutputId, pub geometry: Rect, pub embed: EmbedTarget }

#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackendKind { CinnamonX11, X11Ewmh }

#[derive(Clone, Debug, Serialize)]
pub struct BackendCapabilities {
    pub fullscreen_detection: bool,     // EWMH client list + _NET_WM_STATE available
    pub input_passthrough: bool,        // SHAPE extension with input kind available
    pub hotplug_events: bool,           // RandR >= 1.2 notify
    pub stable_monitor_identity: bool,  // EDID readable on at least one output
}

#[derive(Clone, Debug, Serialize)]
pub struct BackendProbe {
    pub kind: BackendKind,
    pub capabilities: BackendCapabilities,
    pub facts: serde_json::Value,       // backend-specific diagnostic facts (WM name, RandR version, Nemo, ...)
}

#[non_exhaustive]
#[derive(Clone, Debug)]
pub enum BackendEvent {
    OutputsChanged,                               // re-enumerate (already debounced by the backend: 500 ms)
    FullscreenChanged(Vec<Rect>),                 // geometry of every visible fullscreen client on the current desktop
    StackingDisturbed,                            // something may have moved above/below our surfaces
    ConnectionLost(String),                       // display server went away (logout, crash)
}

pub type EventSink = Box<dyn Fn(BackendEvent) + Send + Sync + 'static>;

#[derive(Debug, thiserror::Error)]
pub enum BackendError {
    #[error("could not connect to display {display:?}: {reason}")]
    Connect { display: String, reason: String },
    #[error("required display-server feature missing: {0}")]
    MissingFeature(&'static str),
    #[error("unknown surface {0:?}")]
    UnknownSurface(SurfaceId),
    #[error("display server connection lost")]
    ConnectionLost,
    #[error("display server protocol error: {0}")]
    Protocol(String),
}

pub trait WallpaperBackend: Send {
    fn kind(&self) -> BackendKind;
    /// Collect environment facts and capabilities. Idempotent, read-only.
    fn probe(&mut self) -> Result<BackendProbe, BackendError>;
    /// Currently connected, active outputs. Disabled/disconnected outputs are omitted.
    fn enumerate_outputs(&mut self) -> Result<Vec<OutputInfo>, BackendError>;
    /// Create, configure, map and bottom-stack a wallpaper surface covering `output.geometry`.
    fn create_surface(&mut self, output: &OutputInfo) -> Result<SurfaceHandle, BackendError>;
    fn resize_surface(&mut self, surface: SurfaceId, geometry: Rect) -> Result<(), BackendError>;
    /// Idempotent: destroying an unknown/already-destroyed surface returns Ok.
    fn destroy_surface(&mut self, surface: SurfaceId) -> Result<(), BackendError>;
    /// Re-assert stacking and hints on all live surfaces (called on StackingDisturbed, rate-limited by caller).
    fn refresh(&mut self) -> Result<(), BackendError>;
    /// Start delivering events to `sink` from a backend-owned thread. Called once.
    fn subscribe(&mut self, sink: EventSink) -> Result<(), BackendError>;
    /// Structured diagnostic data (window ids, stacking positions, hints) for `doctor`.
    fn diagnostics(&mut self) -> serde_json::Value;
    /// Destroy all surfaces, stop the event thread, close the connection. Idempotent.
    fn shutdown(&mut self) -> Result<(), BackendError>;
}
```

`serde_json` is allowed in core because it only carries diagnostic data.

**Types that cross the boundary:** `OutputInfo`, `OutputId`, `Rect`, `Rotation`,
`EdidIdentity`, `SurfaceId`, `SurfaceHandle`, `EmbedTarget`, `BackendKind`,
`BackendCapabilities`, `BackendProbe`, `BackendEvent`, `BackendError`. X11 atoms, window
IDs (other than inside `EmbedTarget`) and `x11rb` types never cross it.

**Unsupported sessions** (Wayland, no `DISPLAY`) don't get a backend. The engine holds
`Option<Box<dyn WallpaperBackend>>` together with a
`BackendStatus::Unavailable { reason: UnsupportedReason, message }`.

### 2.2 Threading

- `X11Backend` holds an `Arc<x11rb::rust_connection::RustConnection>`. This is pure Rust
  and doesn't need libxcb.
- `subscribe` spawns the `lucerna-x11-events` thread, which runs `wait_for_event` and
  translates each event into a `BackendEvent`. It debounces RandR events (500 ms) and
  fullscreen recomputation (150 ms) itself, then calls `sink`.
- The daemon's sink forwards events into a `tokio::sync::mpsc::UnboundedSender<EngineMsg>`.
- The engine calls the other trait methods directly. They are short X round trips on a
  local socket.
- `RustConnection` supports concurrent requests and replies from several threads.

### 2.3 The test double

`lucerna_core::testing::FakeBackend` sits behind the `test-support` feature and is used
through `[dev-dependencies]` only. It provides:

- `set_outputs(Vec<OutputInfo>)`
- `emit(BackendEvent)`
- a log of surfaces created and destroyed
- configurable failure injection

It is what lets the daemon's policy be tested without X11 (§48).

---

## 3. Renderer state machine (§9, §51)

### 3.1 Types (in `lucerna-core::renderer`)

```rust
pub enum RendererState {
    Stopped,
    Starting { generation: u64, pause_on_ready: bool },
    Playing  { generation: u64 },
    Paused   { generation: u64 },
    Stopping { generation: u64, escalation: StopEscalation },   // QuitSent | TermSent | KillSent
    Failed   { reason: FailureReason, retry_at: Option<Instant>, pause_on_ready: bool },
}

pub enum FailureReason {           // each has a stable kebab-case code used in D-Bus/doctor
    MpvMissing,                    // "mpv-missing"      spawn() -> ENOENT, or discovery failed
    LaunchFailed(String),          // "launch-failed"    spawn error other than ENOENT; mpv exit code 1
    MediaMissing,                  // "media-missing"    path absent / not a regular file at start time
    MediaUnsupported(String),      // "media-unsupported" end-file reason=error, or mpv exit code 2
    Crashed(ExitInfo),             // "crashed"          killed by signal, or exit code not in {0,1,2}
    UnexpectedExit(ExitInfo),      // "unexpected-exit"  exit 0 without a stop request (loop-file=inf never ends)
    StartupTimeout,                // "startup-timeout"  no IPC socket in 5 s or no file-loaded in 10 s
    IpcFailed(String),             // "ipc-failed"       IPC connection dropped while process alive
    RestartLimit(Box<FailureReason>), // "restart-limit" bounded policy exhausted (wraps last cause)
}

pub enum RendererEvent {
    Start,                         // user/engine intent
    Pause, Resume, Stop,
    Ready       { generation: u64 },                 // IPC connected AND file-loaded received
    Exited      { generation: u64, exit: ExitInfo }, // process reaped
    MediaError  { generation: u64, message: String },
    LaunchError { generation: u64, error: LaunchError },
    IpcLost     { generation: u64, message: String },
    StartupTimedOut { generation: u64 },
    StopTimedOut    { generation: u64 },
    RetryDue,
}

pub enum Effect {
    Spawn { generation: u64, start_paused: bool },
    SetPause(bool),
    SendQuit, SendTerm, SendKill,
    ArmStartupTimer { generation: u64, after: Duration },   // 10 s
    ArmStopTimer    { generation: u64, after: Duration },   // 2 s per escalation step
    ArmRetryTimer   { after: Duration },
    CancelTimers,
    Notify,                        // state changed -> engine emits StatusChanged (coalesced)
    LogFailure(FailureReason),
}

pub enum Outcome { Applied, Normalized /* idempotent no-op */, Ignored /* stale generation */, Rejected(&'static str) }

pub struct RendererMachine { state: RendererState, next_generation: u64, restarts: RestartTracker }
impl RendererMachine {
    pub fn handle(&mut self, event: RendererEvent, now: Instant) -> (Outcome, Vec<Effect>);
    pub fn state(&self) -> &RendererState;
}
```

### 3.2 Transition table

A dash (—) means the event is **normalized**: the state doesn't change and no effect runs.
`stale` means the event carries a generation that isn't the current one. It returns
`Ignored` and is logged at `debug`.

| State \ Event | Start | Pause | Resume | Stop | Ready(g) | Exited(g) | MediaError / LaunchError(g) | IpcLost / StartupTimedOut(g) | StopTimedOut(g) | RetryDue |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| **Stopped** | → Starting{g+1} [Spawn, ArmStartupTimer] | — | — | — | stale | stale | stale | stale | stale | — |
| **Starting** | — | pause_on_ready=true | pause_on_ready=false | → Stopping [SendQuit, ArmStopTimer] | → Playing, or Paused with [SetPause(true)] | → *crash rule* | → Failed(no retry) [SendKill] | → [SendKill] then *crash rule* | stale | — |
| **Playing** | — | → Paused [SetPause(true)] | — | → Stopping [SendQuit, ArmStopTimer] | — | → *crash rule* | → Failed(no retry) [SendKill] | → [SendKill] then *crash rule* | stale | — |
| **Paused** | — | — | → Playing [SetPause(false)] | → Stopping [SendQuit, ArmStopTimer] | — | → *crash rule* | → Failed(no retry) [SendKill] | → [SendKill] then *crash rule* | stale | — |
| **Stopping** | Rejected("stopping") | — | — | — | — | → Stopped [CancelTimers] | — | — | QuitSent→[SendTerm], TermSent→[SendKill], each with ArmStopTimer | — |
| **Failed** | → Starting{g+1}, restart tracker **reset** (explicit user action) | pause_on_ready=true | pause_on_ready=false | → Stopped [CancelTimers] | stale | — (reaping a killed process; bookkeeping only) | stale | stale | stale | if retry_at is set: → Starting{g+1} [Spawn, ArmStartupTimer]; otherwise — |

Notes on the table:

- **Ready while Starting** goes to Playing, or to Paused with [SetPause(true)] if
  `pause_on_ready` is set.
- **MediaError / LaunchError** cause `Failed` without retry because the failure is
  deterministic. `LaunchError::NotFound` maps to `MpvMissing`. Every other error maps to
  `LaunchFailed`, or to `MediaUnsupported` for mpv exit code 2 or an `end-file` error.
- **Exit-code classification:** exit 1 → `LaunchFailed` (no retry). Exit 2 →
  `MediaUnsupported` (no retry). Exit 0 while not Stopping → `UnexpectedExit` (goes
  through the crash rule). A signal, or any other code → `Crashed` (crash rule).

**Crash rule.** Call `restarts.on_failure(now)`:

- `RetryAfter(d)` → `Failed{reason, retry_at: Some(now + d)}` with
  [LogFailure, ArmRetryTimer(d), Notify].
- `GiveUp` → `Failed{RestartLimit(reason), retry_at: None}` with [LogFailure, Notify].

Failed with `retry_at: None` is terminal until the engine sends `Start`. That happens on
user action (`SetWallpaper`, `Start`, `Reload`, `Resume` after `Stop`), a config change
affecting that display, or the missing-media recheck finding the file again (§6.5).

**Invalid transitions** are therefore either normalized (idempotent intent, such as
Pause while Paused or Stop while Stopped) or rejected (`Start` while Stopping; the engine
retries `Start` after `Stopped` if it still wants the renderer). Events from a previous
process are made harmless by the `generation` check. Runtime state is exactly one enum
value per renderer, never a set of unrelated booleans.

### 3.3 Bounded restart policy (§51)

```rust
pub struct RestartPolicy { pub max_restarts: u32 /* 3 */, pub window: Duration /* 60 s */,
                           pub backoff: [Duration; 3] /* 1 s, 2 s, 4 s */ }
pub struct RestartTracker { policy: RestartPolicy, failures: VecDeque<Instant> }
pub enum RestartDecision { RetryAfter(Duration), GiveUp }
impl RestartTracker {
    pub fn on_failure(&mut self, now: Instant) -> RestartDecision; // drop entries older than window; if len >= max -> GiveUp
    pub fn reset(&mut self);
}
```

- The window slides, so a renderer that has run cleanly for longer than the window gets
  its full budget back.
- Values come from `[renderer]` in the config. They are clamped to `max_restarts` in
  1..=10 and `window` in 10..=3600 s, so the policy can never be unbounded.
- **Unit tests** are table-driven with `ManualClock`:
  - three failures inside 60 s give three `RetryAfter`, then `GiveUp`;
  - a fourth failure after the window has slid gives `RetryAfter`;
  - the backoff sequence is 1, 2 and 4 s;
  - a `Start` from Failed resets the tracker;
  - stale generations are ignored;
  - every cell of the transition table is covered by `tests::transition_table`, which
    iterates over all state × event pairs.
- **Integration test** (`testkit/tests/renderer_supervision.rs::restart_storm_is_bounded`):
  fake-mpv crashes after 100 ms with the policy set to max 3 in a 5 s window. Assert
  exactly 4 spawns, final state `Failed(RestartLimit(Crashed))`, and no further spawns
  within the next 3 s.

---

## 4. mpv invocation (§9, §16, §17, §26, §49)

### 4.1 Argument vector

`lucerna_core::mpv::build_args(&RenderSpec) -> Vec<OsString>` is a pure function.
`RenderSpec` = { `embed: Option<EmbedTarget>`, `ipc_socket: PathBuf`, `media: PathBuf`
(absolute, canonical), `scaling`, `hwdec`, `fps`, `audio`, `start_paused`,
`vo_override: Option<String>` (tests only) }.

Order and contents:

| # | Argument | Why |
| --- | --- | --- |
| 1 | `--no-config` | Ignore the user's `mpv.conf`, `input.conf` and scripts (§16). |
| 2 | `--no-input-terminal` | Don't read stdin. |
| 3 | `--quiet` | No status line. |
| 4 | `--msg-level=all=warn` | Keep warnings and errors on stderr for the bounded log (D5). |
| 5 | `--msg-color=no` | Plain log text. |
| 6 | `--no-input-default-bindings` | No key or mouse bindings (§9). |
| 7 | `--input-vo-keyboard=no` | The video window never takes keyboard focus (§14). |
| 8 | `--input-cursor=no` | Ignore the mouse. |
| 9 | `--cursor-autohide=no` | Don't touch the cursor. |
| 10 | `--osc=no` | No on-screen controller. The built-in OSC still loads under `--no-config`. |
| 11 | `--osd-level=0` | No OSD text. |
| 12 | `--load-scripts=no` | No user scripts. |
| 13 | `--load-stats-overlay=no` | No built-in helper scripts. |
| 14 | `--load-osd-console=no` | No built-in helper scripts. |
| 15 | `--load-auto-profiles=no` | No built-in helper scripts. |
| 16 | `--ytdl=no` | No network helpers (§26: no automatic network access). |
| 17 | `--resume-playback=no` | Don't read watch-later state. |
| 18 | `--stop-screensaver=no` | **Don't inhibit the screensaver or lock**, which would otherwise happen for as long as the wallpaper plays. |
| 19 | `--x11-bypass-compositor=never` | Never ask the compositor to unredirect. |
| 20 | `--input-media-keys=no` | Don't grab media keys. |
| 21 | `--loop-file=inf` | Loop forever. |
| 22 | `--idle=no` | Exit if the file ends or fails, which the supervisor then classifies. |
| 23 | `--force-window=no` | Default behaviour, stated explicitly. |
| 24 | audio off (default): `--aid=no --mute=yes`. audio on: `--aid=auto --mute=no --volume=100` | `--aid=no` skips audio decoding entirely. Volume is explicit so nothing is inherited (§16). |
| 25 | hwdec: `--hwdec=auto-safe` for Auto, `--hwdec=no` for Disabled | §17. |
| 26 | fps: nothing for Native, otherwise `--vf=fps=60`, `--vf=fps=30` or `--vf=fps=15` | This really changes behaviour: frames are dropped before rendering, which reduces GPU upload and presentation work but **not** decode work. That is documented honestly in `docs/CONFIGURATION.md`. |
| 27 | Scaling flags, see §4.2 | |
| 28 | `--pause=yes` or `--pause=no` | Follows `start_paused`, so a surface that should start paused never shows a frame of motion. |
| 29 | `--input-ipc-server=<socket>` | See §4.3. |
| 30 | `--wid=<xid>` | Only when `embed` is `Some(X11Window)`. |
| 31 | `--vo=<x>` | Test builds only (`null` or `x11`). Never emitted in production. |
| 32 | `--` | End of options, so nothing after it is parsed as an option. |
| 33 | `<absolute canonical media path>` | A single argument. It is never quoted or interpolated, never goes through a shell, always starts with `/` and follows `--`, so `$(rm -rf ~).mp4` is just a filename (§26). |

Environment is inherited as-is, with no additions. `stdin` is null.

**Option compatibility must be tested (§9):**

- `lucerna_core::mpv::required_options()` returns the option names the builder can emit.
- `lucerna_core::mpv::check_compat(mpv_path)` runs `mpv --list-options` and reports any
  that are missing.
- The unit test `mpv::tests::args_*` checks the exact vectors for every combination of
  settings.
- The integration test `real_mpv::options_supported` asserts the check passes against
  the installed mpv. CI sets `LUCERNA_REQUIRE_MPV=1`, which turns "mpv not installed"
  into a failure instead of a skip.
- The same check runs inside the Ubuntu and Fedora package smoke tests through
  `lucernactl doctor --json` (field `mpv.options_compatible`), which covers both distros'
  mpv versions.

### 4.2 Scaling modes (§13): deterministic and documented

| Mode | mpv properties | Result |
| --- | --- | --- |
| `fill` (default) | `keepaspect=yes panscan=1.0 video-unscaled=no` | Covers the whole output, keeps aspect ratio, crops the overflow. |
| `fit` | `keepaspect=yes panscan=0.0 video-unscaled=no` | The whole video is visible, keeps aspect ratio, letterboxed in black. |
| `stretch` | `keepaspect=no panscan=0.0 video-unscaled=no` | Covers the whole output and distorts the aspect ratio. |
| `center` | `keepaspect=yes panscan=0.0 video-unscaled=yes` | Native pixel size, centred. Black bars if the video is smaller than the output, cropped if larger. |

- These are passed as `--keepaspect=…`, `--panscan=…` and `--video-unscaled=…` at launch.
- A live change sends three `set_property` commands over IPC, with no restart.
- The background is black (mpv's default).

### 4.3 IPC socket strategy

- **Directory:** `$XDG_RUNTIME_DIR/lucerna/`, created with mode `0700`.
  - On every start, verify it is a directory, owned by the current user, and has mode
    `0700`. If not, refuse with a readable error. This protects against another user
    pre-creating it. Core stays std-only here by comparing against
    `std::fs::metadata("/proc/self")?.uid()`.
  - If `XDG_RUNTIME_DIR` is unset, fall back to `/run/user/<uid>` **only** if it exists,
    is owned by us and has mode `0700`. Otherwise the daemon refuses to start renderers
    and reports `runtime-dir-unavailable`. It never falls back to `/tmp` (§26).
- **Socket file:** `mpv-<slot>-<generation>.sock`.
  - `slot` is the first 8 hex characters of FNV-1a-64(`OutputId`), which keeps the path
    short. A unit test asserts the maximum path length is under 108 bytes for a 64-byte
    runtime directory, and the daemon returns a startup error if a real path would
    exceed the limit.
  - The generation number in the name means a new process never collides with a dying one.
- **Stale sockets:** before each spawn, `unlink` the target path if it exists. At daemon
  start, after stale-process recovery (§4.6), remove every `mpv-*.sock` in the directory.
- **Permissions:** mpv creates the socket itself. The `0700` parent directory is what
  makes it private to the user. The socket is never the public API (§19).

### 4.4 IPC protocol use (`lucerna-mpv::MpvIpc`)

- Newline-delimited JSON over a tokio `UnixStream`. Requests are
  `{"command": [...], "request_id": n}` and are matched to responses by `request_id`.
  Messages with an `"event"` field are routed to the event stream.
- **Connecting:** retry every 50 ms for up to **5 s** after spawn. Failure means
  `StartupTimedOut`.
- **Commands used:**
  - `observe_property 1 pause`
  - `set_property` for `pause`, `keepaspect`, `panscan` and `video-unscaled`
  - `get_property` for `mpv-version`, and for `duration`, `width`, `height` and
    `video-codec`, the optional metadata from §11
  - `quit`
- **Events used:**
  - `file-loaded`: combined with a connected IPC socket, this is `Ready`.
  - `end-file`: `reason == "error"` becomes `MediaError(file_error)`.
  - `shutdown`
- **Startup deadline:** 10 s from spawn to `Ready`, otherwise `StartupTimedOut`.
- **Stop escalation:** `quit` via IPC, wait 2 s, then `SIGTERM` (`rustix::process::kill_process`),
  wait 2 s, then `SIGKILL`. Always `wait()` to reap the child.

### 4.5 Bounded capture of stdout and stderr

- Both streams are piped. A reader task per stream splits lines, truncates each line to
  4 KiB, and writes to:
  1. an in-memory ring of the last **200 lines** per renderer, which feeds
     `failure_message` and `doctor`;
  2. `lucerna_core::log::BoundedLog` at
     `$XDG_STATE_HOME/lucerna/logs/renderer-<slot>.log`.
- **Rotation:** before a write would push the file past **512 KiB**, rename it to `.1`
  (replacing the previous `.1`) and start a new file. That caps each output at 1 MiB,
  and the number of outputs is bounded.
- Lines are **not** forwarded to `tracing` one by one. Only the last 5 stderr lines are
  attached to a failure log entry. That keeps journals from flooding (§25).
- The daemon's own log uses the same `BoundedLog` (`lucernad.log`, 1 MiB cap) as a second
  writer next to stderr.
- **Unit test:** write 3 MiB through `BoundedLog` and assert total size ≤ 1 MiB with two
  files present.
- **Integration test:** fake-mpv floods stderr (`stderr-flood=5000000`) and the same size
  bound is asserted.

### 4.6 Process ownership and stale recovery (§52)

- `PidRegistry` is written atomically to `$XDG_RUNTIME_DIR/lucerna/renderers.json` on
  every spawn and reap:
  `[{pid, starttime, socket, output_id, generation}]`, where `starttime` is field 22 of
  `/proc/<pid>/stat`.
- **Recovery at daemon start**, after acquiring the lock. For each record, only when
  **all** of these hold:
  - `/proc/<pid>` exists,
  - its `starttime` matches,
  - `/proc/<pid>/exe` resolves to a file named `mpv`,
  - and `/proc/<pid>/cmdline` contains `--input-ipc-server=<that socket>`,

  send `SIGTERM`, wait 2 s, then `SIGKILL`. Then clear the registry.

  An mpv the user started themselves can never match, because the socket path is ours
  and the start time is recorded (§52: "do not kill unrelated mpv instances").
- **Defence in depth:** children are spawned with `PR_SET_PDEATHSIG=SIGTERM` (the single
  unsafe site). All spawns happen on a long-lived tokio worker thread, so the
  thread-lifetime caveat of `PDEATHSIG` doesn't apply. That caveat is documented in
  `docs/ARCHITECTURE.md`.
- Integration test `stale_process_recovery`:
  - spawn fake-mpv, write a registry and drop the supervisor without stopping;
  - run recovery and assert the process is gone;
  - start an unrelated fake-mpv without a registry entry and assert it survives.

### 4.7 Detecting that mpv is missing (§49)

- **Discovery:** `LUCERNA_MPV` (an absolute path, a debugging override reported by
  `doctor`), else a search of `PATH` entries for an executable regular file named `mpv`.
  This is std-only.
- **At daemon start and at every `Reload`:** if mpv isn't found, set `mpv_available=false`.
  Every renderer that would start goes straight to
  `Failed(MpvMissing)` with the §49 message:

  > Lucerna could not start mpv. Executable "mpv" was not found in PATH. Install mpv
  > (`sudo apt install mpv`) and choose Reload.

  No spawn attempts are made, so there is no storm.
- **Belt and braces:** `spawn()` returning `ErrorKind::NotFound` also maps to `MpvMissing`.
- The **version** is parsed from the first line of `mpv --version`
  (`mpv 0.37.0 Copyright …`).

---

## 5. X11 / Cinnamon integration (§8, §12, §14, §15)

### 5.1 Session and backend selection (daemon, via `lucerna-core::session`)

| Condition, checked in order | Result |
| --- | --- |
| `XDG_SESSION_TYPE=wayland`, or `XDG_SESSION_TYPE` unset and `WAYLAND_DISPLAY` set | `Unavailable(Wayland)`. We never try XWayland, even when `DISPLAY` is set (§50). |
| `DISPLAY` unset | Poll the environment for up to 10 s (§5.6), then `Unavailable(NoDisplay)`. |
| `x11rb::connect` fails | Retry every 500 ms for 10 s, then `Unavailable(X11ConnectFailed{display, reason})`. |
| RandR < 1.2 | `Unavailable(MissingFeature("RandR ≥ 1.2"))` |
| `XDG_CURRENT_DESKTOP` contains `Cinnamon` (for example `X-Cinnamon`), **or** the `_NET_SUPPORTING_WM_CHECK` window's `_NET_WM_NAME` contains `Muffin` | `CinnamonX11` |
| Any other X11 | `X11Ewmh`: best effort, not an acceptance target, stated in the README. |

Only **one** unsupported-session error is emitted per start. `Unavailable` means no
surfaces, no mpv and no retry loop. The daemon keeps serving D-Bus so the GUI, CLI and
`doctor` can explain the state (§50). The §49 message for Wayland is:

> Lucerna could not start wallpaper playback. This release supports X11 sessions only;
> your current session appears to be Wayland. Log in with a "Cinnamon" (X11) session.

### 5.2 Surface primitives (§8)

One surface per active RandR monitor. Each is a child of the root window at
`OutputInfo.geometry`, created by `create_surface`.

| Primitive | X11 implementation |
| --- | --- |
| Geometry-controlled surface | `CreateWindow` with root depth and visual, `InputOutput`, at the monitor rect; `ConfigureWindow` for resize or move. mpv follows the parent's size automatically with `--wid`. |
| Undecorated | `_MOTIF_WM_HINTS` = flags `2` (decorations), decorations `0`. Irrelevant for override-redirect, set anyway. |
| Input-disabled | SHAPE `Rectangles(SET, INPUT, [])`: an **empty input region**. The server's `XYToWindow` skips this window's whole subtree, including mpv's child window, so clicks reach Nemo's desktop window or the root. `WM_HINTS` input=False. Event mask is `StructureNotify` only; we never select key or button events. |
| Skip taskbar / skip pager | `_NET_WM_STATE` = `_SKIP_TASKBAR` + `_SKIP_PAGER`. Override-redirect windows are never in `_NET_CLIENT_LIST`, so they don't appear in taskbars or Alt+Tab. |
| Sticky across workspaces | `_NET_WM_DESKTOP` = `0xFFFFFFFF` and `_NET_WM_STATE_STICKY`. An override-redirect window is on every workspace by nature. |
| Desktop / below hints | `_NET_WM_WINDOW_TYPE` = `_NET_WM_WINDOW_TYPE_DESKTOP`, `_NET_WM_STATE_BELOW` |
| Override-redirect | `override_redirect=1` when the stacking mode is `override-redirect` (default, D7). `0` in `desktop-window` mode, where Muffin manages the window. |
| Compositor hint | `_NET_WM_BYPASS_COMPOSITOR` = `2` (never unredirect) |
| Identification | `WM_CLASS` = `lucerna-wallpaper\0Lucerna\0`, `_NET_WM_NAME` = "Lucerna wallpaper (HDMI-1)", `_NET_WM_PID`, `WM_CLIENT_MACHINE` (from `/proc/sys/kernel/hostname`), and a private `_LUCERNA_WALLPAPER` (UTF8_STRING = `OutputId`) so `doctor` can find our windows. |
| Explicit lowering | Override-redirect mode: `ConfigureWindow(stack_mode=Below)` with no sibling, which puts the window at the bottom of root's children. Desktop-window mode: a `_NET_RESTACK_WINDOW` client message (source 2, detail Below, sibling = Nemo's desktop window if found), falling back to `ConfigureWindow Below`. |
| Re-stacking | The event thread selects `SubstructureNotify` on root. Any `ConfigureNotify`, `MapNotify` or `CirculateNotify` from another window emits `StackingDisturbed`. The engine calls `refresh()`, which re-lowers only if a surface isn't already at the bottom, as shown by `QueryTree`. |
| Cleanup on shutdown | `DestroyWindow` for every surface, flush, disconnect. If the daemon is killed, the X server destroys our windows when the connection closes (the default close-down mode, which we never change). |
| Background | `background_pixel` = black, so there is no garbage before mpv's first frame. |

**Guarding against a restack fight:**

- `refresh()` calls are rate-limited by the engine to at most 5 per 10 s.
- If the limit is hit, one `warn` is logged ("another client keeps restacking the desktop
  layer"). The rate then drops to 1 per 10 s, and the fight counter shows up in `doctor`.
- This stops Lucerna from burning CPU if Muffin and Lucerna disagree.

### 5.3 RandR enumeration and monitor identity (§12)

**Enumeration:**

- **RandR ≥ 1.5:** `GetMonitors(get_active=true)`. Each monitor gives the geometry in
  root coordinates (after rotation, transform and scale) and its outputs.
- **RandR 1.2–1.4:** `GetScreenResourcesCurrent`, then for each output with a CRTC,
  `GetCrtcInfo` for geometry and rotation.
- For each output:
  - the connector name comes from `GetOutputInfo`;
  - `primary` comes from `GetOutputPrimary`;
  - the EDID comes from `GetOutputProperty("EDID")`, reading up to 256 bytes;
  - rotation comes from the CRTC.
- **Mirrored outputs** (one monitor, several outputs) produce one surface. Identity comes
  from the lexicographically first output.

**Identity** is the pure function `lucerna_core::identity::stable_id`:

1. Parse EDID bytes 0–127: header check and checksum; manufacturer PNP ID from bytes 8–9;
   product code from 10–11 (little-endian); serial number from 12–15; descriptors
   `0xFF` (serial string) and `0xFC` (model name).
2. The serial is the text serial if present and non-blank, else the numeric serial if
   non-zero, else none.
3. Build the ID:
   - `edid:<MFG>-<product:04x>-<serial>` if there is a serial
   - `edid:<MFG>-<product:04x>@<connector>` if not
   - `conn:<connector>` if there is no usable EDID
4. If two *present* outputs compute the same ID (identical panels that report identical
   serials), append `@<connector>` to both. The function is deterministic.

This is unit tested with real-world EDID fixture bytes, including a zero serial, a text
serial, a bad checksum, a truncated EDID, and two identical monitors.

**Absent displays:** config entries for displays that aren't present are never deleted.
The planner (§6.4) simply creates no renderer for them, and they return automatically
when the ID reappears.

### 5.4 Fullscreen detection (§15)

- **Event-driven, no polling.** Watch root `PropertyNotify` for `_NET_CLIENT_LIST`,
  `_NET_ACTIVE_WINDOW` and `_NET_CURRENT_DESKTOP`, and select `PropertyChange` and
  `StructureNotify` on every client in `_NET_CLIENT_LIST`, re-selecting when the list
  changes.
- **A client counts as a visible fullscreen occluder when:**
  - `_NET_WM_STATE` contains `_NET_WM_STATE_FULLSCREEN`,
  - it does **not** contain `_NET_WM_STATE_HIDDEN`,
  - its map state is Viewable,
  - and `_NET_WM_DESKTOP` is the current desktop or `0xFFFFFFFF`.
- Its rectangle comes from `GetGeometry` plus `TranslateCoordinates` to root.
- `MAXIMIZED_*` is **ignored** (§15: don't pause for maximized windows).
- The event is `FullscreenChanged(Vec<Rect>)` after a 150 ms debounce.
- **Mapping to outputs** is pure, in core: `occluded_outputs(outputs, rects)`. An output
  is occluded when some rect covers ≥ 90 % of its area, so a window spanning two monitors
  via `_NET_WM_FULLSCREEN_MONITORS` pauses both. Unit tested.
- **Capability:** `fullscreen_detection = false` when the window manager doesn't publish
  `_NET_CLIENT_LIST` or `_NET_WM_STATE_FULLSCREEN` in `_NET_SUPPORTED`. Then
  `pause_on_fullscreen` has no effect, and GetStatus / `doctor` / the Settings page say so.

### 5.5 Cinnamon and Nemo probing (investigate, don't assume)

`X11Backend::probe()` and `diagnostics()` collect the following, and all of it appears in
`doctor`:

- **WM:** the `_NET_SUPPORTING_WM_CHECK` window and its `_NET_WM_NAME`; the
  `_NET_SUPPORTED` atom list, reduced to the atoms we care about.
- **Compositor present:** `_NET_WM_CM_S<screen>` has a selection owner.
- **Nemo desktop:**
  - the *process*, by scanning `/proc/*/comm` for `nemo-desktop` (std only, no shell);
  - the *window*, as a `_NET_CLIENT_LIST` member with `WM_CLASS` instance `nemo-desktop`
    or type `_NET_WM_WINDOW_TYPE_DESKTOP`: its xid, depth (32 suggests an ARGB, possibly
    transparent window; 24 means opaque), geometry, and index in the root stacking order.
- **Our surfaces:** xid, geometry, override-redirect flag, map state, stacking index,
  whether they are below Nemo's window, and the number of mpv child windows (from
  `QueryTree`).
- **Restack fights:** the counter from §5.2.
- **Session variables:** `XDG_SESSION_TYPE`, `XDG_CURRENT_DESKTOP` and `DESKTOP_SESSION`.

`docs/X11-CINNAMON-NOTES.md`, written in v0.2.0, records:

- what the design assumes about Muffin and Nemo, from studying xwinwrap's *behaviour* and
  public descriptions of Mint workarounds. No code is copied (§43).
- which `doctor` field would confirm or refute each assumption on a real desktop.
- the exact manual steps (in `MANUAL-ACCEPTANCE.md`) to switch `[x11] stacking` if the
  default fails.

**No Cinnamon or Nemo settings are ever changed** (§8). v1 has no gsettings writes and no
dconf writes. If manual acceptance shows one is needed, it has to be designed in a later
milestone with record-and-restore (§8). It is out of scope for this plan.

### 5.6 Probed at runtime vs. assumed

| Probed (every start, every `Reload`, and in `doctor`) | Assumed (documented; confirmed only by manual acceptance) |
| --- | --- |
| Session type; whether `DISPLAY` connects | Muffin composites override-redirect windows in X stacking order, so the bottom of the stack is behind Nemo's icons. |
| RandR version; `GetMonitors` availability | Nemo's desktop window under Cinnamon is transparent over the Cinnamon-drawn background, so our video shows through between icons. |
| SHAPE extension (for input pass-through) | Muffin doesn't unredirect a bottom-stacked, full-output override-redirect window. `_NET_WM_BYPASS_COMPOSITOR=2` is also set. |
| WM name, `_NET_SUPPORTED`, compositor selection owner | Muffin doesn't periodically restack override-redirect windows above the desktop window. The restack watchdog mitigates this if it does. |
| Nemo process and desktop window, and its depth | An empty input shape on our window gives click-through to Nemo on a real Cinnamon session. The X protocol semantics are tested under Xvfb. |
| Whether EDID can be read | mpv's `--wid` rendering looks right under Muffin: no tearing, correct scaling. |
| Screensaver D-Bus names (§6.6) | mpv's `--vf=fps=N` works together with hwdec on the user's GPU. |
| mpv path, version and option compatibility | |

### 5.7 X11 protocol tests under Xvfb (§35): protocol, never visual

**Harness:** `lucerna_testkit::xvfb::Xvfb::start(ScreenSpec)`.

- Picks a free display number from `:90` to `:189`, skipping any with an existing
  `/tmp/.X<n>-lock`.
- Spawns `Xvfb :<n> -screen 0 <W>x<H>x24 -nolisten tcp -noreset +extension RANDR`.
- Waits up to 5 s for the socket and a successful connect. It retries the next number if
  the server says the display is already active.
- The display string is passed **explicitly** to `X11Backend::connect(Some(":n"), opts)`.
  Tests never change the process environment.
- If `Xvfb` isn't installed, the test prints `SKIPPED (no Xvfb)`, unless
  `LUCERNA_REQUIRE_XVFB=1` (set in CI), in which case it fails.

**Suite `testkit/tests/x11_protocol.rs`:**

1. `connect_and_probe`: connects, RandR ≥ 1.2 reported, SHAPE present.
2. `enumerate_single_screen`: one output with the geometry of `-screen 0`.
3. `enumerate_virtual_monitors`: the test adds two monitors with RandR 1.5 `SetMonitor`
   (1920×1080+0+0 and 1280×1024+1920+0). Assert two `OutputInfo`s with the correct
   geometry and `conn:` fallback IDs (Xvfb has no EDID).
4. `surface_properties`: after `create_surface`, `GetProperty` and `GetWindowAttributes`
   show:
   - `_NET_WM_WINDOW_TYPE` = DESKTOP
   - `_NET_WM_STATE` ⊇ {BELOW, SKIP_TASKBAR, SKIP_PAGER, STICKY}
   - `_NET_WM_DESKTOP` = `0xFFFFFFFF`
   - `_MOTIF_WM_HINTS` decorations=0
   - `WM_HINTS` input=False
   - `WM_CLASS`, `_NET_WM_BYPASS_COMPOSITOR`=2, `_LUCERNA_WALLPAPER`
   - override_redirect=1, map state Viewable

   Plus `ShapeGetRectangles(INPUT)` → 0 rectangles.
5. `input_passthrough_protocol`: create a plain InputOutput window underneath that
   selects `ButtonPress`, create our surface above it covering the same area, and send an
   XTEST fake click. Assert the lower window receives the event and ours doesn't. This is
   a statement about X event routing, not about Cinnamon.
6. `resize`: `resize_surface`, then `GetGeometry` matches.
7. `lower_and_restack`: map an unrelated sibling window, call `refresh()`, and assert
   `QueryTree(root).children[0]` is our surface.
8. `nemo_simulation_detected`: the test creates a window with `WM_CLASS=nemo-desktop`,
   type DESKTOP, depth 32, and writes `_NET_CLIENT_LIST` on root. It asserts
   `diagnostics()` reports it and our stacking index is lower.
9. `fullscreen_detection`: the test plays the WM's role by writing root
   `_NET_CLIENT_LIST`, `_NET_CURRENT_DESKTOP` and `_NET_SUPPORTED`, then maps a client
   with `_NET_WM_STATE_FULLSCREEN` at 1920×1080+0+0. Expect
   `FullscreenChanged([that rect])`. Replacing FULLSCREEN with MAXIMIZED_VERT/HORZ gives
   `FullscreenChanged([])`. Adding HIDDEN, or putting it on another desktop, also gives
   `[]`.
10. `hotplug_event`: `SetMonitor` / `DeleteMonitor` produce exactly one debounced
    `OutputsChanged`.
11. `destroy_and_shutdown`: after `destroy_surface`, `GetWindowAttributes` returns
    BadWindow. `shutdown()` is idempotent.
12. `connection_lost_and_reconnect`: kill Xvfb and expect `ConnectionLost` with no panic.
    Start a new Xvfb and a new backend connects and creates a surface.
13. `mpv_embeds_into_surface`: requires mpv and Xvfb. A real mpv with
    `--wid=<surface> --vo=x11` on `tests/fixtures/media/sample.webm`. Wait for Ready and
    assert `QueryTree(surface)` has ≥ 1 child. This is **protocol evidence of embedding
    only**. `--vo=x11` avoids needing GL under Xvfb.

Every report, test name and doc refers to these as *protocol tests*. Screenshots are
never taken.

---

## 6. Configuration (§18, §53) and daemon-side state

### 6.1 Files

| Path | Owner | Contents |
| --- | --- | --- |
| `$XDG_CONFIG_HOME/lucerna/config.toml` (else `~/.config/…`) | daemon (only writer, D2) | User intent: settings, library, assignments. Mode `0600`. |
| `$XDG_CONFIG_HOME/lucerna/config.toml.bak-v<N>-<UTC-stamp>` | daemon | Pre-migration backup (§53). |
| `$XDG_CONFIG_HOME/lucerna/config.toml.corrupt-<UTC-stamp>` | daemon | An unparseable config moved aside, never deleted. |
| `$XDG_CONFIG_HOME/autostart/org.lucerna.Lucerna.Daemon.desktop` | daemon (D4) | Autostart entry. Present = enabled. |
| `$XDG_STATE_HOME/lucerna/state.json` | daemon | Last 20 renderer failures (time, output, code, message) and the last config notice. |
| `$XDG_STATE_HOME/lucerna/logs/*.log[.1]` | daemon | Bounded logs (§4.5). |
| `$XDG_CACHE_HOME/lucerna/media-info.json` | daemon | Optional metadata (duration, resolution, codec) learned from mpv while playing. Cache only, safe to delete. |
| `$XDG_RUNTIME_DIR/lucerna/daemon.lock` | daemon | `flock` for single instance (§7.1). |
| `$XDG_RUNTIME_DIR/lucerna/renderers.json` | daemon | PID registry (§4.6). |
| `$XDG_RUNTIME_DIR/lucerna/mpv-*.sock` | mpv | Private IPC sockets. |

Paths are resolved with `dirs` (`config_dir`, `state_dir`, `cache_dir`, `runtime_dir`)
through a `Paths` struct that is **always injected**, so tests use temporary directories
without touching the environment.

### 6.2 Schema v1

```toml
schema_version = 1

[general]
pause_on_fullscreen = true       # §15 default
pause_on_lock = true
audio = false                    # §16 default; never enabled silently
hardware_decode = "auto"         # "auto" | "disabled"
fps_limit = "native"             # "native" | "60" | "30" | "15"   (integers 60/30/15 also accepted)

[renderer]
max_restarts = 3                 # clamped 1..=10
restart_window_secs = 60         # clamped 10..=3600

[x11]
stacking = "auto"                # "auto" | "override-redirect" | "desktop-window"   (auto = override-redirect)

[all_displays]                   # §13 mode 1: one wallpaper on every display without an override
wallpaper = "6f1c7a52-…"         # optional
scaling = "fill"                 # "fill" | "fit" | "stretch" | "center"

[displays."edid:DEL-a0b1-7XJ2K3"]  # §13 mode 2: per-display override, keyed by stable id
wallpaper = "0b8e…"              # optional; absent => inherits all_displays.wallpaper
scaling = "fit"                  # optional; absent => inherits all_displays.scaling
last_seen = "DP-1 — 2560×1440"   # informational label for absent displays in GUI/CLI

[[wallpapers]]
id = "6f1c7a52-…"                # UUID v4
name = "Rain"
path = "/home/user/Videos/rain.webm"   # absolute, canonicalized at add time, UTF-8 (D12)
media_type = "video"             # "video" | "animated-image" | "unknown"  (from extension; informational)
added = 2026-09-30T10:00:00Z     # TOML datetime, UTC
available = true                 # last known existence state (§11); rewritten only when it changes
```

§18's example `[assignments]` table is refined into `[all_displays]` plus
`[displays."<id>"]`, so that each display's scaling sits next to its assignment. The
effective assignment for a present display is `displays[id].wallpaper`, else
`all_displays.wallpaper`, else none.

### 6.3 Parsing, compatibility and writing

**Reading:**

1. Read the file.
2. `toml_edit::DocumentMut::from_str`, keeping the document.
3. Build the typed view by hand from the document, leniently:
   - Unknown keys and tables are ignored (§18).
   - An **unknown enum value** (such as `fps_limit = "144"` written by a newer Lucerna)
     falls back to the default and adds a warning. The original value stays in the
     document, because only fields the user actively changes are rewritten.
   - Values of the wrong type are handled the same way. The warnings go to `GetStatus`,
     `doctor` and the log.
   - A `[[wallpapers]]` entry without a valid `id`, or with a relative `path`, is skipped
     with a warning and preserved in the document.

**`schema_version` outcomes:**

| Case | Behaviour |
| --- | --- |
| File missing | Defaults in memory. The file is created on the first mutation. On first creation only, the autostart entry is enabled (§22 default "autostart = true", D4). |
| `schema_version` missing | Treated as 1, with a warning. Written explicitly on the next save. |
| Equal to `CURRENT_SCHEMA` (1) | Normal. |
| Lower than current | Migrate (§53): parse the old version, copy the original bytes to `config.toml.bak-v<N>-<stamp>`, apply `migrations[N]` through `CURRENT-1` in sequence (each is a pure `fn(&mut DocumentMut) -> Result<()>`), then write atomically. v1 ships an empty migration table. The framework is tested with a test-only v0 → v1 migration. |
| Higher than current (forward) | Load the known fields leniently and mark the config **read-only**: `config_state = "read-only-newer-schema"`. Every mutating D-Bus call returns `ConfigReadOnly` with the message "The configuration was written by a newer Lucerna (schema N). This version will not modify it." Rendering still works. |
| TOML syntax error, or not a table | **Corrupted** (§30 v0.6.0): rename to `config.toml.corrupt-<stamp>` (kept, never deleted), continue with defaults, and set `config_state = "defaults-after-corruption"` with a notice naming the backup file. The notice appears in GetStatus, the GUI banner and `doctor`. |

**Atomic write** (`lucerna_core::fsutil::atomic_write(path, bytes)`):

1. Resolve `path`. If it is a symlink (dotfile managers), write next to the *target*
   instead of replacing the link.
2. Create `.<name>.tmp-<pid>-<nonce>` in the same directory with `O_CREAT | O_EXCL` and
   mode `0600`.
3. `write_all`, then `sync_all`.
4. `rename` over the target.
5. Open the directory and `sync_all` it.
6. On any error, remove the temporary file and return an error. The old file is untouched.

A crash at any point leaves either the old file or the new file, never a zero-byte file
(§18). Leftover `.tmp-*` files are ignored by the loader and removed at the next start.

**Tests:**

- A fault-injection hook (a `#[cfg(test)]` closure run between steps) simulates failure
  after step 3 and after step 4. Assert the target is always a valid config and never
  zero bytes.
- The symlink case.
- Lenient parsing: unknown keys, unknown enum values, wrong types, missing
  `schema_version`, a future schema, and a corrupt file.
- A round trip in which a user-added unknown key and a comment survive a
  `SetSettings` write (§53).

### 6.4 Reconciliation (pure, `lucerna-core::plan`)

```rust
pub struct Desired { pub per_output: BTreeMap<OutputId, DesiredRenderer> }  // only present outputs with an effective, available wallpaper
pub struct DesiredRenderer { pub wallpaper: WallpaperId, pub media: PathBuf, pub scaling: ScalingMode,
                             pub playback: Playback /* Play | Pause(PauseReasons) */, pub geometry: Rect }
pub enum Action { Create(OutputId), Replace(OutputId), Destroy(OutputId), Resize(OutputId, Rect),
                  SetScaling(OutputId, ScalingMode), SetPaused(OutputId, bool) }
pub fn reconcile(desired: &Desired, actual: &BTreeMap<OutputId, ActualRenderer>) -> Vec<Action>;
```

- A change of media, hwdec, fps or audio means `Replace`, because those are launch-time
  flags. A change of scaling is live.
- Actions come out in a deterministic order: Destroy, then Replace, then Create, then the
  live changes.
- Table-driven unit tests cover: hotplug add and remove, identity reappearing, global to
  per-display change, and a missing file producing no renderer plus a
  `Failed(MediaMissing)` status entry.

### 6.5 Missing media (§10, §11)

- At add time: `canonicalize`, check it is a regular file, and reject non-UTF-8 paths
  (D12).
- At render time: if the file is missing, the renderer is `Failed(MediaMissing)` and
  there is no spawn and no restart.
- The library entry's `available` is set to false (written only when it changes).
- While at least one assigned wallpaper is missing, a **60 s** timer calls `stat()` on
  those paths only. When a file comes back, `available=true` and the engine sends
  `Start`. This makes a returning external drive recover automatically, and the timer
  doesn't exist when nothing is missing.
- `RemoveWallpaper` removes only the library entry and any assignments that point to it.
  **It never deletes media** (§10). A test asserts the file still exists afterwards.

### 6.6 Pause policy (pure, `lucerna-core::policy`)

```rust
pub struct PolicyInputs<'a> { pub user_paused: bool, pub session_locked: bool,
                              pub occluded: &'a BTreeSet<OutputId>, pub settings: &'a GeneralSettings,
                              pub fullscreen_capable: bool }
pub struct PauseReasons { pub user: bool, pub lock: bool, pub fullscreen: bool }
pub fn evaluate(output: &OutputId, i: &PolicyInputs) -> PauseReasons;  // paused iff any reason set
```

- `lock` counts only if `pause_on_lock` is on.
- `fullscreen` counts only if `pause_on_fullscreen` is on and the output is in `occluded`.
- Unit tests cover all combinations, and maximized windows never cause a pause.

**Lock detection** (daemon, `session::LockMonitor` trait, v0.5.0). Probed in order, and
the first one that exists wins:

1. `org.cinnamon.ScreenSaver` at `/org/cinnamon/ScreenSaver` on the session bus:
   `GetActive()` plus the `ActiveChanged(b)` signal.
2. `org.freedesktop.ScreenSaver` at `/org/freedesktop/ScreenSaver`: the same pair.
3. logind `LockedHint` property on the system bus, for the session given by
   `XDG_SESSION_ID`. Watched through `PropertiesChanged`.

If none is available, `lock_detection=false` is reported and the limitation documented.
The integration test registers a fake `org.cinnamon.ScreenSaver` on the private test bus.

---

## 7. IPC: D-Bus interface (§19)

### 7.1 Names, single instance, security

| Item | Value |
| --- | --- |
| Bus | Session bus only (`zbus::Connection::session()`) |
| Well-known name | `org.lucerna.Lucerna1` requested with `DoNotQueue`; `Exists` or `InQueue` → already running |
| Object path | `/org/lucerna/Lucerna1` |
| Interface | `org.lucerna.Lucerna1` (+ standard `org.freedesktop.DBus.Properties` / `Introspectable` / `Peer`) |
| Error prefix | `org.lucerna.Lucerna1.Error.` |
| API version | property `ApiVersion` = `1`; additive changes only within `Lucerna1` |

**Single instance** is enforced twice, in this order:

1. A non-blocking `flock` on `$XDG_RUNTIME_DIR/lucerna/daemon.lock` (via `rustix`). This
   covers the per-user runtime directory, so stale recovery can never run against a live
   daemon's children.
2. The D-Bus name.

If either is already held, `lucernad` prints

> Lucerna daemon is already running for this session (PID 1234). Nothing to do.

(the PID comes from the lock file's contents) and **exits 0 cleanly** (§5). There is no
panic and no second engine.

The session bus is per user, and the runtime directory is `0700` and owned by us, so one
user's daemon can't control another user's session (§26). The public API has no
filesystem socket.

### 7.2 Methods

`a{sv}` dictionaries are used everywhere for extensibility. Clients must ignore unknown
keys and must not require keys that were added later.

| Method | In | Out | Semantics | Errors |
| --- | --- | --- | --- | --- |
| `GetStatus` | — | `a{sv}` status | See 7.4 | — |
| `GetDisplays` | — | `aa{sv}` | Present displays **and** configured-but-absent ones (`connected=false`) | `BackendUnavailable` is *not* an error: returns only the absent ones |
| `GetAssignments` | — | `aa{sv}` | One entry per configured display plus `display_id="*"` for all displays | — |
| `ListWallpapers` | — | `aa{sv}` | Library, with live `available` | — |
| `AddWallpaper` | `s path, s name` | `s wallpaper_id` | Canonicalize, validate, add. Idempotent: an existing canonical path returns the existing ID. An empty name means the file stem. | `FileNotFound`, `NotAFile`, `InvalidPath` (relative or non-UTF-8), `ConfigReadOnly`, `ConfigWrite` |
| `RemoveWallpaper` | `s wallpaper_id` | — | Removes the entry and its assignments. Never touches the file. | `UnknownWallpaper`, `ConfigReadOnly`, `ConfigWrite` |
| `SetWallpaper` | `s wallpaper_id, s display_id` | — | `display_id` `"*"` or `""` means all displays. Otherwise a stable ID. Clears a prior user `Stop`. | `UnknownWallpaper`, `UnknownDisplay`, `ConfigReadOnly`, `ConfigWrite` |
| `ClearAssignment` | `s display_id` | — | `"*"` clears the global wallpaper | `UnknownDisplay`, … |
| `SetScaling` | `s display_id, s mode` | — | `fill`, `fit`, `stretch` or `center`. Live. | `InvalidArgument`, `UnknownDisplay` |
| `GetSettings` | — | `a{sv}` | See 7.4 | — |
| `SetSettings` | `a{sv} changes` | — | Partial update, validated as a whole (all or nothing). Unknown key → error. `autostart` writes or removes the autostart file. | `InvalidArgument`, `ConfigReadOnly`, `ConfigWrite`, `AutostartWrite` |
| `Pause` | — | — | Sets the user pause reason on all renderers | — |
| `Resume` | — | — | Clears the user pause reason. Other reasons (lock, fullscreen) still apply. | — |
| `Stop` | — | — | Stops all renderers for the rest of this daemon's lifetime (not persisted). Surfaces are destroyed. | — |
| `Start` | — | — | Clears Stop, resets restart trackers, reconciles | `Unsupported`, `MpvMissing` |
| `Reload` | — | — | Re-read config (with corruption handling), re-probe mpv, re-enumerate outputs, clear Stop, reset trackers, reconcile | `ConfigInvalid` only if the file can't be read at all (I/O) |
| `Quit` | — | — | Clean shutdown (§52), replies before exiting | — |
| `GetDiagnostics` | `b redact` | `s` JSON | The daemon's part of the `doctor` report (§8.3) | — |

Methods not covered by §19's minimum (`ListWallpapers`, `AddWallpaper`,
`RemoveWallpaper`, `ClearAssignment`, `SetScaling`, `Get/SetSettings`, `Start`, `Quit`,
`GetDiagnostics`) are needed because of D2 (the daemon is the only writer).

### 7.3 Properties and signals

- Properties (read-only): `Version s` (workspace version) and `ApiVersion u` (= 1).
- `StatusChanged(a{sv} status)` carries the full GetStatus payload. It is coalesced, at
  most one per 100 ms.
- `DisplaysChanged()`, `LibraryChanged()` and `SettingsChanged()` carry no payload.
  Clients re-fetch.
- `RendererFailed(s display_id, s code, s message)` is sent once per transition into
  Failed.

### 7.4 Dictionary keys

**status** contains:

| Key | Type | Values |
| --- | --- | --- |
| `daemon_version` | `s` | |
| `api_version` | `u` | |
| `backend` | `s` | `cinnamon-x11`, `x11-ewmh` or `none` |
| `session_type` | `s` | `x11`, `wayland`, `tty`, `unknown` |
| `supported` | `b` | |
| `unsupported_reason` | `s` | `""`, `wayland`, `no-display`, `x11-connect-failed`, `missing-feature` or `runtime-dir-unavailable` |
| `unsupported_message` | `s` | |
| `playback` | `s` | `playing`, `paused`, `stopped`, `idle` or `failed` (aggregate) |
| `user_paused` | `b` | |
| `user_stopped` | `b` | |
| `session_locked` | `b` | |
| `lock_detection` | `b` | |
| `fullscreen_detection` | `b` | |
| `mpv_available` | `b` | |
| `mpv_version` | `s` | |
| `schema_version` | `u` | |
| `config_state` | `s` | `ok`, `defaults-after-corruption` or `read-only-newer-schema` |
| `config_notice` | `s` | |
| `config_warnings` | `as` | |
| `renderers` | `aa{sv}` | See below |

Each renderer entry contains:

| Key | Type | Values |
| --- | --- | --- |
| `display_id` | `s` | |
| `connector` | `s` | |
| `wallpaper_id` | `s` | |
| `state` | `s` | `stopped`, `starting`, `playing`, `paused`, `stopping` or `failed` |
| `pause_reasons` | `as` | subset of `user`, `lock`, `fullscreen` |
| `failure_code` | `s` | |
| `failure_message` | `s` | |
| `restarts_in_window` | `u` | |
| `pid` | `u` | 0 if none |
| `scaling` | `s` | |

**display** contains `id s`, `connector s`, `label s` (for example
"eDP-1 — 1920×1080 — Primary", §23), `connected b`, `primary b`, `x i`, `y i`,
`width u`, `height u`, `rotation s` (`normal`, `left`, `inverted` or `right`),
`edid_manufacturer s`, `edid_model s` and `edid_serial s` (redacted to `""` over D-Bus
unless a diagnostics call asks for it), `wallpaper_id s` (effective), `wallpaper_source s`
(`display`, `all` or `none`) and `scaling s`.

**wallpaper** contains `id s`, `name s`, `path s`, `media_type s`, `available b`,
`added s` (RFC 3339) and, when known, `duration_ms t`, `width u`, `height u` and
`codec s`.

**settings** contains `pause_on_fullscreen b`, `pause_on_lock b`, `audio b`,
`hardware_decode s`, `fps_limit s`, `stacking s`, `max_restarts u`,
`restart_window_secs u` and `autostart b` (derived from the file, D4).

### 7.5 Error mapping

Implemented in `lucerna-ipc::LucernaError` with `#[derive(zbus::DBusError)]`. Each
variant carries a complete §49 message.

| D-Bus error (`org.lucerna.Lucerna1.Error.*`) | `lucernactl` exit code | Example message |
| --- | --- | --- |
| *(no reply: name has no owner)* | **3** | "The Lucerna daemon is not running. Start it with `lucernad &` or open Lucerna." |
| `UnknownWallpaper`, `UnknownDisplay` | 4 | "No display matches 'HDMI-9'. Run `lucernactl monitors` to list displays." |
| `InvalidArgument`, `InvalidPath`, `FileNotFound`, `NotAFile` | 4 | "The file '/x/y.mp4' does not exist." |
| `ConfigReadOnly`, `ConfigWrite`, `ConfigInvalid`, `AutostartWrite` | 6 | "Lucerna could not save its configuration to …: Permission denied." |
| `Unsupported`, `BackendUnavailable` | 5 | The Wayland or `DISPLAY` messages from §5.1 |
| `MpvMissing` | 7 | The §4.7 message |
| `Internal` (+ `org.freedesktop.DBus.Error.*`) | 1 | "Unexpected daemon error: …" |

Clap usage errors exit with 2.

### 7.6 Runtime model

- **Daemon:** a tokio `current_thread` runtime runs the **`Engine` actor**. It is the
  only owner of config, library, backend, renderers and policy inputs, and it
  processes `EngineMsg`s one at a time: D-Bus commands with a `oneshot` reply, backend
  events, supervisor state changes, timers, signals and lock events. The zbus interface
  methods only translate arguments, send an `EngineMsg` and await the reply. zbus runs
  its own internal executor thread; tokio's `mpsc` and `oneshot` work across executors.
  Signals are emitted through the interface's `SignalEmitter`.
- **Supervisors:** one `RendererSupervisor` task per output owns an `MpvProcess` and an
  `MpvIpc`, and sends `SupervisorEvent`s to the engine.
- **CLI:** `zbus::blocking` proxy. No async runtime.
- **GUI:** the zbus async proxy is awaited on the glib main context
  (`glib::MainContext::spawn_local`). zbus without the `tokio` feature is independent of
  the runtime, which is exactly why that feature is banned (§1.3).

---

## 8. CLI, GUI, diagnostics, autostart, lifecycle

### 8.1 `lucernactl` (§20)

```text
lucernactl status        [--json]        exit 0 running | 3 not running
lucernactl monitors      [--json]        (alias: displays)
lucernactl wallpapers    [--json]
lucernactl play <path> [--monitor <id|connector>]   AddWallpaper + SetWallpaper
lucernactl pause | resume | stop [--daemon] | reload
lucernactl doctor [--json] [--redact]    works with or without a daemon; exit 0 when a report is produced
lucernactl --version | --help
```

- `--monitor` accepts a stable ID, or a connector name (`HDMI-1`) that matches exactly
  one present display. The CLI resolves it through `GetDisplays`.
- `--json` prints one JSON document to stdout. Human-readable text goes to stdout and
  errors to stderr.
- Unit tests use `Cli::try_parse_from` for every command, and exit-code mapping tests use
  the §7.5 table.

### 8.2 `lucerna` (GTK 4, §23, §24, §34)

**Startup order:**

1. Parse with clap: `--version`, `--help`, `--verbose`. Neither parse touches GTK.
2. If both `DISPLAY` and `WAYLAND_DISPLAY` are unset, print the §24 message and exit 1.
3. `gtk::init()`. If it fails, print the same message plus the GTK error and exit 1.
4. `gtk::Application::new("org.lucerna.Lucerna", NON_UNIQUE=false)`, then
   `run_with_args(&[argv0])` so GTK never re-parses our arguments.

**Pages:** a `gtk::StackSidebar` plus a `gtk::Stack` (§23). A banner sits across the top:
a `gtk::Revealer` with a label and an action button, showing daemon status and errors.

| Page | Contents |
| --- | --- |
| **Wallpapers** | A `ListBox` of entries (name, path, a "Missing" badge). Actions: *Add…* (`gtk::FileDialog` filtered to `video/*` + `image/gif` + All files), *Remove from library* (with a confirmation that says the file will not be deleted), *Set on all displays*, *Stop wallpaper*. |
| **Displays** | An *All displays* row (a wallpaper `DropDown` plus a scaling `DropDown`). One row per display, labelled like "HDMI-1 — 1920×1080" or "eDP-1 — 1920×1080 — Primary". The wallpaper choice includes "Same as all displays". Absent displays appear with "(disconnected)". |
| **Settings** | The §23 switches and dropdowns: autostart, pause when fullscreen, hardware decoding, FPS limit, and audio. Turning audio on needs an explicit confirmation (§16). Pause on lock, and an Advanced expander holding stacking mode. |
| **About** | Name, `env!("CARGO_PKG_VERSION")` (the workspace version), description, MIT, repository URL, and the runtime backend from GetStatus. |

**Daemon control:**

- If `org.lucerna.Lucerna1` has no owner, the banner shows "The Lucerna service is not
  running" with a **Start** button. The GUI also starts it automatically on launch (D3).
- To start it: find `lucernad` next to `current_exe()`, else on `PATH`, and spawn it with
  `process_group(0)` and null stdio, so it outlives the GUI (§5). Then wait up to 5 s for
  `NameOwnerChanged`.
- The banner also offers Pause, Resume and Stop, and **Quit service** (`Quit`).
- Closing the window never stops the daemon.

**Structure:**

- `presenter/`: pure functions turning DTOs into row models and labels, and parsing user
  input. Unit tested with no GTK.
- `strings.rs`: every user-visible string, ready for gettext later.
- `pages/*.rs`: widgets only. No module is longer than about 400 lines.

**Structural test (`lucerna-ui/tests/ui_structure.rs`, `harness=false`):**

- It uses its own `main`, because GTK must stay on one thread. It starts Xvfb through the
  testkit and constructs the main window without presenting it, then asserts the stack
  has the pages `wallpapers`, `displays`, `settings` and `about`, and the settings
  widgets exist.
- **This is structural only.** Visual quality is reported as
  `NOT VALIDATED ON DEVELOPMENT SERVER` (§30 v0.4.0).

### 8.3 `doctor` (§21)

The report model is `lucerna_core::doctor::Report`, with `report_version: 1`.

**Collected locally by the CLI:**

- `lucernactl` version and paths (config, state, cache, runtime)
- `schema_version` read from `config.toml`, without writing anything
- An **allow-list** of environment variables: `XDG_SESSION_TYPE`, `XDG_CURRENT_DESKTOP`,
  `DESKTOP_SESSION`, `DISPLAY`, `WAYLAND_DISPLAY` (presence only),
  `DBUS_SESSION_BUS_ADDRESS` (presence only), `XDG_RUNTIME_DIR`, `LUCERNA_MPV`,
  `LUCERNA_LOG`, `RUST_LOG`. The environment is **never dumped whole** (§21).
- mpv path, version and option compatibility
- Autostart state (file present, enabled, `Exec` target)
- **If the daemon isn't reachable:** a read-only X11 probe through `lucerna_x11::probe`.
  It covers connection status, server vendor, release and protocol, RandR version,
  outputs and identities, WM name, compositor, Nemo detection, and any existing
  `_LUCERNA_WALLPAPER` windows.

**From the daemon via `GetDiagnostics`:**

- daemon version, active backend, capabilities
- renderer states, the last 20 failures from `state.json`
- the backend's `diagnostics()` (surfaces, stacking, Nemo, restack fights)
- config state, notices and warnings

**`--redact`:**

- The `$HOME` prefix becomes `~`, the user name becomes `<user>` and the hostname
  becomes `<host>`.
- Wallpaper paths become `<media-N>.<ext>`.
- EDID serials are removed.
- Unit tests check the redaction with a fixture report.

**Output:** readable text with ✓/✗/! markers per section, or `--json`.

### 8.4 Autostart (§22, D4)

- **One mechanism:** the per-user XDG autostart file
  `$XDG_CONFIG_HOME/autostart/org.lucerna.Lucerna.Daemon.desktop`, rendered from the
  embedded template:

  ```ini
  [Desktop Entry]
  Type=Application
  Name=Lucerna wallpaper service
  Comment=Restores animated wallpapers after login
  Exec=<absolute path of the running lucernad, e.g. /usr/bin/lucernad>
  TryExec=<same path>
  Icon=org.lucerna.Lucerna
  Terminal=false
  X-GNOME-Autostart-enabled=true
  X-Lucerna-Managed=true
  ```

  `TryExec` makes the entry inert if the package is uninstalled.
- **Enabled** means the file exists, `Hidden` isn't true and
  `X-GNOME-Autostart-enabled` isn't false.
- **Disable** deletes our file. If the user has turned it into a `Hidden=true` copy, it's
  left alone and reported as disabled.
- **Enable** writes the file atomically.
- No `/etc/xdg/autostart` file is shipped, no systemd user unit is created, and there is
  no D-Bus activation. That guarantees that "disabled" really means the daemon won't
  relaunch (§22).
- **Login race** (§22): at start, the daemon waits up to 10 s for `DISPLAY`. It doesn't
  need `DBUS_SESSION_BUS_ADDRESS`, because zbus falls back to
  `unix:path=$XDG_RUNTIME_DIR/bus`. If neither is usable it exits 1 with a readable
  message. It also waits up to 10 s for `_NET_SUPPORTING_WM_CHECK` before creating
  surfaces. The restack watchdog then handles `nemo-desktop` starting later.

### 8.5 Daemon lifecycle and clean shutdown (§52)

**Startup:**

1. Parse arguments (`--version`, `--help`, `-v`).
2. Initialise logging.
3. Resolve and verify paths.
4. Take the flock.
5. Connect to the session bus and take the name.
6. Recover stale processes and remove stale sockets.
7. Load config and `state.json`.
8. Discover mpv.
9. Classify the session and select a backend.
10. Serve the D-Bus interface.
11. Enumerate outputs and reconcile.
12. Enter the engine loop.

**Shutdown triggers:** SIGTERM, SIGINT, SIGHUP (the session ending), `Quit`, backend
`ConnectionLost` (logout: the X server has gone), and loss of the session bus.

**Shutdown sequence:**

1. Stop every renderer with the §4.4 escalation, in parallel, 6 s at most.
2. `backend.shutdown()`, which destroys the windows.
3. Remove the sockets and `renderers.json`.
4. Flush `state.json`.
5. Drop the D-Bus connection, which releases the name.
6. Release the flock.
7. Exit 0.

**Tests:** `sigterm_cleans_up` and `quit_cleans_up` assert that after exit, no fake-mpv
PIDs are alive, the runtime directory holds no sockets or registry, and the name has no
owner.

---

## 9. Test strategy (§35)

The four classes, where each lives and how it runs:

| Class | Location | Runs in | Needs |
| --- | --- | --- | --- |
| **Unit** | `#[cfg(test)]` in each crate | `cargo test --workspace` | nothing |
| **Integration** | `crates/lucerna-testkit/tests/*.rs`, plus `crates/<bin-crate>/tests/headless.rs` | `cargo test --workspace` | `dbus-daemon`, fake-mpv (built automatically), optionally real mpv |
| **X11 protocol** | `crates/lucerna-testkit/tests/x11_protocol.rs`, `crates/lucerna-ui/tests/ui_structure.rs` | `cargo test --workspace` | `Xvfb` (required in CI via `LUCERNA_REQUIRE_XVFB=1`) |
| **Packaging** | `scripts/smoke-test-package.sh`, run in fresh containers | `packages.yml`, locally with Docker | Docker / Actions containers |

### 9.1 Unit coverage (§35 list → module)

| Area | Module |
| --- | --- |
| Configuration parsing | `core::config` |
| Schema migration | `core::config::migrate` |
| Renderer state transitions | `core::renderer` (full state × event table) |
| Restart policy | `core::renderer::restart` |
| mpv command generation | `core::mpv::args` (every settings combination, the malicious-filename case) |
| Monitor identifiers | `core::identity` (EDID fixtures) |
| Path handling | `core::paths`, `core::fsutil` (atomic write, symlink, runtime-dir checks, socket-length bound) |
| Policy decisions | `core::policy`, `core::plan`, `core::geometry::occluded_outputs` |
| CLI parsing | `cli::args` |

Also covered by unit tests: DTO ↔ `a{sv}` round trips (`ipc`), presenters (`ui`),
redaction (`core::doctor`), autostart file handling (`core::autostart`), `BoundedLog`,
and `session::classify`.

### 9.2 The fake-mpv technique

`crates/lucerna-testkit/src/bin/fake-mpv.rs` is a small Rust program that stands in for
mpv at the process and IPC level.

- **Argument parsing:** it accepts mpv-style `--opt=value` arguments up to `--`, then a
  media path. It **fails with exit 1** on any option not in
  `lucerna_core::mpv::required_options()`, which catches drift in the argument builder.
- **Behaviour** comes from the media file's contents, not from environment variables,
  so tests running in parallel never share state. A first line
  `FAKE-MPV: <directive>[; <directive>…]` selects it. Directives:

  | Directive | Behaviour |
  | --- | --- |
  | `play` (default) | Create the IPC socket, emit `file-loaded`, serve commands until `quit` → exit 0. |
  | `crash-after=<ms>` | Abort with SIGABRT after the delay. |
  | `crash-first=<n>` | Crash on the first n launches (counted in `<media>.count`), then play. |
  | `exit=<code>[@<ms>]` | Exit with the code. |
  | `unsupported` | Emit `end-file` with `reason=error` and `file_error="unrecognized file format"`, exit 2. |
  | `no-ipc` | Never create the socket. |
  | `hang-before-load` | Socket but no `file-loaded`. |
  | `ipc-drop-after=<ms>` | Close the socket but keep running. |
  | `ignore-quit` / `ignore-term` | Exercise SIGTERM and SIGKILL escalation. |
  | `stderr-flood=<bytes>` | Write that much to stderr. |

- **Recording:** each launch appends a JSON line to `<media>.log` containing its argv,
  PID and every IPC command it received. Tests assert against that file, for example
  that `--wid` was passed, that `pause` was set to true, and that `quit` was received.

The testkit's `tests/` directory lives in the same package as the `fake-mpv` bin, so
`env!("CARGO_BIN_EXE_fake-mpv")` resolves reliably. The same goes for
`lucernad-under-test` and `lucernactl-under-test`.

### 9.3 Integration suites (§35 list → test)

| §35 item | Test(s) |
| --- | --- |
| daemon/CLI D-Bus communication | `cli_e2e.rs`: private `TestBus` plus the in-process engine with `FakeBackend` and fake-mpv. `lucernactl-under-test` is run with `DBUS_SESSION_BUS_ADDRESS` set on the *child* only. Covers status, monitors, wallpapers, play, pause, resume, stop, reload, `--json` shape, and exit codes 0/3/4. |
| renderer subprocess supervision | `renderer_supervision.rs`: play, pause and resume via IPC; stop escalation (`ignore-quit`, `ignore-term`); startup timeout (`no-ipc`, `hang-before-load`); `ipc-drop-after` |
| fake mpv process | All of the above. `argv_matches_builder` |
| crashed renderer | `restart_storm_is_bounded`, `crash_first_2_recovers` |
| stale runtime socket | `stale_socket_is_replaced` (a pre-created regular file and a dead socket at the target path); `stale_process_recovery` (§4.6) |
| duplicate daemon launch | `duplicate_daemon_exits_cleanly`: two `lucernad-under-test` on one TestBus with separate runtime dirs (tests the name), then the same runtime dir (tests the flock). The second exits 0 with the message and the first is unaffected. |
| invalid configuration | `corrupt_config_moved_aside`, `future_schema_read_only`, `unknown_keys_preserved` |
| missing media | `missing_media_no_spawn_then_recovers` (the file is created later and the 60 s timer is shortened through `Clock`) |
| configuration reload | `reload_applies_external_edit` |
| lock pause | `screensaver_active_pauses` (fake `org.cinnamon.ScreenSaver` on the TestBus) |
| hotplug and absent display | `display_absent_keeps_assignment_and_restores` (`FakeBackend::set_outputs` + `OutputsChanged`) |
| fullscreen per monitor | `fullscreen_pauses_only_occluded_output` |
| clean shutdown | `sigterm_cleans_up`, `quit_cleans_up` |
| **real mpv** (`real_mpv.rs`) | `options_supported`; `plays_{mp4,webm,mkv,gif}` (`--vo=null` with no `--wid`: reaches Ready, pause and resume work, quit gives exit 0); `corrupt_is_media_unsupported` (exit 2 means no retry); `fps_filter_accepted`. Skipped without mpv unless `LUCERNA_REQUIRE_MPV=1`. |
| **headless binaries** (§34) | `crates/{lucerna-ui,lucerna-daemon,lucerna-cli}/tests/headless.rs`: `CARGO_BIN_EXE_*` with `DISPLAY` and `WAYLAND_DISPLAY` removed *from the child's environment*. `--help` and `--version` exit 0 and print `<name> <workspace version>`. `lucerna` with no args prints the §24 message, exits 1, and never panics (asserts that stderr doesn't contain "panicked"). |

**`TestBus`:**

- Spawns `dbus-daemon --session --nofork --print-address=1` with a temporary
  `--config-file` that sets `<listen>unix:dir=<tmp></listen>`.
- Reads the address and kills the daemon on `Drop`.
- If `dbus-daemon` is missing, the tests skip unless `LUCERNA_REQUIRE_DBUS=1` (set in CI).

**Fixtures:**

- `tests/fixtures/media/` holds about 1 s, 64×64 synthetic test-pattern clips:
  `sample.mp4` (H.264), `sample.webm` (VP9), `sample.mkv` (H.264), `sample.gif`, and
  `corrupt.mp4` (random bytes).
- `scripts/gen-fixtures.sh` regenerates them from lavfi `testsrc` using ffmpeg or mpv
  encoding mode. Their provenance and CC0 status are recorded in the fixtures README.
- They total under 150 KB and are committed. The runtime never needs ffmpeg (§11).

### 9.4 Traceability

`docs/TEST-MATRIX.md` is created in v0.3.0 and completed in v0.9.0. It maps every
testable directive requirement to its test, and every desktop-only requirement to its
`LUC-Txx` with the status `IMPLEMENTED — MANUAL DESKTOP VALIDATION REQUIRED`.

---

## 10. Milestones (§30) with gates (§47)

### 10.1 Rules that apply to every milestone

- **Branch:** `feat/<milestone-topic>`, or `ci/…` / `fix/…`. Use several coherent
  Conventional Commits (§28). Merge into `main` with `--no-ff` once the gates pass on
  the branch. Never force-push `main` and never rewrite a tag.
- **Version bump:** the final commit before tagging is
  `chore(release): X.Y.Z`. It edits only `[workspace.package] version` in the root
  `Cargo.toml`, runs `cargo update --workspace` so the lockfile's workspace entries
  follow, and moves the `[Unreleased]` changelog entries under `## [X.Y.Z] - YYYY-MM-DD`.
- **Base gates (G0)** must pass locally on `main` after the merge. Record the exact
  commands and results in the milestone summary.

  ```text
  cargo fmt --check
  cargo clippy --workspace --all-targets -- -D warnings
  cargo test --workspace          # with LUCERNA_REQUIRE_XVFB=1 LUCERNA_REQUIRE_DBUS=1 LUCERNA_REQUIRE_MPV=1 locally too
  cargo build --workspace --release
  ```

- **Tag:** `git tag -a vX.Y.Z -m "Lucerna vX.Y.Z"`, then `git push origin main vX.Y.Z`.
  Then wait for `ci.yml` to go green on the tag commit. From v0.8.0 on, `release.yml`
  must also succeed.
- **Honesty:** each milestone summary lists every desktop-only behaviour touched in that
  milestone as `IMPLEMENTED — MANUAL DESKTOP VALIDATION REQUIRED`.

### v0.0.1 — Repository Bootstrap

**Deliverables:**

- Everything in §1.1 that is structural:
  - the root `Cargo.toml` with `[workspace.package]` (version `0.0.1`, edition 2024,
    `rust-version = "1.98"`, license MIT, repository
    `https://github.com/blindicide/lucerna`, `publish = false`), `resolver = "3"`,
    `[workspace.dependencies]` and `[workspace.lints]`;
  - `rust-toolchain.toml`, `rustfmt.toml`, `clippy.toml`, `deny.toml`;
  - LICENSE (D15), README, CHANGELOG (`[Unreleased]` + `[0.0.1]`), `docs/BUILDING.md`,
    and a skeleton `docs/ARCHITECTURE.md` that includes §1.
- All eight crates, compiling:
  - core holds `logging`, `paths` and `version`;
  - the three binaries parse `--version` and `--help` with clap `#[command(version)]`
    (version = `CARGO_PKG_VERSION` = the workspace version);
  - `lucerna` does its display check before GTK and creates an empty
    `gtk::ApplicationWindow` only after parsing;
  - `lucernad` initialises logging and exits.
- `testkit/tests/architecture.rs` and the per-binary `tests/headless.rs`.
- `ci.yml` (§11.3 jobs `fmt`, `clippy`, `test`, `build-release`, `headless-smoke`,
  `deny`, `scripts`).
- **Delete `.github/workflows/probe.yml`.**
- `scripts/version.sh` and its test.

**Exit:** G0 green, and CI green on the tag.

### v0.1.0 — Renderer Core

**Deliverables:**

- core: the `renderer` state machine and `RestartTracker`, `mpv::{args, discovery,
  compat}`, `BoundedLog`, and the runtime-dir and socket-path helpers.
- lucerna-mpv: `MpvProcess`, `MpvIpc`, `RendererSupervisor`, `PidRegistry`, stale
  recovery, `PDEATHSIG`.
- testkit: `fake-mpv`, `renderer_supervision.rs`, `real_mpv.rs`, plus the media fixtures
  and `scripts/gen-fixtures.sh`.
- `docs/ARCHITECTURE.md` gains its renderer section, with the full argv table and the
  rationale from §4.

**Exit:**

- Unit tests cover every cell of the transition table.
- Integration tests pass for crash, bounded restart, hang, unsupported file, missing mpv,
  stale socket, stale process, and the log bound.
- Real-mpv tests pass on mpv 0.37.

### v0.2.0 — Cinnamon/X11 Backend

**Deliverables:**

- core: the `backend` trait and types, `identity` (EDID), `geometry`, and `FakeBackend`.
- lucerna-x11: connect, probe, enumerate (RandR 1.5 plus the 1.2 fallback), surfaces with
  every §5.2 primitive, restack plus rate limiting, the event thread (RandR, stacking,
  fullscreen), Cinnamon/Nemo/compositor probes, `diagnostics()`, and the read-only
  `probe` functions for `doctor`.
- testkit: the Xvfb harness and `x11_protocol.rs` (all 13 tests).
- `docs/X11-CINNAMON-NOTES.md`.
- The first full `docs/MANUAL-ACCEPTANCE.md`: LUC-T01 to LUC-T20 from §45, verbatim, plus
  per-test "diagnostics to attach" (the `lucernactl doctor --json --redact` output) and
  the stacking-mode switch procedure. Every result is `NOT RUN — REQUIRES REAL DESKTOP`.

**Exit:** the protocol suite passes under Xvfb. The milestone summary states: "desktop
appearance: MANUAL VALIDATION REQUIRED."

### v0.3.0 — Daemon and CLI

**Deliverables:**

- core: `config` (schema v1, lenient parsing, `toml_edit` writes, atomic write, the
  migration framework, forward-compatible read-only, corruption move-aside), the
  `library`, `state.json`, `plan::reconcile`, `session::classify`, `doctor` plus
  redaction, `autostart` (the file functions only; the GUI toggle arrives in v0.4.0 and
  lifecycle polish in v0.6.0).
- lucerna-ipc: the whole of §7 (constants, DTOs, errors, proxy).
- lucerna-daemon: the full startup and shutdown of §8.5, flock plus name, the `Engine`
  actor, every §7 method and signal, and backend selection.
  - **Assignments are global only in this milestone:** `SetWallpaper(id, "*")`. A
    specific `display_id` returns `InvalidArgument("per-display assignment arrives in
    0.5.0")`. The interface signature is already final.
  - Missing-media detection (without the recheck timer), and `Pause`/`Resume`/`Stop`/
    `Start`/`Reload`/`Quit`.
- lucerna-cli: every command, `--json`, exit codes, `doctor` (local plus daemon), and
  `--redact`.
- testkit: `TestBus`, `cli_e2e.rs`, the daemon integration tests (duplicate launch,
  invalid config, missing media, reload, stale socket, clean shutdown), and the wrapper
  binaries.
- Docs: `IPC.md` (full), `CONFIGURATION.md` (full), `TROUBLESHOOTING.md` (first pass,
  all §44 topics), `TEST-MATRIX.md` (initial).

**Exit:** G0 green, and CLI↔daemon end-to-end tests pass over the private bus.

### v0.4.0 — GTK Control Application

**Deliverables:**

- The §8.2 GUI: the four pages, the file dialog, library add/remove with missing badges,
  the global display assignment, settings including the autostart toggle via
  `SetSettings`, About, daemon start/stop/pause/resume/quit, the error banner, and
  signal-driven refresh.
- `presenter/` unit tests and `ui_structure.rs`.

**Exit:** G0 green. The summary states: "visual quality: NOT VALIDATED ON DEVELOPMENT
SERVER."

### v0.5.0 — Multi-Monitor and Policy Engine

**Deliverables:**

- Per-display assignment enabled throughout: D-Bus, CLI `--monitor`, the GUI's per-row
  dropdowns.
- Absent displays are kept (`connected=false`) and restored automatically.
- Hotplug: `OutputsChanged` → re-enumerate → reconcile (resize, create, destroy).
- Scaling modes, live via IPC.
- Fullscreen pause per monitor (§5.4 plus `policy`), and the `LockMonitor` with its three
  probes.
- Integration tests: hotplug, absent display restore, per-monitor fullscreen, lock pause.
  The corresponding protocol tests are `fullscreen_detection` and `hotplug_event`.

**Exit:** G0 green. Per-monitor fullscreen is documented, along with the EWMH-only
limitation (D13).

### v0.6.0 — Desktop Lifecycle

**Deliverables:**

- Autostart finished: created on first config creation; the login waits for `DISPLAY`
  and the WM; `TryExec`.
- Logout cleanup (`ConnectionLost`, SIGHUP).
- Restart policy exposed to users (`restarts_in_window`, `RendererFailed`, GUI and CLI
  messages, recovery through Start or Reload).
- Corrupted-config handling surfaced in GUI, CLI and doctor.
- The 60 s missing-media recheck.
- mpv missing/error UX end to end (§49 messages).
- Daemon `BoundedLog`, and a logging review against the §25 event list (nothing logged
  per frame or per poll).
- Crash recovery end to end, including stale recovery after a `kill -9` of the daemon
  (a test kills `lucernad-under-test` with SIGKILL, starts a new one, and asserts the old
  fake-mpv children are reaped and the new ones are running).

**Exit:** G0 green, plus all lifecycle integration tests.

### v0.7.0 — Native Packaging

**Deliverables:**

- `assets/`: the desktop file, validated with `desktop-file-validate`; the metainfo
  (`appstreamcli validate --no-net`, or `appstream-util validate-relax` on Fedora); and
  the SVG icon.
- `packaging/debian/*` and `packaging/rpm/lucerna.spec` (§11.1, §11.2).
- The scripts `build-deb.sh`, `build-rpm.sh`, `make-source-archive.sh` and
  `smoke-test-package.sh`.
- `docs/PACKAGING.md`.
- `packages.yml`, runnable on `workflow_dispatch` and on push. Its release-reuse parts are
  finished in v0.8.0.

**Gates (G0 plus):**

- Build the DEB in Docker `ubuntu:24.04` and the RPM in Docker `fedora:<N>`, locally.
- Inspect them: `dpkg-deb -I/-c`, `lintian --fail-on error`, `rpm -qpi/-qpl`, and an
  informational `rpmlint`.
- Run `smoke-test-package.sh` in fresh containers of each distro (§11.4).
- `packages.yml` is green on the branch.

**Exit:** both packages install and remove cleanly, with user config preserved.

### v0.8.0 — Automated Distribution

**Deliverables:**

- `packages.yml` gains `workflow_call` inputs.
- `release.yml` implements all 12 steps of §40 (§11.5).
- `scripts/check-tag.sh`: annotated tag, and tag equals version.
- `scripts/changelog-section.sh`, and SHA256SUMS generation.
- Script tests in `tests/scripts/`: version mapping, tag-mismatch rejection (`v0.8.0` vs
  `0.7.0` must fail), a lightweight tag being rejected, and changelog extraction.

**Gates:** G0 plus the v0.7.0 gates. The workflow definitions are complete (§47). **The
`v0.8.0` tag itself is the first real release run** and has to produce
`lucerna_0.8.0_amd64.deb`, `lucerna-0.8.0-1.x86_64.rpm`, `lucerna-0.8.0.tar.gz` and
`SHA256SUMS` on a GitHub Release.

**Exit:** the release for v0.8.0 exists with exactly those four assets, and
`sha256sum -c` passes on the downloaded files.

### v0.9.0 — Release Candidate Baseline

**Deliverables:**

- A documentation pass over README (limitations, status) and every `docs/` file.
- `doctor` finalised (field review against §21).
- `TEST-MATRIX.md` complete, and gaps closed.
- Packaging finalised.
- `MANUAL-ACCEPTANCE.md` **frozen** (the header gets "Frozen at v0.9.0").
- A known-defects review that leaves no open server-testable critical defects.
- `cargo deny` clean, or each advisory exception justified in `deny.toml` and
  `docs/BUILDING.md` (§33).

**Gates:** everything above, and the release workflow is green for v0.9.0.

### v1.0.0-rc.1 — Desktop Acceptance Candidate

**Deliverables:**

- Version `1.0.0-rc.1`, a CHANGELOG section, tag `v1.0.0-rc.1`.
- The release pipeline produces `lucerna_1.0.0~rc1_amd64.deb`,
  `lucerna-1.0.0-0.rc1.x86_64.rpm`, `lucerna-1.0.0-rc.1.tar.gz` and `SHA256SUMS`,
  published as a GitHub **prerelease**.
- The §55 final report, stating: "NOT PERFORMED BY DEVELOPMENT AGENT. REQUIRES REAL
  CINNAMON/X11 DESKTOP." The next action is to install the package on the Mint machine
  and run `docs/MANUAL-ACCEPTANCE.md`.
- **Then stop. Do not tag v1.0.0.**

### 10.2 Definition-of-done traceability (§57)

| §57 item | Delivered in |
| --- | --- |
| 1–6 (committed, clean, tests, fmt, clippy, release build) | G0 on every milestone |
| 7 X11 backend | 0.2.0 (+0.5.0) |
| 8 robust mpv management | 0.1.0, 0.6.0 |
| 9 D-Bus daemon/CLI | 0.3.0 |
| 10 GTK GUI | 0.4.0 |
| 11 multi-monitor | 0.5.0 |
| 12 fullscreen pause | 0.5.0 |
| 13 autostart | 0.3.0 file functions, 0.4.0 toggle, 0.6.0 lifecycle |
| 14 diagnostics | 0.3.0, finalised 0.9.0 |
| 15–17 deb/rpm via Actions + smoke | 0.7.0, 0.8.0 |
| 18–20 release workflow, SemVer enforcement, checksums | 0.8.0 |
| 21 docs | continuous, 0.9.0 |
| 22 rc.1 artifacts | 1.0.0-rc.1 |
| 23 visual tests pending, not fabricated | `MANUAL-ACCEPTANCE.md` from 0.2.0, frozen 0.9.0 |

---

## 11. Packaging and CI (§29, §31, §33, §36–§41)

### 11.1 Version: one canonical source

`scripts/version.sh [--semver|--deb|--rpm-version|--rpm-release|--tarball-stem]` reads
the workspace version with
`cargo metadata --no-deps --format-version 1 | jq -r '.packages[]|select(.name=="lucerna-core")|.version'`.
Every crate inherits the workspace version, so any member would do.

| SemVer (Cargo) | `--deb` | `--rpm-version` / `--rpm-release` | tarball |
| --- | --- | --- | --- |
| `0.8.0` | `0.8.0` | `0.8.0` / `1` | `lucerna-0.8.0.tar.gz` |
| `1.0.0-rc.1` | `1.0.0~rc1` | `1.0.0` / `0.rc1` | `lucerna-1.0.0-rc.1.tar.gz` |
| `1.0.0` | `1.0.0` | `1.0.0` / `1` | `lucerna-1.0.0.tar.gz` |
| anything else with a pre-release (`-beta.1`, …) | **error, exit 1** | **error** | — |

These mappings are tested in `tests/scripts/version_test.sh`, which CI's `scripts` job
runs.

No version is hardcoded anywhere else:

- the binaries and the About page use `CARGO_PKG_VERSION`;
- `debian/changelog` is **generated** at build time with a single entry for `--deb`
  (`packaging/debian/` has no changelog file);
- the RPM spec uses `Version: %{lucerna_version}` and `Release: %{lucerna_release}`,
  passed with `rpmbuild --define`, and its `%changelog` is appended at build time from a
  generated entry;
- the metainfo `<release>` entry is generated from the version during the package build.

### 11.2 Package layout (the same logical files for DEB and RPM)

```text
/usr/bin/lucerna
/usr/bin/lucernad
/usr/bin/lucernactl
/usr/share/applications/org.lucerna.Lucerna.desktop
/usr/share/icons/hicolor/scalable/apps/org.lucerna.Lucerna.svg
/usr/share/metainfo/org.lucerna.Lucerna.metainfo.xml
/usr/share/doc/lucerna/{README.md,CHANGELOG.md,MANUAL-ACCEPTANCE.md,TROUBLESHOOTING.md}   (RPM: %doc)
/usr/share/licenses/lucerna/LICENSE (RPM %license) | /usr/share/doc/lucerna/copyright (DEB)
```

The packages ship no `/etc/xdg/autostart` file, no D-Bus service file and no maintainer
scripts that touch `$HOME`. Uninstalling never removes `~/.config/lucerna`, the user's
autostart file (which `TryExec` makes inert) or any media (§37, §38).

**Debian** (`packaging/debian/`):

- `source/format` = `3.0 (native)` (D11).
- `control`:
  - Build-Depends: `debhelper-compat (= 13), pkgconf, libgtk-4-dev (>= 4.10), jq`. The
    Rust toolchain comes from `rust-toolchain.toml` via rustup (D9), which is documented.
  - Package `lucerna`, Architecture `amd64`.
  - Depends: `${shlibs:Depends}, ${misc:Depends}, mpv`.
  - Recommends: `default-dbus-session-bus | dbus-session-bus`.
- `rules` (dh):
  - `override_dh_auto_build` runs `cargo build --release --locked`.
  - `override_dh_auto_install` runs `install -Dm755` / `install -Dm644` for the files
    above.
  - `override_dh_auto_test` is empty, with a comment that tests run in CI.
  - `dh_shlibdeps` computes the libgtk-4-1 dependency and others.

**RPM** (`packaging/rpm/lucerna.spec`):

- `Source0: lucerna-%{lucerna_tarball_version}.tar.gz`, built by
  `make-source-archive.sh`.
- `BuildRequires: gcc pkgconf-pkg-config gtk4-devel desktop-file-utils libappstream-glib`.
- `Requires: mpv`. The GTK libraries are picked up by automatic ELF dependencies.
- `%build` runs `cargo build --release --locked`.
- `%install` does the same installs as Debian.
- `%check` runs `desktop-file-validate` and `appstream-util validate-relax --nonet`.
- `%files` has `%license LICENSE` and `%doc`.

**Architecture:** only `x86_64`/`amd64` is built. Architecture names are mapped in one
place (`scripts/version.sh --deb-arch/--rpm-arch` from `uname -m`), which leaves room
for arm64 later (§39).

### 11.3 `ci.yml` (§33, §34)

Runs on `push` (all branches and tags) and `pull_request`, on `runs-on: ubuntu-24.04`,
with `Swatinem/rust-cache`. Toolchain: rustup reads `rust-toolchain.toml`.

**Apt packages:** `libgtk-4-dev pkgconf mpv xvfb dbus jq`.

| Job | Steps |
| --- | --- |
| `fmt` | `cargo fmt --check` |
| `clippy` | `cargo clippy --workspace --all-targets -- -D warnings` |
| `test` | `LUCERNA_REQUIRE_XVFB=1 LUCERNA_REQUIRE_DBUS=1 LUCERNA_REQUIRE_MPV=1 cargo test --workspace` |
| `build-release` | `cargo build --workspace --release`, then `env -u DISPLAY -u WAYLAND_DISPLAY target/release/{lucerna,lucernad,lucernactl} --help/--version` |
| `deny` | `EmbarkStudios/cargo-deny-action` (advisories, licenses, bans, sources) |
| `scripts` | `tests/scripts/*.sh`, plus `shellcheck scripts/*.sh` |

It also exposes `on: workflow_call` so `release.yml` can reuse it.

### 11.4 `packages.yml` (§36): native builds in containers

**Triggers:** `push` to `main`, `pull_request` affecting `packaging/**`, `assets/**`,
`scripts/**`, `Cargo.*` or `crates/**`, `workflow_dispatch`, and `workflow_call`
(input `expected_version`, optional; if given, the job fails when `version.sh` disagrees).

| Job | Runner / container | Steps |
| --- | --- | --- |
| `deb` | `ubuntu-24.04` / `ubuntu:24.04` | apt: `build-essential debhelper devscripts pkgconf libgtk-4-dev jq git curl ca-certificates lintian desktop-file-utils appstream`, then rustup, then `scripts/build-deb.sh`, then `lintian --fail-on error`, then `dpkg-deb -I` and `-c` into the log, then upload `dist/*.deb` |
| `deb-smoke` | fresh `ubuntu:24.04` (needs `deb`) | `apt-get install -y ./lucerna_*.deb` (dependencies resolve from the archive), then `smoke-test-package.sh deb` |
| `rpm` | `ubuntu-24.04` / `fedora:<N>` (pinned to the current stable Fedora at implementation time, never `latest` or `rawhide`) | dnf: `rpm-build gcc pkgconf-pkg-config gtk4-devel desktop-file-utils libappstream-glib jq git curl rpmlint`, then rustup, then `make-source-archive.sh`, then `build-rpm.sh`, then `rpm -qpi` and `-qpl`, an informational `rpmlint`, then upload `dist/*.rpm` |
| `rpm-smoke` | fresh `fedora:<N>` (needs `rpm`) | `dnf install -y ./lucerna-*.rpm`, then `smoke-test-package.sh rpm` |

The RPM binary is compiled inside Fedora and the DEB binary inside Ubuntu. Nothing is
cross-repackaged (§36).

**`smoke-test-package.sh <deb|rpm>`** (§35 packaging tests):

1. The binaries exist in `/usr/bin`.
2. The desktop file, icon and metainfo exist.
3. With `DISPLAY` and `WAYLAND_DISPLAY` unset: `--version` output equals `version.sh`,
   and `--help` exits 0 for all three binaries.
4. `lucerna` prints the §24 message and exits 1.
5. `lucernactl doctor --json` exits 0 and reports `.mpv.found == true` and
   `.mpv.options_compatible == true` (the distro's mpv accepts our argument vector).
6. `lucernactl status` exits 3 (no daemon).
7. Create `~/.config/lucerna/config.toml` as a test user, uninstall with
   `apt-get remove` or `dnf remove`, then assert the binaries are gone **and the config
   file still exists**.

A wallpaper can't be rendered inside a container, and the script says so in its output.

### 11.5 `release.yml` (§40, §31)

**Trigger:** `push: tags: ['v*']`. `concurrency: release-${{ github.ref_name }}`.

| Job | §40 steps | Details |
| --- | --- | --- |
| `verify` | 1–4 | `actions/checkout` at `ref: ${{ github.ref }}` with `fetch-depth: 0`, then `git fetch --force origin "refs/tags/$TAG:refs/tags/$TAG"` (a shallow checkout can make an annotated tag look lightweight). `scripts/check-tag.sh "$TAG"` then does three things. (a) `git cat-file -t "refs/tags/$TAG"` must be `tag`, otherwise the job fails with "Release tags must be annotated". (b) `V=$(scripts/version.sh --semver)` must equal `${TAG#v}`, otherwise it fails with "Tag v0.8.0 does not match Cargo workspace version 0.7.0" (§31; package metadata is never rewritten). (c) `scripts/changelog-section.sh "$V"` must not be empty (§32). Outputs: `version`, `deb_version`, `rpm_version`, `rpm_release`, `prerelease` (true if `V` contains `-`). |
| `ci` | 5, 6 | `uses: ./.github/workflows/ci.yml`: the full suite plus the release build |
| `packages` | 7, 8 | `uses: ./.github/workflows/packages.yml` with `expected_version`. Builds and smoke-tests both packages. |
| `source` | 9 | `make-source-archive.sh "$TAG"` runs `git archive --format=tar --prefix=lucerna-$V/ "$TAG" \| gzip -n > lucerna-$V.tar.gz` |
| `publish` | 10–12 | `needs: [verify, ci, packages, source]`, so **any failure blocks publishing** (§40: no partial releases). `permissions: contents: write`. Downloads the artifacts, then asserts that exactly `lucerna_<deb>_amd64.deb`, `lucerna-<rpmv>-<rpmrel>.x86_64.rpm` and `lucerna-<V>.tar.gz` are present, failing otherwise. Runs `sha256sum` on them into `SHA256SUMS`, then `sha256sum -c SHA256SUMS`. Takes the release notes from `changelog-section.sh`. If the release doesn't exist, `gh release create "$TAG" --verify-tag --title "Lucerna $V" --notes-file notes.md [--prerelease] <4 files>`; if it does (a re-run), `gh release upload --clobber`. The tag is never modified. |

The workflow sets `permissions: contents: read` at the top level, and only `publish`
elevates. Third-party actions are pinned to major versions.

### 11.6 CHANGELOG (§32)

- Keep a Changelog layout: `## [Unreleased]`, then `## [X.Y.Z] - YYYY-MM-DD` with
  `Added`, `Changed`, `Fixed` and `Removed` subsections, written in prose from the
  milestone's actual deliverables.
- Never a list of commit hashes.
- Enforced by `check-tag.sh` (c).

---

## 12. Risk register

| # | Risk | Likelihood / impact | Mitigation | If the mitigation fails |
| --- | --- | --- | --- | --- |
| R1 | **Cinnamon/Nemo stacking.** The override-redirect bottom window may be drawn *above* Nemo's icons, or not at all, if Muffin restacks, unredirects, or if Nemo's desktop window is opaque. | Medium / high: it is the product. | Two stacking strategies behind `[x11] stacking` (D7). `_NET_WM_BYPASS_COMPOSITOR=2`. A rate-limited, event-driven re-lowering watchdog. `doctor` reports stacking indices, Nemo window depth, compositor owner and fight count. `X11-CINNAMON-NOTES.md` lists every assumption and the field that checks it. LUC-T04–T08 include "if this fails, set `stacking = "desktop-window"`, run `lucernactl reload`, repeat, and attach `doctor --json --redact`." | Documented in TROUBLESHOOTING ("wallpaper above desktop icons", "icons not clickable", "wallpaper not visible"). The fix moves to a later RC informed by the attached diagnostics. **No compensating visual claims.** |
| R2 | **Per-monitor fullscreen detection.** Depends on Muffin's EWMH state and on client geometry. Some games use override-redirect fullscreen windows that aren't in the client list (D13). Multi-monitor fullscreen through `_NET_WM_FULLSCREEN_MONITORS`. | Medium / medium | The 90 % coverage rule (pure, tested). Rect translation via `TranslateCoordinates`. HIDDEN and other-desktop filtering. The capability flag is surfaced. Protocol-tested by simulating the WM's properties under Xvfb. | Documented limitation: override-redirect fullscreen apps don't trigger a pause, and the user can pause manually or with `lucernactl pause`. If per-monitor mapping proves unreliable, §15 allows global pause, and a documented `pause_scope` fallback could be added in a later RC. |
| R3 | **Multi-monitor geometry.** RandR 1.5 monitors vs CRTCs; transforms and `--scale`; mirrored outputs; rotation; the NVIDIA proprietary driver exposing unusual RandR data; EDID missing on docks and KVMs. | Medium / medium | Prefer `GetMonitors`, fall back to CRTCs. Mirrored outputs give one surface. Identity falls back from EDID to connector, with collision handling. `doctor` dumps both the monitor and CRTC views. Protocol tests with virtual monitors. | Documented. LUC-T11–T13 collect `doctor` output for a targeted fix. |
| R4 | **Nothing visual can be validated headlessly.** This covers appearance, smoothness, icon layering, click-through on real Cinnamon, focus behaviour, Alt+Tab and taskbar absence, GUI layout, fonts, the icon, and scaling appearance. | Certain / high | Only protocol evidence is collected: properties, shapes, XTEST routing, `QueryTree` order, embedded child windows. Every such item is `IMPLEMENTED — MANUAL DESKTOP VALIDATION REQUIRED`. LUC-T01–T20 are written with `NOT RUN — REQUIRES REAL DESKTOP` and never filled in by the agent. | That is the plan: v1.0.0 waits for the user's campaign. |
| R5 | mpv `--wid` embedding under Muffin with GL: tearing, black frames, or VAAPI trouble. | Low–medium / medium | `--hwdec=auto-safe`, with `Disabled` available in Settings. `--x11-bypass-compositor=never`. Black window background. | TROUBLESHOOTING entry: switch hardware decoding to Disabled. |
| R6 | `--vf=fps=N` with hwdec may force copy-back and *raise* CPU use on some GPUs. | Medium / low | Real-mpv test that the filter is accepted. The docs say the cap saves render and present work, not decode work. | LUC-T20 records CPU. If the cap turns out harmful, a later RC removes the fps filter when hwdec is on and documents that. |
| R7 | mpv option drift between Ubuntu's 0.37 and Fedora's newer mpv (renamed or removed options). | Medium / high: mpv exits 1 on an unknown option | `check_compat` at runtime and in `doctor`. Package smoke tests assert compatibility on each distro's mpv. fake-mpv rejects unknown options. | Adjust the builder per detected version (a version gate in `args.rs`), with a test for each branch. |
| R8 | Lock detection varies (the `cinnamon-screensaver` D-Bus name, logind availability). | Low / low | Three probes in order. The capability flag is surfaced. | Documented. The lock screen covers the wallpaper anyway, so the only cost is power. |
| R9 | Login race: the daemon starts before Muffin or `nemo-desktop`. | Medium / medium | Wait up to 10 s each for `DISPLAY` and the WM. The restack watchdog reacts when Nemo maps later. | LUC-T14 covers it. Worst case the user can add a delay in Startup Applications (documented). |
| R10 | Orphaned mpv after a daemon `SIGKILL`; the `PDEATHSIG` thread caveat. | Low / low | `PDEATHSIG`, plus a start-time-checked PID registry and recovery (§4.6). | The next daemon start cleans up. It never kills unrelated mpv. |
| R11 | Mixing zbus with GTK and tokio (feature unification). | Low / medium | The `tokio` feature of zbus is banned by the architecture test. The engine talks to zbus through a runtime-agnostic channel. | Compile-time failure caught by CI, not a runtime surprise. |
| R12 | Packaging with the rustup toolchain and network access during `cargo build` isn't Debian or Fedora archive policy. | Certain / low for v1 (GitHub artifacts only) | Documented in `PACKAGING.md`. `--locked` builds. `Cargo.lock` committed. | Vendoring for distro submission is future work. |
| R13 | GTK version floor excludes Mint 21 (D8). | Certain / low | Documented as the supported environment in README. | — |
| R14 | GitHub Actions: `actions/checkout` presents annotated tags as lightweight; the container images' package names drift. | Medium / medium | Explicit tag re-fetch before `cat-file`. Pinned `ubuntu:24.04` and `fedora:<N>`. The script tests cover the tag logic. | Fix the workflow in the milestone where it fails. Never weaken the check. |
| R15 | Non-UTF-8 media paths (D12). | Low / low | Rejected with a clear error at add time. | Documented limitation. |

---

## 13. Instructions to the Phase B agent (summary)

1. Read the directive, then this plan. Execute §10 in order.
2. Keep the crate graph of §1.3. The architecture test is the arbiter. If a boundary
   seems to be in the way, move the code, don't widen the graph.
3. Keep the pure logic pure (state machine, restart, policy, plan, args, identity,
   config). Most of the correctness argument rests on those unit tests.
4. Never claim visual or desktop validation. Protocol tests are called protocol tests.
   Every `LUC-Txx` result stays `NOT RUN — REQUIRES REAL DESKTOP`.
5. Stay out of the §27 non-goals: no thumbnails beyond the optional metadata cache, no
   web or shader content, no Wayland backend, no Span mode, no network access.
6. Stop after `v1.0.0-rc.1` and the §55 report.
