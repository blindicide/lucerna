# Configuration

## Where things live

| Purpose | Path |
| --- | --- |
| Configuration | `$XDG_CONFIG_HOME/lucerna/config.toml` (default `~/.config/lucerna/config.toml`) |
| Backups | `config.toml.bak-v<N>-<UTC stamp>` (before a schema migration) and `config.toml.corrupt-<UTC stamp>` (an unparseable file moved aside; never deleted) |
| Autostart entry | `$XDG_CONFIG_HOME/autostart/org.lucerna.Lucerna.Daemon.desktop` |
| State | `$XDG_STATE_HOME/lucerna/state.json` (the last 20 renderer failures) |
| Logs | `$XDG_STATE_HOME/lucerna/logs/renderer-<slot>.log` (mpv output, at most 1 MiB per display) |
| Cache | `$XDG_CACHE_HOME/lucerna/` |
| Runtime | `$XDG_RUNTIME_DIR/lucerna/` (mode 0700): `daemon.lock`, `renderers.json`, mpv control sockets |

Lucerna never falls back to `/tmp`. `config.toml` is written with mode 0600.

**The daemon is the only writer of `config.toml`.** The GUI and CLI change settings by asking the
daemon over D-Bus, which avoids lost updates. You may edit the file by hand; run
`lucernactl reload` afterwards.

## Example

```toml
schema_version = 1

[general]
pause_on_fullscreen = true      # pause a display while a fullscreen window covers it
pause_on_lock = true            # pause while the screen is locked
audio = false                   # wallpapers are silent unless you turn this on
hardware_decode = "auto"        # "auto" | "disabled"
fps_limit = "native"            # "native" | "60" | "30" | "15"

[renderer]
max_restarts = 3                # automatic restarts allowed ...
restart_window_secs = 60        # ... within this many seconds

[x11]
stacking = "auto"               # "auto" | "override-redirect" | "desktop-window"

[all_displays]                  # one wallpaper on every display without an override
wallpaper = "6f1c7a52-…"        # a library id
scaling = "fill"                # "fill" | "fit" | "stretch" | "center"

[displays."edid:DEL-a0b1-7XJ2K3"]   # a per-display override, keyed by stable display id
wallpaper = "0b8e…"             # absent: inherits [all_displays]
scaling = "fit"                 # absent: inherits [all_displays]
last_seen = "DP-1 — 2560×1440"  # informational label for displays that are unplugged

[[wallpapers]]
id = "6f1c7a52-…"               # UUID
name = "Rain"
path = "/home/user/Videos/rain.webm"   # absolute, UTF-8
media_type = "video"            # "video" | "animated-image" | "unknown" (informational)
added = 2026-09-30T10:00:00Z
available = true                # last known existence state
```

## Settings

| Key | Default | Meaning |
| --- | --- | --- |
| `general.pause_on_fullscreen` | `true` | Pause a display while a fullscreen window covers it. A merely maximised window never pauses anything. |
| `general.pause_on_lock` | `true` | Pause while the screen is locked (where the session reports it). |
| `general.audio` | `false` | Off by default; never enabled silently. Changing it restarts the renderers. |
| `general.hardware_decode` | `"auto"` | `auto` passes mpv `--hwdec=auto-safe`; `disabled` passes `--hwdec=no`. Try `disabled` if a renderer crashes. |
| `general.fps_limit` | `"native"` | `60`, `30` or `15` adds mpv's `fps` filter. **This drops frames before rendering, which reduces upload and presentation work; it does not reduce decode work.** |
| `renderer.max_restarts` | `3` | Automatic restarts after crashes within the window. Clamped to 1–10. |
| `renderer.restart_window_secs` | `60` | The window. Clamped to 10–3600. |
| `x11.stacking` | `"auto"` | `auto` currently means `override-redirect`. See `docs/X11-CINNAMON-NOTES.md`. Takes effect on `lucernactl reload`. |
| `all_displays.wallpaper`, `.scaling` | none, `"fill"` | The wallpaper and scaling for every display without an override. |
| `displays."<id>".wallpaper`, `.scaling` | inherit | Per-display overrides. |

There is intentionally **no** `autostart` key: whether Lucerna starts at login is the presence (and
enabled state) of the autostart file, so there is a single source of truth. Toggle it in the GUI
Settings page, or with `SetSettings`. If you disable the entry from Cinnamon's *Startup
Applications*, Lucerna reports it as disabled instead of fighting you. No systemd unit and no D-Bus
activation file is installed, so disabling autostart really prevents the daemon from coming back.

### Scaling modes

| Mode | Result |
| --- | --- |
| `fill` (default) | Covers the whole display, keeps the aspect ratio, crops what overflows. |
| `fit` | The whole video is visible, keeps the aspect ratio, letterboxed in black. |
| `stretch` | Covers the whole display and distorts the aspect ratio. |
| `center` | Native pixel size, centred; black bars if smaller, cropped if larger. |

Changing the scaling of a running wallpaper needs no restart. The background is black.

## Robustness rules

* **Unknown keys are ignored** and survive a save, together with your comments. Only the fields
  that actually change are rewritten.
* **A value this version does not understand** (for example `fps_limit = "144"` from a newer
  Lucerna) falls back to the default with a warning (shown by `lucernactl status` and `doctor`);
  the original text stays in the file.
* **A `[[wallpapers]]` entry without a valid `id` or with a relative `path`** is skipped with a
  warning and kept in the file.
* **A corrupt file** is renamed to `config.toml.corrupt-<stamp>` (never deleted) and defaults are
  used. `lucernactl status`, the GUI banner and `doctor` say so.
* **A newer `schema_version`** is read leniently, but the daemon then refuses to modify the file
  (`ConfigReadOnly`). Wallpapers keep playing.
* **A missing `schema_version`** is treated as 1 and written on the next save.
* **An older schema** is migrated: the old file is copied to `config.toml.bak-v<N>-<stamp>`, the
  migration runs, and the result is written atomically. (Schema 1 has no older versions yet.)
* **Writes are atomic** (temporary file, `fsync`, rename, directory `fsync`), so a crash never
  leaves a zero-byte configuration. A symlinked `config.toml` (dotfile managers) has its target
  replaced, not the link.
* **Wallpapers are references.** Lucerna never copies media and never deletes your files;
  removing a library entry only removes the entry and its assignments. A file that goes missing
  stays in the library, marked unavailable, and its assignment is preserved. Paths must be valid
  UTF-8; other paths are rejected when added.
* **Absent displays keep their assignment.** When a configured display is unplugged no renderer
  runs for it; when it returns, its wallpaper comes back automatically.

## Environment variables

| Variable | Effect |
| --- | --- |
| `LUCERNA_LOG` / `RUST_LOG` | Log filter, default `lucerna=info` (for example `RUST_LOG=lucerna=debug`). `LUCERNA_LOG` wins. |
| `LUCERNA_MPV` | Absolute path of the mpv to use (debugging aid; reported by `doctor`). |
| `LUCERNA_DISPLAY_WAIT_MS` | How long `lucernad` waits for `DISPLAY` and the X server at login (default 10000). |
