//! §34: `lucernad` must run `--help` and `--version` with no graphical display.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_lucernad");
const VERSION: &str = env!("CARGO_PKG_VERSION");

fn run(args: &[&str]) -> Output {
    Command::new(BIN)
        .args(args)
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .output()
        .expect("spawn lucernad")
}

#[test]
fn version_works_without_display() {
    let out = run(&["--version"]);
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert_eq!(text.trim(), format!("lucernad {VERSION}"));
}

#[test]
fn help_works_without_display() {
    let out = run(&["--help"]);
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("Usage"));
}
