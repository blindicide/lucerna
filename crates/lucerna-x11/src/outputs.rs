//! RandR output enumeration (directive §12).
//!
//! RandR 1.5 `GetMonitors` is preferred because it reports geometry in root coordinates after
//! rotation, transforms and scaling. On RandR 1.2–1.4 the CRTCs are used instead.

use std::collections::BTreeMap;

use lucerna_core::backend::{BackendError, EdidIdentity, OutputInfo, Rect, Rotation};
use lucerna_core::identity::{IdentityInput, parse_edid, stable_ids};
use x11rb::CURRENT_TIME;
use x11rb::connection::Connection as _;
use x11rb::protocol::randr::{
    self, Connection as RandrConnection, ConnectionExt as _, ModeFlag, ModeInfo,
    Rotation as XRotation,
};
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _};

use crate::conn::{Conn, be};

/// What we know about one physical (or virtual) monitor before identities are assigned.
struct Raw {
    connector: String,
    geometry: Rect,
    primary: bool,
    rotation: Rotation,
    refresh_mhz: Option<u32>,
    edid: Option<EdidIdentity>,
}

pub fn enumerate(conn: &Conn) -> Result<Vec<OutputInfo>, BackendError> {
    let Some((major, minor)) = conn.randr else {
        return Err(BackendError::MissingFeature("RandR 1.2 or newer"));
    };
    if major == 1 && minor < 2 {
        return Err(BackendError::MissingFeature("RandR 1.2 or newer"));
    }

    let modes = conn
        .x
        .randr_get_screen_resources_current(conn.root)
        .map_err(be)?
        .reply()
        .map_err(be)?
        .modes;

    let mut raw = if (major, minor) >= (1, 5) {
        from_monitors(conn, &modes)?
    } else {
        from_crtcs(conn, &modes)?
    };

    if raw.is_empty() {
        // No RandR outputs at all (unusual): treat the whole screen as one display.
        let screen = &conn.x.setup().roots[conn.screen_num];
        raw.push(Raw {
            connector: "default".to_owned(),
            geometry: Rect::new(
                0,
                0,
                u32::from(screen.width_in_pixels),
                u32::from(screen.height_in_pixels),
            ),
            primary: true,
            rotation: Rotation::Normal,
            refresh_mhz: None,
            edid: None,
        });
    }

    raw.sort_by(|a, b| a.connector.cmp(&b.connector));
    let inputs: Vec<IdentityInput> = raw
        .iter()
        .map(|r| IdentityInput {
            connector: r.connector.clone(),
            edid: r.edid.clone(),
        })
        .collect();
    let ids = stable_ids(&inputs);

    Ok(raw
        .into_iter()
        .zip(ids)
        .map(|(r, id)| OutputInfo {
            id,
            connector: r.connector,
            geometry: r.geometry,
            primary: r.primary,
            rotation: r.rotation,
            refresh_mhz: r.refresh_mhz,
            edid: r.edid,
        })
        .collect())
}

fn from_monitors(conn: &Conn, modes: &[ModeInfo]) -> Result<Vec<Raw>, BackendError> {
    let monitors = conn
        .x
        .randr_get_monitors(conn.root, true)
        .map_err(be)?
        .reply()
        .map_err(be)?
        .monitors;

    let mut out = Vec::new();
    for monitor in monitors {
        let geometry = Rect::new(
            i32::from(monitor.x),
            i32::from(monitor.y),
            u32::from(monitor.width),
            u32::from(monitor.height),
        );
        if geometry.width == 0 || geometry.height == 0 {
            continue;
        }
        // Mirrored outputs form one monitor: identity comes from the lexicographically first one.
        let mut named = Vec::new();
        for output in &monitor.outputs {
            if let Some(name) = output_name(conn, *output) {
                named.push((name, *output));
            }
        }
        named.sort();

        let (connector, details) = match named.first() {
            Some((name, output)) => (name.clone(), Some(*output)),
            // A monitor defined with `SetMonitor` that claims no output: use its own name.
            None => (conn.atom_name(monitor.name), None),
        };
        let (rotation, refresh_mhz, edid) = match details {
            Some(output) => output_details(conn, output, modes),
            None => (Rotation::Normal, None, None),
        };
        out.push(Raw {
            connector,
            geometry,
            primary: monitor.primary,
            rotation,
            refresh_mhz,
            edid,
        });
    }
    Ok(out)
}

fn from_crtcs(conn: &Conn, modes: &[ModeInfo]) -> Result<Vec<Raw>, BackendError> {
    let resources = conn
        .x
        .randr_get_screen_resources_current(conn.root)
        .map_err(be)?
        .reply()
        .map_err(be)?;
    let primary = conn
        .x
        .randr_get_output_primary(conn.root)
        .map_err(be)?
        .reply()
        .map_err(be)?
        .output;

    // Group outputs by the CRTC that drives them: mirrored outputs share one.
    let mut by_crtc: BTreeMap<u32, Vec<(String, u32)>> = BTreeMap::new();
    for output in resources.outputs {
        let info = conn
            .x
            .randr_get_output_info(output, CURRENT_TIME)
            .map_err(be)?
            .reply()
            .map_err(be)?;
        if info.connection != RandrConnection::CONNECTED || info.crtc == 0 {
            continue;
        }
        by_crtc
            .entry(info.crtc)
            .or_default()
            .push((String::from_utf8_lossy(&info.name).into_owned(), output));
    }

    let mut out = Vec::new();
    for (crtc, mut outputs) in by_crtc {
        outputs.sort();
        let info = conn
            .x
            .randr_get_crtc_info(crtc, CURRENT_TIME)
            .map_err(be)?
            .reply()
            .map_err(be)?;
        if info.width == 0 || info.height == 0 {
            continue;
        }
        let (name, output) = outputs[0].clone();
        let (_, refresh_mhz, edid) = output_details(conn, output, modes);
        out.push(Raw {
            connector: name,
            geometry: Rect::new(
                i32::from(info.x),
                i32::from(info.y),
                u32::from(info.width),
                u32::from(info.height),
            ),
            primary: outputs.iter().any(|(_, o)| *o == primary),
            rotation: rotation_of(info.rotation),
            refresh_mhz,
            edid,
        });
    }
    Ok(out)
}

fn output_name(conn: &Conn, output: u32) -> Option<String> {
    let info = conn
        .x
        .randr_get_output_info(output, CURRENT_TIME)
        .ok()?
        .reply()
        .ok()?;
    Some(String::from_utf8_lossy(&info.name).into_owned())
}

/// Rotation, refresh rate and EDID of one output.
fn output_details(
    conn: &Conn,
    output: u32,
    modes: &[ModeInfo],
) -> (Rotation, Option<u32>, Option<EdidIdentity>) {
    let mut rotation = Rotation::Normal;
    let mut refresh = None;
    if let Some(info) = conn
        .x
        .randr_get_output_info(output, CURRENT_TIME)
        .ok()
        .and_then(|c| c.reply().ok())
        && info.crtc != 0
        && let Some(crtc) = conn
            .x
            .randr_get_crtc_info(info.crtc, CURRENT_TIME)
            .ok()
            .and_then(|c| c.reply().ok())
    {
        rotation = rotation_of(crtc.rotation);
        refresh = modes
            .iter()
            .find(|m| m.id == crtc.mode)
            .and_then(refresh_mhz);
    }
    (rotation, refresh, read_edid(conn, output))
}

fn rotation_of(rotation: XRotation) -> Rotation {
    if rotation.contains(XRotation::ROTATE90) {
        Rotation::Left
    } else if rotation.contains(XRotation::ROTATE180) {
        Rotation::Inverted
    } else if rotation.contains(XRotation::ROTATE270) {
        Rotation::Right
    } else {
        Rotation::Normal
    }
}

fn refresh_mhz(mode: &ModeInfo) -> Option<u32> {
    let total = u64::from(mode.htotal) * u64::from(mode.vtotal);
    if total == 0 || mode.dot_clock == 0 {
        return None;
    }
    let mut mhz = u64::from(mode.dot_clock) * 1000 / total;
    if mode.mode_flags.contains(ModeFlag::INTERLACE) {
        mhz *= 2;
    }
    if mode.mode_flags.contains(ModeFlag::DOUBLE_SCAN) {
        mhz /= 2;
    }
    u32::try_from(mhz).ok().filter(|m| *m > 0)
}

/// Read and parse the `EDID` output property (at most 256 bytes; only the base block is used).
fn read_edid(conn: &Conn, output: u32) -> Option<EdidIdentity> {
    let atom = conn.x.intern_atom(true, b"EDID").ok()?.reply().ok()?.atom;
    if atom == 0 {
        return None;
    }
    let reply = conn
        .x
        .randr_get_output_property(output, atom, AtomEnum::ANY, 0, 64, false, false)
        .ok()?
        .reply()
        .ok()?;
    parse_edid(&reply.data).ok()
}

/// Cheap check used by `probe`: does any output expose a readable EDID?
pub fn any_edid(outputs: &[OutputInfo]) -> bool {
    outputs.iter().any(|o| o.edid.is_some())
}

/// Select RandR change notifications on the root window.
pub fn select_notifications(conn: &Conn) -> Result<(), BackendError> {
    if conn.randr.is_none() {
        return Ok(());
    }
    randr::select_input(
        &conn.x,
        conn.root,
        randr::NotifyMask::SCREEN_CHANGE
            | randr::NotifyMask::CRTC_CHANGE
            | randr::NotifyMask::OUTPUT_CHANGE,
    )
    .map_err(be)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mode(dot_clock: u32, htotal: u16, vtotal: u16, flags: ModeFlag) -> ModeInfo {
        ModeInfo {
            id: 1,
            width: 1920,
            height: 1080,
            dot_clock,
            hsync_start: 0,
            hsync_end: 0,
            htotal,
            hskew: 0,
            vsync_start: 0,
            vsync_end: 0,
            vtotal,
            name_len: 0,
            mode_flags: flags,
        }
    }

    #[test]
    fn refresh_of_a_typical_1080p60_mode() {
        // 148.5 MHz, 2200 x 1125 -> 60.000 Hz
        let m = mode(148_500_000, 2200, 1125, ModeFlag::from(0u32));
        assert_eq!(refresh_mhz(&m), Some(60_000));
    }

    #[test]
    fn interlace_doubles_and_doublescan_halves() {
        let base = mode(74_250_000, 2200, 1125, ModeFlag::from(0u32));
        assert_eq!(refresh_mhz(&base), Some(30_000));
        let interlaced = mode(74_250_000, 2200, 1125, ModeFlag::INTERLACE);
        assert_eq!(refresh_mhz(&interlaced), Some(60_000));
        let doublescan = mode(74_250_000, 2200, 1125, ModeFlag::DOUBLE_SCAN);
        assert_eq!(refresh_mhz(&doublescan), Some(15_000));
    }

    #[test]
    fn degenerate_modes_have_no_refresh() {
        assert_eq!(
            refresh_mhz(&mode(0, 2200, 1125, ModeFlag::from(0u32))),
            None
        );
        assert_eq!(refresh_mhz(&mode(1, 0, 1125, ModeFlag::from(0u32))), None);
    }

    #[test]
    fn rotation_flags_map_to_rotations() {
        assert_eq!(rotation_of(XRotation::ROTATE0), Rotation::Normal);
        assert_eq!(rotation_of(XRotation::ROTATE90), Rotation::Left);
        assert_eq!(rotation_of(XRotation::ROTATE180), Rotation::Inverted);
        assert_eq!(rotation_of(XRotation::ROTATE270), Rotation::Right);
        // Reflection bits do not change the rotation.
        assert_eq!(
            rotation_of(XRotation::ROTATE0 | XRotation::REFLECT_X),
            Rotation::Normal
        );
    }
}
