//! Logging initialisation shared by every Lucerna binary.
//!
//! The filter comes from `LUCERNA_LOG`, then `RUST_LOG`, then the default
//! `lucerna=info`. Filter targets match by string prefix, so
//! `RUST_LOG=lucerna=debug` covers every `lucerna_*` crate.

use std::io::Write;
use std::path::Path;
use std::sync::Mutex;

use tracing_subscriber::EnvFilter;

use crate::bounded_log::{BoundedLog, BoundedLogWriter};

/// Default filter directive when no environment variable is set.
pub const DEFAULT_FILTER: &str = "lucerna=info";

/// Build the log filter from explicit values (in priority order) so it can be tested
/// without touching the process environment.
pub fn build_filter(lucerna_log: Option<&str>, rust_log: Option<&str>) -> EnvFilter {
    for candidate in [lucerna_log, rust_log].into_iter().flatten() {
        if candidate.trim().is_empty() {
            continue;
        }
        if let Ok(filter) = EnvFilter::try_new(candidate) {
            return filter;
        }
    }
    EnvFilter::new(DEFAULT_FILTER)
}

/// Initialise `tracing` for the given component (`lucerna`, `lucernad`, `lucernactl`).
///
/// Logs go to stderr. Calling this twice is harmless: the second call is ignored.
pub fn init(component: &str, verbose: bool) {
    let filter = if verbose {
        EnvFilter::new("lucerna=debug")
    } else {
        build_filter(
            std::env::var("LUCERNA_LOG").ok().as_deref(),
            std::env::var("RUST_LOG").ok().as_deref(),
        )
    };
    let installed = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_target(true)
        .try_init()
        .is_ok();
    if installed {
        tracing::debug!(component, "logging initialised");
    }
}

/// Writes every log line to stderr and, if available, to a size-bounded rotating file.
struct Tee {
    file: Option<BoundedLogWriter>,
}

impl Write for Tee {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let _ = std::io::stderr().write_all(buf);
        if let Some(file) = self.file.as_mut() {
            // A full disk or a vanished directory must never take the daemon down.
            let _ = file.write_all(buf);
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        std::io::stderr().flush()
    }
}

/// Like [`init`], and additionally keep a bounded log file (two files of at most `max_bytes` each).
///
/// If the file cannot be opened the daemon logs to stderr only. Calling this twice is harmless.
pub fn init_with_file(component: &str, verbose: bool, file: &Path, max_bytes: u64) {
    let filter = if verbose {
        EnvFilter::new("lucerna=debug")
    } else {
        build_filter(
            std::env::var("LUCERNA_LOG").ok().as_deref(),
            std::env::var("RUST_LOG").ok().as_deref(),
        )
    };
    let writer = BoundedLog::open(file, max_bytes)
        .ok()
        .map(BoundedLogWriter::new);
    let opened = writer.is_some();
    let installed = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(Mutex::new(Tee { file: writer }))
        .with_target(true)
        .with_ansi(false)
        .try_init()
        .is_ok();
    if installed {
        tracing::debug!(component, log_file = %file.display(), file_logging = opened, "logging initialised");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing::level_filters::LevelFilter;

    #[test]
    fn default_filter_is_info_for_lucerna() {
        let f = build_filter(None, None);
        assert_eq!(f.max_level_hint(), Some(LevelFilter::INFO));
    }

    #[test]
    fn lucerna_log_wins_over_rust_log() {
        let f = build_filter(Some("lucerna=trace"), Some("lucerna=error"));
        assert_eq!(f.max_level_hint(), Some(LevelFilter::TRACE));
    }

    #[test]
    fn rust_log_used_when_lucerna_log_absent_or_blank() {
        let f = build_filter(Some("  "), Some("lucerna=debug"));
        assert_eq!(f.max_level_hint(), Some(LevelFilter::DEBUG));
    }

    #[test]
    fn invalid_directive_falls_back_to_default() {
        let f = build_filter(Some("lucerna=nonsense=="), None);
        assert_eq!(f.max_level_hint(), Some(LevelFilter::INFO));
    }

    #[test]
    fn init_twice_does_not_panic() {
        init("test", false);
        init("test", true);
    }
}
