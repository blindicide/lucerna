//! The D-Bus contract shared by the daemon, the CLI and the GUI (directive §19).
//!
//! * [`names`]: bus name, object path, interface and error prefix.
//! * [`dict`]: `a{sv}` dictionaries, the extensible payload type used everywhere. Clients must
//!   ignore unknown keys and must not require keys that were added later.
//! * [`dto`]: typed views of those dictionaries with round-trip conversions.
//! * [`error`]: the `org.lucerna.Lucerna1.Error.*` errors and their `lucernactl` exit codes.
//! * [`proxy`]: the async and blocking client proxy.
//!
//! This crate contains no server logic, no GTK, no X11 and no mpv.

pub mod dict;
pub mod dto;
pub mod error;
pub mod names;
pub mod proxy;

pub use dict::Dict;
pub use error::{LucernaError, exit_code_for};
pub use proxy::{LucernaProxy, LucernaProxyBlocking};
