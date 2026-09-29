//! `lucernactl`: the Lucerna command-line controller.

use std::process::ExitCode;

use clap::Parser;
use lucerna_core::version::{DESCRIPTION, VERSION};

/// Command line of `lucernactl`.
#[derive(Debug, Parser)]
#[command(name = "lucernactl", version = VERSION, about = format!("Control the Lucerna wallpaper service. {DESCRIPTION}."))]
pub struct Cli {
    /// Log at debug level.
    #[arg(short, long)]
    pub verbose: bool,
}

/// Entry point shared by `lucernactl` and the testkit wrapper.
pub fn cli_main() -> ExitCode {
    let cli = Cli::parse();
    lucerna_core::logging::init("lucernactl", cli.verbose);
    // Sub-commands arrive with the daemon and CLI milestone (v0.3.0).
    println!("lucernactl {VERSION}: no commands are available yet in this build.");
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn command_is_well_formed() {
        Cli::command().debug_assert();
    }

    #[test]
    fn version_flag_reports_workspace_version() {
        let err = Cli::try_parse_from(["lucernactl", "--version"]).unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::DisplayVersion);
        assert!(err.to_string().contains(VERSION));
    }
}
