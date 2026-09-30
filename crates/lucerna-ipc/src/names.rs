//! Well-known names.

/// Session-bus name owned by the daemon.
pub const BUS_NAME: &str = "org.lucerna.Lucerna1";
/// Object path of the service.
pub const OBJECT_PATH: &str = "/org/lucerna/Lucerna1";
/// Interface name.
pub const INTERFACE: &str = "org.lucerna.Lucerna1";
/// Prefix of every error name.
pub const ERROR_PREFIX: &str = "org.lucerna.Lucerna1.Error";
/// Value of the `ApiVersion` property. Additive changes only within `Lucerna1`.
pub const API_VERSION: u32 = 1;
/// `display_id` meaning "all displays" in `SetWallpaper`, `ClearAssignment` and `SetScaling`.
pub const ALL_DISPLAYS: &str = "*";
