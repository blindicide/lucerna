//! Discovery and option-compatibility checks, exercised against the fake mpv and small scripts.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::ffi::OsStr;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use lucerna_core::mpv::{DiscoveryError, check_compat, discover};
use lucerna_testkit::TestEnv;

const FAKE_MPV: &str = env!("CARGO_BIN_EXE_fake-mpv");

fn script(path: &Path, body: &str) {
    fs::write(path, body).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn fake_mpv_is_discovered_through_the_override() {
    let info = discover(Some(OsStr::new(FAKE_MPV)), None).unwrap();
    assert_eq!(info.version, "0.37.0");
    assert_eq!(info.path, Path::new(FAKE_MPV));
}

#[test]
fn mpv_is_discovered_through_path_when_no_override_is_given() {
    let env = TestEnv::new("pathfind");
    let bin = env.root.join("bin");
    fs::create_dir_all(&bin).unwrap();
    script(
        &bin.join("mpv"),
        "#!/bin/sh\necho 'mpv 0.99.1 Copyright x'\n",
    );
    let info = discover(None, Some(bin.as_os_str())).unwrap();
    assert_eq!(info.version, "0.99.1");
    assert!(matches!(
        discover(None, Some(env.root.as_os_str())),
        Err(DiscoveryError::NotFound)
    ));
}

#[test]
fn compat_probe_passes_for_a_double_that_accepts_every_emitted_option() {
    let report = check_compat(Path::new(FAKE_MPV)).unwrap();
    assert!(report.compatible, "{}", report.detail);
}

#[test]
fn compat_probe_reports_an_mpv_that_rejects_an_option() {
    let env = TestEnv::new("compat");
    let strict = env.root.join("strict-mpv");
    script(
        &strict,
        "#!/bin/sh\necho 'Error parsing option x11-bypass-compositor (option not found)' >&2\nexit 1\n",
    );
    let report = check_compat(&strict).unwrap();
    assert!(!report.compatible);
    assert!(
        report.detail.contains("x11-bypass-compositor"),
        "{}",
        report.detail
    );
}

#[test]
fn the_fake_rejects_options_the_builder_does_not_declare() {
    let out = std::process::Command::new(FAKE_MPV)
        .args(["--totally-new-option=1", "--list-options"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("option not found"));
}
