//! The `lucernactl doctor` report model and its redaction (directive §21).
//!
//! `doctor` exists so a remote developer can debug desktop integration without access to the
//! user's machine. It reports facts, never file contents: the environment is read through an
//! allow-list and is never dumped whole.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Serialize;
use serde_json::Value;

pub const REPORT_VERSION: u32 = 1;

/// The only environment variables `doctor` looks at.
pub const ENV_ALLOW_LIST: &[&str] = &[
    "XDG_SESSION_TYPE",
    "XDG_CURRENT_DESKTOP",
    "DESKTOP_SESSION",
    "DISPLAY",
    "WAYLAND_DISPLAY",
    "DBUS_SESSION_BUS_ADDRESS",
    "XDG_RUNTIME_DIR",
    "LUCERNA_MPV",
    "LUCERNA_LOG",
    "RUST_LOG",
];

/// Variables reported as "set" / absent only, because their values are not useful and may be long.
const PRESENCE_ONLY: &[&str] = &["WAYLAND_DISPLAY", "DBUS_SESSION_BUS_ADDRESS"];

/// Collect the allow-listed environment. `get` reads one variable.
pub fn allowed_environment(
    get: impl Fn(&str) -> Option<String>,
) -> BTreeMap<String, Option<String>> {
    ENV_ALLOW_LIST
        .iter()
        .map(|&key| {
            let value = get(key).filter(|v| !v.is_empty()).map(|v| {
                if PRESENCE_ONLY.contains(&key) {
                    "set".to_owned()
                } else {
                    v
                }
            });
            (key.to_owned(), value)
        })
        .collect()
}

#[derive(Clone, Debug, Serialize)]
pub struct MpvReport {
    pub found: bool,
    pub path: Option<String>,
    pub version: Option<String>,
    /// Does the installed mpv accept every option Lucerna can pass it?
    pub options_compatible: Option<bool>,
    pub compat_detail: Option<String>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ConfigReport {
    pub file: String,
    pub exists: bool,
    pub schema_version: Option<i64>,
    pub current_schema_version: u32,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct AutostartReport {
    pub file: String,
    /// `enabled`, `disabled` or `absent`.
    pub state: String,
    pub exec: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct DaemonSection {
    pub reachable: bool,
    pub error: Option<String>,
    /// The daemon's own report (`GetDiagnostics`), when reachable.
    pub diagnostics: Option<Value>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Report {
    pub report_version: u32,
    /// RFC 3339 UTC.
    pub generated: String,
    pub lucerna_version: String,
    pub environment: BTreeMap<String, Option<String>>,
    pub paths: BTreeMap<String, Option<String>>,
    pub config: ConfigReport,
    pub mpv: MpvReport,
    pub autostart: AutostartReport,
    pub daemon: DaemonSection,
    /// A read-only X11 probe, present when the daemon could not be asked.
    pub x11_probe: Option<Value>,
}

/// Who and where, for redaction.
#[derive(Clone, Debug, Default)]
pub struct RedactContext {
    pub home: Option<String>,
    pub user: Option<String>,
    pub host: Option<String>,
}

impl RedactContext {
    /// From the process environment and `/proc/sys/kernel/hostname`.
    pub fn from_env() -> Self {
        Self {
            home: std::env::var("HOME").ok().filter(|h| h.len() > 1),
            user: std::env::var("USER")
                .or_else(|_| std::env::var("LOGNAME"))
                .ok()
                .filter(|u| !u.is_empty()),
            host: std::fs::read_to_string("/proc/sys/kernel/hostname")
                .ok()
                .map(|h| h.trim().to_owned())
                .filter(|h| !h.is_empty()),
        }
    }
}

/// Keys whose values are dropped entirely.
const SECRET_KEYS: &[&str] = &["serial", "edid_serial"];
/// Keys holding a media file path.
const MEDIA_KEYS: &[&str] = &["path", "media", "wallpaper_path"];

/// Replace `token` in `text` only where it is delimited by non-alphanumeric characters.
fn replace_token(text: &str, token: &str, replacement: &str) -> String {
    if token.is_empty() {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(index) = rest.find(token) {
        let before = rest[..index].chars().next_back();
        let after = rest[index + token.len()..].chars().next();
        let boundary = |c: Option<char>| c.is_none_or(|c| !c.is_alphanumeric());
        out.push_str(&rest[..index]);
        if boundary(before) && boundary(after) {
            out.push_str(replacement);
        } else {
            out.push_str(token);
        }
        rest = &rest[index + token.len()..];
    }
    out.push_str(rest);
    out
}

fn scrub(text: &str, ctx: &RedactContext) -> String {
    let mut out = text.to_owned();
    if let Some(home) = &ctx.home {
        out = out.replace(home.as_str(), "~");
    }
    if let Some(user) = &ctx.user {
        out = replace_token(&out, user, "<user>");
    }
    if let Some(host) = &ctx.host {
        out = replace_token(&out, host, "<host>");
    }
    out
}

/// Redact `value` in place so a report can be posted publicly.
///
/// The home directory becomes `~`, the user name `<user>`, the host name `<host>`, EDID serial
/// numbers are removed, and wallpaper paths (and the names derived from them) become
/// `<media-N>.<ext>`, numbered consistently across the report.
pub fn redact(value: &mut Value, ctx: &RedactContext) {
    let mut media: Vec<String> = Vec::new();
    redact_value(value, ctx, &mut media);
}

fn redact_value(value: &mut Value, ctx: &RedactContext, media: &mut Vec<String>) {
    match value {
        Value::String(s) => *s = scrub(s, ctx),
        Value::Array(items) => items.iter_mut().for_each(|v| redact_value(v, ctx, media)),
        Value::Object(map) => {
            for key in SECRET_KEYS {
                map.remove(*key);
            }
            // Number a media path (and its derived name) before generic scrubbing changes it.
            let label = MEDIA_KEYS
                .iter()
                .find_map(|k| {
                    map.get(*k)
                        .and_then(Value::as_str)
                        .filter(|s| s.starts_with('/'))
                })
                .map(|path| {
                    let index = media.iter().position(|p| p == path).unwrap_or_else(|| {
                        media.push(path.to_owned());
                        media.len() - 1
                    });
                    let ext = Path::new(path).extension().and_then(|e| e.to_str());
                    (index + 1, ext.map(str::to_owned))
                });
            if let Some((n, ext)) = label {
                let file =
                    ext.map_or_else(|| format!("<media-{n}>"), |e| format!("<media-{n}>.{e}"));
                for key in MEDIA_KEYS {
                    if map.get(*key).is_some_and(Value::is_string) {
                        map.insert((*key).to_owned(), Value::String(file.clone()));
                    }
                }
                if map.get("name").is_some_and(Value::is_string) {
                    map.insert("name".to_owned(), Value::String(format!("<media-{n}>")));
                }
            }
            for v in map.values_mut() {
                redact_value(v, ctx, media);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ctx() -> RedactContext {
        RedactContext {
            home: Some("/home/alice".into()),
            user: Some("alice".into()),
            host: Some("thinkpad".into()),
        }
    }

    #[test]
    fn environment_is_an_allow_list_and_hides_long_values() {
        let env = allowed_environment(|k| match k {
            "XDG_SESSION_TYPE" => Some("x11".into()),
            "DBUS_SESSION_BUS_ADDRESS" => Some("unix:path=/run/user/1000/bus".into()),
            "WAYLAND_DISPLAY" => Some("wayland-0".into()),
            "AWS_SECRET_ACCESS_KEY" => Some("hunter2".into()),
            "DISPLAY" => Some(String::new()),
            _ => None,
        });
        assert_eq!(env.len(), ENV_ALLOW_LIST.len());
        assert_eq!(env["XDG_SESSION_TYPE"].as_deref(), Some("x11"));
        assert_eq!(
            env["DBUS_SESSION_BUS_ADDRESS"].as_deref(),
            Some("set"),
            "presence only"
        );
        assert_eq!(env["WAYLAND_DISPLAY"].as_deref(), Some("set"));
        assert_eq!(env["DISPLAY"], None, "empty counts as unset");
        assert!(
            !env.contains_key("AWS_SECRET_ACCESS_KEY"),
            "never dumped whole"
        );
    }

    #[test]
    fn home_user_and_host_are_scrubbed_with_word_boundaries() {
        let mut v = json!({
            "config": "/home/alice/.config/lucerna/config.toml",
            "who": "alice on thinkpad",
            "unrelated": "malice and thinkpadx stay",
            "nested": ["/home/alice/x", {"deep": "alice@thinkpad"}]
        });
        redact(&mut v, &ctx());
        assert_eq!(v["config"], "~/.config/lucerna/config.toml");
        assert_eq!(v["who"], "<user> on <host>");
        assert_eq!(v["unrelated"], "malice and thinkpadx stay");
        assert_eq!(v["nested"][0], "~/x");
        assert_eq!(v["nested"][1]["deep"], "<user>@<host>");
    }

    #[test]
    fn wallpaper_paths_become_numbered_media_labels_consistently() {
        let mut v = json!({
            "wallpapers": [
                {"id": "1", "name": "my private holiday", "path": "/home/alice/Videos/holiday.mp4"},
                {"id": "2", "name": "rain", "path": "/mnt/media/rain.webm"}
            ],
            "renderers": [{"media": "/home/alice/Videos/holiday.mp4", "state": "playing"}]
        });
        redact(&mut v, &ctx());
        assert_eq!(v["wallpapers"][0]["path"], "<media-1>.mp4");
        assert_eq!(v["wallpapers"][0]["name"], "<media-1>");
        assert_eq!(v["wallpapers"][1]["path"], "<media-2>.webm");
        assert_eq!(
            v["renderers"][0]["media"], "<media-1>.mp4",
            "the same file gets the same label"
        );
        let text = v.to_string();
        assert!(
            !text.contains("holiday") && !text.contains("alice"),
            "{text}"
        );
    }

    #[test]
    fn edid_serials_are_removed() {
        let mut v = json!({"outputs": [{"connector": "DP-1", "edid": {"manufacturer": "DEL", "serial": "7XJ2K3", "model_name": "U2720Q"}}], "edid_serial": "X"});
        redact(&mut v, &ctx());
        assert!(v["outputs"][0]["edid"].get("serial").is_none());
        assert!(v.get("edid_serial").is_none());
        assert_eq!(
            v["outputs"][0]["edid"]["manufacturer"], "DEL",
            "the rest is kept for debugging"
        );
    }

    #[test]
    fn empty_context_changes_nothing_but_secrets() {
        let mut v = json!({"a": "/home/alice/x", "serial": "s"});
        redact(&mut v, &RedactContext::default());
        assert_eq!(v, json!({"a": "/home/alice/x"}));
    }

    #[test]
    fn a_report_serialises_with_a_version() {
        let report = Report {
            report_version: REPORT_VERSION,
            generated: "2026-09-30T10:00:00Z".into(),
            lucerna_version: "0.3.0".into(),
            environment: BTreeMap::new(),
            paths: BTreeMap::new(),
            config: ConfigReport {
                file: "/c".into(),
                exists: false,
                schema_version: None,
                current_schema_version: 1,
                warnings: vec![],
            },
            mpv: MpvReport {
                found: false,
                path: None,
                version: None,
                options_compatible: None,
                compat_detail: None,
                error: Some("nope".into()),
            },
            autostart: AutostartReport {
                file: "/a".into(),
                state: "absent".into(),
                exec: None,
            },
            daemon: DaemonSection {
                reachable: false,
                error: Some("down".into()),
                diagnostics: None,
            },
            x11_probe: None,
        };
        let v = serde_json::to_value(&report).unwrap();
        assert_eq!(v["report_version"], 1);
        assert_eq!(v["mpv"]["found"], false);
        assert_eq!(v["daemon"]["reachable"], false);
    }
}
