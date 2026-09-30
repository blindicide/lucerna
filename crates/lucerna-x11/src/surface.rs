//! Wallpaper surface windows (directive §8, §14).
//!
//! Every primitive the directive lists is implemented here from the public X11/EWMH/ICCCM
//! specifications; no code is taken from xwinwrap or any other project (§43).

use lucerna_core::backend::{BackendError, OutputInfo, Rect};
use x11rb::COPY_FROM_PARENT;
use x11rb::connection::Connection;
use x11rb::properties::WmHints;
use x11rb::protocol::shape::{self, SK, SO};
use x11rb::protocol::xproto::{
    AtomEnum, ClipOrdering, ConfigureWindowAux, ConnectionExt as _, CreateWindowAux, EventMask,
    PropMode, Rectangle, StackMode, Window, WindowClass,
};
use x11rb::wrapper::ConnectionExt as _;

use crate::backend::StackingMode;
use crate::conn::{Conn, be};

/// `_NET_WM_DESKTOP` value meaning "on all workspaces".
pub const ALL_DESKTOPS: u32 = 0xFFFF_FFFF;

/// `_NET_WM_BYPASS_COMPOSITOR`: 2 = never bypass (never unredirect).
const BYPASS_NEVER: u32 = 2;

/// Class hints: instance name and class name.
pub const WM_CLASS_INSTANCE: &str = "lucerna-wallpaper";
pub const WM_CLASS_CLASS: &str = "Lucerna";

fn clamp_u16(value: u32) -> u16 {
    u16::try_from(value).unwrap_or(u16::MAX).max(1)
}

fn clamp_i16(value: i32) -> i16 {
    i16::try_from(value).unwrap_or(if value < 0 { i16::MIN } else { i16::MAX })
}

/// Create, configure, map and bottom-stack a surface for `output`. Returns the window id.
pub fn create(
    conn: &Conn,
    output: &OutputInfo,
    mode: StackingMode,
) -> Result<Window, BackendError> {
    if !conn.shape {
        // Without an empty input region the window would swallow clicks meant for the desktop.
        return Err(BackendError::MissingFeature(
            "SHAPE extension (needed for click-through)",
        ));
    }
    let x = &conn.x;
    let screen = &x.setup().roots[conn.screen_num];
    let window = x.generate_id().map_err(be)?;
    let geometry = output.geometry;
    let override_redirect = mode == StackingMode::OverrideRedirect;

    x.create_window(
        COPY_FROM_PARENT as u8,
        window,
        conn.root,
        clamp_i16(geometry.x),
        clamp_i16(geometry.y),
        clamp_u16(geometry.width),
        clamp_u16(geometry.height),
        0,
        WindowClass::INPUT_OUTPUT,
        COPY_FROM_PARENT,
        // Black until the first video frame: no garbage flashes through.
        &CreateWindowAux::new()
            .background_pixel(screen.black_pixel)
            .border_pixel(0)
            .override_redirect(u32::from(override_redirect))
            .event_mask(EventMask::STRUCTURE_NOTIFY),
    )
    .map_err(be)?;

    let result = configure(conn, window, output, override_redirect);
    if let Err(err) = result {
        let _ = x.destroy_window(window);
        let _ = x.flush();
        return Err(err);
    }
    Ok(window)
}

fn configure(
    conn: &Conn,
    window: Window,
    output: &OutputInfo,
    override_redirect: bool,
) -> Result<(), BackendError> {
    let x = &conn.x;
    let a = &conn.atoms;

    // Identification: lets `doctor` (and a curious human with xprop) find our windows.
    // WM_CLASS is "instance\0class\0" (ICCCM 4.1.2.5).
    let class = format!("{WM_CLASS_INSTANCE}\0{WM_CLASS_CLASS}\0");
    x.change_property8(
        PropMode::REPLACE,
        window,
        AtomEnum::WM_CLASS,
        AtomEnum::STRING,
        class.as_bytes(),
    )
    .map_err(be)?;
    let title = format!("Lucerna wallpaper ({})", output.connector);
    x.change_property8(
        PropMode::REPLACE,
        window,
        AtomEnum::WM_NAME,
        AtomEnum::STRING,
        title.as_bytes(),
    )
    .map_err(be)?;
    x.change_property8(
        PropMode::REPLACE,
        window,
        a._NET_WM_NAME,
        a.UTF8_STRING,
        title.as_bytes(),
    )
    .map_err(be)?;
    x.change_property32(
        PropMode::REPLACE,
        window,
        a._NET_WM_PID,
        AtomEnum::CARDINAL,
        &[std::process::id()],
    )
    .map_err(be)?;
    if let Ok(host) = std::fs::read_to_string("/proc/sys/kernel/hostname") {
        x.change_property8(
            PropMode::REPLACE,
            window,
            a.WM_CLIENT_MACHINE,
            AtomEnum::STRING,
            host.trim().as_bytes(),
        )
        .map_err(be)?;
    }
    x.change_property8(
        PropMode::REPLACE,
        window,
        a._LUCERNA_WALLPAPER,
        a.UTF8_STRING,
        output.id.as_str().as_bytes(),
    )
    .map_err(be)?;

    // Undecorated (Motif hints: flags = MWM_HINTS_DECORATIONS, decorations = 0).
    x.change_property32(
        PropMode::REPLACE,
        window,
        a._MOTIF_WM_HINTS,
        a._MOTIF_WM_HINTS,
        &[2, 0, 0, 0, 0],
    )
    .map_err(be)?;

    // Never takes keyboard focus.
    let mut hints = WmHints::new();
    hints.input = Some(false);
    hints.set(x, window).map_err(be)?;

    // Desktop-type window that is below, off the taskbar and pager, and on every workspace.
    x.change_property32(
        PropMode::REPLACE,
        window,
        a._NET_WM_WINDOW_TYPE,
        AtomEnum::ATOM,
        &[a._NET_WM_WINDOW_TYPE_DESKTOP],
    )
    .map_err(be)?;
    x.change_property32(
        PropMode::REPLACE,
        window,
        a._NET_WM_STATE,
        AtomEnum::ATOM,
        &[
            a._NET_WM_STATE_BELOW,
            a._NET_WM_STATE_SKIP_TASKBAR,
            a._NET_WM_STATE_SKIP_PAGER,
            a._NET_WM_STATE_STICKY,
        ],
    )
    .map_err(be)?;
    x.change_property32(
        PropMode::REPLACE,
        window,
        a._NET_WM_DESKTOP,
        AtomEnum::CARDINAL,
        &[ALL_DESKTOPS],
    )
    .map_err(be)?;
    x.change_property32(
        PropMode::REPLACE,
        window,
        a._NET_WM_BYPASS_COMPOSITOR,
        AtomEnum::CARDINAL,
        &[BYPASS_NEVER],
    )
    .map_err(be)?;

    // Input-disabled: an *empty* input region. The server's window lookup skips this window and
    // its whole subtree (including mpv's child window), so clicks reach whatever is underneath.
    shape::rectangles(
        x,
        SO::SET,
        SK::INPUT,
        ClipOrdering::UNSORTED,
        window,
        0,
        0,
        &[] as &[Rectangle],
    )
    .map_err(be)?;

    x.map_window(window).map_err(be)?;
    if override_redirect {
        lower(conn, window)?;
    }
    x.flush().map_err(be)?;
    // Surface any error the asynchronous requests above produced.
    x.get_input_focus().map_err(be)?.reply().map_err(be)?;
    Ok(())
}

/// Put `window` at the bottom of the root window's children.
pub fn lower(conn: &Conn, window: Window) -> Result<(), BackendError> {
    conn.x
        .configure_window(
            window,
            &ConfigureWindowAux::new().stack_mode(StackMode::BELOW),
        )
        .map_err(be)?;
    Ok(())
}

pub fn resize(conn: &Conn, window: Window, geometry: Rect) -> Result<(), BackendError> {
    conn.x
        .configure_window(
            window,
            &ConfigureWindowAux::new()
                .x(i32::from(clamp_i16(geometry.x)))
                .y(i32::from(clamp_i16(geometry.y)))
                .width(u32::from(clamp_u16(geometry.width)))
                .height(u32::from(clamp_u16(geometry.height))),
        )
        .map_err(be)?;
    conn.sync()
}

pub fn destroy(conn: &Conn, window: Window) -> Result<(), BackendError> {
    conn.x.destroy_window(window).map_err(be)?;
    conn.x.flush().map_err(be)?;
    // A window that is already gone produces BadWindow; destroying is idempotent, so ignore it.
    let _ = conn.x.get_input_focus().map_err(be)?.reply();
    Ok(())
}
