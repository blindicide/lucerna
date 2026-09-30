# Architecture

Lucerna is a native control application plus a small per-user daemon, with `mpv` doing the
decoding and rendering. This document grows with each milestone; sections not yet
implemented say so.

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
* **Per-display assignment:** the configuration, reconciler and engine already handle it; the
  D-Bus API accepts only `*` until v0.5.0, where the per-display path is enabled and tested.

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

## Status by area

| Area | Status |
| --- | --- |
| Workspace, logging, version, headless-safe binaries | implemented (v0.0.1) |
| Renderer core: argument builder, state machine, supervision, stale recovery | implemented (v0.1.0) |
| X11 backend: RandR, surfaces, hints, click-through, restack, events, Cinnamon/Nemo probes | implemented (v0.2.0) - protocol-tested under Xvfb; **desktop appearance: MANUAL VALIDATION REQUIRED** |
| Daemon, D-Bus API, CLI, configuration, state, diagnostics, single instance, clean shutdown | implemented (v0.3.0) |
| GUI, per-display assignment and lock pause in the API, lifecycle polish, packaging | planned; see `docs/IMPLEMENTATION-PLAN.md` |
