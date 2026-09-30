//! `lucernactl doctor` (directive §21): everything needed to debug desktop integration remotely.

use lucerna_core::autostart;
use lucerna_core::config::{CURRENT_SCHEMA, inspect};
use lucerna_core::doctor::{
    AutostartReport, ConfigReport, DaemonSection, MpvReport, REPORT_VERSION, RedactContext, Report,
    allowed_environment, redact,
};
use lucerna_core::mpv::{check_compat, discover_from_env};
use lucerna_core::paths::Paths;
use lucerna_core::timeutil::now_rfc3339;
use lucerna_core::version::VERSION;
use serde_json::Value;

use crate::client::Client;

fn path_text(p: &std::path::Path) -> Option<String> {
    Some(p.to_string_lossy().into_owned())
}

/// Collect the whole report. Never modifies anything.
pub fn collect() -> Report {
    let paths = Paths::from_env();

    let environment = allowed_environment(|k| std::env::var(k).ok());
    let path_map = [
        ("config_file", path_text(&paths.config_file())),
        ("state_dir", path_text(&paths.state_dir)),
        ("cache_dir", path_text(&paths.cache_dir)),
        (
            "runtime_dir",
            paths.runtime_dir.as_deref().and_then(path_text),
        ),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v))
    .collect();

    let inspection = inspect(&paths.config_file());
    let config = ConfigReport {
        file: paths.config_file().to_string_lossy().into_owned(),
        exists: inspection.exists,
        schema_version: inspection.schema_version,
        current_schema_version: CURRENT_SCHEMA,
        warnings: inspection.warnings,
    };

    let mpv = match discover_from_env() {
        Ok(info) => {
            let compat = check_compat(&info.path);
            MpvReport {
                found: true,
                path: path_text(&info.path),
                version: Some(info.version),
                options_compatible: compat.as_ref().ok().map(|c| c.compatible),
                compat_detail: compat
                    .as_ref()
                    .ok()
                    .map(|c| c.detail.clone())
                    .filter(|d| !d.is_empty()),
                error: compat.err().map(|e| e.to_string()),
            }
        }
        Err(err) => MpvReport {
            found: false,
            path: None,
            version: None,
            options_compatible: None,
            compat_detail: None,
            error: Some(err.to_string()),
        },
    };

    let autostart_file = paths.autostart_file();
    let auto = autostart::state(&autostart_file);
    let autostart = AutostartReport {
        file: autostart_file.to_string_lossy().into_owned(),
        state: auto.code().to_owned(),
        exec: match &auto {
            autostart::AutostartState::Enabled { exec } => Some(exec.clone()),
            _ => None,
        },
    };

    let (daemon, x11_probe) = match ask_daemon() {
        Ok(diagnostics) => (
            DaemonSection {
                reachable: true,
                error: None,
                diagnostics: Some(diagnostics),
            },
            None,
        ),
        Err(message) => {
            // The daemon cannot be asked, so look at the display directly (read-only).
            let probe = lucerna_x11::probe_display(None);
            (
                DaemonSection {
                    reachable: false,
                    error: Some(message),
                    diagnostics: None,
                },
                serde_json::to_value(probe).ok(),
            )
        }
    };

    Report {
        report_version: REPORT_VERSION,
        generated: now_rfc3339(),
        lucerna_version: VERSION.to_owned(),
        environment,
        paths: path_map,
        config,
        mpv,
        autostart,
        daemon,
        x11_probe,
    }
}

fn ask_daemon() -> Result<Value, String> {
    let client = Client::connect().map_err(|e| e.message)?;
    // Ask for the unredacted data; redaction is applied to the whole report at the end so the
    // rules are identical for local and daemon-provided parts.
    let text = client
        .proxy
        .get_diagnostics(false)
        .map_err(|e| crate::client::CliError::from(e).message)?;
    serde_json::from_str(&text)
        .map_err(|e| format!("the daemon returned an unreadable report: {e}"))
}

/// The report as JSON, redacted if asked.
pub fn to_json(report: &Report, redact_output: bool) -> Value {
    let mut value = serde_json::to_value(report).unwrap_or(Value::Null);
    if redact_output {
        redact(&mut value, &RedactContext::from_env());
    }
    value
}

/// A short human-readable summary with ✓ / ✗ / ! markers.
pub fn render_text(value: &Value) -> String {
    let mut out = String::new();
    let mark = |ok: Option<bool>| match ok {
        Some(true) => "✓",
        Some(false) => "✗",
        None => "!",
    };
    let get =
        |path: &[&str]| -> Option<&Value> { path.iter().try_fold(value, |v, key| v.get(*key)) };
    let text = |path: &[&str]| {
        get(path)
            .and_then(Value::as_str)
            .unwrap_or("(unknown)")
            .to_owned()
    };

    out.push_str(&format!(
        "Lucerna doctor — lucernactl {} (report format {})\n\n",
        text(&["lucerna_version"]),
        text(&["report_version"]).replace("(unknown)", "1")
    ));

    let session = |k: &str| {
        get(&["environment", k])
            .and_then(Value::as_str)
            .unwrap_or("(not set)")
            .to_owned()
    };
    out.push_str(&format!(
        "{} Session: XDG_SESSION_TYPE={} XDG_CURRENT_DESKTOP={} DESKTOP_SESSION={} DISPLAY={}\n",
        mark(Some(session("XDG_SESSION_TYPE") != "wayland")),
        session("XDG_SESSION_TYPE"),
        session("XDG_CURRENT_DESKTOP"),
        session("DESKTOP_SESSION"),
        session("DISPLAY"),
    ));
    if session("XDG_SESSION_TYPE") == "wayland" {
        out.push_str("    This release supports X11 sessions only; log in with a \"Cinnamon\" (X11) session.\n");
    }

    let mpv_found = get(&["mpv", "found"]).and_then(Value::as_bool);
    if mpv_found == Some(true) {
        let compat = get(&["mpv", "options_compatible"]).and_then(Value::as_bool);
        out.push_str(&format!(
            "{} mpv: {} (version {}); options {}\n",
            mark(compat),
            text(&["mpv", "path"]),
            text(&["mpv", "version"]),
            match compat {
                Some(true) => "compatible".to_owned(),
                Some(false) => format!("REJECTED: {}", text(&["mpv", "compat_detail"])),
                None => "not checked".to_owned(),
            }
        ));
    } else {
        out.push_str(&format!(
            "✗ mpv: not found\n    {}\n",
            text(&["mpv", "error"]).replace('\n', "\n    ")
        ));
    }

    let reachable = get(&["daemon", "reachable"])
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if reachable {
        out.push_str(&format!(
            "✓ Daemon: running, version {}, backend {}, playback {}\n",
            text(&["daemon", "diagnostics", "daemon_version"]),
            text(&["daemon", "diagnostics", "backend", "kind"]),
            text(&["daemon", "diagnostics", "status", "playback"]),
        ));
        if let Some(reason) = get(&["daemon", "diagnostics", "status", "unsupported_message"])
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
        {
            out.push_str(&format!("!   {}\n", reason.replace('\n', "\n    ")));
        }
        let connection = ["daemon", "diagnostics", "backend_diagnostics", "connection"];
        if get(&connection).is_some_and(|v| v.is_object()) {
            let at = |rest: &[&str]| match get(&[connection.as_slice(), rest].concat()) {
                Some(Value::String(s)) => s.clone(),
                Some(Value::Number(n)) => n.to_string(),
                _ => "(unknown)".to_owned(),
            };
            out.push_str(&format!(
                "✓ X11: {} ({} {}), RandR {}, window manager {}\n",
                at(&["display"]),
                at(&["server", "vendor"]),
                at(&["server", "release"]),
                at(&["randr", "version"]),
                at(&["window_manager", "name"]),
            ));
        }
        let failures = get(&["daemon", "diagnostics", "recent_failures"])
            .and_then(Value::as_array)
            .map_or(0, Vec::len);
        if failures > 0 {
            out.push_str(&format!(
                "! {failures} recent renderer failure(s); see `lucernactl doctor --json` (recent_failures)\n"
            ));
        }
    } else {
        out.push_str(&format!(
            "! Daemon: not reachable\n    {}\n",
            text(&["daemon", "error"]).replace('\n', "\n    ")
        ));
        let connected = get(&["x11_probe", "connected"]).and_then(Value::as_bool);
        out.push_str(&format!(
            "{} X11: {}\n",
            mark(connected),
            match connected {
                Some(true) => "connected (read-only probe)".to_owned(),
                Some(false) => text(&["x11_probe", "error"]).replace('\n', "\n    "),
                None => "not probed".to_owned(),
            }
        ));
    }

    let cfg_exists = get(&["config", "exists"])
        .and_then(Value::as_bool)
        .unwrap_or(false);
    out.push_str(&format!(
        "{} Config: {} ({})\n",
        mark(Some(true)),
        text(&["config", "file"]),
        if cfg_exists {
            "exists"
        } else {
            "not created yet"
        }
    ));
    for warning in get(&["config", "warnings"])
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        out.push_str(&format!("!   {warning}\n"));
    }
    out.push_str(&format!(
        "{} Autostart: {}\n",
        mark(Some(text(&["autostart", "state"]) != "disabled")),
        text(&["autostart", "state"])
    ));
    out.push_str("\nRun `lucernactl doctor --json` for the complete report; add `--redact` before posting it publicly.\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn report_json() -> Value {
        json!({
            "report_version": 1,
            "lucerna_version": "0.3.0",
            "environment": {"XDG_SESSION_TYPE": "x11", "XDG_CURRENT_DESKTOP": "X-Cinnamon", "DESKTOP_SESSION": "cinnamon", "DISPLAY": ":0"},
            "mpv": {"found": true, "path": "/usr/bin/mpv", "version": "0.37.0", "options_compatible": true, "compat_detail": null, "error": null},
            "config": {"file": "/home/u/.config/lucerna/config.toml", "exists": false, "warnings": ["w"]},
            "autostart": {"state": "enabled"},
            "daemon": {"reachable": false, "error": "The Lucerna daemon is not running.\nStart it with `lucernad &`."},
            "x11_probe": {"connected": true}
        })
    }

    #[test]
    fn text_summary_uses_markers_and_hints() {
        let text = render_text(&report_json());
        assert!(text.contains("✓ Session: XDG_SESSION_TYPE=x11 XDG_CURRENT_DESKTOP=X-Cinnamon"));
        assert!(text.contains("✓ mpv: /usr/bin/mpv (version 0.37.0); options compatible"));
        assert!(text.contains("! Daemon: not reachable"));
        assert!(text.contains("✓ X11: connected (read-only probe)"));
        assert!(text.contains("!   w"));
        assert!(text.contains("--redact"));
    }

    #[test]
    fn a_running_daemon_contributes_the_x11_and_failure_lines() {
        let mut v = report_json();
        v["daemon"] = json!({"reachable": true, "diagnostics": {
            "daemon_version": "0.9.0",
            "backend": {"kind": "cinnamon-x11"},
            "status": {"playback": "playing"},
            "backend_diagnostics": {"connection": {
                "display": ":0",
                "server": {"vendor": "The X.Org Foundation", "release": 12101008},
                "randr": {"version": "1.6"},
                "window_manager": {"name": "Muffin"},
            }},
            "recent_failures": [{"display": "a"}, {"display": "b"}],
        }});
        let text = render_text(&v);
        assert!(
            text.contains(
                "✓ X11: :0 (The X.Org Foundation 12101008), RandR 1.6, window manager Muffin"
            ),
            "{text}"
        );
        assert!(text.contains("! 2 recent renderer failure(s)"), "{text}");
    }

    #[test]
    fn wayland_and_missing_mpv_are_flagged() {
        let mut v = report_json();
        v["environment"]["XDG_SESSION_TYPE"] = json!("wayland");
        v["mpv"] = json!({"found": false, "error": "Lucerna could not start mpv.\nInstall mpv."});
        let text = render_text(&v);
        assert!(text.contains("✗ Session"));
        assert!(text.contains("supports X11 sessions only"));
        assert!(text.contains("✗ mpv: not found"));
        assert!(text.contains("Install mpv."));
    }

    #[test]
    fn redaction_applies_to_the_whole_report() {
        let mut v = report_json();
        let ctx = RedactContext {
            home: Some("/home/u".into()),
            user: Some("u".into()),
            host: None,
        };
        redact(&mut v, &ctx);
        assert_eq!(v["config"]["file"], "~/.config/lucerna/config.toml");
    }
}
