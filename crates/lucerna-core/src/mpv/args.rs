//! The mpv argument vector (directive §9, §16, §17, §26).
//!
//! `build_args` is a pure function. The media path is always a single argument placed after
//! `--`, so a file called `$(rm -rf ~).mp4` is simply a filename.

use std::ffi::OsString;
use std::path::PathBuf;

use crate::backend::EmbedTarget;
use crate::types::{FpsLimit, HwDecode, ScalingMode};

/// Every option name `build_args` can emit (without leading dashes, and without a `no-` prefix
/// for negated flags). Used by the fake mpv and by the compatibility probe, so the builder and
/// its test double cannot drift apart.
pub const EMITTED_OPTIONS: &[&str] = &[
    "config",
    "input-terminal",
    "quiet",
    "msg-level",
    "msg-color",
    "input-default-bindings",
    "input-vo-keyboard",
    "input-cursor",
    "cursor-autohide",
    "osc",
    "osd-level",
    "load-scripts",
    "load-stats-overlay",
    "load-osd-console",
    "load-auto-profiles",
    "ytdl",
    "resume-playback",
    "stop-screensaver",
    "x11-bypass-compositor",
    "input-media-keys",
    "loop-file",
    "idle",
    "force-window",
    "aid",
    "mute",
    "volume",
    "hwdec",
    "vf",
    "keepaspect",
    "panscan",
    "video-unscaled",
    "pause",
    "input-ipc-server",
    "wid",
    "vo",
];

/// Everything needed to launch one renderer.
#[derive(Clone, Debug)]
pub struct RenderSpec {
    /// Window to draw into, or `None` (tests, `--vo=null`).
    pub embed: Option<EmbedTarget>,
    /// Private IPC socket path under the runtime directory.
    pub ipc_socket: PathBuf,
    /// Absolute, canonical media path.
    pub media: PathBuf,
    pub scaling: ScalingMode,
    pub hwdec: HwDecode,
    pub fps: FpsLimit,
    pub audio: bool,
    pub start_paused: bool,
    /// Test builds only (`null` or `x11`). Never set in production.
    pub vo_override: Option<String>,
}

/// Build the complete argument vector (excluding the program name).
pub fn build_args(spec: &RenderSpec) -> Vec<OsString> {
    let mut args: Vec<OsString> = Vec::with_capacity(48);
    let mut push = |s: &str| args.push(OsString::from(s));

    // Ignore the user's mpv.conf, input.conf and scripts (§16).
    push("--no-config");
    push("--no-input-terminal");
    push("--quiet");
    // Keep warnings and errors on stderr for the bounded diagnostic log.
    push("--msg-level=all=warn");
    push("--msg-color=no");
    push("--no-input-default-bindings");
    push("--input-vo-keyboard=no");
    push("--input-cursor=no");
    push("--cursor-autohide=no");
    push("--osc=no");
    push("--osd-level=0");
    push("--load-scripts=no");
    push("--load-stats-overlay=no");
    push("--load-osd-console=no");
    push("--load-auto-profiles=no");
    // No network helpers (§26) and no inherited playback state.
    push("--ytdl=no");
    push("--resume-playback=no");
    // A wallpaper must not keep the screen from locking or blanking.
    push("--stop-screensaver=no");
    push("--x11-bypass-compositor=never");
    push("--input-media-keys=no");
    push("--loop-file=inf");
    // Exit if the file ends or fails so the supervisor can classify it.
    push("--idle=no");
    push("--force-window=no");

    if spec.audio {
        push("--aid=auto");
        push("--mute=no");
        push("--volume=100");
    } else {
        push("--aid=no");
        push("--mute=yes");
    }

    match spec.hwdec {
        HwDecode::Auto => push("--hwdec=auto-safe"),
        HwDecode::Disabled => push("--hwdec=no"),
    }

    if let Some(filter) = spec.fps.video_filter() {
        args.push(OsString::from(format!("--vf={filter}")));
    }

    for (name, value) in spec.scaling.mpv_properties() {
        args.push(OsString::from(format!("--{name}={value}")));
    }

    args.push(OsString::from(if spec.start_paused {
        "--pause=yes"
    } else {
        "--pause=no"
    }));

    let mut ipc = OsString::from("--input-ipc-server=");
    ipc.push(&spec.ipc_socket);
    args.push(ipc);

    if let Some(EmbedTarget::X11Window(xid)) = spec.embed {
        args.push(OsString::from(format!("--wid={xid}")));
    }
    if let Some(vo) = &spec.vo_override {
        args.push(OsString::from(format!("--vo={vo}")));
    }

    // End of options: nothing after this is parsed as an option.
    args.push(OsString::from("--"));
    args.push(spec.media.clone().into_os_string());
    args
}

/// The canonical option name of a `--name[=value]` argument, with a `no-` prefix removed when the
/// remainder is a known option. Returns `None` for arguments that are not options.
pub fn option_name(arg: &str) -> Option<String> {
    let body = arg.strip_prefix("--")?;
    if body.is_empty() {
        return None;
    }
    let name = body.split('=').next().unwrap_or(body);
    if let Some(stripped) = name.strip_prefix("no-")
        && EMITTED_OPTIONS.contains(&stripped)
    {
        return Some(stripped.to_owned());
    }
    Some(name.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::ffi::OsStr;

    fn spec() -> RenderSpec {
        RenderSpec {
            embed: Some(EmbedTarget::X11Window(0x0040_0007)),
            ipc_socket: PathBuf::from("/run/user/1000/lucerna/mpv-abcd1234-3.sock"),
            media: PathBuf::from("/home/u/Videos/rain.webm"),
            scaling: ScalingMode::Fill,
            hwdec: HwDecode::Auto,
            fps: FpsLimit::Native,
            audio: false,
            start_paused: false,
            vo_override: None,
        }
    }

    fn strings(args: &[OsString]) -> Vec<String> {
        args.iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn default_vector_is_exactly_the_documented_one() {
        let got = strings(&build_args(&spec()));
        let want = [
            "--no-config",
            "--no-input-terminal",
            "--quiet",
            "--msg-level=all=warn",
            "--msg-color=no",
            "--no-input-default-bindings",
            "--input-vo-keyboard=no",
            "--input-cursor=no",
            "--cursor-autohide=no",
            "--osc=no",
            "--osd-level=0",
            "--load-scripts=no",
            "--load-stats-overlay=no",
            "--load-osd-console=no",
            "--load-auto-profiles=no",
            "--ytdl=no",
            "--resume-playback=no",
            "--stop-screensaver=no",
            "--x11-bypass-compositor=never",
            "--input-media-keys=no",
            "--loop-file=inf",
            "--idle=no",
            "--force-window=no",
            "--aid=no",
            "--mute=yes",
            "--hwdec=auto-safe",
            "--keepaspect=yes",
            "--panscan=1.0",
            "--video-unscaled=no",
            "--pause=no",
            "--input-ipc-server=/run/user/1000/lucerna/mpv-abcd1234-3.sock",
            "--wid=4194311",
            "--",
            "/home/u/Videos/rain.webm",
        ];
        assert_eq!(got, want);
    }

    #[test]
    fn audio_is_muted_by_default_and_explicit_when_enabled() {
        let off = strings(&build_args(&spec()));
        assert!(off.contains(&"--aid=no".to_owned()) && off.contains(&"--mute=yes".to_owned()));
        assert!(!off.iter().any(|a| a.starts_with("--volume")));

        let mut s = spec();
        s.audio = true;
        let on = strings(&build_args(&s));
        for want in ["--aid=auto", "--mute=no", "--volume=100"] {
            assert!(on.contains(&want.to_owned()), "{want}");
        }
        assert!(!on.contains(&"--mute=yes".to_owned()));
    }

    #[test]
    fn hwdec_fps_pause_and_vo_options() {
        let mut s = spec();
        s.hwdec = HwDecode::Disabled;
        s.fps = FpsLimit::Fps30;
        s.start_paused = true;
        s.vo_override = Some("null".into());
        s.embed = None;
        let got = strings(&build_args(&s));
        assert!(got.contains(&"--hwdec=no".to_owned()));
        assert!(got.contains(&"--vf=fps=30".to_owned()));
        assert!(got.contains(&"--pause=yes".to_owned()));
        assert!(got.contains(&"--vo=null".to_owned()));
        assert!(!got.iter().any(|a| a.starts_with("--wid")));
    }

    #[test]
    fn malicious_file_name_is_a_single_trailing_argument() {
        let mut s = spec();
        s.media = PathBuf::from("/home/u/$(rm -rf ~) ; `id` && echo *.mp4");
        let args = build_args(&s);
        let n = args.len();
        assert_eq!(args[n - 2], OsStr::new("--"));
        assert_eq!(
            args[n - 1],
            OsStr::new("/home/u/$(rm -rf ~) ; `id` && echo *.mp4")
        );
        assert_eq!(
            args.iter()
                .filter(|a| a.to_string_lossy().contains("rm -rf"))
                .count(),
            1
        );
    }

    #[test]
    fn media_path_starting_with_dashes_cannot_become_an_option() {
        let mut s = spec();
        s.media = PathBuf::from("--vo=gpu.mp4");
        let args = build_args(&s);
        let sep = args.iter().position(|a| a == "--").unwrap();
        assert_eq!(sep, args.len() - 2);
    }

    #[test]
    fn every_combination_only_emits_known_options() {
        let known: BTreeSet<&str> = EMITTED_OPTIONS.iter().copied().collect();
        let mut seen: BTreeSet<String> = BTreeSet::new();
        for audio in [false, true] {
            for hwdec in [HwDecode::Auto, HwDecode::Disabled] {
                for fps in [
                    FpsLimit::Native,
                    FpsLimit::Fps60,
                    FpsLimit::Fps30,
                    FpsLimit::Fps15,
                ] {
                    for scaling in ScalingMode::ALL {
                        for paused in [false, true] {
                            for embed in [None, Some(EmbedTarget::X11Window(1))] {
                                for vo in [None, Some("null".to_owned())] {
                                    let s = RenderSpec {
                                        embed,
                                        audio,
                                        hwdec,
                                        fps,
                                        scaling,
                                        start_paused: paused,
                                        vo_override: vo,
                                        ..spec()
                                    };
                                    let args = strings(&build_args(&s));
                                    let sep = args.iter().position(|a| a == "--").unwrap();
                                    assert_eq!(sep, args.len() - 2);
                                    assert_eq!(
                                        args.iter().filter(|a| a.starts_with("--pause=")).count(),
                                        1
                                    );
                                    for a in &args[..sep] {
                                        let name = option_name(a).unwrap_or_else(|| panic!("{a}"));
                                        assert!(known.contains(name.as_str()), "unknown {name}");
                                        seen.insert(name);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        let missing: Vec<_> = known.iter().filter(|k| !seen.contains(**k)).collect();
        assert!(
            missing.is_empty(),
            "EMITTED_OPTIONS lists options never emitted: {missing:?}"
        );
    }

    #[test]
    fn scaling_modes_emit_their_documented_properties() {
        for mode in ScalingMode::ALL {
            let got = strings(&build_args(&RenderSpec {
                scaling: mode,
                ..spec()
            }));
            for (name, value) in mode.mpv_properties() {
                assert!(got.contains(&format!("--{name}={value}")), "{mode}: {name}");
            }
        }
    }

    #[test]
    fn option_name_handles_negation_and_values() {
        assert_eq!(option_name("--no-config").as_deref(), Some("config"));
        assert_eq!(
            option_name("--no-input-terminal").as_deref(),
            Some("input-terminal")
        );
        assert_eq!(option_name("--osc=no").as_deref(), Some("osc"));
        assert_eq!(
            option_name("--msg-level=all=warn").as_deref(),
            Some("msg-level")
        );
        assert_eq!(option_name("--").as_deref(), None);
        assert_eq!(option_name("/media/file.mp4").as_deref(), None);
    }
}
