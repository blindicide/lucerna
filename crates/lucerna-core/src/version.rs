//! The canonical Lucerna version.
//!
//! Every crate inherits `version.workspace = true`, so this is the root
//! `Cargo.toml` `[workspace.package] version` and nothing else.

/// The workspace version, for example `0.0.1` or `1.0.0-rc.1`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Project description shown in `--help` output and the About page.
pub const DESCRIPTION: &str = "Animated wallpapers for Linux";

/// Canonical application ID used for the desktop entry, icon and GTK application.
pub const APP_ID: &str = "org.lucerna.Lucerna";

/// Source repository URL.
pub const REPOSITORY: &str = env!("CARGO_PKG_REPOSITORY");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_semver_shaped() {
        let core = VERSION.split(['-', '+']).next().unwrap_or_default();
        let parts: Vec<&str> = core.split('.').collect();
        assert_eq!(parts.len(), 3, "not MAJOR.MINOR.PATCH: {VERSION}");
        assert!(parts.iter().all(|p| p.parse::<u64>().is_ok()), "{VERSION}");
    }

    #[test]
    fn repository_is_set() {
        assert!(REPOSITORY.starts_with("https://"));
    }
}
