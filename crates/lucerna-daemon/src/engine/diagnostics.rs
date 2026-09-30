//! The daemon's part of the `doctor` report (`GetDiagnostics`).

use lucerna_core::autostart;
use lucerna_core::doctor::{RedactContext, redact};
use lucerna_core::version::VERSION;
use serde_json::{Value, json};

use super::{BackendStatus, Engine};

impl Engine {
    pub(crate) fn diagnostics_json(&mut self, redact_output: bool) -> String {
        let status = serde_json::to_value(self.status_dto()).unwrap_or(Value::Null);
        let displays = serde_json::to_value(self.displays(true)).unwrap_or(Value::Null);
        let wallpapers = serde_json::to_value(self.wallpapers()).unwrap_or(Value::Null);
        let settings = serde_json::to_value(self.settings_dto()).unwrap_or(Value::Null);
        let backend = match &self.backend_status {
            BackendStatus::Available { kind, capabilities } => {
                json!({"kind": kind.as_str(), "available": true, "capabilities": capabilities})
            }
            BackendStatus::Unavailable(reason) => {
                json!({"kind": "none", "available": false, "reason": reason.code(), "message": reason.message()})
            }
        };
        let backend_diagnostics = self
            .backend
            .as_mut()
            .map_or(Value::Null, |b| b.diagnostics());
        let autostart_file = self.opts.paths.autostart_file();
        let autostart_state = autostart::state(&autostart_file);

        let mut report = json!({
            "daemon_version": VERSION,
            "api_version": lucerna_ipc::names::API_VERSION,
            "status": status,
            "backend": backend,
            "backend_diagnostics": backend_diagnostics,
            "restack": {"fights": self.restack_fights, "recent_refreshes": self.restack_times.len()},
            "session": {
                "kind": self.session_kind.as_str(),
                "XDG_SESSION_TYPE": self.opts.session.xdg_session_type,
                "XDG_CURRENT_DESKTOP": self.opts.session.xdg_current_desktop,
                "DESKTOP_SESSION": self.opts.session.desktop_session,
                "DISPLAY": self.opts.session.display,
            },
            "mpv": {
                "found": self.mpv.info.is_some(),
                "path": self.mpv.info.as_ref().map(|i| i.path.to_string_lossy().into_owned()),
                "version": self.mpv.info.as_ref().map(|i| i.version.clone()),
                "options_compatible": self.mpv.compat.as_ref().map(|c| c.compatible),
                "compat_detail": self.mpv.compat.as_ref().map(|c| c.detail.clone()),
                "error": self.mpv.error,
            },
            "config": {
                "file": self.opts.paths.config_file().to_string_lossy(),
                "state": self.loaded.state.code(),
                "notice": self.loaded.notice,
                "warnings": self.loaded.warnings,
            },
            "paths": {
                "config_dir": self.opts.paths.config_dir.to_string_lossy(),
                "state_dir": self.opts.paths.state_dir.to_string_lossy(),
                "cache_dir": self.opts.paths.cache_dir.to_string_lossy(),
                "runtime_dir": self.opts.paths.runtime_dir.as_ref().map(|p| p.to_string_lossy().into_owned()),
            },
            "autostart": {"file": autostart_file.to_string_lossy(), "state": autostart_state.code()},
            "settings": settings,
            "displays": displays,
            "wallpapers": wallpapers,
            "recent_failures": self.state.failures,
        });
        if redact_output {
            redact(&mut report, &RedactContext::from_env());
        }
        report.to_string()
    }
}
