//! Finding mpv and reading its version (directive §9, §49).

use std::ffi::OsStr;
use std::io::{self, Read};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Environment variable that overrides the mpv lookup (a debugging aid, reported by `doctor`).
pub const OVERRIDE_ENV: &str = "LUCERNA_MPV";

/// A usable mpv installation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MpvInfo {
    pub path: PathBuf,
    /// The first line of `mpv --version`, for example `mpv 0.37.0 Copyright ...`.
    pub version_line: String,
    /// The version number, for example `0.37.0`.
    pub version: String,
}

#[derive(Debug, thiserror::Error)]
pub enum DiscoveryError {
    #[error(
        "Lucerna could not start mpv.\n\
         Executable \"mpv\" was not found in PATH.\n\
         Install mpv (for example: sudo apt install mpv) and choose Reload."
    )]
    NotFound,
    #[error(
        "Lucerna could not start mpv.\n\
         {path} (from {OVERRIDE_ENV}) is not an executable file.\n\
         Fix or unset {OVERRIDE_ENV}."
    )]
    OverrideInvalid { path: PathBuf },
    #[error(
        "Lucerna could not run {path} --version: {source}\nCheck that mpv is installed correctly."
    )]
    Probe {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("{path} did not answer --version like mpv did (got: {output:?}).")]
    NotMpv { path: PathBuf, output: String },
}

fn is_executable_file(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

/// First executable regular file called `mpv` in a `PATH`-style list.
pub fn find_in_path(path_var: Option<&OsStr>) -> Option<PathBuf> {
    let path_var = path_var?;
    std::env::split_paths(path_var)
        .filter(|dir| dir.is_absolute())
        .map(|dir| dir.join("mpv"))
        .find(|candidate| is_executable_file(candidate))
}

/// Extract `(version_number)` from a line such as `mpv 0.37.0 Copyright ...` or `mpv v0.40.0-dirty`.
pub fn parse_version(line: &str) -> Option<String> {
    let mut words = line.split_whitespace();
    if words.next()? != "mpv" {
        return None;
    }
    let token = words.next()?.trim_start_matches('v');
    let number: String = token
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    let number = number.trim_end_matches('.').to_owned();
    (number.chars().next().is_some_and(|c| c.is_ascii_digit())).then_some(number)
}

/// Locate mpv (override first, then `PATH`) and read its version.
pub fn discover(
    override_path: Option<&OsStr>,
    path_var: Option<&OsStr>,
) -> Result<MpvInfo, DiscoveryError> {
    let path = match override_path.filter(|v| !v.is_empty()) {
        Some(value) => {
            let path = PathBuf::from(value);
            if !path.is_absolute() || !is_executable_file(&path) {
                return Err(DiscoveryError::OverrideInvalid { path });
            }
            path
        }
        None => find_in_path(path_var).ok_or(DiscoveryError::NotFound)?,
    };

    let output = run_capture(&path, &["--version"], Duration::from_secs(5)).map_err(|source| {
        DiscoveryError::Probe {
            path: path.clone(),
            source,
        }
    })?;
    let first = output.lines().next().unwrap_or_default().trim().to_owned();
    match parse_version(&first) {
        Some(version) => Ok(MpvInfo {
            path,
            version_line: first,
            version,
        }),
        None => Err(DiscoveryError::NotMpv {
            path,
            output: first,
        }),
    }
}

/// [`discover`] using `LUCERNA_MPV` and `PATH` from the process environment.
pub fn discover_from_env() -> Result<MpvInfo, DiscoveryError> {
    discover(
        std::env::var_os(OVERRIDE_ENV).as_deref(),
        std::env::var_os("PATH").as_deref(),
    )
}

/// Spawn `command`, retrying briefly while the executable is busy.
///
/// `ETXTBSY` ("text file busy") is returned by `exec` while some process still holds the file open
/// for writing. That happens when mpv is being upgraded, and in tests that write a small script and
/// run it while another thread forks (the child briefly inherits the write descriptor).
pub(crate) fn spawn_retrying(command: &mut Command) -> io::Result<std::process::Child> {
    let mut attempts = 0;
    loop {
        match command.spawn() {
            Err(e) if e.kind() == io::ErrorKind::ExecutableFileBusy && attempts < 50 => {
                attempts += 1;
                std::thread::sleep(Duration::from_millis(20));
            }
            other => return other,
        }
    }
}

/// Run a program with a hard timeout and return its stdout. The output must be small.
fn run_capture(program: &Path, args: &[&str], timeout: Duration) -> io::Result<String> {
    let mut child = spawn_retrying(
        Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null()),
    )?;
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait()? {
            Some(_) => break,
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(io::Error::new(io::ErrorKind::TimedOut, "timed out"));
            }
            None => std::thread::sleep(Duration::from_millis(5)),
        }
    }
    let mut out = String::new();
    if let Some(mut stdout) = child.stdout.take() {
        let mut buf = Vec::new();
        stdout.by_ref().take(64 * 1024).read_to_end(&mut buf)?;
        out = String::from_utf8_lossy(&buf).into_owned();
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tempdir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lucerna-disc-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_script(path: &Path, body: &str) {
        fs::write(path, body).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    fn parses_common_version_lines() {
        assert_eq!(
            parse_version("mpv 0.37.0 Copyright © 2000-2023").as_deref(),
            Some("0.37.0")
        );
        assert_eq!(
            parse_version("mpv v0.40.0-dirty Copyright").as_deref(),
            Some("0.40.0")
        );
        assert_eq!(
            parse_version("mpv 0.36.0-UNKNOWN").as_deref(),
            Some("0.36.0")
        );
        assert_eq!(parse_version("ffmpeg version 6"), None);
        assert_eq!(parse_version("mpv"), None);
        assert_eq!(parse_version(""), None);
    }

    #[test]
    fn path_search_skips_non_executables_and_relative_entries() {
        let a = tempdir("a");
        let b = tempdir("b");
        fs::write(a.join("mpv"), "not executable").unwrap();
        write_script(&b.join("mpv"), "#!/bin/sh\n");
        let path = std::env::join_paths([PathBuf::from("relative"), a, b.clone()]).unwrap();
        assert_eq!(find_in_path(Some(&path)), Some(b.join("mpv")));
        assert_eq!(find_in_path(None), None);
    }

    #[test]
    fn missing_mpv_gives_the_actionable_message() {
        let empty = tempdir("empty");
        let err = discover(None, Some(empty.as_os_str())).unwrap_err();
        assert!(matches!(err, DiscoveryError::NotFound));
        let text = err.to_string();
        assert!(text.contains("Lucerna could not start mpv."));
        assert!(text.contains("not found in PATH"));
        assert!(text.contains("Install mpv"));
    }

    #[test]
    fn override_is_used_and_validated() {
        let dir = tempdir("override");
        let fake = dir.join("fakempv");
        write_script(&fake, "#!/bin/sh\necho 'mpv 9.9.9 Copyright fake'\n");
        let info = discover(Some(fake.as_os_str()), None).unwrap();
        assert_eq!(info.version, "9.9.9");
        assert_eq!(info.path, fake);

        let missing = dir.join("nope");
        assert!(matches!(
            discover(Some(missing.as_os_str()), None),
            Err(DiscoveryError::OverrideInvalid { .. })
        ));
    }

    #[test]
    fn something_that_is_not_mpv_is_rejected() {
        let dir = tempdir("notmpv");
        let fake = dir.join("mpv");
        write_script(&fake, "#!/bin/sh\necho 'GNU thing 1.0'\n");
        assert!(matches!(
            discover(None, Some(dir.as_os_str())),
            Err(DiscoveryError::NotMpv { .. })
        ));
    }

    #[test]
    fn hanging_binary_times_out() {
        let dir = tempdir("hang");
        let fake = dir.join("mpv");
        write_script(&fake, "#!/bin/sh\nsleep 30\n");
        let err = run_capture(&fake, &["--version"], Duration::from_millis(100)).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::TimedOut);
    }
}
