# Manual Desktop Acceptance Campaign

> **Status: NOT RUN — REQUIRES REAL DESKTOP.**
> This campaign was written by the development agent on a headless server. It has **not** been
> executed by the agent and no result below has been filled in. Every result cell says
> `NOT RUN — REQUIRES REAL DESKTOP` until a person runs the test on a real Linux Mint Cinnamon (X11)
> desktop and records the outcome.

Lucerna's automated tests cover logic, process supervision, D-Bus and X11 *protocol* behaviour.
They cannot show that a wallpaper appears, that it sits under the desktop icons, that icons stay
clickable, that windows stack correctly, or that anything looks right. Those are exactly the
things this campaign checks. Until a test here passes on a real desktop, the corresponding
behaviour is only ever described as:

```text
IMPLEMENTED — MANUAL DESKTOP VALIDATION REQUIRED
```

## Before you start

* Machine: **Linux Mint 22.x (or newer) with the Cinnamon desktop, logged in to a "Cinnamon"
  session — not "Cinnamon (Wayland)"**. Check with `echo $XDG_SESSION_TYPE` (must print `x11`).
* Nemo desktop icons enabled (Preferences → Desktop → Show desktop icons), with a few files and
  folders on the desktop.
* A short test video that contains an audio track, and one without (MP4 or WebM).
* Two monitors for LUC-T12 and LUC-T13 (a second display, or a projector/TV).
* Install the release-candidate package (`lucerna_1.0.0~rc1_amd64.deb` or a newer RC). Check its
  checksum against `SHA256SUMS` first.

## What to attach when something fails

Run this while the problem is visible and attach the output (it hides your home directory, user
name, host name and monitor serial numbers):

```sh
lucernactl doctor --json --redact > lucerna-doctor.json
```

Useful extra evidence: a photo or screenshot, `echo $XDG_CURRENT_DESKTOP $XDG_SESSION_TYPE`, and
the last lines of `~/.local/state/lucerna/logs/lucernad.log`.

## If the wallpaper is wrong: switching the stacking strategy

Lucerna's default strategy (`override-redirect`) keeps an unmanaged window at the bottom of the X
stacking order. Whether Cinnamon's compositor and Nemo respect that is the central open
question, and it can only be answered on a real desktop. If a test in LUC-T04 to LUC-T08 fails
(wallpaper invisible, drawn above the icons, icons not clickable, wrong stacking), try the
alternative and repeat the test:

1. `lucernactl pause` is not needed. Edit `~/.config/lucerna/config.toml` and set:

   ```toml
   [x11]
   stacking = "desktop-window"
   ```

2. Run `lucernactl reload`.
3. Repeat the failed test, then attach `lucernactl doctor --json --redact` for **both**
   strategies, noting which one you used for each.

Set it back to `"auto"` afterwards. The `doctor` report contains, per wallpaper surface, its
window id, whether it is override-redirect, its map state, its position in the root window's
stacking order and whether it sits below Nemo's desktop window (`below_nemo`), plus Nemo's
window depth and the compositor status. Those fields are what make a remote fix possible.

## Tests

Record each result in the table at the end. A test passes only when **every** PASS condition holds.

### LUC-T01 — Installation

Install the generated `.deb` on Linux Mint.

PASS if:

- installation succeeds;
- dependencies resolve;
- Lucerna appears in the application menu.

### LUC-T02 — First Launch

Launch Lucerna normally.

PASS if:

- main window appears;
- application remains responsive;
- no unexpected terminal is opened.

### LUC-T03 — Add Wallpaper

Add a known MP4/WebM.

PASS if:

- file appears in library;
- file may be selected.

### LUC-T04 — Apply Wallpaper

Assign video to primary monitor.

PASS if:

- animation appears as desktop background.

### LUC-T05 — Desktop Icons

With Nemo desktop icons enabled:

PASS if:

- icons remain visible;
- icons remain clickable;
- icon drag behaviour still works.

### LUC-T06 — Desktop Context Menu

Right-click empty desktop area.

PASS if:

- Cinnamon/Nemo desktop context menu still works.

### LUC-T07 — Window Stacking

Open ordinary applications.

PASS if:

- wallpaper remains behind all normal windows;
- Lucerna wallpaper does not cover panels or application windows.

### LUC-T08 — Alt+Tab / Taskbar

PASS if:

- wallpaper surface does not appear in Alt+Tab;
- wallpaper surface does not appear in taskbar.

### LUC-T09 — Fullscreen Pause

Open fullscreen application.

PASS if:

- configured renderer pauses;
- renderer resumes afterward.

*How to observe:* `lucernactl status` lists each renderer's state and pause reasons
(`fullscreen`); watch CPU/GPU use drop in a system monitor. With two monitors, a fullscreen window
on one monitor should pause only that monitor's renderer.

### LUC-T10 — Audio Default

Apply video containing audio.

PASS if:

- wallpaper is silent by default.

### LUC-T11 — Scaling

Test Fit/Fill/Stretch/Center.

PASS if:

- each mode behaves according to documentation.

*Documented behaviour* (`docs/CONFIGURATION.md`): **Fill** covers the screen, keeps the aspect ratio
and crops the overflow; **Fit** shows the whole video, keeps the aspect ratio and letterboxes it in
black; **Stretch** covers the screen and distorts the aspect ratio; **Center** shows the video at
its native pixel size, centred.

### LUC-T12 — Dual Monitor

Attach/use two displays.

PASS if:

- both are detected;
- separate wallpaper assignments work.

### LUC-T13 — Monitor Disconnect

Disconnect secondary display.

PASS if:

- daemon remains running;
- primary wallpaper remains functional;
- assignment is retained.

Reconnect monitor.

PASS if:

- assignment can be restored automatically.

### LUC-T14 — Persistence

Log out and back in.

PASS if:

- Lucerna automatically restores wallpaper if autostart is enabled.

### LUC-T15 — GUI Closure

Close Lucerna GUI.

PASS if:

- wallpaper remains active;
- daemon continues running.

### LUC-T16 — CLI

Use:

```text
lucernactl pause
lucernactl resume
lucernactl status
```

PASS if observable state matches commands.

### LUC-T17 — Missing File

Move/delete current wallpaper.

PASS if:

- Lucerna reports the missing file;
- daemon does not crash.

### LUC-T18 — mpv Failure

Temporarily make mpv unavailable or force a bad media file.

PASS if:

- error is visible;
- daemon remains recoverable.

### LUC-T19 — Login Autostart Disabled

Disable autostart.

Log out/in.

PASS if:

- Lucerna does not start automatically.

### LUC-T20 — Idle Resource Sanity

Observe system monitor with wallpaper running.

Record:

```text
CPU
GPU/video decode where available
RAM
```

No universal numeric PASS threshold is required for v1, but obviously pathological use must be treated as a defect.

## Results

Fill this table in by hand. Leave a row as it is until you have actually run the test.

| Test    | Result                             | Notes |
| ------- | ---------------------------------- | ----- |
| LUC-T01 | NOT RUN — REQUIRES REAL DESKTOP    |       |
| LUC-T02 | NOT RUN — REQUIRES REAL DESKTOP    |       |
| LUC-T03 | NOT RUN — REQUIRES REAL DESKTOP    |       |
| LUC-T04 | NOT RUN — REQUIRES REAL DESKTOP    |       |
| LUC-T05 | NOT RUN — REQUIRES REAL DESKTOP    |       |
| LUC-T06 | NOT RUN — REQUIRES REAL DESKTOP    |       |
| LUC-T07 | NOT RUN — REQUIRES REAL DESKTOP    |       |
| LUC-T08 | NOT RUN — REQUIRES REAL DESKTOP    |       |
| LUC-T09 | NOT RUN — REQUIRES REAL DESKTOP    |       |
| LUC-T10 | NOT RUN — REQUIRES REAL DESKTOP    |       |
| LUC-T11 | NOT RUN — REQUIRES REAL DESKTOP    |       |
| LUC-T12 | NOT RUN — REQUIRES REAL DESKTOP    |       |
| LUC-T13 | NOT RUN — REQUIRES REAL DESKTOP    |       |
| LUC-T14 | NOT RUN — REQUIRES REAL DESKTOP    |       |
| LUC-T15 | NOT RUN — REQUIRES REAL DESKTOP    |       |
| LUC-T16 | NOT RUN — REQUIRES REAL DESKTOP    |       |
| LUC-T17 | NOT RUN — REQUIRES REAL DESKTOP    |       |
| LUC-T18 | NOT RUN — REQUIRES REAL DESKTOP    |       |
| LUC-T19 | NOT RUN — REQUIRES REAL DESKTOP    |       |
| LUC-T20 | NOT RUN — REQUIRES REAL DESKTOP    |       |

`v1.0.0` may be tagged only after this campaign passes, or after failures found here are fixed
and a replacement release candidate passes.
