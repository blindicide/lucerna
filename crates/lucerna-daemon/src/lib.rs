//! `lucernad`: the per-user Lucerna wallpaper service.
//!
//! All real code lives in this library so the testkit wrapper binary exercises
//! exactly what ships.

use std::process::ExitCode;

use clap::Parser;
use lucerna_core::version::{DESCRIPTION, VERSION};

/// Command line of `lucernad`.
#[derive(Debug, Parser)]
#[command(name = "lucernad", version = VERSION, about = format!("Lucerna wallpaper service. {DESCRIPTION}."))]
pub struct Args {
    /// Log at debug level.
    #[arg(short, long)]
    pub verbose: bool,
}

/// Entry point shared by `lucernad` and the testkit wrapper.
pub fn cli_main() -> ExitCode {
    let args = Args::parse();
    lucerna_core::logging::init("lucernad", args.verbose);
    tracing::info!(version = VERSION, "lucernad starting");
    // The service loop arrives with the daemon milestone (v0.3.0).
    tracing::info!("nothing to do yet; exiting");
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn command_is_well_formed() {
        Args::command().debug_assert();
    }

    #[test]
    fn version_flag_reports_workspace_version() {
        let err = Args::try_parse_from(["lucernad", "--version"]).unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::DisplayVersion);
        assert!(err.to_string().contains(VERSION));
    }

    #[test]
    fn verbose_flag_parses() {
        let a = Args::try_parse_from(["lucernad", "-v"]).unwrap();
        assert!(a.verbose);
    }
}
