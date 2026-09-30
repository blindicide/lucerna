# Troubleshooting

Logs: the daemon writes `~/.local/state/lucerna/logs/lucernad.log` (bounded) as well as the journal
(`journalctl --user -t lucernad` when started by the session); mpv's own output is in
`renderer-*.log` beside it.

**Slow to start at login?** At login `lucernad` waits up to 10 s for the X server and the window
manager before showing the wallpaper, and requests wait until then. If your session starts them
much later, raise the wait with `LUCERNA_DISPLAY_WAIT_MS` in the autostart entry, or add a delay in
Startup Applications.

Start with:

```sh
lucernactl status
lucernactl doctor          # add --json --redact to post a report publicly
```

`doctor` works even when the GUI cannot start and even when the daemon is not running.

> **Honesty note.** Lucerna's desktop-facing behaviour (the wallpaper appearing, sitting under the
> desktop icons, icons staying clickable, window stacking) has been **implemented but not yet
> validated on a real Cinnamon desktop**: see `docs/MANUAL-ACCEPTANCE.md`. The entries below for
> those symptoms describe how to gather evidence and what to try, not guaranteed fixes.

## mpv not installed

*Symptom:* `lucernactl status` shows `mpv: NOT FOUND`; a renderer is `failed` with `mpv-missing`;
`lucernactl start` exits 7.

*Message:* "Lucerna could not start mpv. Executable "mpv" was not found in PATH. Install mpv
(for example: sudo apt install mpv) and choose Reload."

*Fix:* `sudo apt install mpv` (Fedora: `sudo dnf install mpv`), then `lucernactl reload`. No
surface is created while mpv is missing, so your normal desktop background stays visible. If mpv is
installed in an unusual place, set `LUCERNA_MPV=/full/path/to/mpv` for the daemon.

*Related:* `doctor` reports `mpv.options_compatible`. If it is `false`, the installed mpv rejected
an option Lucerna passes (`mpv.compat_detail` says which); please report it with the mpv version.

## Unsupported Wayland session

*Symptom:* `status` shows `Backend: none` and `Wallpaper playback is not available in this session
(wayland)`.

*Message:* "This release supports X11 sessions only; your current session appears to be Wayland.
Log in with a "Cinnamon" (X11) session."

*Fix:* At the login screen choose the Cinnamon (X11) session, not the Wayland one. Lucerna never
tries XWayland: a window there would not be the desktop. It starts no mpv processes in an
unsupported session.

## DISPLAY missing

*Symptom:* `lucerna` prints "Lucerna could not connect to a graphical display. DISPLAY is not set
or no supported graphical session is available."; `lucernad` reports `no-display`.

*Cause:* Started from ssh, a TTY, a container, or too early in login.

*Fix:* Start from a desktop terminal. At login `lucernad` waits up to 10 s for `DISPLAY` and the
X server (`LUCERNA_DISPLAY_WAIT_MS` changes that). `lucernactl` itself needs only the session bus.

## Daemon already running

*Symptom:* "Lucerna daemon is already running for this session (PID 1234). Nothing to do."

This is not an error (exit status 0): only one daemon runs per session. Use `lucernactl stop
--daemon` to stop it, or `lucernactl reload` to restart its work.

If `lucernactl` says "The Lucerna daemon is not running" but you are sure it is, you are probably in
a different session bus (for example `sudo`, or a terminal from another login). Compare
`echo $DBUS_SESSION_BUS_ADDRESS`.

## Wallpaper file missing

*Symptom:* the renderer is `failed` with `media-missing`, and `lucernactl wallpapers` marks the
entry `[MISSING]`.

*Message:* "The wallpaper file is missing. It may have been moved, deleted or be on a drive that is
not connected."

*What Lucerna does:* keeps the library entry and the assignment, starts no mpv, shows no surface,
and checks every 60 s: **when the file returns (for example an external drive is mounted),
playback resumes by itself.** It never deletes your files.

*Fix:* Restore the file, or choose another wallpaper.

## Renderer crash

*Symptom:* `status` shows `failed` with `crashed`, `unexpected-exit`, `startup-timeout` or
`ipc-failed`, and "N restart(s) recently".

*What Lucerna does:* restarts the renderer at most **3 times in 60 seconds** (1 s, 2 s, 4 s
back-off). After that it stays `failed` with `restart-limit` and does not keep trying.

*Fix:* Read `failure_message` (it includes mpv's last words), or the log
`~/.local/state/lucerna/logs/renderer-*.log`. Try Settings → Hardware decoding → Disabled. Then
`lucernactl reload` (or `lucernactl start`) renews the restart budget. A file mpv cannot play at all
fails immediately with `media-unsupported` and is not retried.

## Monitor not detected

*Symptom:* `lucernactl monitors` misses a display.

*Check:* `xrandr --listmonitors` shows what X sees. `doctor --json` lists RandR's monitors,
EDID manufacturer/model and each stable id. Lucerna needs RandR 1.2 or newer.

*Notes:* Displays are identified by EDID where the monitor reports one (docks and KVM switches
sometimes hide it, in which case the connector name is used). A configured display that is
unplugged keeps its assignment and is listed as disconnected; it is restored when it returns.
After unusual changes (`xrandr --setmonitor`) run `lucernactl reload`.

## Wallpaper not visible

*Symptom:* the daemon says `playing` but you see nothing.

*Gather evidence:* `lucernactl doctor --json --redact`, and look at `daemon.diagnostics.backend_diagnostics`:

* `surfaces[].map_state` should be `viewable`, `override_redirect` `true`.
* `surfaces[].stack_index` and `nemo_desktop_window.stack_index` say where the surface sits
  compared with Nemo's window; `below_nemo` should be `true`.
* `compositor_owner` says whether a compositor is running.

*Try:* set `stacking = "desktop-window"` in the `[x11]` section of `config.toml`, run
`lucernactl reload`, and compare. Set Hardware decoding to Disabled. Attach both reports to a bug
report. This is a known open question of the design (`docs/X11-CINNAMON-NOTES.md`).

## Wallpaper above desktop icons

*Symptom:* the video covers the icons.

*Meaning:* the bottom-of-stack assumption did not hold in your session. Same evidence and same
`stacking = "desktop-window"` experiment as above. `below_nemo: false` in `doctor` confirms it.

## Icons not clickable

*Symptom:* desktop icons are visible but clicks do nothing, or the desktop right-click menu is gone.

*Meaning:* input is reaching the wallpaper surface. Lucerna gives its surfaces an empty input
region so pointer events fall through; if that did not work in your session, please report it with
`doctor` output. Meanwhile `lucernactl stop` removes the surfaces immediately.

## Fullscreen pause not working

*Check:* `lucernactl status` line "Detection: fullscreen yes/no". "no" means the window manager
does not publish the EWMH client list or fullscreen state, so `pause_on_fullscreen` has no effect.
Also check the setting is on (`lucernactl doctor --json`, `settings.pause_on_fullscreen`).

*Known limitations:* only fullscreen windows that appear in the window manager's client list are
detected: fullscreen windows that bypass the window manager (some old games) are not; pause with
`lucernactl pause`. A maximised window never pauses anything. With several monitors only the display
a fullscreen window covers (at least 90 % of it) is paused.

## Pause when locked not working

*Check:* `lucernactl status` line "Detection: … screen lock yes/no". "no" means the session offers
none of the sources Lucerna knows (`org.cinnamon.ScreenSaver`, `org.freedesktop.ScreenSaver`,
logind's `LockedHint`), so `pause_on_lock` has no effect; `doctor` shows
`lock_detection.source`. The lock screen hides the wallpaper anyway; the only cost is power. Use
`lucernactl pause` in a lock script if you need it.

## Configuration problems

* "The configuration was written by a newer Lucerna": update Lucerna; the file is left untouched.
* "could not be parsed … kept as config.toml.corrupt-…": your file had a syntax error; fix it or
  copy back settings from the kept file. Defaults are in use meanwhile.
* "could not be read": fix file permissions, then `lucernactl reload`.
