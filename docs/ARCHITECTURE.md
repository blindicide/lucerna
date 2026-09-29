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

## Status by area

| Area | Status |
| --- | --- |
| Workspace, logging, version, headless-safe binaries | implemented (v0.0.1) |
| Renderer, X11 backend, daemon, D-Bus, GUI, policies, packaging | planned; see `docs/IMPLEMENTATION-PLAN.md` |
