# The Lucerna D-Bus API

The daemon (`lucernad`) is controlled over the **session bus**. The GUI (`lucerna`), the CLI
(`lucernactl`) and any script use the same interface; there is no other public control channel and
no filesystem socket (mpv's own control sockets are private, live in the user's 0700 runtime
directory and are not part of the API).

| Item | Value |
| --- | --- |
| Well-known name | `org.lucerna.Lucerna1` |
| Object path | `/org/lucerna/Lucerna1` |
| Interface | `org.lucerna.Lucerna1` (plus the standard `Properties`, `Introspectable`, `Peer`) |
| Error prefix | `org.lucerna.Lucerna1.Error.` |
| `ApiVersion` | `1`. Within `Lucerna1` changes are additive only. |

The daemon requests the name without queueing and refuses to replace an existing owner, so a second
daemon can never take the session over. Only one daemon runs per user session (see "Single
instance" below).

## Conventions

* Payloads are `a{sv}` dictionaries. **Clients must ignore unknown keys and must not require keys
  that were added later.** Missing keys mean "unknown/empty".
* Every error carries a complete, user-facing message: what happened, why, and what to do.
* `display_id` is a stable display id (see below) or `*` for "all displays". An empty string also
  means all displays.

## Methods

| Method | In | Out | Meaning |
| --- | --- | --- | --- |
| `GetStatus` | | `a{sv}` | Whole-daemon status, see "status". |
| `GetDisplays` | | `aa{sv}` | Connected displays **and** configured displays that are currently absent (`connected = false`). |
| `GetAssignments` | | `aa{sv}` | One entry per configured display plus `display_id = "*"` for the all-displays assignment. |
| `ListWallpapers` | | `aa{sv}` | The library, with live `available`. |
| `AddWallpaper` | `s path, s name` | `s id` | Canonicalise, validate and add. Idempotent: an existing path returns the existing id. An empty name uses the file name without extension. |
| `RemoveWallpaper` | `s id` | | Removes the entry and every assignment that points to it. **Never touches the file.** |
| `SetWallpaper` | `s wallpaper_id, s display_id` | | Assign and play. `*` (or empty) assigns to all displays; a display id makes a per-display override. The display must be connected or already configured (`UnknownDisplay` otherwise). Clears a previous `Stop`. |
| `ClearAssignment` | `s display_id` | | Remove the wallpaper assignment of a display (it then follows the all-displays wallpaper) or of `*`. A per-display entry that then overrides nothing is dropped from the configuration. |
| `SetScaling` | `s display_id, s mode` | | `fill`, `fit`, `stretch` or `center`, for all displays (`*`) or one. For one display `inherit` removes its override. Applied live, no restart. |
| `GetSettings` | | `a{sv}` | See "settings". |
| `SetSettings` | `a{sv}` | | Partial update, validated as a whole: unknown keys, wrong types and bad values are errors and nothing is applied. |
| `Pause` / `Resume` | | | Set / clear the user pause reason. Other reasons (lock, fullscreen) still apply. |
| `Stop` | | | Stop every renderer and remove the wallpaper windows, until `Start`, `Reload` or a `SetWallpaper`. Not persisted across daemon restarts. |
| `Start` | | | Clear `Stop`, renew the restart budget and start again. Errors: `Unsupported`, `MpvMissing`. |
| `Reload` | | | Re-read `config.toml`, re-probe mpv, re-select the backend and start over. |
| `Quit` | | | Clean shutdown; replies before exiting. |
| `GetDiagnostics` | `b redact` | `s` | The daemon's part of the `doctor` report, as JSON. |

## Properties (read-only)

`Version` (`s`, the workspace version) and `ApiVersion` (`u`).

## Signals

| Signal | Arguments | When |
| --- | --- | --- |
| `StatusChanged` | `a{sv}` (the full status) | Renderer or policy state changed. Coalesced: at most one per 100 ms. |
| `DisplaysChanged` | | Displays were added, removed or changed. Re-fetch with `GetDisplays`. |
| `LibraryChanged` | | The wallpaper library changed. |
| `SettingsChanged` | | Settings changed. |
| `RendererFailed` | `s display_id, s code, s message` | Once per transition into `failed`. |

## Dictionaries

### status

| Key | Type | Values |
| --- | --- | --- |
| `daemon_version` | `s` | |
| `api_version` | `u` | |
| `backend` | `s` | `cinnamon-x11`, `x11-ewmh`, or `none` |
| `session_type` | `s` | `x11`, `wayland`, `tty`, `unknown` |
| `supported` | `b` | false when no backend could be started |
| `unsupported_reason` | `s` | empty, `wayland`, `no-display`, `x11-connect-failed`, `missing-feature`, `runtime-dir-unavailable` |
| `unsupported_message` | `s` | user-facing explanation |
| `playback` | `s` | aggregate: `playing`, `paused`, `stopped`, `idle`, `failed` |
| `user_paused`, `user_stopped`, `session_locked` | `b` | |
| `lock_detection`, `fullscreen_detection` | `b` | capabilities of this session (see "Pause policies" in `docs/ARCHITECTURE.md`) |
| `mpv_available` | `b` | |
| `mpv_version` | `s` | |
| `schema_version` | `u` | configuration schema this daemon writes |
| `config_state` | `s` | `ok`, `defaults-after-corruption`, `read-only-newer-schema`, `unreadable` |
| `config_notice` | `s` | what was done to the configuration file, if anything |
| `config_warnings` | `as` | |
| `renderers` | `aa{sv}` | one per connected display that has an assigned wallpaper |

Renderer entry: `display_id`, `connector`, `wallpaper_id`, `state` (`stopped`, `starting`,
`playing`, `paused`, `stopping`, `failed`), `pause_reasons` (`as`, subset of `user`, `lock`,
`fullscreen`), `failure_code`, `failure_message`, `restarts_in_window` (`u`), `pid` (`u`, 0 if none),
`scaling`.

Failure codes: `mpv-missing`, `launch-failed`, `media-missing`, `media-unsupported`, `crashed`,
`unexpected-exit`, `startup-timeout`, `ipc-failed`, `restart-limit`.

### display

`id`, `connector`, `label` (for example `eDP-1 — 1920×1080 — Primary`), `connected` (`b`),
`primary` (`b`), `x`, `y` (`i`), `width`, `height` (`u`), `rotation` (`normal`, `left`, `inverted`,
`right`), `edid_manufacturer`, `edid_model`, `edid_serial` (always empty over D-Bus; only
`GetDiagnostics` includes serial numbers, and `redact` removes them), `wallpaper_id` (effective),
`wallpaper_source` (`display`, `all`, `none`), `scaling` (effective), `scaling_source` (`display` if this display overrides the all-displays scaling, else `all`).

**Stable display ids** come from EDID (`edid:<MANUFACTURER>-<product>-<serial>`, or
`edid:<MANUFACTURER>-<product>@<connector>` when the monitor reports no serial) and fall back to the
connector (`conn:HDMI-1`). Identical monitors that report identical serials are told apart by adding
`@<connector>`. An id does not change when monitors are enumerated in a different order.

### wallpaper

`id`, `name`, `path`, `media_type` (`video`, `animated-image`, `unknown`), `available` (`b`),
`added` (RFC 3339) and, when known, `duration_ms` (`t`), `width`, `height` (`u`), `codec`.

### settings

`pause_on_fullscreen`, `pause_on_lock`, `audio` (`b`), `hardware_decode` (`auto`, `disabled`),
`fps_limit` (`native`, `60`, `30`, `15`), `stacking` (`auto`, `override-redirect`,
`desktop-window`), `max_restarts` (`u`, 1–10), `restart_window_secs` (`u`, 10–3600), `autostart`
(`b`, derived from the autostart file; see `docs/CONFIGURATION.md`).

## Errors and `lucernactl` exit codes

| D-Bus error | Exit | Example message |
| --- | --- | --- |
| *(no owner of the bus name)* | 3 | "The Lucerna daemon is not running. Start it with `lucernad &` or open Lucerna." |
| `UnknownWallpaper`, `UnknownDisplay`, `InvalidArgument`, `InvalidPath`, `FileNotFound`, `NotAFile` | 4 | "No display matches 'HDMI-9'. Run `lucernactl monitors` to list displays." |
| `Unsupported`, `BackendUnavailable` | 5 | the Wayland / `DISPLAY` messages |
| `ConfigReadOnly`, `ConfigWrite`, `ConfigInvalid`, `AutostartWrite` | 6 | "Lucerna could not save its configuration to …: Permission denied." |
| `MpvMissing` | 7 | "Lucerna could not start mpv. Executable "mpv" was not found in PATH. Install mpv …" |
| `Internal`, other transport errors | 1 | "Unexpected daemon error: …" |
| *(command-line usage error)* | 2 | clap's usage message |

## Single instance

Two independent guards, checked in this order:

1. A non-blocking `flock` on `$XDG_RUNTIME_DIR/lucerna/daemon.lock` (the file contains the PID).
2. Ownership of the bus name.

If either is already held, `lucernad` prints
`Lucerna daemon is already running for this session (PID <n>). Nothing to do.` and exits **0**
without touching the running daemon.

## Trying it by hand

```sh
busctl --user introspect org.lucerna.Lucerna1 /org/lucerna/Lucerna1
gdbus call --session --dest org.lucerna.Lucerna1 --object-path /org/lucerna/Lucerna1 \
     --method org.lucerna.Lucerna1.GetStatus
lucernactl status --json
```
