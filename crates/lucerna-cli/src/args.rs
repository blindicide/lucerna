//! Command-line definition of `lucernactl` (directive §20).

use clap::{Args, Parser, Subcommand};
use lucerna_core::version::{DESCRIPTION, VERSION};

#[derive(Debug, Parser)]
#[command(
    name = "lucernactl",
    version = VERSION,
    about = format!("Control the Lucerna wallpaper service. {DESCRIPTION}."),
    after_help = "Exit codes: 0 ok, 1 unexpected error, 2 usage error, 3 daemon not running, \
                  4 bad request (unknown wallpaper/display, bad path), 5 unsupported session, \
                  6 configuration problem, 7 mpv missing."
)]
pub struct Cli {
    /// Log at debug level.
    #[arg(short, long, global = true)]
    pub verbose: bool,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Show what the daemon is doing (exit 3 if it is not running).
    Status(JsonFlag),
    /// List the detected monitors.
    #[command(alias = "displays")]
    Monitors(JsonFlag),
    /// List the wallpaper library.
    Wallpapers(JsonFlag),
    /// Add a video or animated image to the library and play it.
    Play(PlayArgs),
    /// Pause all wallpapers.
    Pause,
    /// Resume playback after `pause`.
    Resume,
    /// Stop all wallpapers, or (with --daemon) the whole service.
    Stop(StopArgs),
    /// Re-read the configuration and start over.
    Reload,
    /// Gather everything needed to debug desktop integration.
    Doctor(DoctorArgs),
}

#[derive(Debug, Args)]
pub struct JsonFlag {
    /// Print machine-readable JSON.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct PlayArgs {
    /// The video or animated image to play.
    pub path: std::path::PathBuf,
    /// Only on this monitor (a monitor id, or a connector name such as HDMI-1).
    #[arg(long, value_name = "ID")]
    pub monitor: Option<String>,
}

#[derive(Debug, Args)]
pub struct StopArgs {
    /// Ask the daemon itself to quit instead of only stopping the wallpapers.
    #[arg(long)]
    pub daemon: bool,
}

#[derive(Debug, Args)]
pub struct DoctorArgs {
    /// Print one JSON document.
    #[arg(long)]
    pub json: bool,
    /// Hide your home directory, user name, host name, wallpaper file names and monitor serial
    /// numbers, so the report is safe to post publicly.
    #[arg(long)]
    pub redact: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
        Cli::try_parse_from(std::iter::once("lucernactl").chain(args.iter().copied()))
    }

    #[test]
    fn command_is_well_formed() {
        Cli::command().debug_assert();
    }

    #[test]
    fn every_documented_command_parses() {
        assert!(matches!(
            parse(&["status"]).unwrap().command,
            Command::Status(JsonFlag { json: false })
        ));
        assert!(matches!(
            parse(&["status", "--json"]).unwrap().command,
            Command::Status(JsonFlag { json: true })
        ));
        assert!(matches!(
            parse(&["monitors"]).unwrap().command,
            Command::Monitors(_)
        ));
        assert!(matches!(
            parse(&["displays", "--json"]).unwrap().command,
            Command::Monitors(JsonFlag { json: true })
        ));
        assert!(matches!(
            parse(&["wallpapers"]).unwrap().command,
            Command::Wallpapers(_)
        ));
        assert!(matches!(parse(&["pause"]).unwrap().command, Command::Pause));
        assert!(matches!(
            parse(&["resume"]).unwrap().command,
            Command::Resume
        ));
        assert!(matches!(
            parse(&["reload"]).unwrap().command,
            Command::Reload
        ));
        assert!(matches!(
            parse(&["stop"]).unwrap().command,
            Command::Stop(StopArgs { daemon: false })
        ));
        assert!(matches!(
            parse(&["stop", "--daemon"]).unwrap().command,
            Command::Stop(StopArgs { daemon: true })
        ));
    }

    #[test]
    fn play_takes_a_path_and_an_optional_monitor() {
        match parse(&["play", "/v/rain.webm"]).unwrap().command {
            Command::Play(p) => {
                assert_eq!(p.path, std::path::Path::new("/v/rain.webm"));
                assert_eq!(p.monitor, None);
            }
            other => panic!("{other:?}"),
        }
        match parse(&["play", "rain.webm", "--monitor", "HDMI-1"])
            .unwrap()
            .command
        {
            Command::Play(p) => assert_eq!(p.monitor.as_deref(), Some("HDMI-1")),
            other => panic!("{other:?}"),
        }
        assert!(parse(&["play"]).is_err(), "a path is required");
    }

    #[test]
    fn doctor_flags() {
        match parse(&["doctor", "--json", "--redact"]).unwrap().command {
            Command::Doctor(d) => assert!(d.json && d.redact),
            other => panic!("{other:?}"),
        }
        match parse(&["doctor"]).unwrap().command {
            Command::Doctor(d) => assert!(!d.json && !d.redact),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn usage_errors_are_clap_errors_with_exit_code_2() {
        let err = parse(&["frobnicate"]).unwrap_err();
        assert_eq!(err.exit_code(), 2);
        assert_eq!(parse(&[]).unwrap_err().exit_code(), 2);
    }

    #[test]
    fn version_and_help_are_available() {
        assert_eq!(
            parse(&["--version"]).unwrap_err().kind(),
            clap::error::ErrorKind::DisplayVersion
        );
        assert_eq!(
            parse(&["--help"]).unwrap_err().kind(),
            clap::error::ErrorKind::DisplayHelp
        );
        assert!(
            Cli::command()
                .render_long_help()
                .to_string()
                .contains("Exit codes")
        );
    }
}
