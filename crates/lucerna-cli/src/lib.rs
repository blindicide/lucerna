//! `lucernactl`: the Lucerna command-line controller (directive §20).
//!
//! A thin D-Bus client. It never implements a second wallpaper engine, and `doctor` works even
//! when the GUI cannot start and the daemon is not running.

mod args;
mod client;
mod doctor;
mod output;

use std::process::ExitCode;

use clap::Parser;
use lucerna_ipc::dto::{DisplayDto, StatusDto, WallpaperDto};
use lucerna_ipc::error::exit;
use lucerna_ipc::names::ALL_DISPLAYS;

use crate::args::{Cli, Command};
use crate::client::{CliError, Client, resolve_monitor};

fn print_json<T: serde::Serialize>(value: &T) -> Result<(), CliError> {
    let text = serde_json::to_string_pretty(value)
        .map_err(|e| CliError::new(exit::INTERNAL, e.to_string()))?;
    println!("{text}");
    Ok(())
}

fn run(cli: Cli) -> Result<(), CliError> {
    match cli.command {
        Command::Doctor(args) => {
            let report = doctor::collect();
            let json = doctor::to_json(&report, args.redact);
            if args.json {
                print_json(&json)
            } else {
                print!("{}", doctor::render_text(&json));
                Ok(())
            }
        }
        Command::Status(flag) => {
            let client = Client::connect()?;
            let status = StatusDto::from_dict(&client.proxy.get_status()?);
            if flag.json {
                print_json(&status)
            } else {
                print!("{}", output::format_status(&status));
                Ok(())
            }
        }
        Command::Monitors(flag) => {
            let client = Client::connect()?;
            let displays: Vec<DisplayDto> = client
                .proxy
                .get_displays()?
                .iter()
                .map(DisplayDto::from_dict)
                .collect();
            if flag.json {
                print_json(&displays)
            } else {
                print!("{}", output::format_monitors(&displays));
                Ok(())
            }
        }
        Command::Wallpapers(flag) => {
            let client = Client::connect()?;
            let wallpapers: Vec<WallpaperDto> = client
                .proxy
                .list_wallpapers()?
                .iter()
                .map(WallpaperDto::from_dict)
                .collect();
            if flag.json {
                print_json(&wallpapers)
            } else {
                print!("{}", output::format_wallpapers(&wallpapers));
                Ok(())
            }
        }
        Command::Play(args) => {
            let client = Client::connect()?;
            let path = std::path::absolute(&args.path).map_err(|e| {
                CliError::new(
                    exit::BAD_REQUEST,
                    format!("Lucerna could not resolve {}: {e}", args.path.display()),
                )
            })?;
            let display = match &args.monitor {
                Some(wanted) => {
                    let displays: Vec<DisplayDto> = client
                        .proxy
                        .get_displays()?
                        .iter()
                        .map(DisplayDto::from_dict)
                        .collect();
                    resolve_monitor(&displays, wanted)?
                }
                None => ALL_DISPLAYS.to_owned(),
            };
            let id = client.proxy.add_wallpaper(&path.to_string_lossy(), "")?;
            client.proxy.set_wallpaper(&id, &display)?;
            let target = args.monitor.as_deref().unwrap_or("all monitors");
            println!("Playing {} on {target}.", path.display());
            Ok(())
        }
        Command::Pause => {
            Client::connect()?.proxy.pause()?;
            println!("Paused.");
            Ok(())
        }
        Command::Resume => {
            Client::connect()?.proxy.resume()?;
            println!("Resumed.");
            Ok(())
        }
        Command::Stop(args) => {
            let client = Client::connect()?;
            if args.daemon {
                client.proxy.quit()?;
                println!("Asked the Lucerna daemon to quit.");
            } else {
                client.proxy.stop()?;
                println!(
                    "Stopped. Use `lucernactl play <file>` or `lucernactl reload` to start again."
                );
            }
            Ok(())
        }
        Command::Reload => {
            Client::connect()?.proxy.reload()?;
            println!("Reloaded.");
            Ok(())
        }
    }
}

/// Entry point shared by `lucernactl` and the testkit wrapper.
pub fn cli_main() -> ExitCode {
    let cli = Cli::parse();
    lucerna_core::logging::init("lucernactl", cli.verbose);
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("lucernactl: {}", err.message);
            ExitCode::from(err.code)
        }
    }
}
