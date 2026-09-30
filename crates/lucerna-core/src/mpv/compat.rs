//! Does the installed mpv accept every option Lucerna can pass? (directive §9)
//!
//! mpv exits with an error at option-parsing time when it does not know an option or its value,
//! even with `--list-options`. So the probe runs `mpv --no-config <every option we emit>
//! --list-options` and checks the exit status: no media, no display, no side effects.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use super::args::{RenderSpec, build_args};
use crate::backend::EmbedTarget;
use crate::types::{FpsLimit, HwDecode, ScalingMode};

/// Result of the compatibility probe, for `doctor` and the package smoke tests.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompatReport {
    pub compatible: bool,
    /// The first line of mpv's complaint when incompatible, else empty.
    pub detail: String,
}

/// The union of options `build_args` can emit, followed by `--list-options`.
pub fn probe_args() -> Vec<OsString> {
    let mut out: Vec<OsString> = Vec::new();
    let mut add = |args: Vec<OsString>| {
        for arg in args.into_iter().take_while(|a| a != "--") {
            if !out.contains(&arg) {
                out.push(arg);
            }
        }
    };
    let base = RenderSpec {
        embed: Some(EmbedTarget::X11Window(1)),
        ipc_socket: PathBuf::from("/nonexistent/lucerna-probe.sock"),
        media: PathBuf::from("/nonexistent/probe.mp4"),
        scaling: ScalingMode::Fill,
        hwdec: HwDecode::Auto,
        fps: FpsLimit::Fps60,
        audio: false,
        start_paused: true,
        vo_override: Some("null".to_owned()),
    };
    add(build_args(&base));
    add(build_args(&RenderSpec {
        audio: true,
        hwdec: HwDecode::Disabled,
        ..base.clone()
    }));
    for scaling in ScalingMode::ALL {
        add(build_args(&RenderSpec {
            scaling,
            start_paused: false,
            ..base.clone()
        }));
    }
    out.push(OsString::from("--list-options"));
    out
}

/// Run the probe against `mpv`.
pub fn check_compat(mpv: &Path) -> io::Result<CompatReport> {
    let mut child = super::discovery::spawn_retrying(
        Command::new(mpv)
            .args(probe_args())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped()),
    )?;
    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "mpv option probe timed out",
            ));
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    if status.success() {
        return Ok(CompatReport {
            compatible: true,
            detail: String::new(),
        });
    }
    let mut text = String::new();
    if let Some(mut stderr) = child.stderr.take() {
        use std::io::Read;
        let mut buf = Vec::new();
        let _ = stderr.by_ref().take(16 * 1024).read_to_end(&mut buf);
        text = String::from_utf8_lossy(&buf).into_owned();
    }
    let detail = text
        .lines()
        .next()
        .unwrap_or("mpv rejected the option probe")
        .trim()
        .to_owned();
    Ok(CompatReport {
        compatible: false,
        detail,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mpv::args::{EMITTED_OPTIONS, option_name};

    #[test]
    fn probe_covers_every_emitted_option_once() {
        let args = probe_args();
        let names: Vec<String> = args
            .iter()
            .filter_map(|a| option_name(&a.to_string_lossy()))
            .collect();
        for option in EMITTED_OPTIONS {
            assert!(names.iter().any(|n| n == option), "probe misses --{option}");
        }
        assert_eq!(args.last().unwrap(), "--list-options");
        assert!(
            !args.iter().any(|a| a == "--"),
            "no media separator in a probe"
        );
    }
}
