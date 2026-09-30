//! A private `dbus-daemon` for tests, so nothing touches the developer's real session bus.

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

pub struct TestBus {
    child: Option<Child>,
    /// For `DBUS_SESSION_BUS_ADDRESS` or `zbus::connection::Builder::address`.
    pub address: String,
    dir: PathBuf,
}

/// True when the tests must fail rather than skip if `dbus-daemon` is unavailable.
pub fn dbus_required() -> bool {
    std::env::var_os("LUCERNA_REQUIRE_DBUS").is_some()
}

impl TestBus {
    /// Start a private session bus, or `None` (skip) when `dbus-daemon` is missing and not required.
    pub fn start() -> Option<Self> {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("lc-bus-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).ok()?;
        let config = dir.join("bus.conf");
        let sockets = dir.join("sock");
        fs::create_dir_all(&sockets).ok()?;
        fs::write(
            &config,
            format!(
                r#"<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type>
  <listen>unix:dir={}</listen>
  <auth>EXTERNAL</auth>
  <policy context="default">
    <allow send_destination="*" eavesdrop="true"/>
    <allow eavesdrop="true"/>
    <allow own="*"/>
  </policy>
</busconfig>
"#,
                sockets.display()
            ),
        )
        .ok()?;

        let mut child = match Command::new("dbus-daemon")
            .arg(format!("--config-file={}", config.display()))
            .args(["--nofork", "--print-address=1"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(child) => child,
            Err(_) => {
                assert!(
                    !dbus_required(),
                    "LUCERNA_REQUIRE_DBUS is set but dbus-daemon is not installed"
                );
                eprintln!("SKIPPED (no dbus-daemon)");
                return None;
            }
        };
        let mut address = String::new();
        if let Some(stdout) = child.stdout.take() {
            let _ = BufReader::new(stdout).read_line(&mut address);
        }
        let address = address.trim().to_owned();
        if address.is_empty() {
            let _ = child.kill();
            assert!(!dbus_required(), "dbus-daemon did not print an address");
            eprintln!("SKIPPED (dbus-daemon gave no address)");
            return None;
        }
        Some(Self {
            child: Some(child),
            address,
            dir,
        })
    }
}

impl Drop for TestBus {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        let _ = fs::remove_dir_all(&self.dir);
    }
}
