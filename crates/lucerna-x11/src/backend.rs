//! `X11Backend`: the [`WallpaperBackend`] implementation for X11 (Cinnamon/Muffin first).

use std::collections::{BTreeMap, HashSet};
use std::sync::{Arc, Mutex};

use lucerna_core::backend::{
    BackendCapabilities, BackendError, BackendKind, BackendProbe, EmbedTarget, EventSink,
    OutputInfo, Rect, SurfaceHandle, SurfaceId, WallpaperBackend,
};
use serde_json::{Value, json};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::Window;

use crate::conn::{Conn, SharedConn};
use crate::events::EventThread;
use crate::{facts, outputs, surface};

/// How wallpaper surfaces are stacked (`[x11] stacking` in the configuration).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StackingMode {
    /// Currently the same as [`StackingMode::OverrideRedirect`].
    #[default]
    Auto,
    /// Unmanaged windows kept at the bottom of the root's children, re-lowered on disturbance.
    OverrideRedirect,
    /// Windows managed by the window manager with `_NET_WM_WINDOW_TYPE_DESKTOP`.
    DesktopWindow,
}

impl StackingMode {
    /// Resolve `Auto` to the concrete strategy.
    pub fn resolved(self) -> Self {
        match self {
            Self::Auto => Self::OverrideRedirect,
            other => other,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::OverrideRedirect => "override-redirect",
            Self::DesktopWindow => "desktop-window",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct X11Options {
    /// `None` means `$DISPLAY`. Tests always pass an explicit display.
    pub display: Option<String>,
    /// `XDG_CURRENT_DESKTOP`, used to recognise Cinnamon.
    pub current_desktop: Option<String>,
    pub stacking: StackingMode,
}

struct Surface {
    window: Window,
    output: OutputInfoLite,
}

/// The parts of an output we need to remember.
struct OutputInfoLite {
    id: String,
    connector: String,
}

pub struct X11Backend {
    conn: SharedConn,
    kind: BackendKind,
    stacking: StackingMode,
    surfaces: BTreeMap<SurfaceId, Surface>,
    next_surface: u64,
    ours: Arc<Mutex<HashSet<u32>>>,
    events: Option<EventThread>,
    shut_down: bool,
}

/// Connect and pick `cinnamon-x11` or `x11-ewmh`.
pub fn connect(options: &X11Options) -> Result<X11Backend, BackendError> {
    X11Backend::connect(options)
}

impl X11Backend {
    pub fn connect(options: &X11Options) -> Result<Self, BackendError> {
        let conn = Conn::connect(options.display.as_deref())?;
        match conn.randr {
            Some((major, minor)) if (major, minor) >= (1, 2) => {}
            _ => return Err(BackendError::MissingFeature("RandR 1.2 or newer")),
        }
        let kind = detect_kind(&conn, options.current_desktop.as_deref());
        tracing::info!(display = %conn.display, backend = kind.as_str(), "connected to the X server");
        Ok(Self {
            conn,
            kind,
            stacking: options.stacking.resolved(),
            surfaces: BTreeMap::new(),
            next_surface: 1,
            ours: Arc::new(Mutex::new(HashSet::new())),
            events: None,
            shut_down: false,
        })
    }

    fn remember(&self, window: Window, present: bool) {
        if let Ok(mut set) = self.ours.lock() {
            if present {
                set.insert(window);
            } else {
                set.remove(&window);
            }
        }
    }
}

fn detect_kind(conn: &Conn, current_desktop: Option<&str>) -> BackendKind {
    let by_desktop = current_desktop.is_some_and(|d| d.to_ascii_lowercase().contains("cinnamon"));
    let by_wm = facts::window_manager(conn)
        .ok()
        .flatten()
        .is_some_and(|(_, name)| name.to_ascii_lowercase().contains("muffin"));
    if by_desktop || by_wm {
        BackendKind::CinnamonX11
    } else {
        BackendKind::X11Ewmh
    }
}

impl WallpaperBackend for X11Backend {
    fn kind(&self) -> BackendKind {
        self.kind
    }

    fn probe(&mut self) -> Result<BackendProbe, BackendError> {
        let mut facts_value = facts::connection_facts(&self.conn)?;
        let supported = facts::supported_atoms(&self.conn)?;
        let outputs = outputs::enumerate(&self.conn)?;
        let capabilities = BackendCapabilities {
            fullscreen_detection: facts::fullscreen_supported(&supported),
            input_passthrough: self.conn.shape,
            hotplug_events: self.conn.randr.is_some_and(|v| v >= (1, 2)),
            stable_monitor_identity: outputs::any_edid(&outputs),
        };
        if let Value::Object(map) = &mut facts_value {
            map.insert("stacking_mode".to_owned(), json!(self.stacking.as_str()));
        }
        Ok(BackendProbe {
            kind: self.kind,
            capabilities,
            facts: facts_value,
        })
    }

    fn enumerate_outputs(&mut self) -> Result<Vec<OutputInfo>, BackendError> {
        outputs::enumerate(&self.conn)
    }

    fn create_surface(&mut self, output: &OutputInfo) -> Result<SurfaceHandle, BackendError> {
        let window = surface::create(&self.conn, output, self.stacking)?;
        let id = SurfaceId(self.next_surface);
        self.next_surface += 1;
        self.remember(window, true);
        self.surfaces.insert(
            id,
            Surface {
                window,
                output: OutputInfoLite {
                    id: output.id.as_str().to_owned(),
                    connector: output.connector.clone(),
                },
            },
        );
        tracing::info!(output = %output.id, xid = window, "wallpaper surface created");
        Ok(SurfaceHandle {
            id,
            output: output.id.clone(),
            geometry: output.geometry,
            embed: EmbedTarget::X11Window(window),
        })
    }

    fn resize_surface(
        &mut self,
        surface_id: SurfaceId,
        geometry: Rect,
    ) -> Result<(), BackendError> {
        let window = self
            .surfaces
            .get(&surface_id)
            .map(|s| s.window)
            .ok_or(BackendError::UnknownSurface(surface_id))?;
        surface::resize(&self.conn, window, geometry)
    }

    fn destroy_surface(&mut self, surface_id: SurfaceId) -> Result<(), BackendError> {
        if let Some(entry) = self.surfaces.remove(&surface_id) {
            self.remember(entry.window, false);
            surface::destroy(&self.conn, entry.window)?;
            tracing::info!(output = %entry.output.id, "wallpaper surface destroyed");
        }
        Ok(())
    }

    fn refresh(&mut self) -> Result<(), BackendError> {
        if self.stacking != StackingMode::OverrideRedirect || self.surfaces.is_empty() {
            // A window-manager-managed surface is stacked by the window manager.
            return Ok(());
        }
        let children = facts::root_children(&self.conn)?;
        let ours: HashSet<Window> = self.surfaces.values().map(|s| s.window).collect();
        let bottom: HashSet<Window> = children.iter().take(ours.len()).copied().collect();
        // Only touch the stack if a surface is not already among the bottom-most windows.
        for window in ours.iter().filter(|w| !bottom.contains(*w)) {
            surface::lower(&self.conn, *window)?;
        }
        self.conn.sync()
    }

    fn subscribe(&mut self, sink: EventSink) -> Result<(), BackendError> {
        if self.events.is_some() {
            return Ok(());
        }
        self.events = Some(EventThread::spawn(
            Arc::clone(&self.conn),
            sink,
            Arc::clone(&self.ours),
            self.stacking,
        )?);
        Ok(())
    }

    fn diagnostics(&mut self) -> Value {
        let children = facts::root_children(&self.conn).unwrap_or_default();
        let nemo = facts::nemo_desktop_window(&self.conn).unwrap_or(Value::Null);
        let nemo_index = nemo.get("stack_index").and_then(Value::as_u64);
        let surfaces: Vec<Value> = self
            .surfaces
            .iter()
            .map(|(id, s)| {
                let mut described = facts::describe_our_window(&self.conn, s.window, Some(id.0));
                if let Value::Object(map) = &mut described {
                    map.insert("connector".to_owned(), json!(s.output.connector));
                    map.insert("output_id".to_owned(), json!(s.output.id));
                    let mine = map.get("stack_index").and_then(Value::as_u64);
                    // "Below Nemo's desktop window" is what the design assumes; `null` = unknown.
                    let below_nemo = match (mine, nemo_index) {
                        (Some(m), Some(n)) => json!(m < n),
                        _ => Value::Null,
                    };
                    map.insert("below_nemo".to_owned(), below_nemo);
                }
                described
            })
            .collect();
        json!({
            "backend": self.kind.as_str(),
            "stacking_mode": self.stacking.as_str(),
            "root_children": children.len(),
            "surfaces": surfaces,
            "nemo_desktop_window": nemo,
            "nemo_process_ids": facts::nemo_desktop_pids(),
            "window_manager": facts::window_manager(&self.conn).ok().flatten()
                .map(|(w, n)| json!({"check_window": w, "name": n})),
            "compositor_owner": facts::compositor_owner(&self.conn).ok().flatten(),
        })
    }

    fn shutdown(&mut self) -> Result<(), BackendError> {
        if self.shut_down {
            return Ok(());
        }
        self.shut_down = true;
        if let Some(mut events) = self.events.take() {
            events.stop();
        }
        let windows: Vec<Window> = self.surfaces.values().map(|s| s.window).collect();
        self.surfaces.clear();
        for window in windows {
            self.remember(window, false);
            // The server is going away or already gone in some shutdown paths; keep going.
            if let Err(err) = surface::destroy(&self.conn, window) {
                tracing::debug!(%err, "could not destroy a surface during shutdown");
            }
        }
        let _ = self.conn.x.flush();
        Ok(())
    }
}

impl Drop for X11Backend {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}
