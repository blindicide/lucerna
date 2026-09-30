//! Loading and saving `config.toml` with the outcomes of docs/IMPLEMENTATION-PLAN.md §6.3.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use toml_edit::DocumentMut;

use super::doc::{apply_to_doc, parse_doc};
use super::migrate::{Migration, MigrationError, migrate_with};
use super::{CURRENT_SCHEMA, Config};
use crate::fsutil::{atomic_write, remove_stale_temp_files};

/// How trustworthy and writable the loaded configuration is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConfigState {
    Ok,
    /// The file could not be parsed. It was moved aside (never deleted) and defaults are in use.
    DefaultsAfterCorruption {
        backup: PathBuf,
    },
    /// Written by a newer Lucerna. Known fields are used; the file is never modified.
    ReadOnlyNewerSchema {
        version: i64,
    },
    /// The file exists but could not be read (permissions, I/O). Defaults are in use and nothing
    /// is written, so the unreadable file is not overwritten.
    Unreadable {
        reason: String,
    },
}

impl ConfigState {
    /// Stable code for D-Bus and `doctor`.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::DefaultsAfterCorruption { .. } => "defaults-after-corruption",
            Self::ReadOnlyNewerSchema { .. } => "read-only-newer-schema",
            Self::Unreadable { .. } => "unreadable",
        }
    }

    pub fn is_read_only(&self) -> bool {
        matches!(
            self,
            Self::ReadOnlyNewerSchema { .. } | Self::Unreadable { .. }
        )
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error(
        "The configuration was written by a newer Lucerna (schema {0}). This version will not modify it.\nUpdate Lucerna to change settings."
    )]
    ReadOnlyNewer(i64),
    #[error(
        "Lucerna could not read its configuration ({0}), so it will not overwrite it.\nFix the file permissions and choose Reload."
    )]
    ReadOnlyUnreadable(String),
    #[error(
        "Lucerna could not save its configuration to {path}: {source}\nCheck that the directory exists and is writable."
    )]
    Write {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("Lucerna could not migrate its configuration: {0}")]
    Migration(#[from] MigrationError),
}

/// A loaded configuration together with the document it came from.
#[derive(Debug)]
pub struct LoadedConfig {
    pub config: Config,
    /// The original document, kept so a save preserves unknown keys and comments.
    pub doc: DocumentMut,
    pub state: ConfigState,
    /// Problems found while reading, for `GetStatus` and `doctor`.
    pub warnings: Vec<String>,
    /// A sentence for the user about what was done to the file (backup, corruption, ...).
    pub notice: Option<String>,
    /// True when the file did not exist. The daemon enables autostart on first creation (D4).
    pub first_run: bool,
}

impl LoadedConfig {
    /// A fresh in-memory configuration with defaults.
    pub fn defaults() -> Self {
        Self {
            config: Config::default(),
            doc: DocumentMut::new(),
            state: ConfigState::Ok,
            warnings: Vec::new(),
            notice: None,
            first_run: false,
        }
    }
}

fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

/// Load `path`. `stamp` is a compact UTC timestamp used in backup file names.
///
/// This never fails: every problem becomes a state, a warning or a notice, so a broken
/// configuration can never stop the daemon from starting.
pub fn load(path: &Path, stamp: &str) -> LoadedConfig {
    load_with(path, stamp, &[])
}

/// [`load`] with an explicit migration table (the shipped table is empty in v1; tests inject one).
pub fn load_with(path: &Path, stamp: &str, migrations: &[Migration]) -> LoadedConfig {
    if let Some(dir) = path.parent() {
        // Leftovers from an interrupted atomic write.
        let _ = remove_stale_temp_files(dir);
    }

    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return LoadedConfig {
                first_run: true,
                ..LoadedConfig::defaults()
            };
        }
        Err(e) => {
            return LoadedConfig {
                state: ConfigState::Unreadable {
                    reason: e.to_string(),
                },
                notice: Some(format!(
                    "The configuration file {} could not be read ({e}); defaults are in use and the file will not be overwritten.",
                    path.display()
                )),
                ..LoadedConfig::defaults()
            };
        }
    };

    let mut doc: DocumentMut = match text.parse() {
        Ok(doc) => doc,
        Err(err) => return corrupt(path, stamp, &err.to_string()),
    };

    let mut warnings = Vec::new();
    let mut notice = None;
    let version = match doc.get("schema_version") {
        None => {
            warnings.push("schema_version is missing; assuming 1".to_owned());
            i64::from(CURRENT_SCHEMA)
        }
        Some(item) => match item.as_integer() {
            Some(v) => v,
            None => {
                warnings.push("schema_version is not an integer; assuming 1".to_owned());
                i64::from(CURRENT_SCHEMA)
            }
        },
    };

    if version > i64::from(CURRENT_SCHEMA) {
        let config = parse_doc(&doc, &mut warnings);
        return LoadedConfig {
            config,
            doc,
            state: ConfigState::ReadOnlyNewerSchema { version },
            warnings,
            notice: Some(format!(
                "The configuration was written by a newer Lucerna (schema {version}); this version reads it but will not modify it."
            )),
            first_run: false,
        };
    }

    if version < i64::from(CURRENT_SCHEMA) {
        let from = u32::try_from(version.max(0)).unwrap_or(0);
        let backup = sibling(path, &format!(".bak-v{from}-{stamp}"));
        let result = fs::copy(path, &backup)
            .map_err(|e| e.to_string())
            .and_then(|_| {
                migrate_with(&mut doc, from, CURRENT_SCHEMA, migrations).map_err(|e| e.to_string())
            });
        match result {
            Ok(()) => {
                notice = Some(format!(
                    "The configuration was upgraded from schema {from} to {CURRENT_SCHEMA}; the old file was saved as {}.",
                    backup.display()
                ));
                if let Err(e) = atomic_write(path, doc.to_string().as_bytes()) {
                    warnings.push(format!("could not write the migrated configuration: {e}"));
                }
            }
            Err(reason) => {
                return LoadedConfig {
                    state: ConfigState::Unreadable {
                        reason: format!("migration failed: {reason}"),
                    },
                    notice: Some(format!(
                        "The configuration could not be migrated ({reason}); it was left untouched."
                    )),
                    ..LoadedConfig::defaults()
                };
            }
        }
    }

    let config = parse_doc(&doc, &mut warnings);
    LoadedConfig {
        config,
        doc,
        state: ConfigState::Ok,
        warnings,
        notice,
        first_run: false,
    }
}

fn corrupt(path: &Path, stamp: &str, why: &str) -> LoadedConfig {
    let backup = sibling(path, &format!(".corrupt-{stamp}"));
    // Move the broken file aside so a later save cannot destroy the user's data; if renaming
    // fails, copy it; if that fails too, stay read-only rather than overwrite it.
    let moved = fs::rename(path, &backup).is_ok() || fs::copy(path, &backup).is_ok();
    if moved {
        LoadedConfig {
            state: ConfigState::DefaultsAfterCorruption {
                backup: backup.clone(),
            },
            notice: Some(format!(
                "The configuration file could not be parsed ({}); it was kept as {} and defaults are in use.",
                first_line(why),
                backup.display()
            )),
            ..LoadedConfig::defaults()
        }
    } else {
        LoadedConfig {
            state: ConfigState::Unreadable { reason: format!("corrupt and could not be moved aside: {}", first_line(why)) },
            notice: Some("The configuration file is corrupt and could not be moved aside, so it will not be overwritten.".to_owned()),
            ..LoadedConfig::defaults()
        }
    }
}

fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or(text)
}

/// Write `config` into `loaded.doc` (changed fields only) and save it atomically to `path`.
pub fn save(loaded: &mut LoadedConfig, path: &Path) -> Result<(), ConfigError> {
    match &loaded.state {
        ConfigState::ReadOnlyNewerSchema { version } => {
            return Err(ConfigError::ReadOnlyNewer(*version));
        }
        ConfigState::Unreadable { reason } => {
            return Err(ConfigError::ReadOnlyUnreadable(reason.clone()));
        }
        ConfigState::Ok | ConfigState::DefaultsAfterCorruption { .. } => {}
    }
    apply_to_doc(&loaded.config, &mut loaded.doc);
    loaded.doc["schema_version"] = toml_edit::value(i64::from(CURRENT_SCHEMA));
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|source| ConfigError::Write {
            path: path.to_path_buf(),
            source,
        })?;
    }
    atomic_write(path, loaded.doc.to_string().as_bytes()).map_err(|source| ConfigError::Write {
        path: path.to_path_buf(),
        source,
    })?;
    loaded.first_run = false;
    Ok(())
}

/// What a read-only look at the configuration file found (for `doctor`). Unlike [`load`], this
/// never renames, migrates or creates anything.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Inspection {
    pub exists: bool,
    pub schema_version: Option<i64>,
    pub warnings: Vec<String>,
}

pub fn inspect(path: &Path) -> Inspection {
    match fs::read_to_string(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Inspection {
            exists: false,
            schema_version: None,
            warnings: Vec::new(),
        },
        Err(e) => Inspection {
            exists: true,
            schema_version: None,
            warnings: vec![format!("cannot read the file: {e}")],
        },
        Ok(text) => match text.parse::<DocumentMut>() {
            Err(e) => Inspection {
                exists: true,
                schema_version: None,
                warnings: vec![format!("not valid TOML: {}", first_line(&e.to_string()))],
            },
            Ok(doc) => {
                let mut warnings = Vec::new();
                let _ = parse_doc(&doc, &mut warnings);
                Inspection {
                    exists: true,
                    schema_version: doc.get("schema_version").and_then(|v| v.as_integer()),
                    warnings,
                }
            }
        },
    }
}

#[cfg(test)]
mod inspect_tests {
    use super::*;

    #[test]
    fn inspection_is_read_only_and_describes_the_file() {
        let dir = std::env::temp_dir().join(format!("lucerna-inspect-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");

        assert_eq!(
            inspect(&path),
            Inspection {
                exists: false,
                schema_version: None,
                warnings: vec![]
            }
        );

        fs::write(
            &path,
            "schema_version = 1\n[general]\nfps_limit = \"144\"\n",
        )
        .unwrap();
        let found = inspect(&path);
        assert!(found.exists);
        assert_eq!(found.schema_version, Some(1));
        assert_eq!(found.warnings.len(), 1);

        fs::write(&path, "broken [").unwrap();
        let found = inspect(&path);
        assert!(found.warnings[0].starts_with("not valid TOML"));
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "broken [",
            "a broken file is left exactly as it was"
        );
        assert_eq!(
            fs::read_dir(&dir).unwrap().count(),
            1,
            "nothing was moved aside"
        );
    }
}
