//! The PID registry and stale-process recovery (directive §52).
//!
//! Every renderer Lucerna starts is recorded with its pid *and* its start time. After a daemon
//! crash, the next daemon only terminates processes that still match a record exactly: same pid,
//! same start time, an `mpv` executable, and our private `--input-ipc-server=` socket on its
//! command line. An mpv the user started themselves can never match.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use lucerna_core::fsutil::atomic_write;
use serde::{Deserialize, Serialize};

use crate::proc::{self, Sig};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PidRecord {
    pub pid: u32,
    pub starttime: u64,
    pub socket: String,
    pub output_id: String,
    pub generation: u64,
}

/// Shared, atomically persisted list of live renderer processes.
pub struct PidRegistry {
    path: PathBuf,
    records: Mutex<Vec<PidRecord>>,
}

impl PidRegistry {
    /// A registry persisted at `path` (normally `$XDG_RUNTIME_DIR/lucerna/renderers.json`).
    /// Existing content is *not* loaded: recovery reads it first, then the registry starts empty.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            records: Mutex::new(Vec::new()),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn register(&self, record: PidRecord) {
        if let Ok(mut records) = self.records.lock() {
            records.retain(|r| r.pid != record.pid);
            records.push(record);
            self.persist(&records);
        }
    }

    pub fn unregister(&self, pid: u32) {
        if let Ok(mut records) = self.records.lock() {
            let before = records.len();
            records.retain(|r| r.pid != pid);
            if records.len() != before {
                self.persist(&records);
            }
        }
    }

    pub fn snapshot(&self) -> Vec<PidRecord> {
        self.records.lock().map(|r| r.clone()).unwrap_or_default()
    }

    fn persist(&self, records: &[PidRecord]) {
        match serde_json::to_vec_pretty(records) {
            Ok(bytes) => {
                if let Err(err) = atomic_write(&self.path, &bytes) {
                    tracing::warn!(path = %self.path.display(), %err, "could not persist renderer registry");
                }
            }
            Err(err) => tracing::warn!(%err, "could not serialise renderer registry"),
        }
    }
}

/// Read records from a registry file. A missing or corrupt file yields no records.
pub fn load(path: &Path) -> Vec<PidRecord> {
    std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

#[derive(Clone, Debug)]
pub struct RecoveryOptions {
    /// File name the process executable must have (`mpv`).
    pub exe_name: String,
    /// How long to wait after SIGTERM before SIGKILL.
    pub grace: Duration,
}

impl Default for RecoveryOptions {
    fn default() -> Self {
        Self {
            exe_name: "mpv".to_owned(),
            grace: Duration::from_secs(2),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RecoveryReport {
    /// Processes that matched a record and were terminated.
    pub terminated: Vec<u32>,
    /// Records that no longer matched a live process, with the reason (pid gone, reused, ...).
    pub skipped: Vec<(u32, &'static str)>,
}

fn matches_record(record: &PidRecord, opts: &RecoveryOptions) -> Result<(), &'static str> {
    if !proc::is_alive(record.pid) {
        return Err("process is gone");
    }
    if proc::start_time(record.pid) != Some(record.starttime) {
        return Err("pid was reused by a different process");
    }
    let exe_ok = proc::exe_path(record.pid)
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .is_some_and(|name| name == opts.exe_name);
    if !exe_ok {
        return Err("executable is not the expected renderer");
    }
    let needle = format!("--input-ipc-server={}", record.socket);
    if !proc::cmdline(record.pid).is_some_and(|args| args.contains(&needle)) {
        return Err("command line does not carry our IPC socket");
    }
    Ok(())
}

/// Terminate renderers left over from a previous daemon, then clear the registry file.
///
/// Blocking (up to `grace` per stubborn process); call it from `spawn_blocking` in async code.
pub fn recover_stale(registry_path: &Path, opts: &RecoveryOptions) -> RecoveryReport {
    let mut report = RecoveryReport::default();
    for record in load(registry_path) {
        if let Err(why) = matches_record(&record, opts) {
            report.skipped.push((record.pid, why));
            continue;
        }
        tracing::warn!(pid = record.pid, output = %record.output_id, "terminating stale renderer from a previous daemon");
        let _ = proc::send_signal(record.pid, Sig::Term);
        let deadline = Instant::now() + opts.grace;
        while Instant::now() < deadline && matches_record(&record, opts).is_ok() {
            std::thread::sleep(Duration::from_millis(25));
        }
        // Re-verify identity before escalating: the pid must not have been reused meanwhile.
        if matches_record(&record, opts).is_ok() {
            let _ = proc::send_signal(record.pid, Sig::Kill);
        }
        report.terminated.push(record.pid);
    }
    let _ = atomic_write(registry_path, b"[]");
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lucerna-reg-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn record(pid: u32) -> PidRecord {
        PidRecord {
            pid,
            starttime: 1,
            socket: "/run/user/1/lucerna/mpv-x-1.sock".into(),
            output_id: "conn:HDMI-1".into(),
            generation: 1,
        }
    }

    #[test]
    fn register_unregister_persist_atomically() {
        let dir = tempdir("persist");
        let path = dir.join("renderers.json");
        let registry = PidRegistry::new(&path);
        registry.register(record(10));
        registry.register(record(11));
        assert_eq!(load(&path).len(), 2);
        registry.unregister(10);
        assert_eq!(load(&path), vec![record(11)]);
        registry.unregister(999); // unknown pid: no-op
        assert_eq!(registry.snapshot(), vec![record(11)]);
    }

    #[test]
    fn corrupt_or_missing_registry_loads_as_empty() {
        let dir = tempdir("corrupt");
        assert!(load(&dir.join("nope.json")).is_empty());
        std::fs::write(dir.join("bad.json"), b"{not json").unwrap();
        assert!(load(&dir.join("bad.json")).is_empty());
    }

    #[test]
    fn recovery_skips_records_that_do_not_match_a_live_process() {
        let dir = tempdir("skip");
        let path = dir.join("renderers.json");
        let me = std::process::id();
        let records = vec![
            record(u32::MAX - 1), // does not exist
            PidRecord {
                starttime: 1,
                ..record(me)
            }, // exists but start time differs
            PidRecord {
                starttime: proc::start_time(me).unwrap(),
                ..record(me)
            }, // right time, wrong exe/cmdline
        ];
        std::fs::write(&path, serde_json::to_vec(&records).unwrap()).unwrap();
        let report = recover_stale(&path, &RecoveryOptions::default());
        assert!(report.terminated.is_empty(), "{report:?}");
        assert_eq!(report.skipped.len(), 3);
        assert!(proc::is_alive(me), "the test process must survive recovery");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "[]");
    }
}
