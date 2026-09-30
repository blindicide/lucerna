#!/usr/bin/env bash
# Tests for the packaging helper scripts that need neither dpkg nor rpm: release-date.sh and
# install-files.sh (with stand-in binaries).
set -uo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
scripts="$here/../../scripts"
failures=0
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

expect_eq() { # description got want
    if [[ "$2" != "$3" ]]; then
        echo "FAIL: $1: got '$2' want '$3'"
        failures=$((failures + 1))
    fi
}

# --- release-date.sh reads the dated heading of the requested version.
cat >"$tmp/CHANGELOG.md" <<'CL'
# Changelog

## [Unreleased]

## [1.0.0-rc.1] - 2026-10-01

## [0.8.0] - 2026-09-30
CL
expect_eq "rc date" "$(LUCERNA_CHANGELOG="$tmp/CHANGELOG.md" "$scripts/release-date.sh" 1.0.0-rc.1)" "2026-10-01"
expect_eq "plain date" "$(LUCERNA_CHANGELOG="$tmp/CHANGELOG.md" "$scripts/release-date.sh" 0.8.0)" "2026-09-30"
expect_eq "undated falls back to today" "$(LUCERNA_CHANGELOG="$tmp/CHANGELOG.md" "$scripts/release-date.sh" 9.9.9)" "$(date -u +%F)"
# A version must not match a longer one that starts the same way (0.8.0 vs 0.8.01 is impossible,
# but 1.0.0 must not match the 1.0.0-rc.1 heading).
expect_eq "1.0.0 is not the rc" "$(LUCERNA_CHANGELOG="$tmp/CHANGELOG.md" "$scripts/release-date.sh" 1.0.0)" "$(date -u +%F)"

# --- install-files.sh installs the right files and resolves the metainfo placeholders.
mkdir -p "$tmp/bin"
for b in lucerna lucernad lucernactl; do printf '#!/bin/sh\n' >"$tmp/bin/$b"; chmod +x "$tmp/bin/$b"; done
"$scripts/install-files.sh" --destdir "$tmp/root" --prefix /usr --bindir "$tmp/bin" || failures=$((failures + 1))
for f in usr/bin/lucerna usr/bin/lucernad usr/bin/lucernactl \
    usr/share/applications/org.lucerna.Lucerna.desktop \
    usr/share/icons/hicolor/scalable/apps/org.lucerna.Lucerna.svg \
    usr/share/metainfo/org.lucerna.Lucerna.metainfo.xml; do
    [[ -e "$tmp/root/$f" ]] || { echo "FAIL: $f was not installed"; failures=$((failures + 1)); }
done
metainfo="$tmp/root/usr/share/metainfo/org.lucerna.Lucerna.metainfo.xml"
version="$("$scripts/version.sh" --semver)"
grep -q "release version=\"$version\"" "$metainfo" || { echo "FAIL: metainfo lacks the workspace version"; failures=$((failures + 1)); }
grep -q '@' "$metainfo" && { echo "FAIL: placeholder left in metainfo"; failures=$((failures + 1)); }

# --- a missing binary is an error, not a half-installed tree.
rm "$tmp/bin/lucernad"
if "$scripts/install-files.sh" --destdir "$tmp/root2" --prefix /usr --bindir "$tmp/bin" 2>/dev/null; then
    echo "FAIL: install succeeded with a missing binary"
    failures=$((failures + 1))
fi

if ((failures > 0)); then
    echo "$failures failure(s)"
    exit 1
fi
echo "packaging_test: all checks passed"
