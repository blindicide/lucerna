//! §34: `lucerna` must run `--help` and `--version` with no graphical display.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_lucerna");
const VERSION: &str = env!("CARGO_PKG_VERSION");

fn run(args: &[&str]) -> Output {
    Command::new(BIN)
        .args(args)
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .output()
        .expect("spawn lucerna")
}

#[test]
fn version_works_without_display() {
    let out = run(&["--version"]);
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert_eq!(text.trim(), format!("lucerna {VERSION}"));
}

#[test]
fn help_works_without_display() {
    let out = run(&["--help"]);
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("Usage"));
}

#[test]
fn launch_without_display_explains_and_exits_1() {
    let out = run(&[]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("could not connect to a graphical display"),
        "{err}"
    );
    assert!(!err.contains("panicked"), "{err}");
}
