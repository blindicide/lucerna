//! Enforces the crate boundaries of docs/IMPLEMENTATION-PLAN.md §1.3–§1.5.
//!
//! Adding a forbidden dependency edge, a GUI/X11/D-Bus token to pure code, a shell
//! invocation, or an oversized `main.rs` breaks this test with a message naming the rule.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("testkit lives two levels below the workspace root")
        .to_path_buf()
}

fn read_toml(path: &Path) -> toml::Table {
    let text = fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    text.parse()
        .unwrap_or_else(|e| panic!("parse {}: {e}", path.display()))
}

/// Allowed internal edges: the complete list from the plan. Every other edge is forbidden.
fn allowed_internal(krate: &str) -> Option<&'static [&'static str]> {
    Some(match krate {
        "lucerna-core" => &[],
        "lucerna-mpv" => &["lucerna-core"],
        "lucerna-x11" => &["lucerna-core"],
        "lucerna-ipc" => &["lucerna-core"],
        "lucerna-daemon" => &["lucerna-core", "lucerna-mpv", "lucerna-x11", "lucerna-ipc"],
        "lucerna-cli" => &["lucerna-core", "lucerna-ipc", "lucerna-x11"],
        "lucerna-ui" => &["lucerna-core", "lucerna-ipc"],
        "lucerna-testkit" => return None, // may depend on anything
        _ => return None,
    })
}

fn forbidden_external(krate: &str) -> &'static [&'static str] {
    match krate {
        "lucerna-core" => &["gtk4", "glib", "gio", "x11rb", "zbus", "tokio", "rustix"],
        "lucerna-mpv" => &["gtk4", "x11rb", "zbus"],
        "lucerna-x11" => &["gtk4", "zbus", "tokio"],
        "lucerna-ipc" => &["gtk4", "x11rb", "tokio"],
        "lucerna-daemon" => &["gtk4", "glib", "x11rb"],
        "lucerna-cli" => &["gtk4", "tokio"],
        "lucerna-ui" => &["x11rb", "tokio", "rustix"],
        _ => &[],
    }
}

fn crate_dirs() -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    let crates = workspace_root().join("crates");
    for entry in fs::read_dir(&crates).expect("crates dir") {
        let dir = entry.expect("dir entry").path();
        if dir.join("Cargo.toml").is_file() {
            let name = dir
                .file_name()
                .expect("name")
                .to_string_lossy()
                .into_owned();
            out.push((name, dir));
        }
    }
    out.sort();
    out
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn all_expected_crates_exist() {
    let names: BTreeSet<String> = crate_dirs().into_iter().map(|(n, _)| n).collect();
    for expected in [
        "lucerna-core",
        "lucerna-mpv",
        "lucerna-x11",
        "lucerna-ipc",
        "lucerna-daemon",
        "lucerna-cli",
        "lucerna-ui",
        "lucerna-testkit",
    ] {
        assert!(names.contains(expected), "missing crate {expected}");
    }
}

#[test]
fn dependency_edges_follow_the_allow_table() {
    let internal: BTreeSet<String> = crate_dirs().into_iter().map(|(n, _)| n).collect();
    for (name, dir) in crate_dirs() {
        let manifest = read_toml(&dir.join("Cargo.toml"));
        let deps: Vec<String> = manifest
            .get("dependencies")
            .and_then(|d| d.as_table())
            .map(|t| t.keys().cloned().collect())
            .unwrap_or_default();

        if let Some(allowed) = allowed_internal(&name) {
            for dep in deps.iter().filter(|d| internal.contains(*d)) {
                assert!(
                    allowed.contains(&dep.as_str()),
                    "RULE crate-graph: {name} must not depend on {dep} (allowed: {allowed:?})"
                );
            }
        }
        for dep in &deps {
            assert!(
                !forbidden_external(&name).contains(&dep.as_str()),
                "RULE forbidden-dependency: {name} must not depend on {dep}"
            );
        }
        // Nothing may depend on the testkit.
        assert!(
            !deps.iter().any(|d| d == "lucerna-testkit"),
            "RULE testkit-is-a-leaf: {name} depends on lucerna-testkit"
        );
    }
}

#[test]
fn zbus_never_enables_tokio_feature() {
    let root = read_toml(&workspace_root().join("Cargo.toml"));
    let zbus = root
        .get("workspace")
        .and_then(|w| w.get("dependencies"))
        .and_then(|d| d.get("zbus"));
    let Some(zbus) = zbus else { return };
    let features = zbus
        .get("features")
        .and_then(|f| f.as_array())
        .cloned()
        .unwrap_or_default();
    assert!(
        !features.iter().any(|f| f.as_str() == Some("tokio")),
        "RULE zbus-no-tokio: the zbus `tokio` feature would force the GTK client to host tokio"
    );
}

#[test]
fn pure_code_names_no_gui_x11_dbus_or_async_runtime() {
    let root = workspace_root();
    let mut files = Vec::new();
    rust_files(&root.join("crates/lucerna-core/src"), &mut files);
    rust_files(&root.join("crates/lucerna-ui/src/presenter"), &mut files);
    for file in files {
        let text = fs::read_to_string(&file).expect("read source");
        for (n, line) in text.lines().enumerate() {
            let code = line.split("//").next().unwrap_or_default();
            for token in ["gtk4::", "gtk::", "x11rb::", "zbus::", "tokio::", "glib::"] {
                assert!(
                    !code.contains(token),
                    "RULE pure-core: {}:{} mentions `{token}`",
                    file.display(),
                    n + 1
                );
            }
        }
    }
}

#[test]
fn shipped_main_files_are_tiny() {
    for (name, dir) in crate_dirs() {
        let main = dir.join("src/main.rs");
        if main.is_file() {
            let lines = fs::read_to_string(&main)
                .expect("read main.rs")
                .lines()
                .count();
            assert!(
                lines <= 30,
                "RULE tiny-main: {name}/src/main.rs has {lines} lines (max 30)"
            );
        }
    }
}

#[test]
fn no_shell_invocation_in_production_sources() {
    let root = workspace_root();
    let mut files = Vec::new();
    rust_files(&root.join("crates"), &mut files);
    for file in files {
        let path = file.to_string_lossy().into_owned();
        // Tests and this very file may mention the forbidden literals.
        if path.contains("/tests/") || path.contains("/lucerna-testkit/") {
            continue;
        }
        let text = fs::read_to_string(&file).expect("read source");
        for (n, line) in text.lines().enumerate() {
            let code = line.split("//").next().unwrap_or_default();
            for literal in ["\"sh\"", "\"bash\"", "\"-c\""] {
                assert!(
                    !code.contains(literal),
                    "RULE no-shell: {}:{} contains {literal}",
                    file.display(),
                    n + 1
                );
            }
        }
    }
}

#[test]
fn every_crate_inherits_workspace_metadata() {
    for (name, dir) in crate_dirs() {
        let manifest = fs::read_to_string(dir.join("Cargo.toml")).expect("manifest");
        for key in [
            "version.workspace = true",
            "edition.workspace = true",
            "publish.workspace = true",
        ] {
            assert!(
                manifest.contains(key),
                "RULE single-version: {name} lacks `{key}`"
            );
        }
        assert!(
            manifest.contains("[lints]\nworkspace = true"),
            "RULE lints: {name} must inherit workspace lints"
        );
    }
}
