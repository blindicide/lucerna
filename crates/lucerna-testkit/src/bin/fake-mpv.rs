//! A stand-in for mpv at the process and IPC level (docs/IMPLEMENTATION-PLAN.md §9.2).
//!
//! Its behaviour is chosen by the first line of the *media file*, `FAKE-MPV: <directive>[; ...]`,
//! so tests running in parallel never share state through the environment. It rejects any option
//! Lucerna's argument builder does not declare in `EMITTED_OPTIONS`, which catches drift between
//! the builder and this double.
//!
//! Directives: `play` (default), `crash-after=<ms>`, `crash-first=<n>`, `exit=<code>[@<ms>]`,
//! `unsupported`, `no-ipc`, `hang-before-load`, `ipc-drop-after=<ms>`, `ignore-quit`,
//! `ignore-term`, `stderr-flood=<bytes>`, `load-delay=<ms>`.
//!
//! Every launch appends JSON lines (argv, pid, each IPC command) to `<media>.log`.

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use lucerna_core::mpv::{EMITTED_OPTIONS, option_name};
use serde_json::{Value, json};

struct Shared {
    clients: Mutex<Vec<UnixStream>>,
    loaded: AtomicBool,
    paused: AtomicBool,
    log: Mutex<File>,
    socket: PathBuf,
    ignore_quit: bool,
}

impl Shared {
    fn record(&self, value: Value) {
        if let Ok(mut log) = self.log.lock() {
            let _ = writeln!(log, "{value}");
        }
    }

    fn broadcast(&self, event: &Value) {
        if let Ok(mut clients) = self.clients.lock() {
            clients.retain_mut(|c| writeln!(c, "{event}").is_ok());
        }
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.iter().any(|a| a == "--version") {
        println!("mpv 0.37.0 Copyright (fake-mpv test double)");
        return ExitCode::SUCCESS;
    }

    let separator = args.iter().position(|a| a == "--");
    let option_args = &args[..separator.unwrap_or(args.len())];

    let mut values: BTreeMap<String, String> = BTreeMap::new();
    for arg in option_args {
        let Some(name) = option_name(arg) else {
            eprintln!("fake-mpv: unexpected argument before `--`: {arg}");
            return ExitCode::from(1);
        };
        if name != "list-options" && !EMITTED_OPTIONS.contains(&name.as_str()) {
            eprintln!("Error parsing option {name} (option not found)");
            return ExitCode::from(1);
        }
        let value = arg.split_once('=').map_or("", |(_, v)| v).to_owned();
        values.insert(name, value);
    }

    if option_args.iter().any(|a| a == "--list-options") {
        println!("Options:");
        return ExitCode::SUCCESS;
    }

    let Some(media) = separator.and_then(|i| args.get(i + 1)).map(PathBuf::from) else {
        eprintln!("fake-mpv: no media file after `--`");
        return ExitCode::from(1);
    };
    if separator.is_some_and(|i| args.len() != i + 2) {
        eprintln!("fake-mpv: exactly one media argument must follow `--`");
        return ExitCode::from(1);
    }

    let directives = read_directives(&media);
    let log_path = PathBuf::from(format!("{}.log", media.display()));
    let Ok(log) = OpenOptions::new().create(true).append(true).open(&log_path) else {
        eprintln!("fake-mpv: cannot open {}", log_path.display());
        return ExitCode::from(1);
    };
    let socket = PathBuf::from(values.get("input-ipc-server").cloned().unwrap_or_default());
    let shared = Arc::new(Shared {
        clients: Mutex::new(Vec::new()),
        loaded: AtomicBool::new(false),
        paused: AtomicBool::new(values.get("pause").is_some_and(|v| v == "yes")),
        log: Mutex::new(log),
        socket: socket.clone(),
        ignore_quit: directives.contains_key("ignore-quit"),
    });
    shared.record(json!({
        "event": "launch",
        "pid": std::process::id(),
        "argv": args,
    }));

    if directives.contains_key("ignore-term") {
        // Registering a handler replaces the default action, so SIGTERM no longer kills us.
        let flag = Arc::new(AtomicBool::new(false));
        let _ = signal_hook::flag::register(signal_hook::consts::SIGTERM, flag);
    }

    if let Some(bytes) = directives
        .get("stderr-flood")
        .and_then(|v| v.parse::<usize>().ok())
    {
        let line = format!("{}\n", "F".repeat(1023));
        let mut written = 0;
        let mut err = std::io::stderr();
        while written < bytes {
            if err.write_all(line.as_bytes()).is_err() {
                break;
            }
            written += line.len();
        }
    }

    if directives.contains_key("no-ipc") {
        loop {
            thread::sleep(Duration::from_secs(3600));
        }
    }

    let _ = fs::remove_file(&socket);
    let Ok(listener) = UnixListener::bind(&socket) else {
        eprintln!("fake-mpv: cannot bind {}", socket.display());
        return ExitCode::from(1);
    };
    {
        let shared = Arc::clone(&shared);
        thread::spawn(move || accept_loop(listener, shared));
    }

    if let Some(ms) = number(&directives, "ipc-drop-after") {
        let shared = Arc::clone(&shared);
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(ms));
            if let Ok(mut clients) = shared.clients.lock() {
                for c in clients.drain(..) {
                    let _ = c.shutdown(std::net::Shutdown::Both);
                }
            }
            let _ = fs::remove_file(&shared.socket);
        });
    }

    // crash-first=<n>: crash on the first n launches, then behave normally.
    let mut crash_after = number(&directives, "crash-after");
    if let Some(n) = number(&directives, "crash-first") {
        let count_path = PathBuf::from(format!("{}.count", media.display()));
        let count: u64 = fs::read_to_string(&count_path)
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0)
            + 1;
        let _ = fs::write(&count_path, count.to_string());
        if count <= n {
            crash_after = Some(50);
        }
    }
    if let Some(ms) = crash_after {
        thread::sleep(Duration::from_millis(ms));
        std::process::abort();
    }

    if directives.contains_key("unsupported") {
        thread::sleep(Duration::from_millis(60));
        shared.broadcast(&json!({
            "event": "end-file", "reason": "error", "file_error": "unrecognized file format"
        }));
        let _ = fs::remove_file(&socket);
        return ExitCode::from(2);
    }

    if let Some(spec) = directives.get("exit") {
        let (code, delay) = spec.split_once('@').unwrap_or((spec, "0"));
        thread::sleep(Duration::from_millis(delay.parse().unwrap_or(0)));
        let _ = fs::remove_file(&socket);
        return ExitCode::from(code.parse().unwrap_or(0));
    }

    if !directives.contains_key("hang-before-load") {
        thread::sleep(Duration::from_millis(
            number(&directives, "load-delay").unwrap_or(30),
        ));
        shared.loaded.store(true, Ordering::SeqCst);
        shared.broadcast(&json!({"event": "file-loaded"}));
    }

    loop {
        thread::sleep(Duration::from_secs(3600));
    }
}

fn number(directives: &BTreeMap<String, String>, key: &str) -> Option<u64> {
    directives.get(key).and_then(|v| v.parse().ok())
}

fn read_directives(media: &Path) -> BTreeMap<String, String> {
    let first_line = fs::File::open(media)
        .ok()
        .and_then(|f| BufReader::new(f).lines().next())
        .and_then(Result::ok)
        .unwrap_or_default();
    let mut out = BTreeMap::new();
    if let Some(rest) = first_line.strip_prefix("FAKE-MPV:") {
        for part in rest.split(';').map(str::trim).filter(|p| !p.is_empty()) {
            let (key, value) = part.split_once('=').unwrap_or((part, ""));
            out.insert(key.to_owned(), value.to_owned());
        }
    }
    out
}

fn accept_loop(listener: UnixListener, shared: Arc<Shared>) {
    for stream in listener.incoming().flatten() {
        if let Ok(writer) = stream.try_clone()
            && let Ok(mut clients) = shared.clients.lock()
        {
            clients.push(writer);
        }
        let shared = Arc::clone(&shared);
        thread::spawn(move || serve(stream, shared));
    }
}

fn serve(stream: UnixStream, shared: Arc<Shared>) {
    let Ok(mut writer) = stream.try_clone() else {
        return;
    };
    for line in BufReader::new(stream).lines().map_while(Result::ok) {
        let Ok(request) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let command = request["command"].as_array().cloned().unwrap_or_default();
        shared.record(json!({"event": "command", "command": command}));
        let name = command.first().and_then(Value::as_str).unwrap_or_default();

        let (error, data) = match name {
            "quit" if shared.ignore_quit => ("success", Value::Null),
            "quit" => {
                let _ = writeln!(
                    writer,
                    "{}",
                    json!({"error": "success", "request_id": request["request_id"]})
                );
                shared.record(json!({"event": "quit"}));
                let _ = fs::remove_file(&shared.socket);
                std::process::exit(0);
            }
            "set_property" => {
                if command.get(1).and_then(Value::as_str) == Some("pause") {
                    shared.paused.store(
                        command.get(2).and_then(Value::as_bool).unwrap_or(false),
                        Ordering::SeqCst,
                    );
                }
                ("success", Value::Null)
            }
            "get_property" => match command.get(1).and_then(Value::as_str) {
                Some("time-pos") if shared.loaded.load(Ordering::SeqCst) => ("success", json!(0.0)),
                Some("time-pos") => ("property unavailable", Value::Null),
                Some("pause") => ("success", json!(shared.paused.load(Ordering::SeqCst))),
                Some("mpv-version") => ("success", json!("mpv 0.37.0")),
                _ => ("property not found", Value::Null),
            },
            _ => ("invalid parameter", Value::Null),
        };
        let mut reply = json!({"error": error, "request_id": request["request_id"]});
        if !data.is_null() {
            reply["data"] = data;
        }
        if writeln!(writer, "{reply}").is_err() {
            break;
        }
    }
}
