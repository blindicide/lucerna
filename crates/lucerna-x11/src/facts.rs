//! Read-only environment facts: window manager, compositor, Nemo, session variables.
//!
//! These feed `probe()`, `diagnostics()` and `lucernactl doctor`. The design *investigates*
//! Cinnamon and Nemo rather than assuming a generic recipe works (directive §8); each fact here
//! either confirms or refutes an assumption listed in `docs/X11-CINNAMON-NOTES.md`.
//! Nothing in this module changes any X11, Cinnamon or Nemo state.

use std::fs;

use lucerna_core::backend::BackendError;
use serde::Serialize;
use serde_json::{Value, json};
use x11rb::connection::Connection as _;
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _, MapState, Window};

use crate::conn::{Conn, be};

/// EWMH atoms in `_NET_SUPPORTED` that matter to Lucerna, reported by name.
const INTERESTING: &[&str] = &[
    "_NET_CLIENT_LIST",
    "_NET_WM_STATE_FULLSCREEN",
    "_NET_WM_STATE_BELOW",
    "_NET_WM_STATE_STICKY",
    "_NET_WM_STATE_SKIP_TASKBAR",
    "_NET_WM_STATE_SKIP_PAGER",
    "_NET_WM_WINDOW_TYPE_DESKTOP",
    "_NET_WM_DESKTOP",
    "_NET_CURRENT_DESKTOP",
    "_NET_ACTIVE_WINDOW",
    "_NET_RESTACK_WINDOW",
    "_NET_WM_BYPASS_COMPOSITOR",
    "_NET_WM_FULLSCREEN_MONITORS",
];

/// Name of the window manager, from `_NET_SUPPORTING_WM_CHECK` → `_NET_WM_NAME`.
pub fn window_manager(conn: &Conn) -> Result<Option<(Window, String)>, BackendError> {
    let check = conn.property_u32(
        conn.root,
        conn.atoms._NET_SUPPORTING_WM_CHECK,
        AtomEnum::WINDOW,
        1,
    )?;
    let Some(&window) = check.first() else {
        return Ok(None);
    };
    let name = conn
        .property_text(window, conn.atoms._NET_WM_NAME)?
        .unwrap_or_default();
    Ok(Some((window, name)))
}

/// The atoms of `_NET_SUPPORTED`, by name, reduced to the ones Lucerna cares about.
pub fn supported_atoms(conn: &Conn) -> Result<Vec<String>, BackendError> {
    let atoms = conn.property_u32(conn.root, conn.atoms._NET_SUPPORTED, AtomEnum::ATOM, 4096)?;
    let mut names: Vec<String> = atoms
        .into_iter()
        .map(|a| conn.atom_name(a))
        .filter(|n| INTERESTING.contains(&n.as_str()))
        .collect();
    names.sort();
    Ok(names)
}

/// Can fullscreen windows be detected on this window manager?
pub fn fullscreen_supported(supported: &[String]) -> bool {
    ["_NET_CLIENT_LIST", "_NET_WM_STATE_FULLSCREEN"]
        .iter()
        .all(|needed| supported.iter().any(|s| s == needed))
}

/// Selection owner of `_NET_WM_CM_S<screen>`: a compositing manager is running if it is set.
pub fn compositor_owner(conn: &Conn) -> Result<Option<Window>, BackendError> {
    let name = format!("_NET_WM_CM_S{}", conn.screen_num);
    let atom = conn
        .x
        .intern_atom(false, name.as_bytes())
        .map_err(be)?
        .reply()
        .map_err(be)?
        .atom;
    let owner = conn
        .x
        .get_selection_owner(atom)
        .map_err(be)?
        .reply()
        .map_err(be)?
        .owner;
    Ok((owner != 0).then_some(owner))
}

/// Pids of running `nemo-desktop` processes, found by scanning `/proc/*/comm` (no shell).
pub fn nemo_desktop_pids() -> Vec<u32> {
    scan_proc_comm("nemo-desktop")
}

fn scan_proc_comm(wanted: &str) -> Vec<u32> {
    let Ok(entries) = fs::read_dir("/proc") else {
        return Vec::new();
    };
    let mut pids: Vec<u32> = entries
        .flatten()
        .filter_map(|e| e.file_name().to_string_lossy().parse::<u32>().ok())
        .filter(|pid| {
            fs::read_to_string(format!("/proc/{pid}/comm")).is_ok_and(|c| c.trim() == wanted)
        })
        .collect();
    pids.sort_unstable();
    pids
}

/// The root's children, bottom to top.
pub fn root_children(conn: &Conn) -> Result<Vec<Window>, BackendError> {
    Ok(conn
        .x
        .query_tree(conn.root)
        .map_err(be)?
        .reply()
        .map_err(be)?
        .children)
}

/// The child of root that contains `window` (or `window` itself if it is one).
pub fn root_ancestor(conn: &Conn, window: Window) -> Option<Window> {
    let mut current = window;
    for _ in 0..16 {
        let tree = conn.x.query_tree(current).ok()?.reply().ok()?;
        if tree.parent == conn.root {
            return Some(current);
        }
        if tree.parent == 0 {
            return None;
        }
        current = tree.parent;
    }
    None
}

fn class_instance(conn: &Conn, window: Window) -> Option<String> {
    let raw = conn
        .property_raw(window, conn.atoms.WM_CLASS, AtomEnum::STRING, 256)
        .ok()??;
    raw.value
        .split(|b| *b == 0)
        .next()
        .map(|s| String::from_utf8_lossy(s).into_owned())
}

/// Find Nemo's desktop window among the EWMH client list and the root's children, and describe it.
///
/// Whether the wallpaper ends up below it is a question only a real Cinnamon session answers;
/// this reports the facts needed to settle it: depth (32 suggests an ARGB, possibly transparent
/// window), geometry and where it sits in the stacking order.
pub fn nemo_desktop_window(conn: &Conn) -> Result<Value, BackendError> {
    let children = root_children(conn)?;
    let clients = conn.property_u32(
        conn.root,
        conn.atoms._NET_CLIENT_LIST,
        AtomEnum::WINDOW,
        4096,
    )?;

    let is_nemo = |window: Window| -> bool {
        class_instance(conn, window).is_some_and(|c| c == "nemo-desktop")
    };
    let candidate = clients
        .iter()
        .copied()
        .chain(children.iter().copied())
        .find(|w| is_nemo(*w));

    let Some(window) = candidate else {
        return Ok(Value::Null);
    };
    let geometry = conn
        .x
        .get_geometry(window)
        .ok()
        .and_then(|c| c.reply().ok());
    let ancestor = root_ancestor(conn, window);
    let stack_index = ancestor.and_then(|a| children.iter().position(|c| *c == a));
    let window_type = conn
        .property_u32(window, conn.atoms._NET_WM_WINDOW_TYPE, AtomEnum::ATOM, 8)?
        .into_iter()
        .map(|a| conn.atom_name(a))
        .collect::<Vec<_>>();
    Ok(json!({
        "xid": window,
        "root_child": ancestor,
        "depth": geometry.as_ref().map(|g| g.depth),
        "geometry": geometry.as_ref().map(|g| json!({"x": g.x, "y": g.y, "width": g.width, "height": g.height})),
        "stack_index": stack_index,
        "window_type": window_type,
    }))
}

/// Windows carrying our `_LUCERNA_WALLPAPER` marker, whichever process created them.
pub fn lucerna_windows(conn: &Conn) -> Result<Vec<Window>, BackendError> {
    Ok(root_children(conn)?
        .into_iter()
        .filter(|w| {
            conn.property_raw(*w, conn.atoms._LUCERNA_WALLPAPER, AtomEnum::ANY, 16)
                .ok()
                .flatten()
                .is_some()
        })
        .collect())
}

/// `XDG_SESSION_TYPE`, `XDG_CURRENT_DESKTOP` and `DESKTOP_SESSION` as seen by this process.
pub fn session_variables() -> Value {
    let get = |key: &str| std::env::var(key).ok();
    json!({
        "XDG_SESSION_TYPE": get("XDG_SESSION_TYPE"),
        "XDG_CURRENT_DESKTOP": get("XDG_CURRENT_DESKTOP"),
        "DESKTOP_SESSION": get("DESKTOP_SESSION"),
    })
}

/// All connection-level facts shared by `probe()` and [`probe_display`].
pub fn connection_facts(conn: &Conn) -> Result<Value, BackendError> {
    let setup = conn.x.setup();
    let vendor = String::from_utf8_lossy(&setup.vendor).into_owned();
    let wm = window_manager(conn)?;
    let supported = supported_atoms(conn)?;
    let compositor = compositor_owner(conn)?;
    let nemo_window = nemo_desktop_window(conn)?;
    let screen = &setup.roots[conn.screen_num];
    Ok(json!({
        "display": conn.display,
        "server": {
            "vendor": vendor,
            "release": setup.release_number,
            "protocol": format!("{}.{}", setup.protocol_major_version, setup.protocol_minor_version),
        },
        "screen": {"number": conn.screen_num, "width": screen.width_in_pixels, "height": screen.height_in_pixels, "root_depth": screen.root_depth},
        "randr": {
            "present": conn.randr.is_some(),
            "version": conn.randr.map(|(a, b)| format!("{a}.{b}")),
            "monitors_api": conn.randr.is_some_and(|v| v >= (1, 5)),
        },
        "shape": conn.shape,
        "window_manager": wm.as_ref().map(|(w, n)| json!({"check_window": w, "name": n})),
        "supported_atoms": supported,
        "compositor": {"running": compositor.is_some(), "owner": compositor},
        "nemo": {"process_ids": nemo_desktop_pids(), "desktop_window": nemo_window},
        "session": session_variables(),
    }))
}

/// Result of [`probe_display`]: a read-only description of an X session, for `doctor`.
#[derive(Debug, Clone, Serialize)]
pub struct X11Probe {
    /// Whether a connection could be made.
    pub connected: bool,
    /// The connection error (already worded for users) when `connected` is false.
    pub error: Option<String>,
    pub facts: Value,
    pub outputs: Vec<lucerna_core::backend::OutputInfo>,
    /// Existing Lucerna wallpaper windows (from a running daemon).
    pub lucerna_windows: Vec<Value>,
}

/// Connect to `display`, describe it, and disconnect. Creates no windows and changes nothing.
pub fn probe_display(display: Option<&str>) -> X11Probe {
    let conn = match Conn::connect(display) {
        Ok(conn) => conn,
        Err(err) => {
            return X11Probe {
                connected: false,
                error: Some(err.to_string()),
                facts: Value::Null,
                outputs: Vec::new(),
                lucerna_windows: Vec::new(),
            };
        }
    };
    let facts = connection_facts(&conn).unwrap_or_else(|e| json!({"error": e.to_string()}));
    let outputs = crate::outputs::enumerate(&conn).unwrap_or_default();
    let windows = lucerna_windows(&conn)
        .unwrap_or_default()
        .into_iter()
        .map(|w| describe_our_window(&conn, w, None))
        .collect();
    X11Probe {
        connected: true,
        error: None,
        facts,
        outputs,
        lucerna_windows: windows,
    }
}

/// Describe one of our windows for diagnostics.
pub fn describe_our_window(conn: &Conn, window: Window, surface_id: Option<u64>) -> Value {
    let children = root_children(conn).unwrap_or_default();
    let attrs = conn
        .x
        .get_window_attributes(window)
        .ok()
        .and_then(|c| c.reply().ok());
    let geometry = conn
        .x
        .get_geometry(window)
        .ok()
        .and_then(|c| c.reply().ok());
    let tree = conn.x.query_tree(window).ok().and_then(|c| c.reply().ok());
    json!({
        "surface": surface_id,
        "xid": window,
        "output": conn.property_text(window, conn.atoms._LUCERNA_WALLPAPER).ok().flatten(),
        "override_redirect": attrs.as_ref().map(|a| a.override_redirect),
        "map_state": attrs.as_ref().map(|a| match a.map_state {
            MapState::UNMAPPED => "unmapped",
            MapState::UNVIEWABLE => "unviewable",
            MapState::VIEWABLE => "viewable",
            _ => "unknown",
        }),
        "geometry": geometry.as_ref().map(|g| json!({"x": g.x, "y": g.y, "width": g.width, "height": g.height})),
        "stack_index": children.iter().position(|c| *c == window),
        "stack_size": children.len(),
        "child_windows": tree.map(|t| t.children.len()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fullscreen_needs_both_atoms() {
        let full = ["_NET_CLIENT_LIST", "_NET_WM_STATE_FULLSCREEN"].map(String::from);
        assert!(fullscreen_supported(&full));
        assert!(!fullscreen_supported(&full[..1]));
        assert!(!fullscreen_supported(&[]));
    }

    #[test]
    fn scanning_proc_finds_our_own_process_name_and_nothing_bogus() {
        let me = fs::read_to_string("/proc/self/comm").unwrap();
        assert!(scan_proc_comm(me.trim()).contains(&std::process::id()));
        assert!(scan_proc_comm("definitely-not-a-process-name").is_empty());
    }
}
