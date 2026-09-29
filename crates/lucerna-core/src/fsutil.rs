//! File-system helpers. Everything here is std-only.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Points inside [`atomic_write`] at which a test can inject a failure.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    AfterTempWrite,
    AfterRename,
}

static NONCE: AtomicU64 = AtomicU64::new(0);

/// Replace `path` with `bytes` atomically.
///
/// A crash at any point leaves either the old file or the new file, never a zero-byte file:
/// data goes to a temporary file in the same directory (mode 0600, `O_EXCL`), is synced, and is
/// renamed over the target; the directory is then synced too. If `path` is a symlink (as with
/// dotfile managers) the link's target is replaced, not the link.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    atomic_write_hooked(path, bytes, &|_| Ok(()))
}

#[doc(hidden)]
pub fn atomic_write_hooked(
    path: &Path,
    bytes: &[u8],
    hook: &dyn Fn(Step) -> io::Result<()>,
) -> io::Result<()> {
    let target = resolve_link_target(path)?;
    let dir = target
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let name = target
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no file name"))?
        .to_string_lossy()
        .into_owned();
    let nonce = NONCE.fetch_add(1, Ordering::Relaxed);
    let tmp = dir.join(format!(".{name}.tmp-{}-{nonce}", std::process::id()));

    let result = (|| {
        let mut file: File = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        hook(Step::AfterTempWrite)?;
        fs::rename(&tmp, &target)?;
        hook(Step::AfterRename)?;
        File::open(&dir)?.sync_all()?;
        Ok(())
    })();

    if result.is_err() {
        // Best effort: the target is either untouched or already replaced.
        let _ = fs::remove_file(&tmp);
    }
    result
}

/// Follow symlinks so a write replaces the real file. A dangling or absent path is returned as is.
fn resolve_link_target(path: &Path) -> io::Result<PathBuf> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => match fs::canonicalize(path) {
            Ok(real) => Ok(real),
            Err(_) => {
                let link = fs::read_link(path)?;
                Ok(if link.is_absolute() {
                    link
                } else {
                    path.parent().unwrap_or_else(|| Path::new(".")).join(link)
                })
            }
        },
        _ => Ok(path.to_path_buf()),
    }
}

/// Remove leftover `.<name>.tmp-*` files from interrupted writes in `dir`.
pub fn remove_stale_temp_files(dir: &Path) -> io::Result<usize> {
    let mut removed = 0;
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') && name.contains(".tmp-") && fs::remove_file(entry.path()).is_ok()
        {
            removed += 1;
        }
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};

    fn tempdir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("lucerna-fsutil-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn writes_new_file_with_private_mode() {
        let dir = tempdir("new");
        let target = dir.join("config.toml");
        atomic_write(&target, b"hello").unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"hello");
        let mode = fs::metadata(&target).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        assert_eq!(
            fs::read_dir(&dir).unwrap().count(),
            1,
            "no temp file left behind"
        );
    }

    #[test]
    fn replaces_existing_file() {
        let dir = tempdir("replace");
        let target = dir.join("a");
        fs::write(&target, b"old").unwrap();
        atomic_write(&target, b"new").unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"new");
    }

    #[test]
    fn failure_after_temp_write_keeps_old_content() {
        let dir = tempdir("fail1");
        let target = dir.join("a");
        fs::write(&target, b"old").unwrap();
        let err = atomic_write_hooked(&target, b"new", &|step| {
            if step == Step::AfterTempWrite {
                Err(io::Error::other("injected"))
            } else {
                Ok(())
            }
        });
        assert!(err.is_err());
        assert_eq!(fs::read(&target).unwrap(), b"old");
        assert_eq!(
            fs::read_dir(&dir).unwrap().count(),
            1,
            "temp file cleaned up"
        );
    }

    #[test]
    fn failure_after_rename_leaves_complete_new_content() {
        let dir = tempdir("fail2");
        let target = dir.join("a");
        fs::write(&target, b"old").unwrap();
        let err = atomic_write_hooked(&target, b"new", &|step| {
            if step == Step::AfterRename {
                Err(io::Error::other("injected"))
            } else {
                Ok(())
            }
        });
        assert!(err.is_err());
        let content = fs::read(&target).unwrap();
        assert_eq!(content, b"new");
        assert!(!content.is_empty(), "never a zero-byte file");
    }

    #[test]
    fn symlink_target_is_replaced_not_the_link() {
        let dir = tempdir("link");
        let real = dir.join("real.toml");
        let link = dir.join("link.toml");
        fs::write(&real, b"old").unwrap();
        symlink(&real, &link).unwrap();
        atomic_write(&link, b"new").unwrap();
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read(&real).unwrap(), b"new");
    }

    #[test]
    fn stale_temp_files_are_removed() {
        let dir = tempdir("stale");
        fs::write(dir.join(".config.toml.tmp-1-0"), b"junk").unwrap();
        fs::write(dir.join("config.toml"), b"keep").unwrap();
        assert_eq!(remove_stale_temp_files(&dir).unwrap(), 1);
        assert!(dir.join("config.toml").exists());
    }
}
