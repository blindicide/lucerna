//! The backend's event thread (directive §15, §12).
//!
//! Two threads: a *reader* that blocks in `wait_for_event`, and a *processor* that turns raw X
//! events into debounced [`BackendEvent`]s. The processor sleeps in `recv_timeout` until the next
//! debounce deadline, so an idle desktop costs no wake-ups at all (no polling).
//!
//! Events are debounced: RandR changes by 500 ms, fullscreen recomputation by 150 ms, stacking
//! disturbances by 100 ms.

use std::collections::HashSet;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use lucerna_core::backend::{BackendError, BackendEvent, EventSink, Rect};
use x11rb::COPY_FROM_PARENT;
use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{
    AtomEnum, ChangeWindowAttributesAux, ClientMessageEvent, ConnectionExt as _, CreateWindowAux,
    EventMask, MapState, Place, Window, WindowClass,
};

use crate::backend::StackingMode;
use crate::conn::{SharedConn, be};
use crate::outputs;
use crate::surface::ALL_DESKTOPS;

const OUTPUTS_DEBOUNCE: Duration = Duration::from_millis(500);
const FULLSCREEN_DEBOUNCE: Duration = Duration::from_millis(150);
const STACKING_DEBOUNCE: Duration = Duration::from_millis(100);

enum Msg {
    Event(Box<Event>),
    Lost(String),
    Stop,
}

/// Handle to the running threads.
pub struct EventThread {
    tx: mpsc::Sender<Msg>,
    wake_window: Window,
    conn: SharedConn,
    reader: Option<JoinHandle<()>>,
    processor: Option<JoinHandle<()>>,
}

impl EventThread {
    pub fn spawn(
        conn: SharedConn,
        sink: EventSink,
        ours: Arc<Mutex<HashSet<u32>>>,
        stacking: StackingMode,
    ) -> Result<Self, BackendError> {
        // A private, never-mapped window the reader can be woken through on shutdown: an event
        // sent to a window with an empty event mask is delivered to the client that created it.
        let wake_window = conn.x.generate_id().map_err(be)?;
        conn.x
            .create_window(
                COPY_FROM_PARENT as u8,
                wake_window,
                conn.root,
                0,
                0,
                1,
                1,
                0,
                WindowClass::INPUT_ONLY,
                COPY_FROM_PARENT,
                &CreateWindowAux::new(),
            )
            .map_err(be)?;

        conn.x
            .change_window_attributes(
                conn.root,
                &ChangeWindowAttributesAux::new()
                    .event_mask(EventMask::SUBSTRUCTURE_NOTIFY | EventMask::PROPERTY_CHANGE),
            )
            .map_err(be)?;
        outputs::select_notifications(&conn)?;
        // Wait until the server has applied the selections: once `subscribe` returns, no event
        // that happens afterwards can be missed.
        conn.sync()?;

        let (tx, rx) = mpsc::channel::<Msg>();

        let reader = {
            let conn = Arc::clone(&conn);
            let tx = tx.clone();
            thread::Builder::new()
                .name("lucerna-x11-events".to_owned())
                .spawn(move || {
                    loop {
                        match conn.x.wait_for_event() {
                            Ok(Event::ClientMessage(m)) if m.type_ == conn.atoms._LUCERNA_QUIT => {
                                break;
                            }
                            Ok(event) => {
                                if tx.send(Msg::Event(Box::new(event))).is_err() {
                                    break;
                                }
                            }
                            Err(err) => {
                                let _ = tx.send(Msg::Lost(err.to_string()));
                                break;
                            }
                        }
                    }
                })
                .map_err(|e| BackendError::Protocol(format!("cannot start event thread: {e}")))?
        };

        let processor = {
            let conn = Arc::clone(&conn);
            thread::Builder::new()
                .name("lucerna-x11-process".to_owned())
                .spawn(move || Processor::new(conn, sink, ours, stacking).run(&rx))
                .map_err(|e| BackendError::Protocol(format!("cannot start event thread: {e}")))?
        };

        Ok(Self {
            tx,
            wake_window,
            conn,
            reader: Some(reader),
            processor: Some(processor),
        })
    }

    /// Stop both threads and wait for them. Safe to call on a dead connection.
    pub fn stop(&mut self) {
        let _ = self.tx.send(Msg::Stop);
        let quit = ClientMessageEvent::new(
            32,
            self.wake_window,
            self.conn.atoms._LUCERNA_QUIT,
            [0u32; 5],
        );
        if self
            .conn
            .x
            .send_event(false, self.wake_window, EventMask::NO_EVENT, quit)
            .is_ok()
        {
            let _ = self.conn.x.flush();
        }
        for handle in [self.reader.take(), self.processor.take()]
            .into_iter()
            .flatten()
        {
            let _ = handle.join();
        }
        let _ = self.conn.x.destroy_window(self.wake_window);
        let _ = self.conn.x.flush();
    }
}

impl Drop for EventThread {
    fn drop(&mut self) {
        self.stop();
    }
}

#[derive(Default)]
struct Deadlines {
    outputs: Option<Instant>,
    fullscreen: Option<Instant>,
    stacking: Option<Instant>,
}

impl Deadlines {
    fn earliest(&self) -> Option<Instant> {
        [self.outputs, self.fullscreen, self.stacking]
            .into_iter()
            .flatten()
            .min()
    }
}

struct Processor {
    conn: SharedConn,
    sink: EventSink,
    ours: Arc<Mutex<HashSet<u32>>>,
    stacking: StackingMode,
    deadlines: Deadlines,
    clients: HashSet<Window>,
    last_fullscreen: Vec<Rect>,
}

impl Processor {
    fn new(
        conn: SharedConn,
        sink: EventSink,
        ours: Arc<Mutex<HashSet<u32>>>,
        stacking: StackingMode,
    ) -> Self {
        Self {
            conn,
            sink,
            ours,
            stacking,
            deadlines: Deadlines::default(),
            clients: HashSet::new(),
            last_fullscreen: Vec::new(),
        }
    }

    fn run(mut self, rx: &mpsc::Receiver<Msg>) {
        // Learn the current clients and fullscreen state, so windows that already exist count.
        self.track_clients();
        self.recompute_fullscreen();

        loop {
            let message = match self.deadlines.earliest() {
                Some(deadline) => {
                    rx.recv_timeout(deadline.saturating_duration_since(Instant::now()))
                }
                None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
            };
            match message {
                Ok(Msg::Event(event)) => self.on_event(&event),
                Ok(Msg::Lost(reason)) => {
                    (self.sink)(BackendEvent::ConnectionLost(reason));
                    return;
                }
                Ok(Msg::Stop) | Err(RecvTimeoutError::Disconnected) => return,
                Err(RecvTimeoutError::Timeout) => {}
            }
            self.fire_due();
        }
    }

    fn fire_due(&mut self) {
        let now = Instant::now();
        if self.deadlines.outputs.is_some_and(|d| d <= now) {
            self.deadlines.outputs = None;
            (self.sink)(BackendEvent::OutputsChanged);
        }
        if self.deadlines.fullscreen.is_some_and(|d| d <= now) {
            self.deadlines.fullscreen = None;
            self.recompute_fullscreen();
        }
        if self.deadlines.stacking.is_some_and(|d| d <= now) {
            self.deadlines.stacking = None;
            (self.sink)(BackendEvent::StackingDisturbed);
        }
    }

    fn schedule(slot: &mut Option<Instant>, after: Duration) {
        // Trailing debounce: every new event pushes the deadline out, so a burst produces exactly
        // one notification after things have settled.
        *slot = Some(Instant::now() + after);
    }

    fn is_ours(&self, window: Window) -> bool {
        self.ours.lock().is_ok_and(|set| set.contains(&window))
    }

    fn on_event(&mut self, event: &Event) {
        let a = &self.conn.atoms;
        let root = self.conn.root;
        match event {
            Event::RandrScreenChangeNotify(_) | Event::RandrNotify(_) => {
                Self::schedule(&mut self.deadlines.outputs, OUTPUTS_DEBOUNCE);
            }
            Event::PropertyNotify(e) if e.window == root => {
                if e.atom == a._NET_CLIENT_LIST {
                    self.track_clients();
                    Self::schedule(&mut self.deadlines.fullscreen, FULLSCREEN_DEBOUNCE);
                } else if e.atom == a._NET_ACTIVE_WINDOW
                    || e.atom == a._NET_CURRENT_DESKTOP
                    || e.atom == a._NET_SUPPORTED
                {
                    Self::schedule(&mut self.deadlines.fullscreen, FULLSCREEN_DEBOUNCE);
                }
            }
            Event::PropertyNotify(e) if self.clients.contains(&e.window) => {
                if e.atom == a._NET_WM_STATE || e.atom == a._NET_WM_DESKTOP {
                    Self::schedule(&mut self.deadlines.fullscreen, FULLSCREEN_DEBOUNCE);
                }
            }
            Event::ConfigureNotify(e) => {
                if self.clients.contains(&e.window) {
                    Self::schedule(&mut self.deadlines.fullscreen, FULLSCREEN_DEBOUNCE);
                }
                if e.event == root && self.stacking == StackingMode::OverrideRedirect {
                    let ours = self.is_ours(e.window);
                    // Directly above nothing = at the very bottom.
                    let at_bottom = e.above_sibling == 0;
                    let above_ours = self.is_ours(e.above_sibling);
                    // Somebody was sent to the bottom, below our surfaces...
                    let intruder = !ours && at_bottom;
                    // ...or one of our surfaces is no longer at the bottom.
                    let displaced = ours && !at_bottom && !above_ours;
                    if intruder || displaced {
                        Self::schedule(&mut self.deadlines.stacking, STACKING_DEBOUNCE);
                    }
                }
            }
            Event::CirculateNotify(e)
                if e.event == root && self.stacking == StackingMode::OverrideRedirect =>
            {
                let ours = self.is_ours(e.window);
                if (!ours && e.place == Place::ON_BOTTOM) || (ours && e.place == Place::ON_TOP) {
                    Self::schedule(&mut self.deadlines.stacking, STACKING_DEBOUNCE);
                }
            }
            Event::MapNotify(e) if self.clients.contains(&e.window) => {
                Self::schedule(&mut self.deadlines.fullscreen, FULLSCREEN_DEBOUNCE);
            }
            Event::UnmapNotify(e) if self.clients.contains(&e.window) => {
                Self::schedule(&mut self.deadlines.fullscreen, FULLSCREEN_DEBOUNCE);
            }
            Event::DestroyNotify(e) if self.clients.remove(&e.window) => {
                Self::schedule(&mut self.deadlines.fullscreen, FULLSCREEN_DEBOUNCE);
            }
            // Errors from fire-and-forget requests (a client vanished) and everything else.
            _ => {}
        }
    }

    /// Re-read `_NET_CLIENT_LIST` and watch every client for state and geometry changes.
    fn track_clients(&mut self) {
        let list = self
            .conn
            .property_u32(
                self.conn.root,
                self.conn.atoms._NET_CLIENT_LIST,
                AtomEnum::WINDOW,
                4096,
            )
            .unwrap_or_default();
        let current: HashSet<Window> = list.into_iter().collect();
        for &window in current.difference(&self.clients) {
            let _ = self.conn.x.change_window_attributes(
                window,
                &ChangeWindowAttributesAux::new()
                    .event_mask(EventMask::PROPERTY_CHANGE | EventMask::STRUCTURE_NOTIFY),
            );
        }
        let _ = self.conn.x.flush();
        self.clients = current;
    }

    fn recompute_fullscreen(&mut self) {
        let mut rects = compute_fullscreen(&self.conn);
        rects.sort_by_key(|r| (r.x, r.y, r.width, r.height));
        if rects != self.last_fullscreen {
            self.last_fullscreen.clone_from(&rects);
            (self.sink)(BackendEvent::FullscreenChanged(rects));
        }
    }
}

/// Geometry of every visible fullscreen client on the current desktop.
///
/// A client counts when it is `_NET_WM_STATE_FULLSCREEN`, not `_NET_WM_STATE_HIDDEN`, viewable,
/// and on the current desktop (or on all desktops). Maximised windows never count (§15).
pub fn compute_fullscreen(conn: &crate::conn::Conn) -> Vec<Rect> {
    let a = &conn.atoms;
    let Ok(clients) = conn.property_u32(conn.root, a._NET_CLIENT_LIST, AtomEnum::WINDOW, 4096)
    else {
        return Vec::new();
    };
    let current_desktop = conn
        .property_u32(conn.root, a._NET_CURRENT_DESKTOP, AtomEnum::CARDINAL, 1)
        .ok()
        .and_then(|v| v.first().copied());

    let mut out = Vec::new();
    for window in clients {
        let Ok(state) = conn.property_u32(window, a._NET_WM_STATE, AtomEnum::ATOM, 64) else {
            continue;
        };
        if !state.contains(&a._NET_WM_STATE_FULLSCREEN) || state.contains(&a._NET_WM_STATE_HIDDEN) {
            continue;
        }
        let desktop = conn
            .property_u32(window, a._NET_WM_DESKTOP, AtomEnum::CARDINAL, 1)
            .ok()
            .and_then(|v| v.first().copied());
        let on_current = match (desktop, current_desktop) {
            (Some(d), Some(c)) => d == c || d == ALL_DESKTOPS,
            _ => true,
        };
        if !on_current {
            continue;
        }
        let viewable = conn
            .x
            .get_window_attributes(window)
            .ok()
            .and_then(|c| c.reply().ok())
            .is_some_and(|attrs| attrs.map_state == MapState::VIEWABLE);
        if !viewable {
            continue;
        }
        let Some(geometry) = conn
            .x
            .get_geometry(window)
            .ok()
            .and_then(|c| c.reply().ok())
        else {
            continue;
        };
        let origin = conn
            .x
            .translate_coordinates(window, conn.root, 0, 0)
            .ok()
            .and_then(|c| c.reply().ok());
        let (x, y) = origin.map_or((geometry.x, geometry.y), |o| (o.dst_x, o.dst_y));
        out.push(Rect::new(
            i32::from(x),
            i32::from(y),
            u32::from(geometry.width),
            u32::from(geometry.height),
        ));
    }
    out
}
