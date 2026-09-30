# Building Lucerna

## Requirements

| Need | Notes |
| --- | --- |
| Rust | Pinned by `rust-toolchain.toml` (currently 1.98.1). Install with [rustup](https://rustup.rs); it picks the pinned toolchain automatically. |
| GTK 4 development files | `libgtk-4-dev` (Debian/Ubuntu/Mint) or `gtk4-devel` (Fedora). GTK 4.10 or newer. |
| `pkgconf` | Used by the GTK bindings' build scripts. |
| `mpv` | Runtime dependency, and needed by the integration tests that use a real mpv. |
| `jq` | Used by `scripts/version.sh`. |
| `xvfb`, `dbus` | `Xvfb` runs the X11 protocol tests; `dbus-daemon` provides the private session bus for the daemon and CLI integration tests. |

Debian/Ubuntu/Mint:

```sh
sudo apt-get install build-essential pkgconf libgtk-4-dev mpv jq xvfb dbus
```

## Local build

```sh
cargo build --workspace            # debug
cargo build --workspace --release  # release
```

The workspace version comes from `[workspace.package] version` in the root `Cargo.toml`.
`scripts/version.sh` prints it in the forms that Debian, RPM and the source archive need.

## Headless (server) builds

No display is needed to build or test. `DISPLAY` may be unset. All three binaries
(`lucerna`, `lucernad`, `lucernactl`) run `--help` and `--version` without a display, and
`lucerna` with no arguments prints a readable message and exits with status 1.

Anything the tests do with X11 uses `Xvfb` and checks protocol behaviour only. It says
nothing about how the wallpaper looks on a real desktop; see `docs/MANUAL-ACCEPTANCE.md`.

## Quality gates

These four commands must pass before every tagged milestone:

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace --release
```

Set `LUCERNA_REQUIRE_XVFB=1`, `LUCERNA_REQUIRE_DBUS=1` and `LUCERNA_REQUIRE_MPV=1` to turn
"tool not installed" from a skipped test into a failure. CI sets all three.

Script tests:

```sh
for t in tests/scripts/*_test.sh; do "$t"; done
```

## Dependency auditing

CI runs `cargo deny` using `deny.toml` (advisories, licenses, bans, sources). Any exception
must be justified inside `deny.toml`.

## Package builds

Added in the packaging milestone; see `docs/PACKAGING.md` once it exists.
