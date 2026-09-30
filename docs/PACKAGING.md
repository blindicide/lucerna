# Packaging

Lucerna ships native `.deb` and `.rpm` packages plus a source archive and `SHA256SUMS`. Each
package is compiled inside its own distribution's environment; nothing is converted between
formats.

## One version

The only place a version is written is `[workspace.package] version` in the root `Cargo.toml`.
`scripts/version.sh` maps it for each format:

| SemVer | `--deb` | RPM version / release | Source archive |
| --- | --- | --- | --- |
| `0.7.0` | `0.7.0` | `0.7.0` / `1` | `lucerna-0.7.0.tar.gz` |
| `1.0.0-rc.1` | `1.0.0~rc1` | `1.0.0` / `0.rc1` | `lucerna-1.0.0-rc.1.tar.gz` |
| `1.0.0` | `1.0.0` | `1.0.0` / `1` | `lucerna-1.0.0.tar.gz` |

Other pre-release forms are rejected. The Debian changelog, the RPM `%changelog`, the AppStream
`<release>` and the man-page headers are generated at build time from that version and the dated
`## [x.y.z] - date` heading in `CHANGELOG.md` (`scripts/release-date.sh`). Neither
`packaging/debian` nor the spec contains a version.

Artifact names: `lucerna_<deb>_amd64.deb`, `lucerna-<rpm version>-<release>.x86_64.rpm`,
`lucerna-<semver>.tar.gz`, `SHA256SUMS`.

## Contents (identical for both formats)

```text
/usr/bin/{lucerna,lucernad,lucernactl}
/usr/share/man/man1/{lucerna,lucernad,lucernactl}.1.gz
/usr/share/applications/org.lucerna.Lucerna.desktop
/usr/share/icons/hicolor/scalable/apps/org.lucerna.Lucerna.svg
/usr/share/metainfo/org.lucerna.Lucerna.metainfo.xml
/usr/share/doc/lucerna/   README, CHANGELOG, MANUAL-ACCEPTANCE, TROUBLESHOOTING (+ license)
```

`scripts/install-files.sh` installs the first five groups for both formats. There is no autostart
file, no systemd unit and no D-Bus service file: the GUI writes the per-user autostart entry.
Maintainer scripts never touch `$HOME`, so removing the package keeps `~/.config/lucerna` and the
user's media.

Dependencies: `mpv`, the GTK 4 runtime libraries (found automatically from the binaries: Debian
`libgtk-4-1 (>= 4.10)`, RPM ELF requirements) and, as a recommendation on Debian, a D-Bus session
bus. No private GTK is bundled.

## Building locally

Needs the Rust toolchain from rustup (`rust-toolchain.toml` selects the version; distribution
`rustc` packages are not used), plus:

* Debian/Ubuntu: `build-essential debhelper devscripts fakeroot pkgconf libgtk-4-dev jq git lintian`
* Fedora: `rpm-build gcc pkgconf-pkg-config gtk4-devel desktop-file-utils libappstream-glib jq git`

```sh
scripts/build-deb.sh   # -> dist/lucerna_<version>_<arch>.deb
scripts/build-rpm.sh   # -> dist/lucerna-<version>-<release>.<arch>.rpm
scripts/make-source-archive.sh [ref]
```

Both builders package the **committed** tree (`git archive HEAD`); commit first. Docker gives a
clean environment identical to CI: `ubuntu:24.04` and `fedora:44`.

Inspect: `dpkg-deb -I -c dist/*.deb`, `lintian --fail-on error dist/*.deb`, `rpm -qpi -qpl
dist/*.rpm`, `rpmlint dist/*.rpm` (informational).

## Smoke test

`scripts/smoke-test-package.sh <deb|rpm> --expect-version <semver>` runs in a **fresh** container
after installing the package, with no display and no session bus. It checks the installed files,
`--version`/`--help` of all three programs, that `lucerna` explains a missing display without a
panic, `lucernactl status` (exit 3) and `doctor` (mpv found, and this distribution's mpv accepts
every option Lucerna passes), and that removing the package keeps the user's configuration.

A wallpaper cannot be shown in a container. The smoke test says so; that check is
`docs/MANUAL-ACCEPTANCE.md`.

## Continuous integration

`.github/workflows/packages.yml` (push to `main`, pull requests touching packaging, manual runs,
and reuse by the release workflow) runs four jobs: `deb` (build in `ubuntu:24.04`, lintian,
AppStream and desktop-file validation), `deb-smoke` (fresh `ubuntu:24.04`), `rpm` (build in
`fedora:44`) and `rpm-smoke` (fresh `fedora:44`). Fedora is pinned to a numbered release, never
`latest` or `rawhide`.

## Known limits

* Builds download crates and the pinned toolchain, so they need network access; this is not
  distribution-archive policy (vendored sources for archive submission are future work).
* Only x86_64/amd64 is built and tested; nothing else is architecture-specific
  (`scripts/version.sh --deb-arch/--rpm-arch` map `uname -m`).
* The Rust crates are linked statically; their licenses are checked by `cargo deny` in CI.
