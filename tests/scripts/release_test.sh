#!/usr/bin/env bash
# Tests for the release gate scripts: tag checks and changelog extraction.
set -uo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
scripts="$here/../../scripts"
failures=0
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

expect() { # description, expected exit code, command...
    local description="$1" want="$2"
    shift 2
    "$@" >"$tmp/out" 2>&1
    local got=$?
    if [[ "$got" != "$want" ]]; then
        echo "FAIL: $description: exit $got, wanted $want"
        sed 's/^/    /' "$tmp/out"
        failures=$((failures + 1))
    fi
}

# --- changelog-section.sh
cat >"$tmp/CHANGELOG.md" <<'CL'
# Changelog

## [Unreleased]

## [0.8.0] - 2026-09-30

### Added
- Thing one.

### Fixed
- Thing two.

## [0.7.0] - 2026-09-29

### Added
- Older.
CL
out="$(LUCERNA_CHANGELOG="$tmp/CHANGELOG.md" "$scripts/changelog-section.sh" 0.8.0)"
[[ "$out" == $'### Added\n- Thing one.\n\n### Fixed\n- Thing two.' ]] || { echo "FAIL: section body was: $out"; failures=$((failures + 1)); }
[[ -z "$(LUCERNA_CHANGELOG="$tmp/CHANGELOG.md" "$scripts/changelog-section.sh" 9.9.9)" ]] || { echo "FAIL: missing version should be empty"; failures=$((failures + 1)); }
[[ -z "$(LUCERNA_CHANGELOG="$tmp/CHANGELOG.md" "$scripts/changelog-section.sh" Unreleased 2>/dev/null)" ]] || true

# --- check-tag.sh in a throwaway repository whose workspace version we control.
repo="$tmp/repo"
mkdir -p "$repo/scripts" "$repo/crates/lucerna-core"
cp "$scripts/check-tag.sh" "$scripts/changelog-section.sh" "$repo/scripts/"
cat >"$repo/scripts/version.sh" <<'V'
#!/usr/bin/env bash
echo "${FAKE_WORKSPACE_VERSION:-0.7.0}"
V
chmod +x "$repo/scripts/"*.sh
cp "$tmp/CHANGELOG.md" "$repo/CHANGELOG.md"
(
    cd "$repo" || exit 1
    git init -q
    git config user.email t@example.invalid
    git config user.name t
    git add -A
    git commit -q -m init
    git tag -a v0.7.0 -m "Lucerna v0.7.0"
    git tag -a v0.8.0 -m "Lucerna v0.8.0"
    git tag v0.9.9 # lightweight
)
run_check() { (cd "$repo" && FAKE_WORKSPACE_VERSION="$1" scripts/check-tag.sh "$2"); }

expect "annotated tag equal to the workspace version passes" 0 run_check 0.7.0 v0.7.0
expect "tag newer than the workspace version fails (v0.8.0 vs 0.7.0)" 1 run_check 0.7.0 v0.8.0
grep -q "does not match Cargo workspace version 0.7.0" "$tmp/out" || { echo "FAIL: mismatch message"; failures=$((failures + 1)); }
expect "lightweight tag fails" 1 run_check 0.9.9 v0.9.9
grep -q "must be annotated" "$tmp/out" || { echo "FAIL: lightweight message"; failures=$((failures + 1)); }
expect "missing tag fails" 1 run_check 0.7.0 v1.2.3
expect "a version with no changelog section fails" 1 bash -c "cd '$repo' && git tag -a v0.6.1 -m x && FAKE_WORKSPACE_VERSION=0.6.1 scripts/check-tag.sh v0.6.1"
expect "usage error without a tag" 2 bash -c "cd '$repo' && scripts/check-tag.sh"

if ((failures > 0)); then
    echo "$failures failure(s)"
    exit 1
fi
echo "release_test: all checks passed"
