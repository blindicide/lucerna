# X11 and Cinnamon: what the design assumes, and how to check it

Lucerna's X11 backend (`lucerna-x11`) is written from the public X11, ICCCM, EWMH and RandR
specifications and from studying the *behaviour* of existing animated-wallpaper setups on Mint.
No code is taken from `xwinwrap` or any other project (directive §43), and `xwinwrap` is not a
runtime dependency.

**Nothing in this document has been verified on a real Cinnamon desktop.** The development server
is headless. The automated tests run against Xvfb, which has no window manager, no compositor, no
Nemo and no Cinnamon; they verify *protocol* behaviour (window properties, event routing, RandR
data) and nothing about appearance. Every desktop-facing behaviour below is
`IMPLEMENTED — MANUAL DESKTOP VALIDATION REQUIRED`.

## Background

On Cinnamon the desktop icons and the desktop context menu are provided by a separate process,
`nemo-desktop`, whose window sits over the background drawn by the shell. Existing Mint wallpaper
setups embed mpv in an X11 window placed behind everything else; keeping the icons on the right
layer is known to need care. So the backend treats this as a real integration problem, gives the
operator two strategies to try, and reports enough facts (`lucernactl doctor`) to settle the
question remotely.

## What a wallpaper surface is

One window per active monitor, a child of the root window at the monitor's geometry, with black
background until mpv draws (mpv is embedded with `--wid`).

| Requirement | Implementation | Verified by (Xvfb, protocol only) |
| --- | --- | --- |
| Geometry-controlled | `CreateWindow` at the monitor rectangle; `ConfigureWindow` on resize | `resize`, `surface_properties` |
| Undecorated | `_MOTIF_WM_HINTS` decorations = 0 | `surface_properties` |
| Never takes focus | `WM_HINTS.input = False`; event mask is `StructureNotify` only | `surface_properties` |
| Click-through | Empty SHAPE input region: the server skips the window (and its child, mpv's window) when finding the pointer target | `surface_properties`, `input_passthrough_protocol` (with a negative control) |
| Off taskbar and pager | `_NET_WM_STATE_SKIP_TASKBAR`, `_SKIP_PAGER`; override-redirect windows are never in `_NET_CLIENT_LIST` | `surface_properties` |
| All workspaces | `_NET_WM_DESKTOP = 0xFFFFFFFF`, `_NET_WM_STATE_STICKY` | `surface_properties` |
| Desktop / below | `_NET_WM_WINDOW_TYPE_DESKTOP`, `_NET_WM_STATE_BELOW` | `surface_properties` |
| Never unredirected | `_NET_WM_BYPASS_COMPOSITOR = 2` | `surface_properties` |
| Identifiable | `WM_CLASS lucerna-wallpaper/Lucerna`, `_NET_WM_NAME`, `_NET_WM_PID`, `WM_CLIENT_MACHINE`, private `_LUCERNA_WALLPAPER` = output id | `surface_properties`, `nemo_simulation_detected` |
| Explicit lowering | Override-redirect mode: `ConfigureWindow(stack_mode = Below)` | `lower_and_restack` |
| Cleanup | `DestroyWindow` on destroy/shutdown/drop; the X server destroys windows if the daemon is killed | `destroy_and_shutdown`, `dropping_the_backend_cleans_up_like_shutdown` |

## Two stacking strategies (`[x11] stacking`)

| Value | Behaviour |
| --- | --- |
| `auto` (default) | Currently `override-redirect`. |
| `override-redirect` | Unmanaged windows, kept at the bottom of the root's children. When another window is sent below them, or a surface is raised, the backend reports `StackingDisturbed` and the daemon calls `refresh()`, which re-lowers only surfaces that are not already among the bottom-most windows. |
| `desktop-window` | Managed windows of type `_NET_WM_WINDOW_TYPE_DESKTOP`; Muffin decides the stacking. `refresh()` does nothing. |

Re-stacking is rate limited in the daemon (at most 5 per 10 s, then 1 per 10 s with one warning)
so Lucerna cannot spin the CPU if Muffin and Lucerna disagree.

## Assumptions, and the `doctor` field that checks each

| # | Assumption | How to confirm or refute it on a real desktop |
| --- | --- | --- |
| 1 | Muffin composites override-redirect windows in X stacking order, so the bottom of the stack is behind Nemo's icons. | `diagnostics.surfaces[].below_nemo` is `true`, and LUC-T04/T05 pass. |
| 2 | Nemo's desktop window is transparent over the shell-drawn background, so the video shows through between icons. | `nemo_desktop_window.depth` (32 suggests an ARGB window; 24 means opaque) and LUC-T04. |
| 3 | Muffin does not unredirect a full-monitor, bottom-stacked override-redirect window (`_NET_WM_BYPASS_COMPOSITOR = 2` is also set). | `compositor.running`, LUC-T04, LUC-T20. |
| 4 | Muffin does not periodically restack override-redirect windows above the desktop layer. | `surfaces[].stack_index` stays 0 over time; `restack_fights` (daemon) stays 0. |
| 5 | An empty input shape gives click-through to Nemo. The X protocol semantics are tested under Xvfb. | LUC-T05, LUC-T06. |
| 6 | mpv's `--wid` rendering looks right under Muffin (no tearing, correct scaling). | LUC-T04, LUC-T11. |
| 7 | Nemo's window is found by `WM_CLASS` `nemo-desktop` in the EWMH client list or as a root child. | `nemo.desktop_window` is non-null while Nemo runs. |
| 8 | `_NET_WM_STATE_FULLSCREEN` on Muffin's client list reflects fullscreen applications. | LUC-T09; `capabilities.fullscreen_detection`. |

If assumption 1, 2 or 5 fails, switch to `desktop-window` (see `docs/MANUAL-ACCEPTANCE.md`) and
report the `doctor` output for both strategies.

## Facts collected (`probe()` and `diagnostics()`)

* Display, X server vendor/release/protocol, screen size and root depth.
* RandR version and whether `GetMonitors` (1.5) is available; SHAPE presence.
* Window manager name (`_NET_SUPPORTING_WM_CHECK` → `_NET_WM_NAME`), the interesting subset of
  `_NET_SUPPORTED`, and whether a compositing manager owns `_NET_WM_CM_S<screen>`.
* Nemo: pids of `nemo-desktop` (from `/proc/*/comm`, no shell) and its window: id, depth,
  geometry, type, and position in the root stacking order.
* Per surface: window id, output, geometry, override-redirect flag, map state, stacking index,
  `below_nemo` (null when unknown) and the number of child windows (mpv's embedded window).
* `XDG_SESSION_TYPE`, `XDG_CURRENT_DESKTOP`, `DESKTOP_SESSION`.

## Monitors

RandR 1.5 `GetMonitors` is preferred (geometry after rotation, transforms and scaling); RandR
1.2–1.4 falls back to CRTCs. Mirrored outputs make one surface, named after the lexicographically
first output. Identity comes from EDID (manufacturer, product code, serial) with a connector
fallback and collision handling (`lucerna-core::identity`); the unit tests use synthetic,
spec-conformant EDID blocks because the server has no real monitors to capture.

Limitations found while testing on Xvfb: virtual monitors created with `SetMonitor` do not
produce a RandR notification, so only real output/mode changes trigger `OutputsChanged`. Physical
hotplug produces those events.

## Fullscreen detection

Event-driven (no polling): root property changes (`_NET_CLIENT_LIST`, `_NET_ACTIVE_WINDOW`,
`_NET_CURRENT_DESKTOP`, `_NET_SUPPORTED`) and per-client property/structure changes, debounced by
150 ms. A client counts when it is `_NET_WM_STATE_FULLSCREEN`, not `_NET_WM_STATE_HIDDEN`,
viewable, and on the current desktop or all desktops. Maximised windows never count. The daemon
maps rectangles to monitors with the pure `occluded_outputs` rule (a rectangle covering at least
90% of a monitor occludes it), so a fullscreen window on one monitor pauses only that monitor.

Limitation: fullscreen windows that are override-redirect and therefore absent from
`_NET_CLIENT_LIST` (some old games) are not detected. Pause manually with `lucernactl pause`.
