//! Lucerna test harness. Never shipped and never a dependency of another crate.
//!
//! Integration suites live in `tests/`. This library holds the helpers they share: a private
//! runtime directory, fake-mpv media files and log readers, and state-waiting utilities.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

pub mod xvfb;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use lucerna_core::renderer::{RendererSnapshot, RendererStateKind};
use lucerna_mpv::RendererSupervisor;
use serde_json::Value;

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// A throw-away directory tree with a private (0700) runtime directory.
pub struct TestEnv {
    pub root: PathBuf,
    pub runtime_dir: PathBuf,
    pub log_dir: PathBuf,
    pub media_dir: PathBuf,
}

impl TestEnv {
    pub fn new(name: &str) -> Self {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        // Keep the path short: unix socket paths are limited to 107 bytes.
        let root = std::env::temp_dir().join(format!("lc-{name}-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let runtime_dir = root.join("rt");
        let log_dir = root.join("logs");
        let media_dir = root.join("media");
        for dir in [&runtime_dir, &log_dir, &media_dir] {
            fs::create_dir_all(dir).expect("create test dir");
        }
        fs::set_permissions(&runtime_dir, fs::Permissions::from_mode(0o700)).expect("chmod");
        Self {
            root,
            runtime_dir,
            log_dir,
            media_dir,
        }
    }

    /// Write a fake-mpv "media" file whose first line selects the behaviour.
    pub fn media(&self, file_name: &str, directives: &str) -> PathBuf {
        let path = self.media_dir.join(file_name);
        fs::write(
            &path,
            format!("FAKE-MPV: {directives}\nnot really a video\n"),
        )
        .expect("write");
        path
    }

    pub fn registry_path(&self) -> PathBuf {
        self.runtime_dir.join("renderers.json")
    }
}

impl Drop for TestEnv {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// Everything fake-mpv recorded for `media`, one JSON value per line.
pub fn read_fake_log(media: &Path) -> Vec<Value> {
    let path = format!("{}.log", media.display());
    fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

/// The `launch` records only.
pub fn launches(media: &Path) -> Vec<Value> {
    read_fake_log(media)
        .into_iter()
        .filter(|v| v["event"] == "launch")
        .collect()
}

/// All IPC commands received, as `["set_property", "pause", true]`-style arrays.
pub fn commands(media: &Path) -> Vec<Value> {
    read_fake_log(media)
        .into_iter()
        .filter(|v| v["event"] == "command")
        .map(|v| v["command"].clone())
        .collect()
}

/// Wait until `predicate` holds for the supervisor's snapshot, or panic after `timeout`.
pub async fn wait_for(
    supervisor: &RendererSupervisor,
    timeout: Duration,
    what: &str,
    predicate: impl Fn(&RendererSnapshot) -> bool,
) -> RendererSnapshot {
    let mut rx = supervisor.subscribe();
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        {
            let snapshot = rx.borrow_and_update().clone();
            if predicate(&snapshot) {
                return snapshot;
            }
        }
        match tokio::time::timeout_at(deadline, rx.changed()).await {
            Ok(Ok(())) => {}
            _ => panic!(
                "timed out after {timeout:?} waiting for {what}; last: {:?}",
                supervisor.snapshot()
            ),
        }
    }
}

pub async fn wait_for_state(
    supervisor: &RendererSupervisor,
    state: RendererStateKind,
    timeout: Duration,
) -> RendererSnapshot {
    wait_for(supervisor, timeout, state.as_str(), |s| s.state == state).await
}

/// Poll a condition (for things that are not on the snapshot, like log files).
pub async fn eventually(timeout: Duration, what: &str, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + timeout;
    while !condition() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// True if a process with this pid exists and is not a zombie.
pub fn pid_alive(pid: u64) -> bool {
    fs::read_to_string(format!("/proc/{pid}/stat"))
        .ok()
        .and_then(|s| {
            s.rsplit_once(')')
                .map(|(_, rest)| rest.trim_start().chars().next())
        })
        .flatten()
        .is_some_and(|state| state != 'Z' && state != 'X')
}

/// Total size of all files directly inside `dir`.
pub fn dir_size(dir: &Path) -> u64 {
    fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|e| e.metadata().ok())
                .map(|m| m.len())
                .sum()
        })
        .unwrap_or(0)
}
