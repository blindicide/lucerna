//! The About page.

use lucerna_core::version::{DESCRIPTION, REPOSITORY, VERSION};
use lucerna_ipc::dto::StatusDto;

use crate::strings;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AboutInfo {
    pub name: &'static str,
    pub version: String,
    pub description: &'static str,
    pub license: &'static str,
    pub repository: &'static str,
    /// Where the wallpaper is drawn: `cinnamon-x11`, `x11-ewmh`, "none", or unknown.
    pub backend: String,
}

pub fn info(status: Option<&StatusDto>) -> AboutInfo {
    AboutInfo {
        name: strings::APP_NAME,
        version: VERSION.to_owned(),
        description: DESCRIPTION,
        license: "MIT",
        repository: REPOSITORY,
        backend: match status {
            Some(s) if s.backend == "none" => strings::BACKEND_NONE.to_owned(),
            Some(s) if !s.backend.is_empty() => s.backend.clone(),
            _ => strings::BACKEND_UNKNOWN.to_owned(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_version_comes_from_the_workspace() {
        assert_eq!(info(None).version, env!("CARGO_PKG_VERSION"));
        assert!(info(None).repository.starts_with("https://"));
        assert_eq!(info(None).license, "MIT");
    }

    #[test]
    fn the_backend_reflects_the_daemon_status() {
        let s = StatusDto {
            backend: "cinnamon-x11".into(),
            ..StatusDto::default()
        };
        assert_eq!(info(Some(&s)).backend, "cinnamon-x11");
        let none = StatusDto {
            backend: "none".into(),
            ..StatusDto::default()
        };
        assert_eq!(info(Some(&none)).backend, strings::BACKEND_NONE);
        assert_eq!(info(None).backend, strings::BACKEND_UNKNOWN);
    }
}
