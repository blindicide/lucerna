# LUCERNA
## Linux Animated Wallpaper Engine
### Development and Implementation Directive

**Project name:** Lucerna  
**Primary package name:** `lucerna`  
**Primary platform:** Linux  
**Initial desktop target:** Linux Mint / Cinnamon / X11  
**Implementation language:** Rust  
**GUI toolkit:** GTK 4  
**Video renderer:** mpv  
**Versioning:** Semantic Versioning  
**Source control:** Git  
**Release artifacts:** `.deb`, `.rpm`, source archive, checksums  
**Build and release system:** GitHub Actions  
**Development environment:** remote/headless Linux server

---

# 1. Mission

Lucerna is a native Linux animated wallpaper engine intended to provide a polished, persistent, low-overhead alternative to the assortment of shell scripts, `xwinwrap` commands, manual `mpv` invocations, and abandoned animated-wallpaper utilities commonly used on Linux desktops.

The initial goal is intentionally narrower than recreating Wallpaper Engine in its entirety.

Lucerna v1 shall provide a reliable graphical application capable of:

- registering local animated wallpaper files;
- assigning them to one or more displays;
- continuously playing them as actual desktop backgrounds;
- keeping ordinary desktop interaction functional;
- persisting configuration between sessions;
- automatically restoring wallpapers after login;
- pausing or reducing unnecessary rendering when appropriate;
- exposing both graphical and command-line controls;
- integrating naturally with Linux desktop application menus;
- installing cleanly through native Debian and RPM packages.

The primary initial environment is **Linux Mint with Cinnamon running under X11**.

Lucerna must be architected so that rendering backends are replaceable. X11/Cinnamon support is the first backend, not an assumption baked into every other part of the program.

Wayland, KDE Plasma-specific integration, web wallpapers, shaders, reactive wallpapers and Wallpaper Engine import are future directions and must not be implemented by hacking them into the initial X11 backend.

The project should feel like an actual desktop application rather than a graphical wrapper around:

```text
mpv --loop whatever.mp4
```

---

# 2. Critical Development Constraint: Headless Server

Development will be performed on a remote server.

The agent therefore **does not have access to the actual graphical desktop on which Lucerna will ultimately run.**

This has important consequences.

The coding agent MUST NOT claim to have visually tested Lucerna.

The coding agent MUST NOT claim that:

- the wallpaper visually appears correctly;
- the wallpaper is definitely underneath Cinnamon desktop icons;
- GTK layout looks correct;
- animations look smooth;
- panel integration looks correct;
- windows stack correctly from a human observer's perspective;
- multi-monitor geometry looks correct;
- scaling looks correct;
- themes look correct;
- fonts look correct;
- any UI element is aesthetically satisfactory.

Those properties cannot honestly be validated on the development server.

The agent must not attempt to work around this restriction by pretending that:

- Xvfb screenshots constitute human visual validation;
- synthetic screenshots prove correct Cinnamon integration;
- GTK widget construction alone proves good layout;
- a virtual X server accurately reproduces Cinnamon's compositor behaviour.

Headless graphical infrastructure MAY be used for **protocol-level automated testing only**.

For example, Xvfb may be used to verify:

- successful X11 connection;
- creation/destruction of windows;
- EWMH property assignment;
- RandR enumeration;
- process lifecycle;
- IPC behaviour;
- absence of crashes.

It must never be cited as evidence that the result visually looks correct.

All actual desktop/visual acceptance must be represented as a documented manual testing campaign for the user to execute on a real Mint Cinnamon desktop.

The agent shall create:

```text
docs/MANUAL-ACCEPTANCE.md
```

containing those tests.

Where a desktop-specific behaviour cannot be automatically demonstrated on the server, the correct status is:

```text
IMPLEMENTED — MANUAL DESKTOP VALIDATION REQUIRED
```

not:

```text
PASS
```

This distinction is mandatory throughout development reports.

---

# 3. Engineering Philosophy

Lucerna should remain simple where simplicity helps reliability.

Do not introduce an Electron application, embedded Chromium instance, web frontend, Node.js runtime or similar architecture for v1.

The initial product consists of a native control application plus a small user-session daemon.

Rendering should be delegated to `mpv`, which already provides mature hardware decoding, codec handling and rendering.

Lucerna's job is therefore primarily:

1. desktop integration;
2. renderer lifecycle;
3. configuration;
4. multi-monitor assignment;
5. pause/resume policy;
6. process supervision;
7. user interface;
8. diagnostics;
9. packaging.

Do not reimplement a video decoder.

Do not reimplement FFmpeg.

Do not vendor mpv.

Do not require root privileges during normal operation.

Do not modify system files at runtime unless absolutely unavoidable.

Do not require the user to paste arbitrary shell commands into configuration files.

Do not execute wallpaper filenames through a shell.

Do not make the application depend on brittle grep/awk shell pipelines.

The normal runtime should consist of compiled Lucerna binaries plus distro-provided runtime libraries and `mpv`.

---

# 4. Technical Stack

Use Rust as the primary implementation language.

Preferred libraries include:

```text
gtk4
glib
gio
clap
serde
toml
serde_json
tokio
zbus
tracing
tracing-subscriber
thiserror
anyhow
uuid
dirs
x11rb
```

Equivalent well-maintained crates may be substituted when technically justified.

The agent must avoid adding dependencies merely for convenience when a simple standard-library implementation is sufficient.

Dependency versions must be locked through:

```text
Cargo.lock
```

and committed.

Rust's normal workspace layout should be used.

---

# 5. Program Architecture

Lucerna shall be composed of several logical components rather than one monolithic executable.

The expected binaries are:

```text
lucerna
lucernad
lucernactl
```

## `lucerna`

The GTK graphical control application.

Responsibilities:

- wallpaper library management;
- display assignment;
- preferences;
- daemon status display;
- start/stop/pause/resume controls;
- autostart settings;
- user-facing diagnostics;
- About/version information.

The GUI itself must not own long-running wallpaper renderer processes.

Closing the GUI should not terminate the wallpaper.

## `lucernad`

The per-user background service.

Responsibilities:

- load configuration;
- discover displays;
- select rendering backend;
- create wallpaper surfaces;
- start and supervise mpv;
- assign wallpapers to displays;
- detect renderer crashes;
- pause/resume playback;
- respond to display configuration changes;
- respond to graphical controller requests;
- maintain current state;
- expose D-Bus IPC;
- log diagnostics.

Only one daemon instance may run for a user session.

Attempts to launch another instance must exit cleanly and report that an existing daemon owns the session.

## `lucernactl`

CLI controller for administration, scripting and debugging.

It communicates with `lucernad` over D-Bus.

It must not implement a second independent wallpaper engine.

---

# 6. Workspace Layout

Use a repository structure broadly resembling:

```text
lucerna/
├── Cargo.toml
├── Cargo.lock
├── README.md
├── CHANGELOG.md
├── LICENSE
├── .gitignore
├── rustfmt.toml
├── crates/
│   ├── lucerna-core/
│   ├── lucerna-daemon/
│   ├── lucerna-ui/
│   ├── lucerna-cli/
│   └── lucerna-x11/
├── assets/
│   ├── icons/
│   └── desktop/
├── packaging/
│   ├── debian/
│   └── rpm/
│       └── lucerna.spec
├── docs/
│   ├── ARCHITECTURE.md
│   ├── BUILDING.md
│   ├── CONFIGURATION.md
│   ├── IPC.md
│   ├── PACKAGING.md
│   ├── MANUAL-ACCEPTANCE.md
│   └── TROUBLESHOOTING.md
├── tests/
├── scripts/
└── .github/
    └── workflows/
        ├── ci.yml
        ├── packages.yml
        └── release.yml
```

Exact internal crate names may vary slightly if a better Rust structure becomes apparent, but architecture boundaries must remain clear.

---

# 7. Backend Abstraction

Desktop integration must be represented through an explicit backend interface.

Conceptually:

```text
WallpaperBackend
    probe()
    enumerate_outputs()
    create_surface()
    resize_surface()
    destroy_surface()
    refresh()
    shutdown()
```

Do not place X11 calls throughout daemon code.

The X11 implementation belongs in:

```text
lucerna-x11
```

or an equivalent backend module.

The daemon should interact with it through typed interfaces.

Future backends should be possible without rewriting:

- configuration;
- GUI;
- library handling;
- mpv control;
- IPC;
- pause policy;
- diagnostics.

---

# 8. X11 / Cinnamon Backend

The initial backend is:

```text
cinnamon-x11
```

A more generic EWMH-compatible X11 mode may exist where practical, but Linux Mint Cinnamon/X11 is the release acceptance target.

Current Cinnamon still uses `nemo-desktop` to provide desktop icons and the desktop context menu.

Existing Mint animated-wallpaper solutions commonly combine X11 wallpaper windows with mpv, including `xwinwrap`-style desktop windows.

Lucerna should implement the necessary X11 integration itself rather than requiring the user to separately install `xwinwrap`.

`xwinwrap` may be studied as a reference implementation.

It must not become an undocumented mandatory runtime dependency.

The X11 backend shall implement the equivalent primitives required by Lucerna:

- geometry-controlled wallpaper surface;
- undecorated window;
- input-disabled window;
- skip taskbar;
- skip pager;
- sticky across workspaces;
- desktop/below window hints where appropriate;
- override-redirect mode where appropriate;
- explicit lowering/restacking;
- cleanup on shutdown.

The implementation must investigate and document Cinnamon/Nemo behaviour rather than assuming a generic window-manager recipe works everywhere.

No destructive Cinnamon settings changes are permitted.

If integration requires a compatibility adjustment, the old value must be recorded and restorable.

---

# 9. Renderer Architecture

Video decoding and presentation shall be delegated to mpv.

Lucerna must invoke mpv directly using process argument APIs.

Never construct a shell command string such as:

```text
sh -c "mpv $FILE"
```

The path must be passed as an individual argument.

A typical conceptual renderer invocation may contain options equivalent to:

```text
--no-config
--really-quiet
--no-terminal
--loop-file=inf
--mute=yes
--no-input-default-bindings
--input-ipc-server=<runtime socket>
--wid=<wallpaper surface XID>
--hwdec=auto-safe
```

The exact final options must be tested against the mpv version available in CI and documented.

Lucerna owns the mpv process.

The daemon must detect:

- normal exit;
- crash;
- failed launch;
- unsupported file;
- IPC failure;
- hung startup where practical.

mpv stdout/stderr must be captured into diagnostic logs without endlessly growing files.

Renderer state must be explicit:

```text
Stopped
Starting
Playing
Paused
Failed
Stopping
```

Invalid transitions should be rejected or normalized.

Do not represent runtime state with a handful of unrelated booleans.

---

# 10. Wallpaper File Support

v1 is primarily a **video wallpaper engine**.

Support local files that mpv can decode.

At minimum, test representative files using containers or test fixtures for:

```text
.mp4
.webm
.mkv
.gif
```

Lucerna should not implement its own codec whitelist beyond reasonable file chooser filtering.

If mpv can open the file, Lucerna should generally permit it.

Unsupported files must fail gracefully with a readable error.

Lucerna shall store references to user files.

It must NOT silently copy multi-gigabyte videos into a private application folder when the user adds them.

The user may remove a library entry without deleting the original video.

Lucerna must never delete original wallpaper media unless a future explicit feature specifically requests this and obtains confirmation.

---

# 11. Wallpaper Library

The GUI shall contain a simple wallpaper library.

v1 does not need a Steam Workshop-style online catalogue.

Each entry shall contain at least:

```text
UUID
display name
absolute file path
media type
last known existence state
date added
```

Optional metadata may include:

```text
duration
resolution
codec
thumbnail path
```

Thumbnail generation is optional for the first implementation if doing it cleanly would significantly increase complexity.

Do not make v1 depend on FFmpeg command-line tools merely to produce thumbnails.

A missing wallpaper file shall remain represented in configuration but be marked unavailable.

The application must not crash because an external drive disappeared.

---

# 12. Display Discovery

Displays shall be discovered using XRandR through X11 APIs.

Each display record should expose at least:

```text
connector name
geometry
primary status
resolution
position
rotation
EDID-derived identity where available
```

Assignments should not rely solely on array order such as:

```text
monitor[0]
monitor[1]
```

because monitor enumeration order can change.

Prefer a stable identifier derived from:

```text
EDID manufacturer
model
serial
connector
```

with reasonable fallbacks.

When a configured display is absent:

- preserve its wallpaper assignment;
- do not delete it;
- do not launch a renderer for it.

When the display returns, restore its configured wallpaper.

---

# 13. Multi-Monitor Behaviour

Lucerna shall support:

1. one wallpaper used on all monitors;
2. different wallpapers on individual monitors.

Each monitor may have its own renderer surface and mpv process.

This is acceptable for v1.

Optimization into shared decode contexts is explicitly unnecessary.

For each display, support scaling modes:

```text
Fill
Fit
Stretch
Center
```

The scaling implementation may use mpv properties or rendering geometry.

Behaviour must be deterministic and documented.

A future `Span` mode may be added later but is not mandatory for v1.

---

# 14. Desktop Interaction

The wallpaper must not behave like an ordinary application window.

It must not:

- appear in Alt+Tab;
- appear in the taskbar;
- appear in normal window lists;
- steal focus;
- capture keyboard input;
- intercept desktop mouse interaction;
- prevent desktop icons from being clicked;
- prevent Cinnamon's desktop context menu;
- cover normal windows.

Because these properties cannot be visually validated on the development server, their implementation shall be unit/protocol tested where possible and then marked:

```text
MANUAL DESKTOP VALIDATION REQUIRED
```

Existing Mint animated-wallpaper workarounds specifically note that keeping Nemo icons on the correct layer can require compositor/stacking handling, so this must be treated as a real integration problem rather than assumed solved merely because a video process started.

---

# 15. Pause Policies

Continuous rendering while the wallpaper is invisible is wasteful.

Lucerna shall support configurable pause behaviour.

At minimum:

```text
pause on fullscreen application
pause when session is locked
pause when daemon is explicitly paused
```

Fullscreen detection should use EWMH/X11 window state where available.

For multi-monitor systems, fullscreen pause should preferably affect only the monitor obscured by the fullscreen application.

If reliable per-monitor determination cannot be achieved in the initial implementation, global pause is acceptable, but the limitation must be documented.

Default:

```text
pause_on_fullscreen = true
```

Do not default to pausing merely because an ordinary window is maximized.

---

# 16. Audio

Animated wallpapers shall be muted by default.

Default:

```text
audio = false
```

Audio support may be exposed as an explicit per-wallpaper or global option.

Enabling audio must never happen silently.

Volume must never unexpectedly inherit an mpv user configuration because Lucerna shall launch mpv using:

```text
--no-config
```

unless a future explicit advanced setting enables user mpv configuration.

---

# 17. Performance Controls

Expose a small number of meaningful controls.

At minimum:

```text
hardware decoding
FPS cap
pause-on-fullscreen
audio
```

Hardware decode options:

```text
Auto
Disabled
```

`Auto` should map to a safe mpv hardware-decoding option.

FPS options should include sensible presets such as:

```text
Native
60
30
15
```

Do not implement pseudo-optimization features that merely change labels without changing renderer behaviour.

Lucerna itself should remain lightweight when the GUI is closed.

The daemon should use negligible CPU outside renderer management events.

---

# 18. Configuration

Use an XDG-compliant layout.

Primary configuration:

```text
$XDG_CONFIG_HOME/lucerna/config.toml
```

or:

```text
~/.config/lucerna/config.toml
```

State/cache:

```text
$XDG_CACHE_HOME/lucerna/
$XDG_STATE_HOME/lucerna/
```

Runtime sockets:

```text
$XDG_RUNTIME_DIR/lucerna/
```

Configuration shall contain a schema version.

Example conceptual structure:

```toml
schema_version = 1

[general]
autostart = true
pause_on_fullscreen = true
audio = false
hardware_decode = "auto"
fps_limit = "native"

[[wallpapers]]
id = "..."
name = "Rain"
path = "/home/user/Videos/rain.webm"

[assignments]
"monitor-stable-id-1" = "wallpaper-uuid"
```

The exact schema may be refined.

All writes must be atomic.

A crash during configuration write must not leave a zero-byte configuration file.

Unknown future configuration keys should ideally be ignored rather than causing total failure.

---

# 19. IPC

Use session D-Bus for communication between GUI/CLI and daemon.

Suggested bus name:

```text
org.lucerna.Lucerna1
```

Suggested object path:

```text
/org/lucerna/Lucerna1
```

At minimum expose operations equivalent to:

```text
GetStatus
GetDisplays
GetAssignments
SetWallpaper
Pause
Resume
Stop
Reload
```

Signals should exist for state changes where useful.

The exact interface must be documented in:

```text
docs/IPC.md
```

Do not use world-writable filesystem sockets as the main public control API.

mpv's own IPC sockets remain private implementation details under the user's runtime directory.

---

# 20. CLI

`lucernactl` must function even when the GUI cannot start.

Commands:

```text
lucernactl status
lucernactl monitors
lucernactl wallpapers
lucernactl play <path>
lucernactl play <path> --monitor <id>
lucernactl pause
lucernactl resume
lucernactl stop
lucernactl reload
lucernactl doctor
lucernactl doctor --json
lucernactl --version
lucernactl --help
```

Commands should have meaningful exit codes.

Machine-readable output must be available where useful.

---

# 21. Diagnostic System

`lucernactl doctor` is mandatory.

It should gather information required to debug desktop integration without requiring the development agent to access the user's graphical machine.

Report:

```text
Lucerna version
daemon version
configuration schema version
XDG_SESSION_TYPE
XDG_CURRENT_DESKTOP
DESKTOP_SESSION
DISPLAY
X11 connection status
X server information
RandR availability
detected outputs
monitor identities
mpv path
mpv version
active backend
wallpaper renderer states
Nemo desktop detection
recent renderer failures
configuration paths
runtime paths
autostart state
```

Where relevant, expose X11 window information used for wallpaper integration.

Provide:

```text
--json
```

for machine-readable reports.

Provide:

```text
--redact
```

or equivalent behaviour to obscure user-specific home paths when reports are intended for public bug reports.

The diagnostic command must never dump arbitrary user files, environment secrets or authentication tokens.

---

# 22. Autostart

Lucerna shall support restoring wallpapers after graphical login.

Prefer an XDG desktop-session compatible mechanism.

The daemon must not require root.

The implementation must account for availability of:

```text
DISPLAY
DBUS_SESSION_BUS_ADDRESS
XDG_RUNTIME_DIR
```

during startup.

Autostart must be configurable from the GUI.

Disabling Lucerna autostart must actually prevent the wallpaper daemon from relaunching on the next session.

Do not create multiple competing startup mechanisms.

---

# 23. GUI

Use GTK 4.

The visual target is a clean native Linux utility.

Do not attempt to imitate Wallpaper Engine pixel-for-pixel.

The main UI should provide four conceptual areas:

```text
Wallpapers
Displays
Settings
About
```

The exact navigation implementation may use a sidebar, stack or equivalent GTK structure.

## Wallpapers

Must allow:

- Add wallpaper;
- remove library entry;
- display missing-file status;
- select wallpaper;
- start wallpaper;
- stop wallpaper.

## Displays

Show detected monitors.

Allow assigning one wallpaper per monitor.

Display a human-readable identity such as:

```text
HDMI-1 — 1920×1080
eDP-1 — 1920×1080 — Primary
```

## Settings

Expose:

```text
Start Lucerna automatically
Pause when fullscreen
Hardware decoding
FPS limit
Audio
```

## About

Include:

```text
Lucerna
version
project description
license
repository information
runtime backend
```

All user-visible strings should be centralized enough that localization can be added later.

English is sufficient for v1.

Again: the agent must compile and structurally test the GUI, but **must not mark its visual appearance as tested**.

---

# 24. UI Failure Behaviour

If no graphical session exists and the user launches:

```text
lucerna
```

the program should print a useful message rather than a Rust panic.

For example:

```text
Lucerna could not connect to a graphical display.
DISPLAY is not set or no supported graphical session is available.
```

CLI options such as:

```text
lucerna --version
lucerna --help
```

must function without a display.

This is particularly important because the development server is headless and package smoke tests must be able to execute those commands.

---

# 25. Logging

Use structured application logging.

Default runtime logs should be concise.

Support an environment variable or CLI setting such as:

```text
RUST_LOG=lucerna=debug
```

Do not log continuously once per rendered frame.

Do not flood journals with harmless polling information.

Important events include:

```text
daemon startup
backend selection
display detection
wallpaper assignment
renderer launch
renderer termination
pause/resume
configuration reload
display hotplug
backend error
mpv error
```

---

# 26. Security and Robustness

Although Lucerna is a desktop utility, normal secure coding rules still apply.

Requirements:

- never execute wallpaper paths through a shell;
- never treat wallpaper metadata as executable input;
- canonicalize paths where useful;
- quote nothing manually when process APIs already separate arguments;
- create runtime files with user-only permissions;
- avoid predictable temporary files in `/tmp`;
- clean stale IPC sockets;
- ensure one daemon cannot accidentally control another user's session;
- no root daemon;
- no setuid binaries;
- no automatic network access in v1;
- no telemetry;
- no analytics;
- no update checker making unsolicited network requests.

A maliciously named file such as:

```text
$(rm -rf ~).mp4
```

must simply be treated as a filename.

---

# 27. Explicit v1 Non-Goals

Do NOT implement the following during the initial development campaign:

- Wallpaper Engine Workshop integration;
- Wallpaper Engine scene import;
- HTML wallpapers;
- arbitrary JavaScript wallpapers;
- Chromium/WebKit renderer;
- GLSL shader editor;
- audio-reactive scenes;
- mouse-reactive scenes;
- online wallpaper browser;
- user accounts;
- cloud sync;
- automatic media downloading;
- Steam integration;
- Windows support;
- macOS support;
- mobile support;
- full GNOME support;
- full Wayland support;
- compositor-specific Plasma wallpaper plugins;
- Hyprland backend;
- Sway backend;
- screensaver replacement;
- lock-screen replacement.

These belong to future versions.

Do not let attractive scope creep delay the basic product.

---

# 28. Git Policy

Git is mandatory from the first meaningful file.

Primary branch:

```text
main
```

`main` must remain buildable.

Use short-lived branches where useful:

```text
feat/x11-backend
feat/mpv-controller
feat/gui
fix/renderer-restart
ci/rpm-build
```

Commit messages must be in clear English.

Use Conventional Commit style:

```text
feat:
fix:
refactor:
test:
docs:
build:
ci:
chore:
```

Examples:

```text
feat(x11): add wallpaper surface abstraction
feat(renderer): supervise mpv process lifecycle
fix(config): preserve monitor assignments after disconnect
test(ipc): cover daemon pause transitions
ci(packaging): build rpm artifact on Fedora
docs: add Cinnamon manual acceptance campaign
```

Commits must be coherent.

Do not create one gigantic commit named:

```text
implement app
```

Do not make hundreds of meaningless one-line commits either.

Never rewrite published release tags.

Never force-push `main`.

---

# 29. Semantic Versioning

Lucerna follows SemVer:

```text
MAJOR.MINOR.PATCH
```

During initial development:

```text
0.x.y
```

is unstable development.

The first stable public release is:

```text
1.0.0
```

The canonical version source shall be the root Cargo workspace metadata.

All binaries must derive their version from the same source.

Do not separately hardcode versions in:

- GTK About window;
- RPM spec;
- Debian package;
- CLI;
- daemon.

Packaging scripts must obtain the version from the canonical project version or receive it from CI.

---

# 30. Mandatory Version / Tag Sequence

The initial campaign shall use the following milestone releases.

## v0.0.1 — Repository Bootstrap

Must contain:

- Rust workspace;
- basic crate structure;
- logging;
- CI skeleton;
- README;
- build documentation;
- functioning `--version`;
- basic unit-test harness.

Tag:

```text
v0.0.1
```

## v0.1.0 — Renderer Core

Must contain:

- mpv detection;
- mpv process wrapper;
- IPC socket management;
- renderer state machine;
- pause/resume/stop;
- crash detection;
- tests using mocked renderer processes.

Tag:

```text
v0.1.0
```

## v0.2.0 — Cinnamon/X11 Backend

Must contain:

- X11 connection;
- RandR output enumeration;
- wallpaper surface creation;
- window hints;
- input suppression;
- geometry handling;
- cleanup;
- Cinnamon/Nemo probing;
- X11 diagnostic data.

Tag:

```text
v0.2.0
```

Desktop appearance remains:

```text
MANUAL VALIDATION REQUIRED
```

## v0.3.0 — Daemon and CLI

Must contain:

- `lucernad`;
- single-instance enforcement;
- D-Bus API;
- `lucernactl`;
- configuration loading;
- state persistence;
- diagnostics;
- daemon integration tests.

Tag:

```text
v0.3.0
```

## v0.4.0 — GTK Control Application

Must contain:

- working GTK application;
- wallpaper library;
- file selection;
- display page;
- settings;
- About page;
- daemon control;
- error handling.

Tag:

```text
v0.4.0
```

Visual quality:

```text
NOT VALIDATED ON DEVELOPMENT SERVER
```

## v0.5.0 — Multi-Monitor and Policy Engine

Must contain:

- persistent monitor identities;
- per-monitor assignments;
- monitor hotplug handling;
- scaling modes;
- fullscreen pause;
- lock pause where available;
- missing monitor restoration.

Tag:

```text
v0.5.0
```

## v0.6.0 — Desktop Lifecycle

Must contain:

- login autostart;
- logout cleanup;
- renderer restart policy;
- corrupted-config handling;
- missing wallpaper handling;
- mpv missing/error handling;
- improved logging;
- recovery after renderer crash.

Tag:

```text
v0.6.0
```

## v0.7.0 — Native Packaging

Must contain:

- Debian package definition;
- RPM package definition;
- desktop entry;
- application icon;
- AppStream metadata where practical;
- dependency metadata;
- install/uninstall smoke tests;
- package documentation.

Tag:

```text
v0.7.0
```

## v0.8.0 — Automated Distribution

Must contain complete GitHub Actions pipelines for:

- normal CI;
- `.deb` package;
- `.rpm` package;
- tagged GitHub Releases;
- source archive;
- SHA-256 checksums.

Tag:

```text
v0.8.0
```

## v0.9.0 — Release Candidate Baseline

Must contain:

- documentation cleanup;
- diagnostics finalized;
- test coverage finalized;
- packaging finalized;
- no known server-testable critical defects;
- manual acceptance checklist frozen.

Tag:

```text
v0.9.0
```

## v1.0.0-rc.1 — Desktop Acceptance Candidate

Must be produced automatically through the final release pipeline.

Artifacts must include:

```text
lucerna_1.0.0~rc1_amd64.deb
lucerna-1.0.0-0.rc1.x86_64.rpm
lucerna-1.0.0-rc.1.tar.gz
SHA256SUMS
```

Exact distro-compliant prerelease formatting may differ slightly where required.

Tag:

```text
v1.0.0-rc.1
```

This is the build intended for actual desktop-side testing.

## v1.0.0 — First Stable Release

`v1.0.0` may be tagged only after either:

1. desktop manual acceptance passes; or
2. failures discovered during manual acceptance are corrected and a replacement RC passes.

Tag:

```text
v1.0.0
```

Expected artifacts:

```text
lucerna_1.0.0_amd64.deb
lucerna-1.0.0-1.x86_64.rpm
lucerna-1.0.0.tar.gz
SHA256SUMS
```

---

# 31. Tag Rules

All release tags must be annotated Git tags.

Example:

```bash
git tag -a v0.4.0 -m "Lucerna v0.4.0"
git push origin v0.4.0
```

CI must reject a release where:

```text
Git tag version != Cargo workspace version
```

For example:

```text
tag: v0.8.0
Cargo version: 0.7.0
```

must fail the release workflow.

Do not silently rewrite package metadata to hide a mismatch.

---

# 32. CHANGELOG

Maintain:

```text
CHANGELOG.md
```

Use a simple Keep-a-Changelog style.

Every tagged version must have a section.

Example:

```text
## [0.5.0]

### Added
- Per-monitor wallpaper assignments.
- Fullscreen pause policy.

### Fixed
- Renderer restart after monitor reconnection.
```

Do not auto-generate useless changelogs consisting solely of Git commit hashes.

---

# 33. Continuous Integration

Create:

```text
.github/workflows/ci.yml
```

Trigger on:

```text
push
pull_request
```

CI must perform at minimum:

```text
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace --release
```

Where practical, also perform:

```text
cargo deny
```

or equivalent dependency/license/security auditing.

A security audit warning caused exclusively by an abandoned transitive dependency should not simply be ignored; document and address it.

---

# 34. Headless GUI Build Testing

CI must compile the GTK application.

It does not have to launch the full GUI.

Tests must ensure:

```text
lucerna --help
lucerna --version
lucernactl --help
lucernactl --version
lucernad --help
lucernad --version
```

work without a graphical display.

GTK initialization must therefore occur after argument parsing where appropriate.

This allows packaging and smoke testing on the development server.

---

# 35. Automated Test Classes

Use several explicit test categories.

## Unit tests

Cover:

- configuration parsing;
- schema migration;
- renderer state transitions;
- mpv command generation;
- monitor identifiers;
- path handling;
- policy decisions;
- CLI parsing.

## Integration tests

Cover:

- daemon/CLI D-Bus communication;
- renderer subprocess supervision;
- fake mpv process;
- crashed renderer;
- stale runtime socket;
- duplicate daemon launch;
- invalid configuration;
- missing media;
- configuration reload.

## X11 protocol tests

Where possible under Xvfb:

- connect;
- enumerate screen;
- create wallpaper surface;
- assign required EWMH properties;
- resize;
- lower;
- destroy;
- reconnect handling.

These are protocol tests only.

They are not visual tests.

## Packaging tests

Install the generated package into clean distro environments where practical.

Verify:

```text
package installs
package dependencies resolve
binaries exist
desktop file exists
icon exists
--version works
--help works
package removes successfully
```

A full wallpaper cannot be tested inside these containers.

That is expected.

---

# 36. GitHub Actions Packaging

This requirement is mandatory.

The coding agent must implement GitHub Actions capable of creating real installation packages without requiring a developer workstation.

Create:

```text
.github/workflows/packages.yml
```

The workflow shall build both:

```text
DEB
RPM
```

Use native distro build environments.

Do not take an Ubuntu-linked binary and merely place it inside an RPM archive.

The RPM binary must be built inside an appropriate Fedora/RPM-family build environment.

The Debian binary must be built inside an appropriate Debian/Ubuntu-family build environment.

A matrix/container approach is recommended.

Conceptually:

```text
Ubuntu build environment
    -> compile
    -> Debian package
    -> package smoke test

Fedora build environment
    -> compile
    -> RPM package
    -> package smoke test
```

Packaging tools may include native:

```text
dpkg-buildpackage
rpmbuild
```

or another reproducible method if properly justified.

Prefer native package definitions over opaque one-command package generators.

---

# 37. Debian Package

Package name:

```text
lucerna
```

Target family:

```text
Debian / Ubuntu / Linux Mint
```

Primary v1 validation target:

```text
Linux Mint Cinnamon
```

Install files approximately as follows:

```text
/usr/bin/lucerna
/usr/bin/lucernad
/usr/bin/lucernactl

/usr/share/applications/org.lucerna.Lucerna.desktop
/usr/share/icons/hicolor/.../apps/org.lucerna.Lucerna.*
/usr/share/metainfo/org.lucerna.Lucerna.metainfo.xml
/usr/share/doc/lucerna/
```

Runtime dependencies must include mpv and required graphical libraries.

The package manager must be allowed to install dependencies normally.

Do not embed an entire private GTK stack into the `.deb`.

Package uninstall must not delete:

```text
~/.config/lucerna
```

or the user's wallpaper media.

---

# 38. RPM Package

Package name:

```text
lucerna
```

Target family:

```text
Fedora / RPM-based Linux
```

Create:

```text
packaging/rpm/lucerna.spec
```

Use conventional RPM file ownership and dependency metadata.

Install the same logical application files as the Debian package.

RPM uninstall must also preserve user configuration.

---

# 39. Package Architecture

Initial official binary package architecture:

```text
x86_64 / amd64
```

Do not block future ARM64 support.

Architecture-specific assumptions should be isolated.

ARM64 CI is not mandatory for v1.

---

# 40. GitHub Release Workflow

Create:

```text
.github/workflows/release.yml
```

Trigger:

```text
push tag matching v*
```

The workflow must:

1. check out the exact tag;
2. verify the tag is annotated;
3. obtain project version;
4. verify tag/version match;
5. run the complete test suite;
6. build release binaries;
7. build `.deb`;
8. build `.rpm`;
9. build source archive;
10. calculate SHA-256 hashes;
11. create or update the GitHub Release;
12. attach artifacts.

Release must fail if any mandatory artifact fails.

Do not publish a successful release containing only one package because the other package job failed.

---

# 41. Release Artifacts

Stable release:

```text
lucerna_1.0.0_amd64.deb
lucerna-1.0.0-1.x86_64.rpm
lucerna-1.0.0.tar.gz
SHA256SUMS
```

Optional additional artifacts:

```text
SBOM
debug symbols
```

are welcome but must not block v1.

The GitHub release description should contain human-readable release notes extracted or adapted from `CHANGELOG.md`.

---

# 42. Application Metadata

Ship a valid desktop entry.

Conceptually:

```ini
[Desktop Entry]
Name=Lucerna
Comment=Animated wallpapers for Linux
Exec=lucerna
Icon=org.lucerna.Lucerna
Terminal=false
Type=Application
Categories=Utility;Settings;
```

Use a proper application ID consistently.

Ship AppStream metadata if packaging validation allows it.

A simple deterministic vector icon may be produced.

The agent is not expected to aesthetically validate that icon on the server.

---

# 43. Licensing

If the repository already contains a license, preserve it.

If starting from an entirely new repository with no existing licensing instruction, use:

```text
MIT
```

for Lucerna's own source code.

Do not copy code from xwinwrap or other projects without verifying license compatibility and attribution requirements.

Studying behaviour and independently implementing required X11 logic is preferred.

Third-party dependency licenses must remain their own.

---

# 44. Documentation

The following documents are mandatory.

## README.md

Must explain:

- what Lucerna is;
- supported environment;
- screenshots may be added later;
- installation;
- quick start;
- major features;
- current limitations;
- development status.

## docs/ARCHITECTURE.md

Explain:

- daemon;
- GUI;
- CLI;
- backend abstraction;
- renderer;
- D-Bus;
- configuration;
- X11 integration.

## docs/BUILDING.md

Document:

- build dependencies;
- Rust toolchain;
- local build;
- server/headless build;
- tests;
- package builds.

## docs/CONFIGURATION.md

Document:

- config file location;
- config schema;
- available settings;
- defaults.

## docs/IPC.md

Document the public daemon D-Bus API.

## docs/PACKAGING.md

Explain:

- Debian build;
- RPM build;
- CI build;
- artifact naming.

## docs/TROUBLESHOOTING.md

Include common problems:

```text
mpv not installed
unsupported Wayland session
DISPLAY missing
daemon already running
wallpaper file missing
renderer crash
monitor not detected
wallpaper not visible
wallpaper above desktop icons
icons not clickable
fullscreen pause not working
```

## docs/MANUAL-ACCEPTANCE.md

This is especially important because the development environment is headless.

---

# 45. Required Manual Desktop Acceptance Campaign

The coding agent must WRITE this campaign.

The agent must NOT execute it on the server and must NOT fabricate results.

The campaign shall contain at least the following tests.

## LUC-T01 — Installation

Install the generated `.deb` on Linux Mint.

PASS if:

- installation succeeds;
- dependencies resolve;
- Lucerna appears in the application menu.

## LUC-T02 — First Launch

Launch Lucerna normally.

PASS if:

- main window appears;
- application remains responsive;
- no unexpected terminal is opened.

## LUC-T03 — Add Wallpaper

Add a known MP4/WebM.

PASS if:

- file appears in library;
- file may be selected.

## LUC-T04 — Apply Wallpaper

Assign video to primary monitor.

PASS if:

- animation appears as desktop background.

## LUC-T05 — Desktop Icons

With Nemo desktop icons enabled:

PASS if:

- icons remain visible;
- icons remain clickable;
- icon drag behaviour still works.

## LUC-T06 — Desktop Context Menu

Right-click empty desktop area.

PASS if:

- Cinnamon/Nemo desktop context menu still works.

## LUC-T07 — Window Stacking

Open ordinary applications.

PASS if:

- wallpaper remains behind all normal windows;
- Lucerna wallpaper does not cover panels or application windows.

## LUC-T08 — Alt+Tab / Taskbar

PASS if:

- wallpaper surface does not appear in Alt+Tab;
- wallpaper surface does not appear in taskbar.

## LUC-T09 — Fullscreen Pause

Open fullscreen application.

PASS if:

- configured renderer pauses;
- renderer resumes afterward.

## LUC-T10 — Audio Default

Apply video containing audio.

PASS if:

- wallpaper is silent by default.

## LUC-T11 — Scaling

Test Fit/Fill/Stretch/Center.

PASS if:

- each mode behaves according to documentation.

## LUC-T12 — Dual Monitor

Attach/use two displays.

PASS if:

- both are detected;
- separate wallpaper assignments work.

## LUC-T13 — Monitor Disconnect

Disconnect secondary display.

PASS if:

- daemon remains running;
- primary wallpaper remains functional;
- assignment is retained.

Reconnect monitor.

PASS if:

- assignment can be restored automatically.

## LUC-T14 — Persistence

Log out and back in.

PASS if:

- Lucerna automatically restores wallpaper if autostart is enabled.

## LUC-T15 — GUI Closure

Close Lucerna GUI.

PASS if:

- wallpaper remains active;
- daemon continues running.

## LUC-T16 — CLI

Use:

```text
lucernactl pause
lucernactl resume
lucernactl status
```

PASS if observable state matches commands.

## LUC-T17 — Missing File

Move/delete current wallpaper.

PASS if:

- Lucerna reports the missing file;
- daemon does not crash.

## LUC-T18 — mpv Failure

Temporarily make mpv unavailable or force a bad media file.

PASS if:

- error is visible;
- daemon remains recoverable.

## LUC-T19 — Login Autostart Disabled

Disable autostart.

Log out/in.

PASS if:

- Lucerna does not start automatically.

## LUC-T20 — Idle Resource Sanity

Observe system monitor with wallpaper running.

Record:

```text
CPU
GPU/video decode where available
RAM
```

No universal numeric PASS threshold is required for v1, but obviously pathological use must be treated as a defect.

---

# 46. Acceptance Evidence

Manual campaign results should be recorded later in a simple table:

```text
Test       Result      Notes
LUC-T01    PASS
LUC-T02    PASS
...
```

If the user has not performed the campaign, the agent must leave those fields:

```text
NOT RUN — REQUIRES REAL DESKTOP
```

Never invent PASS results.

---

# 47. Automated Acceptance Gates

Unlike visual tests, these CAN be executed by the server agent.

Before every tagged milestone:

```text
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace --release
```

must pass.

From v0.7.0 onward:

```text
DEB build
RPM build
package inspection
headless install smoke test
```

must pass.

From v0.8.0 onward:

the actual GitHub Actions workflow definitions must be complete.

Before `v1.0.0-rc.1`:

all server-executable gates must be green.

---

# 48. Code Quality Requirements

Avoid:

```text
unwrap()
expect()
panic!()
```

in user-triggerable production paths unless failure genuinely indicates an internal invariant violation.

Prefer typed errors.

Important errors should retain source/context.

No enormous 2,000-line `main.rs`.

No circular architecture where GTK objects directly manipulate X11 windows and mpv subprocesses.

Core logic must be testable without creating GTK widgets.

Renderer policy must be testable without actually launching mpv.

X11 backend must be separable from daemon policy.

---

# 49. Error UX

Errors presented to users should explain what happened and suggest an actionable correction.

Bad:

```text
Error 2
```

Good:

```text
Lucerna could not start mpv.

Executable "mpv" was not found in PATH.
Install mpv and restart Lucerna.
```

Bad:

```text
X11 error
```

Good:

```text
Lucerna could not connect to the X11 display ":0".

This release currently supports X11 sessions.
Your current session appears to be Wayland.
```

---

# 50. Unsupported Session Behaviour

Wayland detection is mandatory.

If:

```text
XDG_SESSION_TYPE=wayland
```

and no Wayland backend exists, Lucerna must clearly report that the current version does not support that session.

It must not:

- crash;
- silently do nothing;
- start hundreds of failed mpv processes;
- pretend wallpaper playback succeeded.

The GUI may still open in a diagnostic/settings mode if practical.

---

# 51. Renderer Recovery

If mpv crashes unexpectedly:

1. mark renderer failed;
2. log reason;
3. attempt a bounded restart.

Do not create an infinite restart storm.

Example policy:

```text
maximum 3 automatic restarts in 60 seconds
```

After the limit:

```text
Failed
```

and require user action or configuration change.

The exact values may be adjusted but must be bounded.

---

# 52. Clean Shutdown

When the daemon exits normally:

- stop mpv children;
- remove wallpaper windows;
- remove runtime sockets;
- release D-Bus name;
- flush state;
- leave Cinnamon desktop functional.

If the daemon is killed and orphaned mpv children survive, the next daemon start should attempt safe stale-process recovery.

Do not kill unrelated mpv instances started by the user.

Track child process IDs explicitly.

---

# 53. Configuration Compatibility

Once a tagged release introduces configuration schema version 1, later releases must not casually break it.

If a migration becomes necessary:

```text
schema_version = N
```

must be upgraded intentionally.

Before migration:

- parse old config;
- create backup;
- perform migration;
- atomically write new config.

Do not erase unknown settings merely because the parser does not understand them where avoidable.

---

# 54. Final v1 Repository State

At completion the repository must contain:

```text
compiling Rust workspace
unit tests
integration tests
X11 protocol tests
GTK application
daemon
CLI
Cinnamon/X11 backend
mpv renderer controller
persistent configuration
multi-monitor assignments
autostart
pause policies
diagnostics
DEB packaging
RPM packaging
GitHub Actions
release automation
documentation
manual desktop acceptance campaign
CHANGELOG
LICENSE
```

The repository must build from a fresh checkout according to `docs/BUILDING.md`.

No undocumented state from the development server may be required.

---

# 55. Final Agent Report

At the end of the campaign, provide a final report containing:

```text
Lucerna version:
Git commit:
Git tag:
Branch:
Rust version:
Build environment:
```

Then report:

### Implemented

Concise feature list.

### Automated verification

Include exact commands and their results.

### Packaging

State:

```text
DEB: PASS/FAIL
RPM: PASS/FAIL
```

and artifact paths/names.

### GitHub Actions

State which workflows exist and what triggers them.

### Manual desktop validation

Must explicitly state:

```text
NOT PERFORMED BY DEVELOPMENT AGENT.
REQUIRES REAL CINNAMON/X11 DESKTOP.
```

unless real results have subsequently been supplied by the user.

### Known limitations

List real limitations rather than hiding them.

### Next action

For `v1.0.0-rc.1`, the next action should normally be:

```text
Install the generated package on the target Mint Cinnamon machine and execute docs/MANUAL-ACCEPTANCE.md.
```

---

# 56. Future Roadmap — Do Not Implement During v1 Campaign

The architecture should leave room for later work.

Potential releases may include:

## v1.1

Quality-of-life improvements discovered from real desktop use.

## v1.2

Thumbnail cache and richer wallpaper library.

## v1.3

Profiles and scheduled wallpaper rotation.

## v1.4

More aggressive power-management policies.

## v2.0

Wayland backend work.

Potential compositor integrations:

```text
KDE Plasma
wlroots / layer-shell
Hyprland
Sway
```

## Later

Possible scene engine:

```text
HTML
WebGL
GLSL
interactive wallpapers
audio-reactive effects
cursor-reactive effects
```

Wallpaper Engine compatibility may eventually be investigated as a separate import/runtime subsystem.

It should never contaminate the simple video renderer architecture.

---

# 57. Definition of Done

Lucerna's initial development campaign is complete when:

1. all source code is committed;
2. `main` is clean;
3. all mandatory automated tests pass;
4. Rust formatter passes;
5. Clippy passes with warnings denied;
6. release build succeeds;
7. X11 backend is implemented;
8. mpv process management is robust;
9. daemon and CLI communicate over D-Bus;
10. GTK GUI is complete at the code/structure level;
11. multi-monitor configuration exists;
12. fullscreen pause exists;
13. autostart exists;
14. diagnostics exist;
15. `.deb` is generated through GitHub Actions;
16. `.rpm` is generated through GitHub Actions;
17. package smoke tests pass;
18. GitHub tagged release workflow exists;
19. SemVer version/tag consistency is enforced;
20. release checksums are produced;
21. documentation is complete;
22. `v1.0.0-rc.1` artifacts are available;
23. real visual/desktop tests are clearly marked as pending rather than fabricated.

The agent should proceed autonomously through the implementation phases.

When a minor implementation decision is ambiguous, choose the simplest maintainable solution consistent with this specification rather than stopping development for trivial confirmation.

Do not broaden scope simply because additional features are technically possible.

The objective is to reach a clean, testable and installable **Lucerna v1**, not to spend the development campaign recreating all of Wallpaper Engine.

The first real proof is simple:

**install the package on Mint, select a video, and have it quietly become the desktop without the rest of Cinnamon noticing anything unusual.**
