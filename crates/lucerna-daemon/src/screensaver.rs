//! Screen-lock detection (docs/IMPLEMENTATION-PLAN.md §6.6, directive §15).
//!
//! Probed in order; the first source that answers wins:
//!
//! 1. `org.cinnamon.ScreenSaver` on the session bus: `GetActive()` and `ActiveChanged(b)`.
//! 2. `org.freedesktop.ScreenSaver` on the session bus: the same pair.
//! 3. logind's `LockedHint` property of this session on the system bus.
//!
//! If none exists, lock detection is reported as unavailable and `pause_on_lock` has no effect.
//! (A lock screen covers the wallpaper anyway, so the only cost is power.)

use futures_util::StreamExt as _;
use tokio::sync::mpsc;
use zbus::Connection;
use zbus::proxy;

use crate::messages::EngineMsg;

// Both screensaver interfaces have an `ActiveChanged` signal, which generates same-named helper
// types, so each proxy lives in its own module.
mod cinnamon {
    #[zbus::proxy(
        interface = "org.cinnamon.ScreenSaver",
        default_service = "org.cinnamon.ScreenSaver",
        default_path = "/org/cinnamon/ScreenSaver"
    )]
    pub trait CinnamonScreenSaver {
        fn get_active(&self) -> zbus::Result<bool>;
        #[zbus(signal)]
        fn active_changed(&self, active: bool) -> zbus::Result<()>;
    }
}

mod freedesktop {
    #[zbus::proxy(
        interface = "org.freedesktop.ScreenSaver",
        default_service = "org.freedesktop.ScreenSaver",
        default_path = "/org/freedesktop/ScreenSaver"
    )]
    pub trait FreedesktopScreenSaver {
        fn get_active(&self) -> zbus::Result<bool>;
        #[zbus(signal)]
        fn active_changed(&self, active: bool) -> zbus::Result<()>;
    }
}

use cinnamon::CinnamonScreenSaverProxy;
use freedesktop::FreedesktopScreenSaverProxy;

#[proxy(
    interface = "org.freedesktop.login1.Manager",
    default_service = "org.freedesktop.login1",
    default_path = "/org/freedesktop/login1"
)]
trait Login1Manager {
    fn get_session(&self, id: &str) -> zbus::Result<zbus::zvariant::OwnedObjectPath>;
}

#[proxy(
    interface = "org.freedesktop.login1.Session",
    default_service = "org.freedesktop.login1"
)]
trait Login1Session {
    #[zbus(property)]
    fn locked_hint(&self) -> zbus::Result<bool>;
}

/// Where lock state comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LockSource {
    CinnamonScreenSaver,
    FreedesktopScreenSaver,
    Logind,
}

impl LockSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CinnamonScreenSaver => "org.cinnamon.ScreenSaver",
            Self::FreedesktopScreenSaver => "org.freedesktop.ScreenSaver",
            Self::Logind => "logind LockedHint",
        }
    }
}

/// Start watching the first available source; report changes to the engine. Returns the source
/// used, or `None` if the session offers no way to know.
pub async fn start(
    session_bus: &Connection,
    system_bus: Option<Connection>,
    session_id: Option<&str>,
    tx: mpsc::UnboundedSender<EngineMsg>,
) -> Option<LockSource> {
    if watch_cinnamon(session_bus, tx.clone()).await {
        return Some(LockSource::CinnamonScreenSaver);
    }
    if watch_freedesktop(session_bus, tx.clone()).await {
        return Some(LockSource::FreedesktopScreenSaver);
    }
    if let (Some(system), Some(id)) = (system_bus, session_id)
        && watch_logind(&system, id, tx).await
    {
        return Some(LockSource::Logind);
    }
    None
}

async fn watch_cinnamon(conn: &Connection, tx: mpsc::UnboundedSender<EngineMsg>) -> bool {
    let Ok(proxy) = CinnamonScreenSaverProxy::new(conn).await else {
        return false;
    };
    // Subscribe before reading, so a change in between cannot be missed.
    let Ok(mut changes) = proxy.receive_active_changed().await else {
        return false;
    };
    let Ok(active) = proxy.get_active().await else {
        return false;
    };
    let _ = tx.send(EngineMsg::Lock(active));
    tokio::spawn(async move {
        while let Some(signal) = changes.next().await {
            if let Ok(args) = signal.args()
                && tx.send(EngineMsg::Lock(args.active)).is_err()
            {
                break;
            }
        }
    });
    true
}

async fn watch_freedesktop(conn: &Connection, tx: mpsc::UnboundedSender<EngineMsg>) -> bool {
    let Ok(proxy) = FreedesktopScreenSaverProxy::new(conn).await else {
        return false;
    };
    let Ok(mut changes) = proxy.receive_active_changed().await else {
        return false;
    };
    let Ok(active) = proxy.get_active().await else {
        return false;
    };
    let _ = tx.send(EngineMsg::Lock(active));
    tokio::spawn(async move {
        while let Some(signal) = changes.next().await {
            if let Ok(args) = signal.args()
                && tx.send(EngineMsg::Lock(args.active)).is_err()
            {
                break;
            }
        }
    });
    true
}

async fn watch_logind(
    system: &Connection,
    session_id: &str,
    tx: mpsc::UnboundedSender<EngineMsg>,
) -> bool {
    let Ok(manager) = Login1ManagerProxy::new(system).await else {
        return false;
    };
    let Ok(path) = manager.get_session(session_id).await else {
        return false;
    };
    let Ok(builder) = Login1SessionProxy::builder(system).path(path) else {
        return false;
    };
    let Ok(session) = builder.build().await else {
        return false;
    };
    let mut changes = session.receive_locked_hint_changed().await;
    let Ok(locked) = session.locked_hint().await else {
        return false;
    };
    let _ = tx.send(EngineMsg::Lock(locked));
    tokio::spawn(async move {
        while let Some(change) = changes.next().await {
            if let Ok(locked) = change.get().await
                && tx.send(EngineMsg::Lock(locked)).is_err()
            {
                break;
            }
        }
    });
    true
}
