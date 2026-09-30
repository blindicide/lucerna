# Lucerna

Lucerna is a native Linux animated-wallpaper engine. It plays local video wallpapers
(`.mp4`, `.webm`, `.mkv`, `.gif`, anything [mpv](https://mpv.io) can decode) as your actual
desktop background, keeps them running after the control window is closed, restores them at
login, and pauses them when they are not visible.

It is meant to replace the pile of `xwinwrap` one-liners and `mpv --loop` scripts people use
today with a proper desktop application: a GTK 4 control app, a small per-user daemon, and a
command-line controller, installed from native `.deb` and `.rpm` packages.

## Status

**Development status: pre-release (`0.x`).** This repository is being built milestone by
milestone toward `v1.0.0-rc.1` (see [CHANGELOG.md](CHANGELOG.md)). Today the background service
(`lucernad`), the command-line controller (`lucernactl`) and the GTK control application
(`lucerna`) work end to end: renderer supervision, the Cinnamon/X11 backend, configuration, the
D-Bus API, diagnostics and the four-page GUI. **Nothing has been validated on a real desktop
yet: the GUI's look and the wallpaper's behaviour under Cinnamon are unverified.**

> Lucerna is developed on a headless server. Automated tests cover logic, process
> supervision, D-Bus and X11 *protocol* behaviour. Nothing about how the wallpaper *looks* or
> how it stacks under real desktop icons can be validated there, so all such behaviour is
> tracked in a manual acceptance campaign (`docs/MANUAL-ACCEPTANCE.md`) that must be run by a
> person on a real desktop. Until that is done, every desktop-facing feature is
> "implemented — manual desktop validation required", never "works".

## Supported environment

- **Target:** Linux Mint 22.x (Ubuntu 24.04 base) and newer, **Cinnamon on X11**.
- Requires GTK 4.10 or newer and `mpv`.
- Other X11 desktops may work on a best-effort basis; they are not acceptance targets.
- **Wayland sessions are not supported** in v1. Lucerna detects them and says so.

## Components

| Binary | Role |
| --- | --- |
| `lucerna` | GTK 4 control application (library, displays, settings). |
| `lucernad` | Per-user background service that owns the wallpaper renderers. |
| `lucernactl` | Command-line controller and `doctor` diagnostics. |

## Installation

Native `.deb` (Debian, Ubuntu, Linux Mint 22+) and `.rpm` (Fedora) packages, a source archive and
`SHA256SUMS` are attached to each GitHub Release by the release pipeline (verify with
`sha256sum -c SHA256SUMS`); see
[docs/PACKAGING.md](docs/PACKAGING.md). Install with `sudo apt install ./lucerna_*.deb` or
`sudo dnf install ./lucerna-*.rpm`; `mpv` and the GTK 4 libraries are pulled in automatically.
To build from source, see [docs/BUILDING.md](docs/BUILDING.md).

## Quick start (from a source build)

```sh
cargo build --release
target/release/lucernad &                          # the per-user service
target/release/lucernactl play ~/Videos/rain.webm  # add it to the library and play it
target/release/lucernactl status
target/release/lucernactl pause     # / resume / stop / reload
target/release/lucernactl doctor    # everything needed to debug desktop integration
```

## Documentation

- [docs/BUILDING.md](docs/BUILDING.md) - build dependencies, local and headless builds, tests
- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) - components and crate boundaries
- [docs/CONFIGURATION.md](docs/CONFIGURATION.md) - config file, settings and defaults
- [docs/IPC.md](docs/IPC.md) - the D-Bus API
- [docs/TROUBLESHOOTING.md](docs/TROUBLESHOOTING.md) - common problems
- [docs/MANUAL-ACCEPTANCE.md](docs/MANUAL-ACCEPTANCE.md) - the desktop test campaign (not yet run)
- [docs/X11-CINNAMON-NOTES.md](docs/X11-CINNAMON-NOTES.md) - what the X11 backend assumes about Cinnamon
- [docs/PACKAGING.md](docs/PACKAGING.md) - package layout, versions and builds
- [docs/TEST-MATRIX.md](docs/TEST-MATRIX.md) - which test covers which requirement
- [docs/IMPLEMENTATION-PLAN.md](docs/IMPLEMENTATION-PLAN.md) - the v1 plan

## Non-goals for v1

Wallpaper Engine/Workshop compatibility, web/shader wallpapers, Wayland backends, cloud
features and telemetry are out of scope. Lucerna makes no network requests.

## License

MIT. See [LICENSE](LICENSE).
