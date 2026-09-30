//! A throw-away Xvfb server for X11 *protocol* tests.
//!
//! Xvfb is never evidence of visual correctness: it has no window manager, no compositor and no
//! Cinnamon. It is used to check window properties, event routing, RandR data and process
//! lifecycle, nothing more (directive §2).

use std::fs;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use x11rb::rust_connection::RustConnection;

pub struct Xvfb {
    child: Option<Child>,
    /// `:N`, ready to pass explicitly to `X11Backend::connect`. Tests never touch `$DISPLAY`.
    pub display: String,
    number: u32,
}

/// True when the tests must fail rather than skip if Xvfb is unavailable.
pub fn xvfb_required() -> bool {
    std::env::var_os("LUCERNA_REQUIRE_XVFB").is_some()
}

impl Xvfb {
    /// Start a server with a single `width`x`height` 24-bit screen, or `None` (skip) when Xvfb is
    /// not installed and not required.
    pub fn start(width: u32, height: u32) -> Option<Self> {
        if Command::new("Xvfb")
            .arg("-version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_err()
        {
            assert!(
                !xvfb_required(),
                "LUCERNA_REQUIRE_XVFB is set but Xvfb is not installed"
            );
            eprintln!("SKIPPED (no Xvfb)");
            return None;
        }
        // Spread parallel test processes (by pid) and parallel tests inside one process (by a
        // counter) over the display numbers.
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let offset = std::process::id() % 50 + NEXT.fetch_add(3, Ordering::Relaxed);
        for attempt in 0..100u32 {
            let number = 90 + (offset + attempt) % 100;
            if lock_path(number).exists() || socket_path(number).exists() {
                continue;
            }
            let display = format!(":{number}");
            let child = Command::new("Xvfb")
                .arg(&display)
                .args(["-screen", "0", &format!("{width}x{height}x24")])
                .args(["-nolisten", "tcp", "-noreset", "+extension", "RANDR"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .ok()?;
            let mut server = Self {
                child: Some(child),
                display: display.clone(),
                number,
            };
            if server.wait_ready() {
                return Some(server);
            }
            // Someone else won the race for this number (or it failed to start): try the next.
            server.terminate();
        }
        assert!(
            !xvfb_required(),
            "could not start Xvfb on any display number"
        );
        eprintln!("SKIPPED (Xvfb would not start)");
        None
    }

    fn wait_ready(&mut self) -> bool {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if let Some(child) = self.child.as_mut()
                && matches!(child.try_wait(), Ok(Some(_)))
            {
                return false;
            }
            if RustConnection::connect(Some(&self.display)).is_ok() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        false
    }

    /// Kill the server abruptly, as a crashing X server or a logout would.
    pub fn kill(&mut self) {
        self.terminate();
    }

    fn terminate(&mut self) {
        if let Some(mut child) = self.child.take() {
            if let Some(pid) = i32::try_from(child.id())
                .ok()
                .and_then(rustix::process::Pid::from_raw)
            {
                let _ = rustix::process::kill_process(pid, rustix::process::Signal::TERM);
            }
            let deadline = Instant::now() + Duration::from_secs(2);
            while Instant::now() < deadline {
                if matches!(child.try_wait(), Ok(Some(_))) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            let _ = child.kill();
            let _ = child.wait();
            let _ = fs::remove_file(lock_path(self.number));
            let _ = fs::remove_file(socket_path(self.number));
        }
    }
}

impl Drop for Xvfb {
    fn drop(&mut self) {
        self.terminate();
    }
}

fn lock_path(number: u32) -> PathBuf {
    PathBuf::from(format!("/tmp/.X{number}-lock"))
}

fn socket_path(number: u32) -> PathBuf {
    PathBuf::from(format!("/tmp/.X11-unix/X{number}"))
}
