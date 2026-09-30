//! `state.json`: things worth remembering across daemon restarts but that are not user intent.

use std::collections::VecDeque;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::fsutil::atomic_write;

/// The state file keeps at most this many recent renderer failures.
pub const MAX_FAILURES: usize = 20;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FailureRecord {
    /// RFC 3339 UTC.
    pub time: String,
    pub output: String,
    pub code: String,
    pub message: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateFile {
    #[serde(default)]
    pub failures: VecDeque<FailureRecord>,
    #[serde(default)]
    pub config_notice: Option<String>,
}

impl StateFile {
    /// Load, tolerating a missing or corrupt file (state is disposable).
    pub fn load(path: &Path) -> Self {
        fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    pub fn record_failure(&mut self, record: FailureRecord) {
        self.failures.push_back(record);
        while self.failures.len() > MAX_FAILURES {
            self.failures.pop_front();
        }
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let bytes = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        atomic_write(path, &bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(n: usize) -> FailureRecord {
        FailureRecord {
            time: format!("t{n}"),
            output: "conn:A".into(),
            code: "crashed".into(),
            message: format!("m{n}"),
        }
    }

    #[test]
    fn keeps_only_the_last_twenty_failures() {
        let mut state = StateFile::default();
        for n in 0..25 {
            state.record_failure(record(n));
        }
        assert_eq!(state.failures.len(), MAX_FAILURES);
        assert_eq!(state.failures.front().unwrap().time, "t5");
        assert_eq!(state.failures.back().unwrap().time, "t24");
    }

    #[test]
    fn round_trips_and_survives_corruption() {
        let dir = std::env::temp_dir().join(format!("lucerna-state-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let path = dir.join("nested").join("state.json");
        let mut state = StateFile::default();
        state.record_failure(record(1));
        state.config_notice = Some("hello".into());
        state.save(&path).unwrap();
        assert_eq!(StateFile::load(&path), state);

        fs::write(&path, b"{ not json").unwrap();
        assert_eq!(StateFile::load(&path), StateFile::default());
        assert_eq!(
            StateFile::load(&dir.join("absent.json")),
            StateFile::default()
        );
    }

    #[test]
    fn unknown_fields_from_a_newer_version_are_ignored() {
        let dir = std::env::temp_dir().join(format!("lucerna-state2-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("state.json");
        fs::write(
            &path,
            br#"{"failures": [], "config_notice": null, "from_the_future": 1}"#,
        )
        .unwrap();
        assert_eq!(StateFile::load(&path), StateFile::default());
    }
}
