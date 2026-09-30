//! `lucernad`: the per-user Lucerna wallpaper service (directive §5).
//!
//! All real code lives in this library so the testkit's wrapper binary exercises exactly what
//! ships, and so integration tests can run the whole daemon in-process against a private bus, a
//! fake backend and a fake mpv.

mod engine;
mod lock;
mod messages;
mod options;
mod service;

use std::process::ExitCode;

use clap::Parser;
use lucerna_core::runtime::ensure_private_dir;
use lucerna_core::version::{DESCRIPTION, VERSION};
use lucerna_ipc::names::{BUS_NAME, OBJECT_PATH};
use lucerna_mpv::{RecoveryOptions, recover_stale};

use crate::engine::{Engine, EngineOpts};
use crate::lock::{DaemonLock, LockError};
use crate::messages::EngineMsg;
use crate::service::LucernaService;

pub use options::{BackendChoice, BusChoice, DaemonOptions};

/// Command line of `lucernad`.
#[derive(Debug, Parser)]
#[command(name = "lucernad", version = VERSION, about = format!("Lucerna wallpaper service. {DESCRIPTION}."))]
pub struct Args {
    /// Log at debug level.
    #[arg(short, long)]
    pub verbose: bool,
}

/// How the daemon ended.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Stopped normally (request, signal, or the display going away).
    Clean,
    /// Another daemon already owns this session; nothing was started.
    AlreadyRunning { pid: Option<u32> },
    /// Could not start; the message says why and what to do.
    Failed(String),
}

fn already_running_message(pid: Option<u32>) -> String {
    match pid {
        Some(pid) => format!(
            "Lucerna daemon is already running for this session (PID {pid}). Nothing to do."
        ),
        None => "Lucerna daemon is already running for this session. Nothing to do.".to_owned(),
    }
}

/// Run the daemon until it is asked to stop.
pub async fn run(options: DaemonOptions) -> Outcome {
    let DaemonOptions {
        mut paths,
        session,
        bus,
        backend,
        mpv_override,
        path_var,
        daemon_exe,
        display_wait,
        renderer_timings,
        vo_override,
        extra_env,
        recheck_interval,
        handle_signals,
        autostart_on_first_run,
    } = options;

    // 1. The private runtime directory (§26). Without one, renderers are refused and the reason
    //    is reported, but the daemon still serves D-Bus so clients can explain it.
    if let Some(dir) = paths.runtime_dir.clone()
        && let Err(err) = ensure_private_dir(&dir)
    {
        tracing::warn!(%err, "no usable runtime directory");
        paths.runtime_dir = None;
    }

    // 2. Single instance, part one: the flock.
    let _lock = match paths.lock_file() {
        Some(path) => match DaemonLock::acquire(&path) {
            Ok(lock) => Some(lock),
            Err(LockError::AlreadyRunning { pid }) => return Outcome::AlreadyRunning { pid },
            Err(err) => return Outcome::Failed(err.to_string()),
        },
        None => None,
    };

    // 3. Single instance, part two: the bus name. Serve the interface at the same time so calls
    //    that arrive early queue up until the engine is ready.
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<EngineMsg>();
    let builder = match &bus {
        BusChoice::Session => zbus::connection::Builder::session(),
        BusChoice::Address(address) => zbus::connection::Builder::address(address.as_str()),
    };
    let built = match builder
        // zbus defaults to *replacing* the current owner of a name. For single-instance
        // enforcement the second daemon must be refused, never take the name over.
        .map(|b| {
            b.allow_name_replacements(false)
                .replace_existing_names(false)
        })
        .and_then(|b| b.name(BUS_NAME))
        .and_then(|b| b.serve_at(OBJECT_PATH, LucernaService { tx: tx.clone() }))
    {
        Ok(builder) => builder.build().await,
        Err(err) => Err(err),
    };
    let conn = match built {
        Ok(conn) => conn,
        Err(zbus::Error::NameTaken) => return Outcome::AlreadyRunning { pid: None },
        Err(err) => {
            return Outcome::Failed(format!(
                "Lucerna could not connect to the session D-Bus: {err}\n\
                 Run it from inside a desktop session, or start a session bus with dbus-run-session."
            ));
        }
    };

    // 4. Renderers left behind by a crashed predecessor (§52), then stale sockets.
    if let Some(registry) = paths.registry_file() {
        let exe_name = mpv_override
            .as_ref()
            .and_then(|p| {
                std::path::Path::new(p)
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
            })
            .unwrap_or_else(|| "mpv".to_owned());
        let report = tokio::task::spawn_blocking(move || {
            recover_stale(
                &registry,
                &RecoveryOptions {
                    exe_name,
                    ..RecoveryOptions::default()
                },
            )
        })
        .await;
        if let Ok(report) = report
            && !report.terminated.is_empty()
        {
            tracing::warn!(pids = ?report.terminated, "terminated stale renderers from a previous daemon");
        }
    }
    if let Some(dir) = &paths.runtime_dir {
        let _ = lucerna_core::runtime::remove_stale_sockets(dir);
    }

    if handle_signals {
        spawn_signal_forwarders(&tx);
    }

    let opts = EngineOpts {
        paths,
        session,
        daemon_exe,
        renderer_timings,
        vo_override,
        extra_env,
        recheck_interval,
        display_wait,
        mpv_override,
        path_var,
        autostart_on_first_run,
    };
    let mut engine = Engine::new(opts, conn, tx, rx, backend);
    tracing::info!(version = VERSION, "lucernad starting");
    engine.initialise().await;
    engine.run().await;
    Outcome::Clean
}

/// Forward SIGTERM, SIGINT and SIGHUP (the session ending) to the engine.
fn spawn_signal_forwarders(tx: &tokio::sync::mpsc::UnboundedSender<EngineMsg>) {
    use tokio::signal::unix::{SignalKind, signal};
    for (kind, name) in [
        (SignalKind::terminate(), "SIGTERM"),
        (SignalKind::interrupt(), "SIGINT"),
        (SignalKind::hangup(), "SIGHUP"),
    ] {
        match signal(kind) {
            Ok(mut stream) => {
                let tx = tx.clone();
                tokio::spawn(async move {
                    if stream.recv().await.is_some() {
                        let _ = tx.send(EngineMsg::Signal(name));
                    }
                });
            }
            Err(err) => tracing::warn!(%err, signal = name, "could not install a signal handler"),
        }
    }
}

/// Entry point shared by `lucernad` and the testkit wrapper.
pub fn cli_main() -> ExitCode {
    let args = Args::parse();
    lucerna_core::logging::init("lucernad", args.verbose);

    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(err) => {
            eprintln!("Lucerna could not start its async runtime: {err}");
            return ExitCode::FAILURE;
        }
    };
    match runtime.block_on(run(DaemonOptions::from_env())) {
        Outcome::Clean => ExitCode::SUCCESS,
        Outcome::AlreadyRunning { pid } => {
            println!("{}", already_running_message(pid));
            ExitCode::SUCCESS
        }
        Outcome::Failed(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
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
        assert!(Args::try_parse_from(["lucernad", "-v"]).unwrap().verbose);
    }

    #[test]
    fn already_running_message_names_the_pid() {
        assert_eq!(
            already_running_message(Some(1234)),
            "Lucerna daemon is already running for this session (PID 1234). Nothing to do."
        );
        assert!(!already_running_message(None).contains("PID"));
    }
}
