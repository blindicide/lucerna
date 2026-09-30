//! Fake session services for lock-detection tests: screensavers on the session bus and logind on
//! a private "system" bus.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use zbus::object_server::SignalEmitter;
use zbus::zvariant::OwnedObjectPath;

/// Which lock source the fake session offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LockProvider {
    None,
    Cinnamon,
    Freedesktop,
    Logind,
}

const CINNAMON_PATH: &str = "/org/cinnamon/ScreenSaver";
const FREEDESKTOP_PATH: &str = "/org/freedesktop/ScreenSaver";
const LOGIND_SESSION_PATH: &str = "/org/freedesktop/login1/session/_342";
/// The session id the fake logind knows.
pub const LOGIND_SESSION_ID: &str = "42";

struct CinnamonScreenSaver {
    active: Arc<AtomicBool>,
}

#[zbus::interface(name = "org.cinnamon.ScreenSaver")]
impl CinnamonScreenSaver {
    fn get_active(&self) -> bool {
        self.active.load(Ordering::SeqCst)
    }

    #[zbus(signal)]
    async fn active_changed(emitter: &SignalEmitter<'_>, active: bool) -> zbus::Result<()>;
}

struct FreedesktopScreenSaver {
    active: Arc<AtomicBool>,
}

#[zbus::interface(name = "org.freedesktop.ScreenSaver")]
impl FreedesktopScreenSaver {
    fn get_active(&self) -> bool {
        self.active.load(Ordering::SeqCst)
    }

    #[zbus(signal)]
    async fn active_changed(emitter: &SignalEmitter<'_>, active: bool) -> zbus::Result<()>;
}

struct Login1Manager;

#[zbus::interface(name = "org.freedesktop.login1.Manager")]
impl Login1Manager {
    fn get_session(&self, id: &str) -> zbus::fdo::Result<OwnedObjectPath> {
        if id == LOGIND_SESSION_ID {
            OwnedObjectPath::try_from(LOGIND_SESSION_PATH)
                .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
        } else {
            Err(zbus::fdo::Error::Failed(format!("no session '{id}'")))
        }
    }
}

struct Login1Session {
    locked: bool,
}

#[zbus::interface(name = "org.freedesktop.login1.Session")]
impl Login1Session {
    #[zbus(property)]
    fn locked_hint(&self) -> bool {
        self.locked
    }
}

/// A running fake service. Dropping it disconnects it from the bus.
pub struct LockProviderHandle {
    kind: LockProvider,
    conn: zbus::Connection,
    active: Arc<AtomicBool>,
}

impl LockProviderHandle {
    /// Serve `kind` on the bus at `address`, initially in the `initially_locked` state.
    pub async fn start(kind: LockProvider, address: &str, initially_locked: bool) -> Option<Self> {
        let active = Arc::new(AtomicBool::new(initially_locked));
        let builder = zbus::connection::Builder::address(address).ok()?;
        let builder = match kind {
            LockProvider::None => return None,
            LockProvider::Cinnamon => builder
                .name("org.cinnamon.ScreenSaver")
                .ok()?
                .serve_at(
                    CINNAMON_PATH,
                    CinnamonScreenSaver {
                        active: Arc::clone(&active),
                    },
                )
                .ok()?,
            LockProvider::Freedesktop => builder
                .name("org.freedesktop.ScreenSaver")
                .ok()?
                .serve_at(
                    FREEDESKTOP_PATH,
                    FreedesktopScreenSaver {
                        active: Arc::clone(&active),
                    },
                )
                .ok()?,
            LockProvider::Logind => builder
                .name("org.freedesktop.login1")
                .ok()?
                .serve_at("/org/freedesktop/login1", Login1Manager)
                .ok()?
                .serve_at(
                    LOGIND_SESSION_PATH,
                    Login1Session {
                        locked: initially_locked,
                    },
                )
                .ok()?,
        };
        let conn = builder.build().await.ok()?;
        Some(Self { kind, conn, active })
    }

    /// Lock or unlock the fake screen and announce it the way the real service would.
    pub async fn set_locked(&self, locked: bool) {
        self.active.store(locked, Ordering::SeqCst);
        match self.kind {
            LockProvider::Cinnamon => {
                if let Ok(emitter) = SignalEmitter::new(&self.conn, CINNAMON_PATH) {
                    let _ = CinnamonScreenSaver::active_changed(&emitter, locked).await;
                }
            }
            LockProvider::Freedesktop => {
                if let Ok(emitter) = SignalEmitter::new(&self.conn, FREEDESKTOP_PATH) {
                    let _ = FreedesktopScreenSaver::active_changed(&emitter, locked).await;
                }
            }
            LockProvider::Logind => {
                if let Ok(iface) = self
                    .conn
                    .object_server()
                    .interface::<_, Login1Session>(LOGIND_SESSION_PATH)
                    .await
                {
                    iface.get_mut().await.locked = locked;
                    let _ = iface
                        .get()
                        .await
                        .locked_hint_changed(iface.signal_emitter())
                        .await;
                }
            }
            LockProvider::None => {}
        }
    }
}
