//! Everything about mpv that needs no I/O beyond running `mpv --version`.

pub mod args;
pub mod compat;
pub mod discovery;

pub use args::{EMITTED_OPTIONS, RenderSpec, build_args, option_name};
pub use compat::{CompatReport, check_compat, probe_args};
pub use discovery::{DiscoveryError, MpvInfo, discover, discover_from_env};
