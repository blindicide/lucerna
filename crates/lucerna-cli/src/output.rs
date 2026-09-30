//! Human-readable output. Pure functions from DTOs to text, so they are unit-testable.

use std::fmt::Write as _;

use lucerna_ipc::dto::{DisplayDto, StatusDto, WallpaperDto};

fn yes(v: bool) -> &'static str {
    if v { "yes" } else { "no" }
}

pub fn format_status(s: &StatusDto) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "Lucerna daemon {} (API {})",
        s.daemon_version, s.api_version
    );
    let _ = writeln!(out, "Backend:   {}", s.backend);
    let _ = writeln!(out, "Session:   {}", s.session_type);
    let _ = writeln!(out, "Playback:  {}", s.playback);
    if !s.supported {
        let _ = writeln!(
            out,
            "\nWallpaper playback is not available in this session ({}):",
            s.unsupported_reason
        );
        for line in s.unsupported_message.lines() {
            let _ = writeln!(out, "  {line}");
        }
    }
    let _ = writeln!(
        out,
        "Paused by you: {}   Stopped by you: {}   Screen locked: {}",
        yes(s.user_paused),
        yes(s.user_stopped),
        yes(s.session_locked)
    );
    let _ = writeln!(
        out,
        "mpv:       {}",
        if s.mpv_available {
            format!("found (version {})", s.mpv_version)
        } else {
            "NOT FOUND".to_owned()
        }
    );
    let _ = writeln!(
        out,
        "Detection: fullscreen {}, screen lock {}",
        yes(s.fullscreen_detection),
        yes(s.lock_detection)
    );
    let _ = writeln!(
        out,
        "Config:    {} (schema {})",
        s.config_state, s.schema_version
    );
    if !s.config_notice.is_empty() {
        let _ = writeln!(out, "           note: {}", s.config_notice);
    }
    for warning in &s.config_warnings {
        let _ = writeln!(out, "           warning: {warning}");
    }
    if s.renderers.is_empty() {
        let _ = writeln!(out, "\nNo wallpapers are assigned to a connected display.");
    } else {
        let _ = writeln!(out, "\nRenderers:");
        for r in &s.renderers {
            let mut line = format!("  {} — {}", r.connector, r.state);
            if !r.pause_reasons.is_empty() {
                let _ = write!(line, " (paused: {})", r.pause_reasons.join(", "));
            }
            let _ = write!(
                line,
                " — wallpaper {} — scaling {}",
                r.wallpaper_id, r.scaling
            );
            if r.pid != 0 {
                let _ = write!(line, " — pid {}", r.pid);
            }
            if r.restarts_in_window > 0 {
                let _ = write!(line, " — {} restart(s) recently", r.restarts_in_window);
            }
            let _ = writeln!(out, "{line}");
            for detail in r.failure_message.lines() {
                let _ = writeln!(out, "      {detail}");
            }
        }
    }
    out
}

pub fn format_monitors(displays: &[DisplayDto]) -> String {
    if displays.is_empty() {
        return "No monitors were detected.\n".to_owned();
    }
    let mut out = String::new();
    for d in displays {
        let mut line = format!("{}\n    id: {}", d.label, d.id);
        if d.connected {
            let _ = write!(
                line,
                "   position: {},{}   rotation: {}",
                d.x, d.y, d.rotation
            );
        }
        let wallpaper = if d.wallpaper_id.is_empty() {
            "none".to_owned()
        } else {
            format!("{} ({})", d.wallpaper_id, d.wallpaper_source)
        };
        let _ = write!(
            line,
            "\n    wallpaper: {wallpaper}   scaling: {}",
            d.scaling
        );
        let _ = writeln!(out, "{line}");
    }
    out
}

pub fn format_wallpapers(wallpapers: &[WallpaperDto]) -> String {
    if wallpapers.is_empty() {
        return "The library is empty. Add a wallpaper with `lucernactl play <file>`.\n".to_owned();
    }
    let mut out = String::new();
    for w in wallpapers {
        let missing = if w.available { "" } else { "  [MISSING]" };
        let _ = writeln!(out, "{}  {}{missing}\n    {}", w.id, w.name, w.path);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use lucerna_ipc::dto::RendererDto;

    fn status() -> StatusDto {
        StatusDto {
            daemon_version: "0.3.0".into(),
            api_version: 1,
            backend: "cinnamon-x11".into(),
            session_type: "x11".into(),
            supported: true,
            playback: "failed".into(),
            mpv_available: true,
            mpv_version: "0.37.0".into(),
            schema_version: 1,
            config_state: "ok".into(),
            renderers: vec![RendererDto {
                connector: "HDMI-1".into(),
                state: "failed".into(),
                wallpaper_id: "w1".into(),
                scaling: "fill".into(),
                pause_reasons: vec!["user".into()],
                failure_message: "The wallpaper file is missing.\nRestore the file.".into(),
                restarts_in_window: 2,
                ..RendererDto::default()
            }],
            ..StatusDto::default()
        }
    }

    #[test]
    fn status_text_shows_failures_with_their_full_explanation() {
        let text = format_status(&status());
        assert!(text.contains("Lucerna daemon 0.3.0 (API 1)"));
        assert!(text.contains(
            "HDMI-1 — failed (paused: user) — wallpaper w1 — scaling fill — 2 restart(s) recently"
        ));
        assert!(text.contains("      The wallpaper file is missing.\n      Restore the file."));
        assert!(text.contains("mpv:       found (version 0.37.0)"));
    }

    #[test]
    fn unsupported_sessions_are_explained_in_status() {
        let mut s = status();
        s.supported = false;
        s.backend = "none".into();
        s.unsupported_reason = "wayland".into();
        s.unsupported_message =
            "Lucerna could not start wallpaper playback.\nLog in with an X11 session.".into();
        s.renderers.clear();
        let text = format_status(&s);
        assert!(text.contains("not available in this session (wayland)"));
        assert!(text.contains("  Log in with an X11 session."));
        assert!(text.contains("No wallpapers are assigned"));
    }

    #[test]
    fn missing_mpv_is_loud() {
        let mut s = status();
        s.mpv_available = false;
        assert!(format_status(&s).contains("NOT FOUND"));
    }

    #[test]
    fn monitors_and_wallpapers_render_sensibly_including_empty_cases() {
        assert!(format_monitors(&[]).contains("No monitors"));
        assert!(format_wallpapers(&[]).contains("library is empty"));
        let d = DisplayDto {
            id: "conn:HDMI-1".into(),
            label: "HDMI-1 — 1920×1080".into(),
            connected: true,
            rotation: "normal".into(),
            wallpaper_id: "w1".into(),
            wallpaper_source: "all".into(),
            scaling: "fill".into(),
            ..DisplayDto::default()
        };
        let text = format_monitors(&[d]);
        assert!(
            text.contains("HDMI-1 — 1920×1080")
                && text.contains("id: conn:HDMI-1")
                && text.contains("w1 (all)")
        );
        let w = WallpaperDto {
            id: "w1".into(),
            name: "Rain".into(),
            path: "/v/rain.webm".into(),
            available: false,
            ..WallpaperDto::default()
        };
        let text = format_wallpapers(&[w]);
        assert!(text.contains("[MISSING]") && text.contains("/v/rain.webm"));
    }
}
