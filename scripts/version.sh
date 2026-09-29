#!/usr/bin/env bash
# Print the Lucerna version in the form a given packaging system needs.
#
# The ONLY source of truth is the root Cargo workspace metadata (directive §29). Nothing else
# in the repository hardcodes a version.
#
#   version.sh [--semver|--deb|--rpm-version|--rpm-release|--tarball-stem|--deb-arch|--rpm-arch]
#
#   SemVer        --deb        --rpm-version  --rpm-release  --tarball-stem
#   0.8.0         0.8.0        0.8.0          1              lucerna-0.8.0
#   1.0.0-rc.1    1.0.0~rc1    1.0.0          0.rc1          lucerna-1.0.0-rc.1
#   1.0.0         1.0.0        1.0.0          1              lucerna-1.0.0
#
# Any other pre-release (-beta.1, ...) is an error: it has no agreed package mapping.
#
# For tests, LUCERNA_VERSION_OVERRIDE replaces the value read from cargo metadata; the
# mapping code is what is under test, not cargo.
set -euo pipefail

mode="${1:---semver}"
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

read_workspace_version() {
    if [[ -n "${LUCERNA_VERSION_OVERRIDE:-}" ]]; then
        printf '%s\n' "$LUCERNA_VERSION_OVERRIDE"
        return
    fi
    local metadata
    cd "$here/.."
    if ! metadata="$(cargo metadata --no-deps --format-version 1 --offline 2>/dev/null)"; then
        metadata="$(cargo metadata --no-deps --format-version 1)"
    fi
    jq -r '.packages[] | select(.name=="lucerna-core") | .version' <<<"$metadata"
}

fail() {
    echo "version.sh: $*" >&2
    exit 1
}

semver="$(read_workspace_version)"
[[ -n "$semver" && "$semver" != "null" ]] || fail "could not read the workspace version"

release_re='^([0-9]+)\.([0-9]+)\.([0-9]+)$'
rc_re='^([0-9]+)\.([0-9]+)\.([0-9]+)-rc\.([0-9]+)$'

if [[ "$semver" =~ $release_re ]]; then
    base="$semver"
    rc=""
elif [[ "$semver" =~ $rc_re ]]; then
    base="${BASH_REMATCH[1]}.${BASH_REMATCH[2]}.${BASH_REMATCH[3]}"
    rc="${BASH_REMATCH[4]}"
else
    fail "version '$semver' has no defined package mapping (only X.Y.Z and X.Y.Z-rc.N are supported)"
fi

case "$mode" in
    --semver) echo "$semver" ;;
    --deb) if [[ -n "$rc" ]]; then echo "${base}~rc${rc}"; else echo "$base"; fi ;;
    --rpm-version) echo "$base" ;;
    --rpm-release) if [[ -n "$rc" ]]; then echo "0.rc${rc}"; else echo "1"; fi ;;
    --tarball-stem) echo "lucerna-${semver}" ;;
    --deb-arch)
        case "$(uname -m)" in
            x86_64) echo amd64 ;;
            aarch64) echo arm64 ;;
            *) fail "unsupported architecture $(uname -m)" ;;
        esac
        ;;
    --rpm-arch) uname -m ;;
    -h | --help) sed -n '2,15p' "${BASH_SOURCE[0]}" ;;
    *) fail "unknown option '$mode'" ;;
esac
