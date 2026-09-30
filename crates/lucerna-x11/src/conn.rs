//! The shared X connection and small property helpers.

use std::sync::Arc;

use lucerna_core::backend::BackendError;
use x11rb::connection::{Connection, RequestConnection};
use x11rb::errors::{ConnectionError, ReplyError, ReplyOrIdError};
use x11rb::protocol::randr::ConnectionExt as _;
use x11rb::protocol::shape::ConnectionExt as _;
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _, GetPropertyReply, Window};
use x11rb::rust_connection::RustConnection;

use crate::atoms::Atoms;

/// One X connection plus what was learned about the server when connecting.
pub struct Conn {
    pub x: RustConnection,
    pub screen_num: usize,
    pub root: Window,
    pub atoms: Atoms,
    /// The display string used, for messages (`:0`, or the `DISPLAY` value, or `(default)`).
    pub display: String,
    /// `(major, minor)` of the RandR extension, if present.
    pub randr: Option<(u32, u32)>,
    pub shape: bool,
}

pub type SharedConn = Arc<Conn>;

pub fn protocol_error(err: impl std::fmt::Display) -> BackendError {
    BackendError::Protocol(err.to_string())
}

/// Map any x11rb error to a [`BackendError`], recognising a dead connection.
pub trait IntoBackendError {
    fn into_backend(self) -> BackendError;
}

/// Shorthand usable in `map_err` for any x11rb error type.
pub fn be<E: IntoBackendError>(err: E) -> BackendError {
    err.into_backend()
}

impl IntoBackendError for ConnectionError {
    fn into_backend(self) -> BackendError {
        match self {
            ConnectionError::IoError(_) | ConnectionError::UnknownError => {
                BackendError::ConnectionLost
            }
            other => protocol_error(other),
        }
    }
}

impl IntoBackendError for ReplyError {
    fn into_backend(self) -> BackendError {
        match self {
            ReplyError::ConnectionError(e) => e.into_backend(),
            ReplyError::X11Error(e) => protocol_error(format!("{:?}", e.error_kind)),
        }
    }
}

impl IntoBackendError for ReplyOrIdError {
    fn into_backend(self) -> BackendError {
        match self {
            ReplyOrIdError::ConnectionError(e) => e.into_backend(),
            ReplyOrIdError::X11Error(e) => protocol_error(format!("{:?}", e.error_kind)),
            ReplyOrIdError::IdsExhausted => protocol_error("X resource ids exhausted"),
        }
    }
}

impl Conn {
    /// Connect to `display` (`None` means `$DISPLAY`), intern atoms and query extensions.
    pub fn connect(display: Option<&str>) -> Result<SharedConn, BackendError> {
        let shown = display
            .map(str::to_owned)
            .or_else(|| std::env::var("DISPLAY").ok())
            .unwrap_or_else(|| "(unset)".to_owned());
        let connect_err = |reason: String| BackendError::Connect {
            display: shown.clone(),
            reason,
        };

        let (x, screen_num) =
            RustConnection::connect(display).map_err(|e| connect_err(e.to_string()))?;
        let root = x
            .setup()
            .roots
            .get(screen_num)
            .map(|s| s.root)
            .ok_or_else(|| connect_err("the server reported no screen".to_owned()))?;
        let atoms = Atoms::new(&x)
            .map_err(|e| connect_err(e.to_string()))?
            .reply()
            .map_err(|e| connect_err(e.to_string()))?;

        let randr = match x.extension_information(x11rb::protocol::randr::X11_EXTENSION_NAME) {
            Ok(Some(_)) => x
                .randr_query_version(1, 5)
                .ok()
                .and_then(|c| c.reply().ok())
                .map(|r| (r.major_version, r.minor_version)),
            _ => None,
        };
        let shape = matches!(
            x.extension_information(x11rb::protocol::shape::X11_EXTENSION_NAME),
            Ok(Some(_))
        ) && x
            .shape_query_version()
            .ok()
            .and_then(|c| c.reply().ok())
            .is_some();

        Ok(Arc::new(Self {
            x,
            screen_num,
            root,
            atoms,
            display: shown,
            randr,
            shape,
        }))
    }

    /// Read a 32-bit-format property as a list of values. Missing property gives an empty list.
    pub fn property_u32(
        &self,
        window: Window,
        property: u32,
        type_: impl Into<u32>,
        max_items: u32,
    ) -> Result<Vec<u32>, BackendError> {
        let reply = self
            .x
            .get_property(false, window, property, type_, 0, max_items)
            .map_err(IntoBackendError::into_backend)?
            .reply();
        match reply {
            Ok(r) => Ok(r.value32().map(Iterator::collect).unwrap_or_default()),
            // BadWindow and friends: the window is gone; treat as no data.
            Err(ReplyError::X11Error(_)) => Ok(Vec::new()),
            Err(e) => Err(e.into_backend()),
        }
    }

    pub fn property_raw(
        &self,
        window: Window,
        property: u32,
        type_: impl Into<u32>,
        max_words: u32,
    ) -> Result<Option<GetPropertyReply>, BackendError> {
        match self
            .x
            .get_property(false, window, property, type_, 0, max_words)
            .map_err(IntoBackendError::into_backend)?
            .reply()
        {
            Ok(r) if r.type_ != u32::from(AtomEnum::NONE) => Ok(Some(r)),
            Ok(_) => Ok(None),
            Err(ReplyError::X11Error(_)) => Ok(None),
            Err(e) => Err(e.into_backend()),
        }
    }

    /// A text property (`UTF8_STRING` or `STRING`) as a lossy string.
    pub fn property_text(
        &self,
        window: Window,
        property: u32,
    ) -> Result<Option<String>, BackendError> {
        Ok(self
            .property_raw(window, property, AtomEnum::ANY, 1024)?
            .map(|r| {
                String::from_utf8_lossy(&r.value)
                    .trim_end_matches('\0')
                    .to_owned()
            }))
    }

    /// Flush and wait until the server has processed every request sent so far, so callers can
    /// promise that a change is in effect when they return.
    pub fn sync(&self) -> Result<(), BackendError> {
        self.x.get_input_focus().map_err(be)?.reply().map_err(be)?;
        Ok(())
    }

    pub fn atom_name(&self, atom: u32) -> String {
        self.x
            .get_atom_name(atom)
            .ok()
            .and_then(|c| c.reply().ok())
            .map(|r| String::from_utf8_lossy(&r.name).into_owned())
            .unwrap_or_else(|| format!("atom-{atom}"))
    }
}
