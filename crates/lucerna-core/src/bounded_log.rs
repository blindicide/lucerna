//! A size-bounded rotating log file (directive §9: "without endlessly growing files").
//!
//! One file plus one rotated `.1` sibling, so the on-disk footprint never exceeds
//! roughly `2 × max_bytes`.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

pub struct BoundedLog {
    path: PathBuf,
    max_bytes: u64,
    file: File,
    size: u64,
}

impl BoundedLog {
    /// Open (creating parent directories with mode 0700 and the file with mode 0600) for append.
    pub fn open(path: impl Into<PathBuf>, max_bytes: u64) -> io::Result<Self> {
        let path = path.into();
        if let Some(parent) = path.parent() {
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(parent)?;
        }
        let file = Self::open_file(&path)?;
        let size = file.metadata()?.len();
        Ok(Self {
            path,
            max_bytes: max_bytes.max(1),
            file,
            size,
        })
    }

    fn open_file(path: &Path) -> io::Result<File> {
        OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(path)
    }

    /// Path of the rotated sibling, `<path>.1`.
    pub fn rotated_path(&self) -> PathBuf {
        rotated(&self.path)
    }

    /// Append one line (a newline is added). A single line longer than the whole budget is
    /// truncated so the bound always holds.
    pub fn write_line(&mut self, line: &str) -> io::Result<()> {
        let max_line = usize::try_from(self.max_bytes.saturating_sub(1)).unwrap_or(usize::MAX);
        let mut text = line.as_bytes();
        if text.len() > max_line {
            text = &text[..max_line];
        }
        let needed = text.len() as u64 + 1;
        if self.size > 0 && self.size + needed > self.max_bytes {
            self.rotate()?;
        }
        self.file.write_all(text)?;
        self.file.write_all(b"\n")?;
        self.size += needed;
        Ok(())
    }

    fn rotate(&mut self) -> io::Result<()> {
        fs::rename(&self.path, self.rotated_path())?;
        self.file = Self::open_file(&self.path)?;
        self.size = 0;
        Ok(())
    }
}

fn rotated(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".1");
    PathBuf::from(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lucerna-blog-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn three_mib_through_512k_files_stays_within_one_mib_total() {
        let dir = tempdir("bound");
        let path = dir.join("logs").join("renderer.log");
        let mut log = BoundedLog::open(&path, 512 * 1024).unwrap();
        let line = "x".repeat(1023); // 1 KiB with the newline
        for _ in 0..3 * 1024 {
            log.write_line(&line).unwrap();
        }
        let main = fs::metadata(&path).unwrap().len();
        let old = fs::metadata(log.rotated_path()).unwrap().len();
        assert!(main <= 512 * 1024 && old <= 512 * 1024, "{main} {old}");
        assert!(main + old <= 1024 * 1024);
        assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 2);
    }

    #[test]
    fn oversized_single_line_is_truncated() {
        let dir = tempdir("oversize");
        let path = dir.join("a.log");
        let mut log = BoundedLog::open(&path, 100).unwrap();
        log.write_line(&"y".repeat(10_000)).unwrap();
        assert!(fs::metadata(&path).unwrap().len() <= 100);
    }

    #[test]
    fn reopening_continues_with_existing_size() {
        let dir = tempdir("reopen");
        let path = dir.join("a.log");
        {
            let mut log = BoundedLog::open(&path, 64).unwrap();
            log.write_line(&"a".repeat(40)).unwrap();
        }
        let mut log = BoundedLog::open(&path, 64).unwrap();
        log.write_line(&"b".repeat(40)).unwrap(); // would exceed 64, so it rotates
        assert!(log.rotated_path().exists());
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            format!("{}\n", "b".repeat(40))
        );
    }

    #[test]
    fn files_are_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempdir("mode");
        let path = dir.join("a.log");
        let _log = BoundedLog::open(&path, 64).unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }
}
