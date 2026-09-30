# Changelog

All notable changes to Lucerna are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.8.0] - 2026-09-30

### Added
- Automated releases. Pushing a `v*` tag runs the `release` workflow, which checks out the exact
  tag, verifies it is annotated and equals the Cargo workspace version, runs the full CI suite,
  builds and smoke-tests the `.deb` and `.rpm`, makes the source archive, writes `SHA256SUMS`, and
  creates the GitHub Release (a pre-release for `-rc` versions) with the notes taken from this
  changelog and all four files attached.
- `scripts/check-tag.sh` and `scripts/changelog-section.sh`, with `tests/scripts/release_test.sh`.

### Changed
- A release is published only if every job succeeds; a failed package build produces no release.
  A mismatching tag fails the run instead of rewriting package metadata.

### Notes
- Packages built by the pipeline are unsigned; `SHA256SUMS` protects against corrupted downloads
  only.

## [0.7.0] - 2026-09-30

### Added
- Native packages. A Debian package built and tested on Ubuntu 24.04 (Linux Mint 22's base) and
  an RPM built and tested on Fedora 44, each compiled in its own distribution rather than
  converted. Both install the same files: the three programs, their man pages, a desktop entry,
  a scalable icon and AppStream metadata, plus the documentation and license.
- Dependency metadata: `mpv` is required, the GTK 4 libraries are detected from the binaries (no
  private GTK is bundled), and a D-Bus session bus is recommended on Debian.
- Package builders (`scripts/build-deb.sh`, `scripts/build-rpm.sh`) and a source-archive script.
  The Debian changelog, RPM changelog, AppStream release entry and man-page headers are generated
  from the single workspace version and the dated CHANGELOG heading, so no second version exists.
- An install/uninstall smoke test that runs in a fresh container without a display or session
  bus: files installed, `--version` and `--help` of all three programs, a clear message from
  `lucerna` without a display, `lucernactl doctor` seeing an mpv that accepts every option
  Lucerna passes, and removal that leaves the user's configuration alone.
- A `packages` GitHub Actions workflow that builds and smoke-tests both packages, and
  `docs/PACKAGING.md`.
- Manual pages for `lucerna`, `lucernad` and `lucernactl`.

### Notes
- The icon, desktop entry and menu presence have not been seen on a desktop (LUC-T01, LUC-T02).

## [0.6.0] - 2026-09-30

### Added
- Login handling. At session start the daemon waits, within a bound, for the display, the X
  server and then the window manager before it creates any wallpaper surface, and warns and
  continues if no window manager ever appears. The autostart entry is created on the first run
  only; an entry the user removed or disabled (including from Cinnamon's Startup Applications) is
  never brought back.
- A bounded daemon log, `lucernad.log` (two files of at most 512 KiB), next to the journal output.
  The events the directive lists are logged concisely: daemon start and stop, backend selection,
  displays, wallpaper assignment, renderer launch, exit, pause and resume, reloads, and failures.
  A test proves that polling the daemon adds no log lines.
- Recovery tests that cover the whole path: a renderer that crashes twice and recovers, a crash
  storm that stops at the restart limit and is recovered with Start, an unplayable file that
  fails once with an explanation, a wallpaper on a drive that comes back by itself, and mpv
  installed after the daemon started.
- Process-level tests for SIGHUP (logout), a killed daemon (the kernel stops mpv) and a killed
  daemon whose renderer survives (the next daemon terminates it by pid, start time, executable
  and socket), and a test showing an unrelated mpv is never touched.
- The command-line status and doctor output now show a corrupt-configuration notice and an
  mpv-missing explanation, covered by end-to-end tests.

### Changed
- A renderer that fails for good no longer keeps a black surface over the desktop background;
  the surface is removed until the user acts (Start, Reload, or choosing a wallpaper).
- Renderer exits are logged.

### Notes
- Login ordering, logout, autostart across sessions and behaviour on a real Cinnamon session
  (LUC-T14, T17, T18, T19) remain IMPLEMENTED — MANUAL DESKTOP VALIDATION REQUIRED.

## [0.5.0] - 2026-09-30

### Added
- Per-display wallpapers and scaling. A display can override the all-displays wallpaper and
  scaling; `SetWallpaper`, `ClearAssignment` and `SetScaling` take a display id, and
  `lucernactl play --monitor` accepts a display id or a connector name such as `HDMI-1`. The
  Displays page has a wallpaper and a scaling choice on each row, including "Same as all
  displays".
- Displays report where their scaling comes from (`scaling_source`), so clients can tell an
  override from an inherited value.
- Display hotplug: displays that appear get their wallpaper, displays that disappear lose only
  their own renderer, and a display whose geometry changes has its surface resized without a
  restart. Assignments follow the stable display identity, not the order in which the X server
  lists monitors.
- Absent displays keep their assignment: they are listed as disconnected with their last-seen
  name, nothing runs for them, and their wallpaper returns automatically when they do.
- Fullscreen pause per monitor: only the display covered by a fullscreen window pauses, a window
  spanning two displays pauses both, and windows that merely cover part of a display or are only
  maximised never pause anything.
- Screen-lock pause, using the first available source among `org.cinnamon.ScreenSaver`,
  `org.freedesktop.ScreenSaver` and logind's `LockedHint`. Pause reasons (user, lock, fullscreen)
  combine, and a renderer created while one holds starts paused.
- A restack watchdog with a rate limit, so Lucerna cannot fight the window manager for the
  bottom of the window stack; suppressed requests are reported by `doctor`.
- Multi-monitor and policy tests against the whole daemon: per-display assignments and scaling,
  unplugging and re-plugging a display, enumeration-order independence, resizing, per-monitor
  fullscreen, all three lock sources, combined pause reasons and the restack limit.

### Fixed
- The mpv test double could interleave the log lines of several processes writing the same file.
- Executing a script while another thread forks no longer fails with "text file busy" (a brief
  retry while probing mpv, which also helps while mpv is being upgraded).

### Notes
- Per-monitor fullscreen detection uses the window manager's client list only. Fullscreen windows
  that bypass the window manager (some older games) are not detected; use `lucernactl pause`.
- How any of this behaves on a real Cinnamon desktop (LUC-T09 fullscreen pause, LUC-T12 two
  monitors, LUC-T13 monitor disconnect) is IMPLEMENTED — MANUAL DESKTOP VALIDATION REQUIRED.

## [0.4.0] - 2026-09-30

### Added
- The `lucerna` GTK 4 control application: a sidebar with the Wallpapers, Displays, Settings and
  About pages, a header menu (Pause, Resume, Reload, Quit service) and a banner for problems.
- Wallpapers page: the library with a "Missing" badge for files that cannot be found, *Add…*
  through a file chooser filtered to videos and animated images (with an "All files" choice),
  *Remove from library* behind a confirmation that says the file will not be deleted, *Set on all
  displays* and *Stop wallpaper*.
- Displays page: the wallpaper and scaling (fill, fit, stretch, center) for all displays, and the
  detected displays with readable names such as "eDP-1 — 1920×1080 — Primary", including
  displays that are currently unplugged.
- Settings page: start automatically, pause when fullscreen, pause when the screen is locked,
  hardware decoding, FPS limit, audio (behind an explicit confirmation, so it is never turned on
  silently) and, under Advanced, window stacking.
- About page: version (from the workspace), description, license, repository and the runtime
  backend reported by the daemon.
- Daemon control from the GUI: it starts the service itself when it is not running (in its own
  process group so the wallpaper survives closing the window), notices when the service appears or
  disappears, and follows the daemon's change signals for immediate updates.
- Error handling: the daemon's own messages (what happened, why, what to do) appear in a banner;
  a missing mpv, an unsupported session, configuration problems and failed renderers each have
  their own text and a matching action.
- A structural test that builds the real window on an Xvfb display against a real daemon and
  checks the pages, the controls and their wiring. It does not, and cannot, check appearance.

### Fixed
- A client that kept calling the daemon while it was shutting down could stop the daemon from
  ever finishing its shutdown. Requests that arrive during shutdown are now answered with an
  error immediately.

### Notes
- Visual quality of the GUI is NOT VALIDATED ON DEVELOPMENT SERVER: layout, spacing, fonts,
  theming and the banner colours have not been seen on a desktop (LUC-T02, LUC-T03).
- Choosing a different wallpaper for each display arrives with the multi-monitor release.

## [0.3.0] - 2026-09-30

### Added
- `lucernad`, the per-user service. One engine task owns configuration, the wallpaper library,
  the display backend and the renderers, and makes every start/stop decision by comparing what
  should run (configuration, connected displays, file availability, pause policy) with what
  runs. Renderers get their surface only when they can really start: a missing mpv or a
  missing file never leaves a black window over the normal desktop background.
- Single-instance enforcement in two independent layers, a file lock in the runtime directory and
  ownership of the session-bus name. A second `lucernad` prints who is running and exits with
  status 0.
- The `org.lucerna.Lucerna1` session D-Bus API: status, displays, assignments, wallpaper library
  management, scaling, settings, pause/resume/stop/start/reload/quit, diagnostics, change
  signals, and specific errors with actionable messages. Documented in `docs/IPC.md`.
- `lucernactl`: `status`, `monitors`, `wallpapers`, `play`, `pause`, `resume`, `stop`
  (`--daemon` to quit the service), `reload` and `doctor`, with `--json` output and meaningful
  exit codes (3 when the daemon is not running, 4 for bad requests, 5 unsupported session, 6
  configuration problems, 7 mpv missing).
- Configuration schema version 1: lenient reading (unknown keys ignored and preserved together
  with comments, unrecognised values fall back with a warning), atomic writes, a migration
  framework with automatic backups, a corrupt file moved aside instead of overwritten, and a
  file written by a newer Lucerna treated as read-only.
- The wallpaper library: absolute canonical paths, idempotent adds, removal that never touches
  the media, and availability tracking for files on drives that come and go.
- The pure policy pieces used by the daemon: the pause policy, the reconciliation planner and
  session classification with a clear message for Wayland sessions and for a missing display.
- `lucernactl doctor`: environment (through an allow-list), mpv and option compatibility,
  configuration and autostart state, the daemon's own diagnostics, and a read-only X11 probe when
  the daemon cannot be asked. `--redact` hides the home directory, user name, host name, wallpaper
  file names and monitor serial numbers.
- Autostart as a single per-user XDG autostart entry, created on first run and switched from the
  settings API. Disabling it removes the only startup mechanism.
- Clean shutdown on SIGTERM, SIGINT, SIGHUP, `Quit` or loss of the display connection: renderers
  are stopped, wallpaper windows removed, sockets and registry cleaned, the bus name and lock
  released.
- Documentation: the D-Bus API, the configuration reference, troubleshooting for the common
  problems, and a test matrix tying each requirement to the tests that exercise it.
- Integration test suites that run the whole daemon against a private D-Bus, a fake backend and a
  fake mpv, run the real `lucernad` and `lucernactl` binaries (including against Xvfb), and cover
  duplicate launch, signals, logout, unsupported sessions, corrupt configuration and missing
  files.

### Changed
- Assigning a wallpaper to one specific display is not available yet through the API; assign to
  all displays with `*`. Per-display assignment arrives with the multi-monitor release.

### Fixed
- The daemon must not take over the bus name from a running daemon (the D-Bus library replaces
  the current owner by default); the name request is now non-replacing.

### Notes
- The GUI is still a stub, and nothing has been validated on a real desktop: wallpaper
  appearance, icon layering, click-through, stacking and per-monitor fullscreen behaviour remain
  IMPLEMENTED — MANUAL DESKTOP VALIDATION REQUIRED.

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
