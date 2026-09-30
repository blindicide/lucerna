# Architecture

Lucerna is a native control application plus a small per-user daemon, with `mpv` doing the
decoding and rendering. Each section says which milestone implemented it; desktop-facing behaviour is marked
as needing manual validation.

## Components

- **`lucernad`** - the per-user service. Owns configuration, display assignments, the
  wallpaper surfaces and the mpv renderer processes. One instance per user session.
- **`lucerna`** - the GTK 4 control application. Never owns a renderer; closing it does not
  stop the wallpaper.
- **`lucernactl`** - the CLI controller. A thin D-Bus client, never a second engine.

The daemon is the only writer of `config.toml`. The GUI and CLI change configuration only
through the daemon's session-bus API.

## Workspace and crate boundaries

```text
crates/
  lucerna-core     pure logic + domain types + the WallpaperBackend trait
  lucerna-mpv      mpv process supervision + JSON IPC
  lucerna-x11      WallpaperBackend implementation for X11 / Cinnamon
  lucerna-ipc      D-Bus contract shared by daemon, CLI and GUI
  lucerna-daemon   library + bin `lucernad`
  lucerna-cli      library + bin `lucernactl`
  lucerna-ui       library + bin `lucerna` (GTK 4)
  lucerna-testkit  fake mpv, private D-Bus, Xvfb harness, integration suites (never shipped)
```

Allowed dependencies between workspace crates (everything else is forbidden):

| Crate | May depend on |
| --- | --- |
| lucerna-core | none |
| lucerna-mpv, lucerna-x11, lucerna-ipc | core |
| lucerna-daemon | core, mpv, x11, ipc |
| lucerna-cli | core, ipc, x11 (read-only probing for `doctor`) |
| lucerna-ui | core, ipc |
| lucerna-testkit | anything (tests only) |

Forbidden external crates, checked on direct dependencies:

| Crate | Must not use |
| --- | --- |
| core | gtk4, glib, gio, x11rb, zbus, tokio, rustix |
| mpv | gtk4, x11rb, zbus |
| x11 | gtk4, zbus, tokio |
| ipc | gtk4, x11rb, tokio |
| daemon | gtk4, glib, x11rb |
| cli | gtk4, tokio |
| ui | x11rb, tokio, rustix |

The consequences are structural rather than a matter of discipline:

- Core logic is testable without a GTK widget, a display, an X server or a D-Bus.
- The daemon cannot name an X11 type; it holds a `dyn WallpaperBackend`.
- The GUI cannot link the X11 backend or the mpv supervisor.

`crates/lucerna-testkit/tests/architecture.rs` runs with `cargo test --workspace` and fails,
naming the broken rule, if any of this is violated.

## Conventions

- **Errors:** typed with `thiserror` in libraries, `anyhow` only at the top level of
  binaries. User-facing text explains what happened, why, and what to do.
- **No shell, ever:** processes are spawned with argument vectors.
- **Logging:** `tracing`; `LUCERNA_LOG` or `RUST_LOG` controls the filter (default
  `lucerna=info`). Nothing logs per frame or per poll.
- **Version:** exactly one, in the root `Cargo.toml`.

## Renderer (implemented in v0.1.0)

Video decoding and presentation are delegated to mpv. Lucerna owns the mpv *process*; it never
implements a decoder.

### Layers

| Layer | Crate | Responsibility |
| --- | --- | --- |
| Policy | `lucerna-core::renderer` | Pure state machine and restart policy. Takes an event and the current time, returns *effects*. Never starts a process or reads a clock. |
| Arguments | `lucerna-core::mpv::args` | Pure function producing the mpv argument vector. |
| Discovery | `lucerna-core::mpv::{discovery, compat}` | Finds `mpv`, reads its version, checks that it accepts every option Lucerna can emit. |
| Execution | `lucerna-mpv` | `RendererSupervisor`: one task per output. Spawns mpv, talks to its IPC socket, captures its output, arms timers and performs the machine's effects. |
| Bookkeeping | `lucerna-mpv::registry` | PID registry and stale-process recovery. |

### States

`Stopped`, `Starting`, `Playing`, `Paused`, `Stopping`, `Failed` - exactly one enum value per
renderer, never a set of booleans. Every event that belongs to a particular process carries that
process's *generation*; events from an earlier process are answered with `Ignored` and can never
disturb the current one. Invalid or redundant requests are either *normalized* (pausing a paused
renderer is a no-op) or *rejected* (starting while stopping).

The complete state x event table is asserted cell by cell in
`lucerna-core/src/renderer/machine.rs` (`transition_table`).

Failure reasons have stable kebab-case codes used over D-Bus and in `doctor`: `mpv-missing`,
`launch-failed`, `media-missing`, `media-unsupported`, `crashed`, `unexpected-exit`,
`startup-timeout`, `ipc-failed`, `restart-limit`.

* **Deterministic failures are not retried.** mpv exit status 1 (initialisation error), exit
  status 2 or an `end-file` error (the file cannot be played), a missing file and a missing
  `mpv` all go straight to `Failed`. Retrying would only repeat the failure.
* **Crashes are retried, boundedly.** A signal, an unexpected exit 0 (a looping wallpaper never
  ends), an unexpected exit code, a lost control socket and a startup timeout follow the *crash
  rule*: up to 3 restarts inside a sliding 60 s window, with 1 s, 2 s and 4 s back-off. The 4th
  failure becomes `Failed(restart-limit)` and stays there until a user action (`Start`,
  `Reload`, a new wallpaper). The limits are configurable but clamped (1-10 restarts, 10-3600 s),
  so the policy can never be unbounded.
* **A late startup timer cannot kill a healthy renderer.** `StartupTimedOut` is only honoured
  while `Starting`.
* **A closed control socket waits 300 ms for the process exit** before it counts as an IPC
  failure, because a crash closes the socket and ends the process, and the exit is the truer cause.

### mpv argument vector

Built by `build_args`; passed as individual process arguments, never through a shell. The media
path is the single argument after `--`, is always absolute, and is never quoted or interpolated,
so a file called `$(rm -rf ~).mp4` is only a filename (directive §26).

| Argument | Why |
| --- | --- |
| `--no-config` | Ignore the user's `mpv.conf`, `input.conf` and scripts (§16). |
| `--no-input-terminal` | Do not read stdin. |
| `--quiet`, `--msg-level=all=warn`, `--msg-color=no` | Warnings and errors stay on stderr for the bounded diagnostic log. `--really-quiet`/`--no-terminal` are deliberately *not* used: they would suppress the very output §9 asks us to capture. |
| `--no-input-default-bindings`, `--input-vo-keyboard=no`, `--input-cursor=no`, `--cursor-autohide=no`, `--input-media-keys=no` | The video window takes no input and does not touch the cursor or media keys (§14). |
| `--osc=no`, `--osd-level=0` | No on-screen controller or text. The built-in OSC still loads under `--no-config`. |
| `--load-scripts=no`, `--load-stats-overlay=no`, `--load-osd-console=no`, `--load-auto-profiles=no` | No user or built-in helper scripts. |
| `--ytdl=no` | No network helpers (§26). |
| `--resume-playback=no` | Do not read watch-later state. |
| `--stop-screensaver=no` | Without it the wallpaper would keep the screen from locking or blanking for as long as it plays. |
| `--x11-bypass-compositor=never` | Never ask the compositor to unredirect. |
| `--loop-file=inf`, `--idle=no`, `--force-window=no` | Loop forever; exit if the file ends or fails so the supervisor can classify it. |
| audio off (default): `--aid=no --mute=yes`; on: `--aid=auto --mute=no --volume=100` | `--aid=no` skips audio decoding entirely; the volume is explicit so nothing is inherited (§16). |
| `--hwdec=auto-safe` (Auto) / `--hwdec=no` (Disabled) | §17. |
| `--vf=fps=N` (60/30/15; nothing for Native) | Frames are dropped before rendering, which reduces upload and presentation work. It does **not** reduce decode work. |
| `--keepaspect`, `--panscan`, `--video-unscaled` | Scaling mode, see below. |
| `--pause=yes|no` | A surface that should start paused never shows a frame of motion. |
| `--input-ipc-server=<socket>` | Private control socket. |
| `--wid=<xid>` | Only when the backend provided an embed target. |
| `--vo=<x>` | Test builds only. |
| `--` then the media path | End of options. |

**Scaling modes** (deterministic; documented for users in `docs/CONFIGURATION.md`):

| Mode | `keepaspect` | `panscan` | `video-unscaled` | Result |
| --- | --- | --- | --- | --- |
| fill | yes | 1.0 | no | Covers the output, keeps aspect ratio, crops the overflow. |
| fit | yes | 0.0 | no | Whole video visible, letterboxed in black. |
| stretch | no | 0.0 | no | Covers the output and distorts the aspect ratio. |
| center | yes | 0.0 | yes | Native pixel size, centred. |

A scaling change on a running renderer is three `set_property` commands; no restart.

**Option compatibility (§9).** mpv exits with an error at option-parsing time when it does not
know an option or a value, even with `--list-options`. `check_compat` therefore runs
`mpv --no-config <every option we can emit> --list-options` and checks the exit status. Tests:
the exact vectors for all setting combinations (`args::tests`), the probe against the installed
mpv (`real_mpv::options_supported`), and the fake mpv, which rejects any option not declared in
`EMITTED_OPTIONS` so the builder and its test double cannot drift.

### Control socket

* Directory `$XDG_RUNTIME_DIR/lucerna/`, created 0700 and verified on every start (a directory,
  owned by us, mode exactly 0700). Without `XDG_RUNTIME_DIR`, `/run/user/<uid>` is used only if it
  passes the same checks. Lucerna never falls back to `/tmp` (§26).
* Socket file `mpv-<slot>-<generation>.sock`. The slot is 8 hex characters of FNV-1a-64 of the
  output id; the generation means a new process can never collide with a dying one. Path length
  is checked against the 107-byte `sun_path` limit.
* A socket file left over at the target path is removed before each spawn, and again when the
  process is reaped.
* mpv is asked to `quit` politely; if it ignores that, `SIGTERM` after 2 s, `SIGKILL` after 2 more.
  Signals are delivered from inside the task that owns the `Child`, so they can never reach a pid
  that has already been reaped and reused.

**Readiness.** A renderer is `Playing` when the control socket is connected *and* the file is
loaded. mpv can finish loading before the client connects, so its `file-loaded` event may be
missed. After connecting, the supervisor asks for `time-pos`: mpv only answers once playback has
been initialised (before it sends the event), so an answer means "already loaded" and an error
means the event is still to come. This is pinned on the real mpv by
`real_mpv::time_pos_answers_once_loaded_even_for_a_late_client`, in both playing and paused start
modes.

### Output capture

Both streams are piped. Each line is truncated to 4 KiB; the last 200 stderr lines are kept in
memory for failure messages; everything goes to `renderer-<slot>.log` under
`$XDG_STATE_HOME/lucerna/logs`, which rotates at 512 KiB to a single `.1` file (so at most 1 MiB
per output). Lines are **not** forwarded to `tracing` one by one: only the last five stderr lines
are attached to a failure log entry, so a noisy renderer cannot flood the journal (§25).

### Ownership and stale-process recovery (§52)

Every spawn is recorded in `$XDG_RUNTIME_DIR/lucerna/renderers.json` (atomically) with the pid
and the process start time (`/proc/<pid>/stat` field 22). When a daemon starts, it terminates a
leftover renderer only if **all** of these hold: the pid is alive, its start time matches, its
executable is named `mpv`, and its command line carries our private `--input-ipc-server=<socket>`.
An mpv the user started can never match. Before escalating to `SIGKILL` the identity is checked
again.

As defence in depth children are spawned with `PR_SET_PDEATHSIG=SIGTERM` - the single `unsafe` block
in the workspace (`die_with_parent`, with a `SAFETY:` comment). `PDEATHSIG` is tied to the
*thread* that spawned the child, not the process; the daemon spawns from its long-lived runtime
thread, so the usual caveat does not bite.

### Tests

* Pure unit tests: the transition table, restart windows and back-off, argument vectors.
* `lucerna-testkit/tests/renderer_supervision.rs`: the fake mpv (`fake-mpv`), whose behaviour comes
  from the first line of the media file: play, pause/resume, stop escalation, crash storm
  (exactly 4 spawns, then quiet), recovery after two crashes, unsupported media, missing media,
  missing mpv, hung startup, lost control socket, stderr flood within the log bound, stale
  sockets, stale-process recovery sparing an unrelated renderer.
* `lucerna-testkit/tests/real_mpv.rs`: the installed mpv with `--vo=null` on the fixtures in
  `tests/fixtures/media` (mp4, webm, mkv, gif, and a corrupt file).

None of this says anything about how a wallpaper *looks*; that is the manual acceptance campaign.

## X11 backend (implemented in v0.2.0)

`lucerna-x11` implements `WallpaperBackend` (`lucerna-core::backend`): `probe`,
`enumerate_outputs`, `create_surface`, `resize_surface`, `destroy_surface`, `refresh`,
`subscribe`, `diagnostics`, `shutdown`. The daemon will hold a `Box<dyn WallpaperBackend>` and can
never name an X11 type; the GUI cannot link the backend at all. `lucerna_core::testing::FakeBackend`
lets daemon policy be tested without an X server.

* **Threads.** `subscribe` starts a *reader* thread blocked in `wait_for_event` and a *processor*
  thread that sleeps until the next debounce deadline (RandR 500 ms, fullscreen 150 ms, stacking
  100 ms). An idle desktop costs no wake-ups. Shutdown wakes the reader through an event sent to a
  private window.
* **Events** delivered to the sink: `OutputsChanged`, `FullscreenChanged(rects)`,
  `StackingDisturbed`, `ConnectionLost`.
* **Kind.** `cinnamon-x11` when `XDG_CURRENT_DESKTOP` contains Cinnamon or the window manager is
  Muffin, otherwise `x11-ewmh` (best effort, not an acceptance target).
* **Doctor.** `lucerna_x11::probe_display` connects, describes the session and disconnects
  without creating anything.

What the design assumes about Cinnamon and Nemo, and how to check each assumption, is in
`docs/X11-CINNAMON-NOTES.md`. The manual desktop campaign is `docs/MANUAL-ACCEPTANCE.md`.

## Daemon, CLI and D-Bus (implemented in v0.3.0)

### `lucernad` startup and shutdown

1. Parse arguments, initialise logging.
2. Create/verify the private runtime directory (0700, ours). Without one, renderers are refused
   and the reason is reported, but the daemon still serves D-Bus.
3. **Single instance, guard one:** non-blocking `flock` on `daemon.lock` (the file holds the PID).
4. **Guard two:** connect to the session bus and take `org.lucerna.Lucerna1` *without* replacing an
   existing owner (zbus would replace by default; a test caught this). The interface is served at
   the same moment, so early calls queue until the engine is ready.
5. Terminate stale renderers from a crashed predecessor (registry + start time + `mpv` executable
   + our socket path on the command line), remove stale `mpv-*.sock`.
6. Load configuration and `state.json` (never fails: problems become notices), find mpv.
7. Classify the session; wait up to 10 s for `DISPLAY`; connect the X11 backend; enumerate
   displays; reconcile.

Shutdown (SIGTERM, SIGINT, SIGHUP, `Quit`, or the display connection being lost): stop every
renderer in parallel with the quit → TERM → KILL escalation, destroy the wallpaper windows,
remove sockets and the registry, flush state, release the bus name, release the lock, exit 0.

### The engine

One `Engine` task owns configuration, library, backend, renderers and policy inputs. It handles one
message at a time (D-Bus commands with a `oneshot` reply, backend events, renderer events, lock
changes, signals, timers), so nothing needs a lock and the GUI, CLI and daemon cannot lose each
other's updates. The zbus interface object holds no state; it only turns calls into messages.

Everything that starts or stops a renderer goes through **reconciliation**: compute the desired set
of renderers from configuration, connected displays, file availability and pause policy (pure
functions in `lucerna-core::{plan, policy}`), diff it against what is running, and apply the
resulting `Destroy`, `Replace`, `Create`, `Resize`, `SetScaling` and `SetPaused` actions.

Design decisions worth knowing:

* **A failed renderer never leaves a black window over your normal background.** No surface is
  created when mpv or the file is missing; after a terminal failure the surface is removed.
* **Failures that retrying cannot fix are terminal** (see the renderer section) and wait for a user
  action: `Start`, `Reload` or choosing a wallpaper.
* **The library flags are honest:** `available` is rewritten only when it changes.
* **`Reload` starts over completely:** stop renderers, re-read configuration, re-probe mpv,
  reconnect the backend, reconcile.
* **Stacking fights are bounded:** at most five `refresh()` calls per ten seconds, then one per ten
  seconds with a single warning; `doctor` reports the count.
* **Per-display assignment** (enabled in v0.5.0): a display id keys an override in `[displays]`;
  a display without an override follows `[all_displays]`.

### Configuration

Schema v1 in `lucerna-core::config`. Reading is lenient and keeps the `toml_edit` document; saving
writes only the fields that changed. See `docs/CONFIGURATION.md` for the rules.

### D-Bus and CLI

The contract lives in `lucerna-ipc` (names, `a{sv}` dictionaries and their typed views, errors and
exit codes, and the generated async/blocking proxy) and is documented in `docs/IPC.md`. `zbus` is
used without its `tokio` feature so the GTK client never needs to host a tokio runtime; the daemon
uses zbus's own executor threads and talks to its tokio engine through channels.

`lucernactl` is a blocking client: text or `--json` output, meaningful exit codes, `doctor` that
works with or without a daemon and never modifies anything.

## Desktop lifecycle (implemented in v0.6.0)

**Login.** The autostart entry starts `lucernad` early in the session, possibly before the X
server accepts connections or before the window manager (Muffin) or `nemo-desktop` exist. The
daemon therefore waits (bounded, `LUCERNA_DISPLAY_WAIT_MS`, default 10 s) first for `DISPLAY`, then
for the X server, then for `_NET_SUPPORTING_WM_CHECK`, and only then creates surfaces. If the window
manager never appears it proceeds with a warning; if `nemo-desktop` maps later, the stacking
watchdog re-asserts the bottom position. D-Bus requests that arrive during this wait are answered as
soon as the daemon is ready. The autostart entry is created on the first run only, and an entry the
user removed or disabled (also from Cinnamon's *Startup Applications*) is never brought back.

**Logout and crashes.** SIGTERM, SIGINT and SIGHUP, a `Quit` request, and the X server
disappearing all run the same clean shutdown. If the daemon is killed outright:

1. the kernel stops mpv (`PR_SET_PDEATHSIG`), and X destroys the windows of a client that vanished;
2. if a renderer nevertheless survives (it ignores SIGTERM), the next daemon finds it in
   `renderers.json` by pid, process start time, executable name and its private socket path, and
   terminates it before starting anything. An mpv that does not match every one of those is never
   touched.

**Renderer failure policy.** A crash is retried at most three times within sixty seconds (1, 2 and
4 second back-off); then the renderer stays failed with `restart-limit` until the user acts.
Deterministic failures (missing mpv or file, unplayable file, mpv initialisation error) are never
retried. While retries are pending the surface stays; once a failure is terminal the surface is
removed so the normal desktop background shows. Recovery paths: `Start` or `lucernactl reload`
(renews the budget), choosing a wallpaper, or - for a missing file - the file reappearing
(checked every 60 s, only while something is missing). Every failure is announced with a
`RendererFailed` signal, shown in `lucernactl status`, the GUI banner and `doctor`, and remembered
in `state.json`.

**mpv missing.** Reported at start and on every reload with the actionable message; no surface is
created. Installing mpv and choosing Reload is enough - no restart of the daemon is needed.

**Configuration problems** never stop the daemon: a corrupt file is moved aside and defaults are
used, a newer schema is read-only, an unreadable file is not overwritten; each is reported by
`lucernactl status`, the GUI banner and `doctor`.

**Logging (§25).** `lucernad` logs to stderr (the journal) and to
`$XDG_STATE_HOME/lucerna/logs/lucernad.log`, which is bounded (two files of at most 512 KiB).
Events at `info`: daemon start and stop, backend selection, each detected display, display changes,
wallpaper assignment, renderer start, launch (with pid), pause and resume (with the display),
renderer exit, configuration reload and screen-lock source. Failures are `warn`. Nothing is logged
per frame or per poll: a test proves that hundreds of status calls add no log lines. mpv's own
output goes to separate bounded per-display logs and only its last few lines reach the daemon log,
attached to a failure.

## Multi-monitor and pause policies (implemented in v0.5.0)

**Identity.** Assignments are keyed by the stable display id (EDID-based, connector fallback), never
by position, so a different enumeration order changes nothing. A configured display that is
unplugged is listed with `connected = false` and its last-seen label; nothing runs for it and its
assignment is kept. When the id reappears, the reconciler creates its surface and renderer again,
without touching the other displays.

**Hotplug.** The backend debounces RandR changes (500 ms) into one `OutputsChanged`; the engine
re-enumerates and reconciles: new displays get renderers, removed ones lose theirs, moved or
resized ones get their surface resized (mpv follows its parent window, so no restart), and only a
change of launch-time options (file, hardware decoding, FPS cap, audio) restarts a renderer.

**Pause policy** (`lucerna-core::policy`, pure). A renderer is paused if any reason holds:

| Reason | When |
| --- | --- |
| `user` | `Pause` was called and `Resume` has not been |
| `lock` | the screen is locked and `pause_on_lock` is on |
| `fullscreen` | `pause_on_fullscreen` is on and a fullscreen window covers **this** display |

Reasons combine (`Resume` clears only the user's), and a renderer created while a reason holds
starts paused (`--pause=yes`), so no frame of motion is shown behind a lock screen or a fullscreen
window. Maximised windows never pause anything.

**Per-monitor fullscreen.** The backend reports the rectangles of visible fullscreen windows on the
current workspace; a display is occluded when one rectangle covers at least 90 % of it, so a window
spanning two displays pauses both and a window on one pauses only that one. *Limitation (D13):*
detection uses EWMH only (`_NET_CLIENT_LIST` and `_NET_WM_STATE_FULLSCREEN`); fullscreen windows
that bypass the window manager (override-redirect, some old games) are not detected. If the window
manager publishes neither atom, `fullscreen_detection` is false and the setting has no effect;
`lucernactl status` says so.

**Screen lock detection** probes, in order, and uses the first that answers: the session bus
`org.cinnamon.ScreenSaver` (`GetActive`, `ActiveChanged`), then `org.freedesktop.ScreenSaver`, then
logind's `LockedHint` for `XDG_SESSION_ID` on the system bus. If none exists, `lock_detection` is
false and `pause_on_lock` has no effect (a lock screen hides the wallpaper anyway, so the cost is
only power). Which source was used appears in `doctor`.

**Stacking watchdog.** `StackingDisturbed` events trigger `refresh()`, at most five per ten seconds,
then one per ten seconds with a single warning; the count of suppressed requests is reported by
`doctor` as `restack.fights`.

## GTK control application (implemented in v0.4.0)

`lucerna` is a *client*. It never owns a renderer or touches X11 windows; `lucerna-ui` can depend
only on `lucerna-core` and `lucerna-ipc`, so it cannot link the backend or the mpv supervisor.
Closing the window never stops the wallpaper - the daemon owns it.

* **Start-up order** (§24, §34): parse arguments with clap, check that `DISPLAY` or
  `WAYLAND_DISPLAY` is set, and only then initialise GTK. `--help` and `--version` therefore work
  on a headless machine, and running `lucerna` without a display prints a message instead of a panic.
* **Layers.** `DaemonLink` (async wrapper over the D-Bus proxy, runtime-agnostic) →
  `Controller` (state, operations, signal following, all on the glib main context) →
  pages (widgets only). Pages never see D-Bus; they call the controller and re-render when it
  publishes new state.
* **`presenter/`** turns D-Bus DTOs into plain view models (library rows, drop-down contents and
  the index ↔ value mapping, the banner decision, the About text). It contains no GTK and is fully
  unit-tested; the architecture test enforces that it names no GTK, X11, D-Bus or async type.
* **`strings.rs`** holds every user-visible string.
* **Daemon control.** If `org.lucerna.Lucerna1` has no owner the banner says so and offers *Start*;
  the GUI also starts the service itself on launch (D3), from next to its own executable or `PATH`,
  in its own process group so it outlives the GUI, and waits up to 5 s for the bus name. The header
  menu has Pause, Resume, Reload and Quit service. A 2 s poll notices a daemon that appears or
  disappears; signals (`StatusChanged`, `LibraryChanged`, ...) give immediate updates.
* **Pages.** Wallpapers (library list, *Add…* with a file chooser filtered to videos and animated
  images, *Remove from library* with a confirmation that the file is kept, *Set on all displays*,
  *Stop wallpaper*, a "Missing" badge); Displays (the all-displays wallpaper and scaling, and the
  detected displays with readable labels, each with its own wallpaper and scaling choice, from v0.5.0); Settings (autostart,
  pause when fullscreen / locked, hardware decoding, FPS limit, audio behind a confirmation,
  window stacking under *Advanced*); About (name, version from the workspace, description, license,
  repository, runtime backend).
* **Errors** appear in the banner with the daemon's own wording (what happened, why, what to do)
  until dismissed; a failed renderer, a missing mpv, an unsupported session and configuration
  problems each get their own message.

**What is and is not tested.** `crates/lucerna-ui/tests/ui_structure.rs` builds the real window on
an Xvfb display against a real daemon on a private bus and checks structure and wiring: the pages
and controls exist, settings round-trip through the daemon, library operations reach the daemon
and the page updates, errors reach the banner, and the GUI notices the daemon leaving. It does
**not** and cannot say anything about layout, spacing, fonts, theming or how anything looks:
**visual quality is NOT VALIDATED ON DEVELOPMENT SERVER** (LUC-T02 and LUC-T03 cover it manually).

## Status by area

| Area | Status |
| --- | --- |
| Workspace, logging, version, headless-safe binaries | implemented (v0.0.1) |
| Renderer core: argument builder, state machine, supervision, stale recovery | implemented (v0.1.0) |
| X11 backend: RandR, surfaces, hints, click-through, restack, events, Cinnamon/Nemo probes | implemented (v0.2.0) - protocol-tested under Xvfb; **desktop appearance: MANUAL VALIDATION REQUIRED** |
| Daemon, D-Bus API, CLI, configuration, state, diagnostics, single instance, clean shutdown | implemented (v0.3.0) |
| GTK control application: pages, daemon control, error banner | implemented (v0.4.0) — structure tested under Xvfb; **visual quality NOT VALIDATED ON DEVELOPMENT SERVER** |
| Per-display assignments, hotplug and absent-display restoration, per-monitor fullscreen pause, screen-lock pause | implemented (v0.5.0) — logic tested; **real-desktop behaviour MANUAL VALIDATION REQUIRED** (LUC-T09, T12, T13) |
| Login race handling, logout/kill cleanup, stale-process recovery, bounded daemon log, failure recovery paths | implemented (v0.6.0) — tested against Xvfb and fakes; **real-login behaviour MANUAL VALIDATION REQUIRED** (LUC-T14, T17, T18, T19) |
| Native `.deb` / `.rpm`, install/removal smoke tests | implemented (v0.7.0) - built and installed in fresh Ubuntu 24.04 and Fedora 44 containers; **menu entry and icon: MANUAL VALIDATION REQUIRED** (LUC-T01, T02) |
| Automated release: annotated-tag and version check, full suite, packages, source archive, SHA-256, GitHub Release | implemented (v0.8.0, hardened v0.9.0); see `docs/PACKAGING.md` |
| `doctor` report matches the directive's list; test matrix complete; manual acceptance frozen | v0.9.0 |
