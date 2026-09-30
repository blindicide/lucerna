# Changelog

All notable changes to Lucerna are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.2.0] - 2026-09-30

### Added
- The Cinnamon/X11 wallpaper backend (`lucerna-x11`) behind a replaceable `WallpaperBackend`
  trait. Wallpaper surfaces are undecorated, focus-less, click-through (empty input shape),
  off the taskbar and pager, sticky on all workspaces, marked as desktop-type and below, and
  identifiable by class, name, pid and a private marker property.
- RandR display discovery: RandR 1.5 monitors with a CRTC fallback for older servers, rotation,
  refresh rate, primary flag, mirrored outputs, and EDID reading.
- Stable monitor identity from EDID (manufacturer, product code, serial) with a connector
  fallback and collision handling, so assignments survive enumeration-order changes.
- Two stacking strategies selectable through configuration: an override-redirect surface kept at
  the bottom of the stack (default) and a window-manager-managed desktop-type window, plus
  re-lowering when something disturbs the stack.
- An event thread that reports display changes, fullscreen windows (per rectangle, ignoring
  maximised, hidden and other-workspace windows), stacking disturbances and loss of the display
  connection, with debouncing and no polling.
- Cinnamon, Muffin, compositor and Nemo probes and structured diagnostics (window ids, stacking
  positions, whether each surface sits below Nemo's desktop window), plus a read-only display
  probe for `lucernactl doctor`.
- Pure fullscreen-to-monitor mapping, and a scriptable fake backend for testing the daemon
  without an X server.
- X11 protocol test suite under Xvfb: connection, RandR enumeration including virtual monitors,
  every window property, click-through event routing with a negative control, resize, restack,
  a simulated Nemo window, fullscreen detection, hotplug, cleanup, connection loss and reconnect,
  and mpv embedding into a surface. These are protocol tests only.
- `docs/MANUAL-ACCEPTANCE.md`, the 20-test manual desktop campaign (LUC-T01 to LUC-T20), and
  `docs/X11-CINNAMON-NOTES.md`, which lists every assumption about Cinnamon and Nemo together
  with the diagnostic field that confirms or refutes it. Every result is "NOT RUN — REQUIRES REAL
  DESKTOP".

### Notes
- Desktop appearance, icon layering, click-through on a real Cinnamon session, window stacking
  and per-monitor fullscreen behaviour are IMPLEMENTED — MANUAL DESKTOP VALIDATION REQUIRED. The
  Xvfb tests prove protocol behaviour only.
- Virtual RandR monitors created with `SetMonitor` do not emit change events on Xvfb; real
  output and mode changes do.

## [0.1.0] - 2026-09-30

### Added
- Renderer core. A pure, table-tested state machine (`Stopped`, `Starting`, `Playing`,
  `Paused`, `Stopping`, `Failed`) decides what happens after every event; a supervisor executes
  its decisions by spawning mpv without a shell, talking to it over a private IPC socket and
  reaping it.
- Pause, resume and stop, with a polite `quit` first and escalation to `SIGTERM` and `SIGKILL`.
- Crash detection with a bounded restart policy: at most three automatic restarts in a
  60-second window with 1, 2 and 4 second back-off, after which the renderer stays `Failed`
  until a user action. Deterministic failures (unplayable file, missing file, missing mpv,
  mpv initialisation error) are reported immediately and never retried.
- mpv discovery (`LUCERNA_MPV`, then `PATH`), version parsing and an option-compatibility probe
  that checks the installed mpv accepts every option Lucerna can emit.
- The mpv argument builder, including audio-off-by-default, hardware decoding and FPS cap
  options and the four scaling modes (fill, fit, stretch, center), which can be changed on a
  running renderer without a restart.
- Bounded, rotating renderer logs (at most 1 MiB per output) and an in-memory tail of mpv's
  stderr used in failure messages.
- A PID registry with start times and stale-process recovery that terminates only renderers
  Lucerna itself started, never unrelated mpv instances.
- A private runtime directory (mode 0700, ownership checked) for control sockets; no fallback to
  `/tmp`.
- Atomic file writes, used by the registry and reusable for configuration.
- Test infrastructure: a fake mpv whose behaviour is chosen by the media file (crashes, hangs,
  unsupported files, ignored signals, stderr floods), tiny synthetic media fixtures, and tests
  against the real mpv with the null video output for mp4, webm, mkv and gif.
- Architecture documentation of the renderer, including the full mpv argument table.

### Notes
- No desktop integration exists yet: nothing is drawn onto a desktop until the X11 backend
  lands. All behaviour here is verified without a display.

## [0.0.1] - 2026-09-30

### Added
- Cargo workspace with eight crates that encode Lucerna's architecture boundaries: core logic,
  mpv supervision, the X11 backend, the D-Bus contract, the daemon, the CLI, the GTK
  application and a test kit. The root `Cargo.toml` is the single source of the version.
- Three binaries, `lucerna`, `lucernad` and `lucernactl`, that report the workspace version
  through `--version` and print `--help` without needing a graphical display. `lucerna`
  checks for a graphical session before starting GTK and explains what is missing instead
  of failing with a panic.
- Shared logging set-up controlled by `LUCERNA_LOG` or `RUST_LOG` (default `lucerna=info`)
  and XDG path resolution.
- An architecture test that fails the build when a crate gains a forbidden dependency, when
  pure code mentions GTK, X11, D-Bus or an async runtime, or when a `main.rs` grows large.
- Continuous integration for formatting, Clippy (warnings denied), tests, release builds,
  headless smoke tests, dependency auditing and shell script checks.
- `scripts/version.sh`, which maps the workspace version to Debian, RPM and tarball naming.
- README, build documentation and an architecture overview.
