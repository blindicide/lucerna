//! The wallpaper library (directive §10, §11): references to the user's own files.
//!
//! Lucerna stores paths, never copies media, and never deletes a user's file. Removing a library
//! entry removes the entry and any assignment that points to it, nothing more.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::backend::OutputId;
use crate::config::{Config, Wallpaper, WallpaperId};
use crate::types::MediaType;

#[derive(Debug, thiserror::Error)]
pub enum LibraryError {
    #[error("The path {0} is not absolute.\nGive the full path of the file, starting with '/'.")]
    Relative(PathBuf),
    #[error(
        "The path {0} contains characters that are not valid UTF-8, which Lucerna cannot store.\nRename the file and add it again."
    )]
    NotUtf8(PathBuf),
    #[error("The file {0} does not exist.\nCheck the path, or connect the drive it is on.")]
    NotFound(PathBuf),
    #[error("{0} is not a regular file.\nChoose a video or animated image file.")]
    NotAFile(PathBuf),
    #[error("Lucerna could not read {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error(
        "No wallpaper in the library has the id '{0}'.\nRun `lucernactl wallpapers` to list them."
    )]
    Unknown(String),
}

impl Config {
    pub fn find_wallpaper(&self, id: &WallpaperId) -> Option<&Wallpaper> {
        self.wallpapers.iter().find(|w| &w.id == id)
    }

    /// Add `path` to the library and return its id.
    ///
    /// Idempotent: a path that is already in the library returns the existing id. `name` may be
    /// empty, in which case the file stem is used.
    pub fn add_wallpaper(
        &mut self,
        path: &Path,
        name: &str,
        added: &str,
    ) -> Result<WallpaperId, LibraryError> {
        if !path.is_absolute() {
            return Err(LibraryError::Relative(path.to_path_buf()));
        }
        let canonical = fs::canonicalize(path).map_err(|source| {
            if source.kind() == io::ErrorKind::NotFound {
                LibraryError::NotFound(path.to_path_buf())
            } else {
                LibraryError::Io {
                    path: path.to_path_buf(),
                    source,
                }
            }
        })?;
        if !canonical.is_file() {
            return Err(LibraryError::NotAFile(canonical));
        }
        if canonical.to_str().is_none() {
            return Err(LibraryError::NotUtf8(canonical));
        }
        if let Some(existing) = self.wallpapers.iter_mut().find(|w| w.path == canonical) {
            existing.available = true;
            return Ok(existing.id.clone());
        }
        let name = match name.trim() {
            "" => canonical.file_stem().map_or_else(
                || "Wallpaper".to_owned(),
                |s| s.to_string_lossy().into_owned(),
            ),
            given => given.to_owned(),
        };
        let id = WallpaperId::new_random();
        self.wallpapers.push(Wallpaper {
            id: id.clone(),
            name,
            media_type: MediaType::from_path(&canonical),
            path: canonical,
            added: added.to_owned(),
            available: true,
        });
        Ok(id)
    }

    /// Remove a library entry and every assignment that points to it. The media file is never touched.
    pub fn remove_wallpaper(&mut self, id: &WallpaperId) -> Result<Wallpaper, LibraryError> {
        let index = self
            .wallpapers
            .iter()
            .position(|w| &w.id == id)
            .ok_or_else(|| LibraryError::Unknown(id.to_string()))?;
        let removed = self.wallpapers.remove(index);
        if self.all_displays.wallpaper.as_ref() == Some(id) {
            self.all_displays.wallpaper = None;
        }
        for display in self.displays.values_mut() {
            if display.wallpaper.as_ref() == Some(id) {
                display.wallpaper = None;
            }
        }
        Ok(removed)
    }

    /// Update every entry's `available` flag using `exists`. Returns true if anything changed.
    pub fn refresh_availability(&mut self, exists: impl Fn(&Path) -> bool) -> bool {
        let mut changed = false;
        for wallpaper in &mut self.wallpapers {
            let now = exists(&wallpaper.path);
            if wallpaper.available != now {
                wallpaper.available = now;
                changed = true;
            }
        }
        changed
    }

    /// Ids of wallpapers that are assigned to at least one of `outputs` (or globally).
    pub fn assigned_wallpapers<'a>(
        &'a self,
        outputs: impl IntoIterator<Item = &'a OutputId>,
    ) -> Vec<&'a WallpaperId> {
        let mut ids: Vec<&WallpaperId> = outputs
            .into_iter()
            .filter_map(|o| self.effective_wallpaper(o).0)
            .collect();
        ids.sort();
        ids.dedup();
        ids
    }
}

/// True for an existing regular file (the default availability test).
pub fn file_exists(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|m| m.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::DisplayConfig;

    fn tempdir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lucerna-lib-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn add_canonicalises_names_and_is_idempotent() {
        let dir = tempdir("add");
        fs::write(dir.join("Rain Loop.webm"), b"x").unwrap();
        let mut cfg = Config::default();
        let via_dots = dir.join(".").join("Rain Loop.webm");
        let id = cfg
            .add_wallpaper(&via_dots, "", "2026-09-30T10:00:00Z")
            .unwrap();
        let w = cfg.find_wallpaper(&id).unwrap();
        assert_eq!(w.name, "Rain Loop");
        assert_eq!(
            w.path,
            fs::canonicalize(dir.join("Rain Loop.webm")).unwrap()
        );
        assert_eq!(w.media_type, MediaType::Video);
        assert!(w.available);

        let again = cfg
            .add_wallpaper(&dir.join("Rain Loop.webm"), "Other", "later")
            .unwrap();
        assert_eq!(again, id);
        assert_eq!(cfg.wallpapers.len(), 1);
    }

    #[test]
    fn add_follows_symlinks_to_one_entry() {
        let dir = tempdir("symlink");
        fs::write(dir.join("real.mp4"), b"x").unwrap();
        std::os::unix::fs::symlink(dir.join("real.mp4"), dir.join("link.mp4")).unwrap();
        let mut cfg = Config::default();
        let a = cfg.add_wallpaper(&dir.join("real.mp4"), "", "t").unwrap();
        let b = cfg.add_wallpaper(&dir.join("link.mp4"), "", "t").unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn add_rejects_relative_missing_and_non_files_with_actionable_messages() {
        let dir = tempdir("reject");
        let mut cfg = Config::default();
        assert!(matches!(
            cfg.add_wallpaper(Path::new("rain.mp4"), "", "t"),
            Err(LibraryError::Relative(_))
        ));
        assert!(matches!(
            cfg.add_wallpaper(&dir.join("nope.mp4"), "", "t"),
            Err(LibraryError::NotFound(_))
        ));
        assert!(matches!(
            cfg.add_wallpaper(&dir, "", "t"),
            Err(LibraryError::NotAFile(_))
        ));
        let msg = LibraryError::NotFound(PathBuf::from("/x.mp4")).to_string();
        assert!(msg.contains("does not exist") && msg.contains("Check the path"));
        assert!(cfg.wallpapers.is_empty());
    }

    #[test]
    fn a_shell_metacharacter_file_name_is_just_a_name() {
        let dir = tempdir("evil");
        let evil = dir.join("$(rm -rf ~).mp4");
        fs::write(&evil, b"x").unwrap();
        let mut cfg = Config::default();
        let id = cfg.add_wallpaper(&evil, "", "t").unwrap();
        assert_eq!(cfg.find_wallpaper(&id).unwrap().name, "$(rm -rf ~)");
        assert!(evil.exists());
    }

    #[test]
    fn remove_clears_assignments_and_never_touches_the_file() {
        let dir = tempdir("remove");
        let file = dir.join("keep.mp4");
        fs::write(&file, b"precious").unwrap();
        let mut cfg = Config::default();
        let id = cfg.add_wallpaper(&file, "", "t").unwrap();
        cfg.all_displays.wallpaper = Some(id.clone());
        cfg.displays.insert(
            OutputId::new("edid:X"),
            DisplayConfig {
                wallpaper: Some(id.clone()),
                ..Default::default()
            },
        );
        cfg.remove_wallpaper(&id).unwrap();
        assert!(cfg.wallpapers.is_empty());
        assert_eq!(cfg.all_displays.wallpaper, None);
        assert_eq!(cfg.displays[&OutputId::new("edid:X")].wallpaper, None);
        assert_eq!(
            fs::read(&file).unwrap(),
            b"precious",
            "the media file is never deleted"
        );
        assert!(matches!(
            cfg.remove_wallpaper(&id),
            Err(LibraryError::Unknown(_))
        ));
    }

    #[test]
    fn availability_is_tracked_and_reports_changes_only() {
        let dir = tempdir("avail");
        let file = dir.join("a.mp4");
        fs::write(&file, b"x").unwrap();
        let mut cfg = Config::default();
        cfg.add_wallpaper(&file, "", "t").unwrap();
        assert!(
            !cfg.refresh_availability(file_exists),
            "still there: no change"
        );
        fs::remove_file(&file).unwrap();
        assert!(cfg.refresh_availability(file_exists));
        assert!(!cfg.wallpapers[0].available);
        assert!(!cfg.refresh_availability(file_exists), "no repeat report");
        fs::write(&file, b"x").unwrap();
        assert!(cfg.refresh_availability(file_exists));
        assert!(cfg.wallpapers[0].available);
    }

    #[test]
    fn assigned_wallpapers_are_deduplicated() {
        let mut cfg = Config::default();
        let id = WallpaperId::parse("w1").unwrap();
        cfg.all_displays.wallpaper = Some(id.clone());
        let outputs = [OutputId::new("a"), OutputId::new("b")];
        assert_eq!(cfg.assigned_wallpapers(outputs.iter()), vec![&id]);
    }
}
