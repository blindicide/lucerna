# Changelog

All notable changes to Lucerna are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.0.1] - 2026-09-30

### Added
- Cargo workspace with eight crates that encode Lucerna's architecture boundaries: core logic,
  mpv supervision, the X11 backend, the D-Bus contract, the daemon, the CLI, the GTK
  application and a test kit. The root `Cargo.toml` is the single source of the version.
- Three binaries, `lucerna`, `lucernad` and `lucernactl`, that report the workspace version
  through `--version` and print `--help` without needing a graphical display. `lucerna`
  checks for a graphical session before starting GTK and explains what is missing instead
  of failing with a panic.
- Shared logging set-up controlled by `LUCERNA_LOG` or `RUST_LOG` (default `lucerna=info`)
  and XDG path resolution.
- An architecture test that fails the build when a crate gains a forbidden dependency, when
  pure code mentions GTK, X11, D-Bus or an async runtime, or when a `main.rs` grows large.
- Continuous integration for formatting, Clippy (warnings denied), tests, release builds,
  headless smoke tests, dependency auditing and shell script checks.
- `scripts/version.sh`, which maps the workspace version to Debian, RPM and tarball naming.
- README, build documentation and an architecture overview.
