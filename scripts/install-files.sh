#!/usr/bin/env bash
# Install Lucerna's files into a staging directory. Both the Debian rules file and the RPM spec
# call this, so the two packages contain the same logical files.
#
#   install-files.sh --destdir DIR [--prefix /usr] [--bindir DIR]
#
# --bindir defaults to $CARGO_TARGET_DIR/release, or target/release in the source tree.
#
# Installs: the three binaries, the desktop entry, the icon and the AppStream metainfo (with its
# release entry generated from the workspace version - never a second hard-coded copy).
# Documentation and the license are installed by the packaging tools (dh_installdocs, %doc).
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$here/.."
destdir=""
prefix="/usr"
bindir="${CARGO_TARGET_DIR:-$root/target}/release"

while (($# > 0)); do
    case "$1" in
        --destdir) destdir="$2"; shift 2 ;;
        --prefix) prefix="$2"; shift 2 ;;
        --bindir) bindir="$2"; shift 2 ;;
        *) echo "install-files.sh: unknown option '$1'" >&2; exit 2 ;;
    esac
done
[[ -n "$destdir" ]] || { echo "install-files.sh: --destdir is required" >&2; exit 2; }

semver="$("$here/version.sh" --semver)"
release_date="$("$here/release-date.sh" "$semver")"

for bin in lucerna lucernad lucernactl; do
    [[ -x "$bindir/$bin" ]] || { echo "install-files.sh: $bindir/$bin is missing; build it first" >&2; exit 1; }
    install -Dm755 "$bindir/$bin" "$destdir$prefix/bin/$bin"
done

install -Dm644 "$root/assets/desktop/org.lucerna.Lucerna.desktop" \
    "$destdir$prefix/share/applications/org.lucerna.Lucerna.desktop"
install -Dm644 "$root/assets/icons/hicolor/scalable/apps/org.lucerna.Lucerna.svg" \
    "$destdir$prefix/share/icons/hicolor/scalable/apps/org.lucerna.Lucerna.svg"

metainfo="$destdir$prefix/share/metainfo/org.lucerna.Lucerna.metainfo.xml"
install -Dm644 "$root/assets/desktop/org.lucerna.Lucerna.metainfo.xml" "$metainfo"
sed -i -e "s/@VERSION@/$semver/" -e "s/@DATE@/$release_date/" "$metainfo"
if grep -q '@[A-Z]*@' "$metainfo"; then
    echo "install-files.sh: unresolved placeholder in $metainfo" >&2
    exit 1
fi
