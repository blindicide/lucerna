//! Single-instance enforcement, part one: a `flock` on the runtime directory (directive §5).
//!
//! The lock lives in the per-user runtime directory, so one daemon can never touch another
//! user's session, and stale-process recovery can never run against a live daemon's children.
//! The D-Bus name is the second, independent guard.

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use rustix::fs::{FlockOperation, flock};

/// Holds the lock for as long as it lives.
pub struct DaemonLock {
    _file: File,
}

#[derive(Debug, thiserror::Error)]
pub enum LockError {
    /// Another daemon holds the lock; `pid` is what it wrote into the file, if readable.
    #[error("another daemon holds the lock")]
    AlreadyRunning { pid: Option<u32> },
    #[error("could not use the lock file {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

impl DaemonLock {
    /// Take the lock, or report the pid of the daemon that has it.
    pub fn acquire(path: &Path) -> Result<Self, LockError> {
        let io_err = |source| LockError::Io {
            path: path.to_path_buf(),
            source,
        };
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(path)
            .map_err(io_err)?;
        match flock(&file, FlockOperation::NonBlockingLockExclusive) {
            Ok(()) => {
                file.set_len(0).map_err(io_err)?;
                file.seek(SeekFrom::Start(0)).map_err(io_err)?;
                writeln!(file, "{}", std::process::id()).map_err(io_err)?;
                file.flush().map_err(io_err)?;
                Ok(Self { _file: file })
            }
            Err(rustix::io::Errno::WOULDBLOCK) => {
                let mut text = String::new();
                let _ = file.read_to_string(&mut text);
                Err(LockError::AlreadyRunning {
                    pid: text.trim().parse().ok(),
                })
            }
            Err(e) => Err(io_err(io::Error::from(e))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lock_path(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lucerna-lock-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("daemon.lock")
    }

    #[test]
    fn second_holder_is_refused_with_the_first_pid() {
        let path = lock_path("dup");
        let first = DaemonLock::acquire(&path).unwrap();
        match DaemonLock::acquire(&path) {
            Err(LockError::AlreadyRunning { pid }) => assert_eq!(pid, Some(std::process::id())),
            other => panic!("expected AlreadyRunning, got {:?}", other.err()),
        }
        drop(first);
        // Released on drop (or process death): the next daemon can start.
        assert!(DaemonLock::acquire(&path).is_ok());
    }

    #[test]
    fn lock_file_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let path = lock_path("mode");
        let _lock = DaemonLock::acquire(&path).unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn unusable_location_is_an_io_error() {
        let err = DaemonLock::acquire(Path::new("/nonexistent-dir-for-lucerna/daemon.lock"))
            .err()
            .unwrap();
        assert!(matches!(err, LockError::Io { .. }));
    }
}
