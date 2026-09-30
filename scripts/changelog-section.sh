#!/usr/bin/env bash
# Print the body of the CHANGELOG.md section for a version (used for release notes and to make sure
# every tagged version has a real section, directive §32).
#
#   changelog-section.sh [semver]      (default: the workspace version)
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
version="${1:-$("$here/version.sh" --semver)}"
changelog="${LUCERNA_CHANGELOG:-$here/../CHANGELOG.md}"

awk -v heading="## [$version]" '
    index($0, heading) == 1 { inside = 1; next }
    inside && /^## \[/ { exit }
    inside { print }
' "$changelog" | sed -e '/./,$!d' | sed -e ':a' -e '/^\n*$/{$d;N;ba' -e '}'
