//! XDG path resolution (directive §18).
//!
//! `Paths` is always injected into components so tests can use temporary
//! directories without touching the process environment.

use std::path::{Path, PathBuf};

const APP_DIR: &str = "lucerna";

/// Where Lucerna keeps its files.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Paths {
    /// `$XDG_CONFIG_HOME/lucerna`
    pub config_dir: PathBuf,
    /// `$XDG_STATE_HOME/lucerna`
    pub state_dir: PathBuf,
    /// `$XDG_CACHE_HOME/lucerna`
    pub cache_dir: PathBuf,
    /// `$XDG_CONFIG_HOME/autostart` (shared with other applications).
    pub autostart_dir: PathBuf,
    /// `$XDG_RUNTIME_DIR/lucerna`, or `None` when no safe runtime directory exists.
    pub runtime_dir: Option<PathBuf>,
}

impl Paths {
    /// Resolve from the environment using the `dirs` crate.
    pub fn from_env() -> Self {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
        Self {
            config_dir: dirs::config_dir()
                .unwrap_or_else(|| home.join(".config"))
                .join(APP_DIR),
            state_dir: dirs::state_dir()
                .unwrap_or_else(|| home.join(".local").join("state"))
                .join(APP_DIR),
            cache_dir: dirs::cache_dir()
                .unwrap_or_else(|| home.join(".cache"))
                .join(APP_DIR),
            autostart_dir: dirs::config_dir()
                .unwrap_or_else(|| home.join(".config"))
                .join("autostart"),
            runtime_dir: crate::runtime::current_uid().ok().and_then(|uid| {
                crate::runtime::resolve_base(std::env::var_os("XDG_RUNTIME_DIR"), uid)
                    .map(|base| base.join(APP_DIR))
            }),
        }
    }

    /// Everything below `base`, for tests.
    pub fn under(base: &Path) -> Self {
        Self {
            config_dir: base.join("config").join(APP_DIR),
            state_dir: base.join("state").join(APP_DIR),
            cache_dir: base.join("cache").join(APP_DIR),
            autostart_dir: base.join("config").join("autostart"),
            runtime_dir: Some(base.join("runtime").join(APP_DIR)),
        }
    }

    /// `config.toml`
    pub fn config_file(&self) -> PathBuf {
        self.config_dir.join("config.toml")
    }

    /// `state.json`
    pub fn state_file(&self) -> PathBuf {
        self.state_dir.join("state.json")
    }

    /// The autostart entry.
    pub fn autostart_file(&self) -> PathBuf {
        self.autostart_dir.join(crate::autostart::FILE_NAME)
    }

    /// `renderers.json` in the runtime directory, if there is one.
    pub fn registry_file(&self) -> Option<PathBuf> {
        self.runtime_dir.as_ref().map(|d| d.join("renderers.json"))
    }

    /// `daemon.lock` in the runtime directory, if there is one.
    pub fn lock_file(&self) -> Option<PathBuf> {
        self.runtime_dir.as_ref().map(|d| d.join("daemon.lock"))
    }

    /// `logs/`
    pub fn log_dir(&self) -> PathBuf {
        self.state_dir.join("logs")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn under_places_everything_in_base() {
        let p = Paths::under(Path::new("/tmp/x"));
        assert_eq!(
            p.config_file(),
            Path::new("/tmp/x/config/lucerna/config.toml")
        );
        assert_eq!(p.log_dir(), Path::new("/tmp/x/state/lucerna/logs"));
        assert_eq!(
            p.runtime_dir.as_deref(),
            Some(Path::new("/tmp/x/runtime/lucerna"))
        );
    }

    #[test]
    fn from_env_ends_in_app_dir() {
        let p = Paths::from_env();
        assert!(p.config_dir.ends_with("lucerna"));
        assert!(p.state_dir.ends_with("lucerna"));
        assert!(p.cache_dir.ends_with("lucerna"));
    }
}
