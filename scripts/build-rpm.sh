#!/usr/bin/env bash
# Build the RPM natively with rpmbuild, from the committed tree.
#
#   build-rpm.sh       -> dist/lucerna-<version>-<release>.<arch>.rpm
#
# Needs: rpm-build, gcc, pkgconf-pkg-config, gtk4-devel, desktop-file-utils, libappstream-glib,
# git, jq, and the Rust toolchain from rustup on PATH. Version, release and the source archive
# name come from scripts/version.sh; the spec contains none of them.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/.." && pwd)"
semver="$("$here/version.sh" --semver)"
rpm_version="$("$here/version.sh" --rpm-version)"
rpm_release="$("$here/version.sh" --rpm-release)"
release_date="$("$here/release-date.sh" "$semver")"

top="$root/build/rpm"
rm -rf "$top"
mkdir -p "$top"/{SOURCES,SPECS,BUILD,RPMS,SRPMS} "$root/dist"

"$here/make-source-archive.sh" >/dev/null
cp "$root/dist/lucerna-$semver.tar.gz" "$top/SOURCES/"

spec="$top/SPECS/lucerna.spec"
cp "$root/packaging/rpm/lucerna.spec" "$spec"
changelog_day="$(LC_ALL=C date -u -d "$release_date" '+%a %b %d %Y')"
{
    echo "* $changelog_day The Lucerna authors <lucerna-maintainers@users.noreply.github.com> - $rpm_version-$rpm_release"
    echo "- Release $semver. See CHANGELOG.md for the release notes."
} >>"$spec"

rpmbuild -bb \
    --define "_topdir $top" \
    --define "lucerna_version $rpm_version" \
    --define "lucerna_release $rpm_release" \
    --define "lucerna_tarball_version $semver" \
    "$spec"

rpm="$(find "$top/RPMS" -name 'lucerna-*.rpm' | head -n1)"
[[ -n "$rpm" ]] || { echo "build-rpm.sh: no .rpm was produced" >&2; exit 1; }
cp "$rpm" "$root/dist/"
echo "$root/dist/$(basename "$rpm")"
