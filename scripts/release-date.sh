#!/usr/bin/env bash
# Print the release date (YYYY-MM-DD) of a version, taken from its CHANGELOG.md heading
#   ## [1.0.0-rc.1] - 2026-10-01
# so that package metadata (AppStream, Debian changelog, RPM changelog) is reproducible. If the
# version has no dated heading (a build of an unreleased tree), today's UTC date is used.
#
#   release-date.sh [semver]        (default: the workspace version)
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
version="${1:-$("$here/version.sh" --semver)}"
changelog="${LUCERNA_CHANGELOG:-$here/../CHANGELOG.md}"

date_line="$(grep -E -m1 "^## \[${version//./\\.}\] - [0-9]{4}-[0-9]{2}-[0-9]{2}" "$changelog" || true)"
if [[ -n "$date_line" ]]; then
    echo "${date_line##* - }"
else
    date -u +%F
fi
