//! `lucerna`: the GTK 4 control application.
//!
//! Command-line parsing and the display check happen *before* GTK is
//! initialised, so `--help` and `--version` work on a headless machine
//! (directive §24, §34).

mod display;
mod strings;

use std::process::ExitCode;

use clap::Parser;
use gtk4::prelude::*;
use lucerna_core::version::{APP_ID, DESCRIPTION, VERSION};

/// Command line of `lucerna`.
#[derive(Debug, Parser)]
#[command(name = "lucerna", version = VERSION, about = format!("Lucerna control application. {DESCRIPTION}."))]
pub struct Args {
    /// Log at debug level.
    #[arg(short, long)]
    pub verbose: bool,
}

/// Entry point of the `lucerna` binary.
pub fn cli_main() -> ExitCode {
    let args = Args::parse();
    lucerna_core::logging::init("lucerna", args.verbose);

    if !display::graphical_session_available(|k| std::env::var_os(k)) {
        eprintln!("{}", strings::NO_DISPLAY);
        return ExitCode::FAILURE;
    }
    if let Err(err) = gtk4::init() {
        eprintln!("{}\nGTK reported: {err}", strings::NO_DISPLAY);
        return ExitCode::FAILURE;
    }

    let app = gtk4::Application::builder().application_id(APP_ID).build();
    app.connect_activate(|app| {
        let window = gtk4::ApplicationWindow::builder()
            .application(app)
            .title(strings::WINDOW_TITLE)
            .default_width(900)
            .default_height(600)
            .build();
        window.present();
    });
    // GTK must not re-parse our command line.
    app.run_with_args::<&str>(&[]).into()
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
        let err = Args::try_parse_from(["lucerna", "--version"]).unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::DisplayVersion);
        assert!(err.to_string().contains(VERSION));
    }
}
