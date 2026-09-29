//! The per-user runtime directory (directive §26) and mpv IPC socket paths.
//!
//! Lucerna never falls back to `/tmp`. If no private runtime directory exists, renderers are
//! not started and the reason is reported.

use std::ffi::OsString;
use std::fs;
use std::io;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

use crate::backend::OutputId;

/// Longest usable `sun_path` (108 bytes including the trailing NUL).
pub const SOCKET_PATH_MAX: usize = 107;

const APP_DIR: &str = "lucerna";

#[derive(Debug, thiserror::Error)]
pub enum RuntimeDirError {
    #[error(
        "Lucerna could not find a private runtime directory.\n\
         XDG_RUNTIME_DIR is not set and /run/user/<uid> is not usable.\n\
         Log in through a normal desktop session (systemd-logind provides this directory)."
    )]
    Unavailable,
    #[error("The runtime directory {0} does not exist. Log in through a normal desktop session.")]
    MissingParent(PathBuf),
    #[error("{0} exists but is not a directory. Remove or rename it and restart Lucerna.")]
    NotDirectory(PathBuf),
    #[error(
        "{path} is owned by uid {owner} but Lucerna runs as uid {uid}.\n\
         Refusing to use a runtime directory another user controls."
    )]
    WrongOwner { path: PathBuf, owner: u32, uid: u32 },
    #[error(
        "{path} has permissions {mode:o}; Lucerna requires 700.\n\
         Fix them with: chmod 700 {path}"
    )]
    WrongMode { path: PathBuf, mode: u32 },
    #[error("could not prepare the runtime directory {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

/// The uid of the current process, read without libc from `/proc/self`.
pub fn current_uid() -> io::Result<u32> {
    Ok(fs::metadata("/proc/self")?.uid())
}

/// Decide which directory is the per-user runtime base (normally `$XDG_RUNTIME_DIR`).
///
/// `xdg_runtime_dir` is the value of the environment variable. Without it, `/run/user/<uid>` is
/// used only if it exists, belongs to us and is mode 0700.
pub fn resolve_base(xdg_runtime_dir: Option<OsString>, uid: u32) -> Option<PathBuf> {
    if let Some(value) = xdg_runtime_dir {
        let path = PathBuf::from(value);
        if path.is_absolute() {
            return Some(path);
        }
    }
    let fallback = PathBuf::from(format!("/run/user/{uid}"));
    verify_private_dir(&fallback, uid).ok().map(|()| fallback)
}

/// Check that `path` is a directory owned by `uid` with mode exactly 0700.
pub fn verify_private_dir(path: &Path, uid: u32) -> Result<(), RuntimeDirError> {
    let meta = fs::symlink_metadata(path).map_err(|source| RuntimeDirError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    if !meta.is_dir() {
        return Err(RuntimeDirError::NotDirectory(path.to_path_buf()));
    }
    if meta.uid() != uid {
        return Err(RuntimeDirError::WrongOwner {
            path: path.to_path_buf(),
            owner: meta.uid(),
            uid,
        });
    }
    let mode = meta.permissions().mode() & 0o777;
    if mode != 0o700 {
        return Err(RuntimeDirError::WrongMode {
            path: path.to_path_buf(),
            mode,
        });
    }
    Ok(())
}

/// Create (if needed) and verify `<base>/lucerna` with mode 0700, owned by us.
pub fn ensure_app_dir(base: &Path) -> Result<PathBuf, RuntimeDirError> {
    let uid = current_uid().map_err(|source| RuntimeDirError::Io {
        path: PathBuf::from("/proc/self"),
        source,
    })?;
    if !base.is_dir() {
        return Err(RuntimeDirError::MissingParent(base.to_path_buf()));
    }
    let dir = base.join(APP_DIR);
    match fs::DirBuilder::new().mode(0o700).create(&dir) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
        Err(source) => return Err(RuntimeDirError::Io { path: dir, source }),
    }
    verify_private_dir(&dir, uid)?;
    Ok(dir)
}

#[derive(Debug, thiserror::Error)]
#[error(
    "The renderer socket path {path} is {len} bytes long; the limit is {SOCKET_PATH_MAX}.\n\
     Use a shorter XDG_RUNTIME_DIR."
)]
pub struct SocketPathError {
    pub path: PathBuf,
    pub len: usize,
}

/// `<dir>/mpv-<slot>-<generation>.sock`.
///
/// The generation is part of the name so a new process can never collide with a dying one.
pub fn socket_path(
    dir: &Path,
    output: &OutputId,
    generation: u64,
) -> Result<PathBuf, SocketPathError> {
    let path = dir.join(format!("mpv-{}-{generation}.sock", output.slot()));
    let len = path.as_os_str().len();
    if len > SOCKET_PATH_MAX {
        Err(SocketPathError { path, len })
    } else {
        Ok(path)
    }
}

/// True when `name` looks like one of our mpv sockets.
pub fn is_mpv_socket_name(name: &str) -> bool {
    name.starts_with("mpv-") && name.ends_with(".sock")
}

/// Remove every `mpv-*.sock` in `dir`; returns how many were removed.
pub fn remove_stale_sockets(dir: &Path) -> io::Result<usize> {
    let mut removed = 0;
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if is_mpv_socket_name(&entry.file_name().to_string_lossy())
            && fs::remove_file(entry.path()).is_ok()
        {
            removed += 1;
        }
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lucerna-rt-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn creates_private_app_dir() {
        let base = tempdir("create");
        let dir = ensure_app_dir(&base).unwrap();
        assert_eq!(dir, base.join("lucerna"));
        let mode = fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
        // Idempotent.
        assert_eq!(ensure_app_dir(&base).unwrap(), dir);
    }

    #[test]
    fn refuses_a_dir_with_loose_permissions() {
        let base = tempdir("loose");
        let dir = base.join("lucerna");
        fs::create_dir(&dir).unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(matches!(
            ensure_app_dir(&base),
            Err(RuntimeDirError::WrongMode { mode: 0o755, .. })
        ));
    }

    #[test]
    fn refuses_a_file_in_place_of_the_dir() {
        let base = tempdir("file");
        fs::write(base.join("lucerna"), b"x").unwrap();
        assert!(matches!(
            ensure_app_dir(&base),
            Err(RuntimeDirError::NotDirectory(_))
        ));
    }

    #[test]
    fn missing_base_is_reported() {
        let base = tempdir("missing").join("nope");
        assert!(matches!(
            ensure_app_dir(&base),
            Err(RuntimeDirError::MissingParent(_))
        ));
    }

    #[test]
    fn explicit_absolute_env_wins_and_relative_is_ignored() {
        assert_eq!(
            resolve_base(Some("/run/user/1234".into()), 1234),
            Some(PathBuf::from("/run/user/1234"))
        );
        // A relative value is not trusted; without a usable /run/user/<uid> there is no base.
        assert_eq!(resolve_base(Some("relative".into()), u32::MAX), None);
    }

    #[test]
    fn socket_path_is_short_and_deterministic() {
        let id = OutputId::new("edid:DEL-a0b1-7XJ2K3");
        let dir = Path::new("/run/user/1000/lucerna");
        let a = socket_path(dir, &id, 7).unwrap();
        assert_eq!(a, socket_path(dir, &id, 7).unwrap());
        assert_ne!(a, socket_path(dir, &id, 8).unwrap());
        assert!(is_mpv_socket_name(
            &a.file_name().unwrap().to_string_lossy()
        ));
    }

    #[test]
    fn socket_path_length_is_bounded_for_a_64_byte_runtime_dir() {
        let dir = PathBuf::from(format!("/{}", "d".repeat(63)));
        assert_eq!(dir.as_os_str().len(), 64);
        let id = OutputId::new("edid:DEL-a0b1-7XJ2K3");
        let path = socket_path(&dir, &id, u64::MAX).unwrap();
        assert!(path.as_os_str().len() < 108, "{}", path.display());
    }

    #[test]
    fn overlong_socket_path_is_an_error() {
        let dir = PathBuf::from(format!("/{}", "d".repeat(100)));
        let err = socket_path(&dir, &OutputId::new("x"), 1).unwrap_err();
        assert!(err.len > SOCKET_PATH_MAX);
    }

    #[test]
    fn stale_sockets_are_removed_but_other_files_stay() {
        let dir = tempdir("stale");
        fs::write(dir.join("mpv-aaaaaaaa-1.sock"), b"").unwrap();
        fs::write(dir.join("renderers.json"), b"[]").unwrap();
        assert_eq!(remove_stale_sockets(&dir).unwrap(), 1);
        assert!(dir.join("renderers.json").exists());
    }
}
