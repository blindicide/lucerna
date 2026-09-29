//! Small process helpers: signals and `/proc` inspection.

use std::io;
use std::path::PathBuf;

use rustix::process::{Pid, Signal, kill_process};

/// A signal Lucerna sends to its own renderers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sig {
    Term,
    Kill,
}

impl From<Sig> for Signal {
    fn from(sig: Sig) -> Self {
        match sig {
            Sig::Term => Signal::TERM,
            Sig::Kill => Signal::KILL,
        }
    }
}

/// Send `sig` to `pid`. Returns `Ok(false)` if the process is already gone.
pub fn send_signal(pid: u32, sig: Sig) -> io::Result<bool> {
    let Some(pid) = i32::try_from(pid).ok().and_then(Pid::from_raw) else {
        return Ok(false);
    };
    match kill_process(pid, sig.into()) {
        Ok(()) => Ok(true),
        Err(rustix::io::Errno::SRCH) => Ok(false),
        Err(e) => Err(io::Error::from(e)),
    }
}

/// Field 22 (`starttime`) of `/proc/<pid>/stat`, in clock ticks since boot.
///
/// Together with the pid it identifies one process incarnation, defeating pid reuse.
pub fn start_time(pid: u32) -> Option<u64> {
    parse_stat(&std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?).map(|s| s.start_time)
}

/// True if the process exists and is not a zombie.
pub fn is_alive(pid: u32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .ok()
        .and_then(|s| parse_stat(&s))
        .is_some_and(|s| s.state != 'Z' && s.state != 'X')
}

#[derive(Debug, PartialEq, Eq)]
pub struct Stat {
    pub state: char,
    pub start_time: u64,
}

/// Parse `/proc/<pid>/stat`. The command name (field 2) may contain spaces and parentheses, so
/// everything is located relative to the *last* `)`.
pub fn parse_stat(text: &str) -> Option<Stat> {
    let rest = &text[text.rfind(')')? + 1..];
    let mut fields = rest.split_whitespace();
    let state = fields.next()?.chars().next()?; // field 3
    let start_time = fields.nth(18)?.parse().ok()?; // field 22
    Some(Stat { state, start_time })
}

/// The executable behind `/proc/<pid>/exe`, without a trailing ` (deleted)`.
pub fn exe_path(pid: u32) -> Option<PathBuf> {
    let link = std::fs::read_link(format!("/proc/{pid}/exe")).ok()?;
    let text = link.to_string_lossy();
    Some(PathBuf::from(
        text.strip_suffix(" (deleted)").unwrap_or(&text).to_owned(),
    ))
}

/// `/proc/<pid>/cmdline` split at NUL bytes.
pub fn cmdline(pid: u32) -> Option<Vec<String>> {
    let raw = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    Some(
        raw.split(|b| *b == 0)
            .filter(|part| !part.is_empty())
            .map(|part| String::from_utf8_lossy(part).into_owned())
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_stat_with_awkward_command_names() {
        let line = "4242 (mpv (weird) name) S 1 4242 4242 0 -1 4194560 100 0 0 0 1 2 0 0 20 0 3 0 987654 1000 200 18446744073709551615 0 0 0 0 0 0 0 0 0 0 0 0 17 0 0 0 0 0 0";
        let stat = parse_stat(line).unwrap();
        assert_eq!(
            stat,
            Stat {
                state: 'S',
                start_time: 987_654
            }
        );
    }

    #[test]
    fn own_process_has_a_start_time_and_is_alive() {
        let me = std::process::id();
        assert!(start_time(me).is_some());
        assert!(is_alive(me));
        assert!(exe_path(me).is_some());
        assert!(cmdline(me).is_some_and(|c| !c.is_empty()));
    }

    #[test]
    fn missing_process_is_not_alive() {
        assert!(!is_alive(u32::MAX - 1));
        assert_eq!(start_time(u32::MAX - 1), None);
        assert!(!send_signal(u32::MAX - 1, Sig::Term).unwrap());
    }
}
