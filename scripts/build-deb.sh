#!/usr/bin/env bash
# Build the Debian package natively, from the committed tree (git archive HEAD).
#
#   build-deb.sh       -> dist/lucerna_<deb version>_<arch>.deb
#
# Needs: dpkg-dev, debhelper, fakeroot, pkgconf, libgtk-4-dev, git, jq, and the Rust toolchain
# from rustup on PATH. The Debian changelog is generated here from the workspace version and the
# release date in CHANGELOG.md; packaging/debian has no changelog file to keep in sync.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/.." && pwd)"
semver="$("$here/version.sh" --semver)"
debver="$("$here/version.sh" --deb)"
release_date="$("$here/release-date.sh" "$semver")"

stage="$root/build/deb"
src="$stage/lucerna-$debver"
rm -rf "$stage"
mkdir -p "$stage" "$root/dist"

git -C "$root" archive --format=tar --prefix="lucerna-$debver/" HEAD | tar -x -C "$stage"
cp -r "$src/packaging/debian" "$src/debian"

rfc2822="$(LC_ALL=C date -u -R -d "$release_date 12:00:00")"
cat >"$src/debian/changelog" <<CHANGELOG
lucerna ($debver) unstable; urgency=medium

  * Release $semver. See CHANGELOG.md for the release notes.

 -- The Lucerna authors <lucerna-maintainers@users.noreply.github.com>  $rfc2822
CHANGELOG

(cd "$src" && dpkg-buildpackage -us -uc -b)

deb="$(find "$stage" -maxdepth 1 -name 'lucerna_*.deb' | head -n1)"
[[ -n "$deb" ]] || { echo "build-deb.sh: no .deb was produced" >&2; exit 1; }
cp "$deb" "$root/dist/"
echo "$root/dist/$(basename "$deb")"
