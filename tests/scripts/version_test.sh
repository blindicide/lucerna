#!/usr/bin/env bash
# Tests for scripts/version.sh: the SemVer -> deb/rpm/tarball mapping and its failure modes.
set -uo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
script="$here/../../scripts/version.sh"
failures=0

expect() { # version mode expected
    local got
    got="$(LUCERNA_VERSION_OVERRIDE="$1" "$script" "$2" 2>&1)"
    if [[ "$got" != "$3" ]]; then
        echo "FAIL: version=$1 mode=$2 expected '$3' got '$got'"
        failures=$((failures + 1))
    fi
}

expect_fail() { # version mode
    if LUCERNA_VERSION_OVERRIDE="$1" "$script" "$2" >/dev/null 2>&1; then
        echo "FAIL: version=$1 mode=$2 should have failed"
        failures=$((failures + 1))
    fi
}

expect 0.8.0 --semver 0.8.0
expect 0.8.0 --deb 0.8.0
expect 0.8.0 --rpm-version 0.8.0
expect 0.8.0 --rpm-release 1
expect 0.8.0 --tarball-stem lucerna-0.8.0

expect 1.0.0-rc.1 --semver 1.0.0-rc.1
expect 1.0.0-rc.1 --deb '1.0.0~rc1'
expect 1.0.0-rc.1 --rpm-version 1.0.0
expect 1.0.0-rc.1 --rpm-release 0.rc1
expect 1.0.0-rc.1 --tarball-stem lucerna-1.0.0-rc.1

expect 1.0.0 --deb 1.0.0
expect 1.0.0 --rpm-release 1
expect 1.0.0 --tarball-stem lucerna-1.0.0

expect_fail 1.0.0-beta.1 --deb
expect_fail 1.0.0-beta.1 --rpm-release
expect_fail 1.2 --semver
expect_fail garbage --semver
expect_fail 1.0.0 --no-such-mode

# The real workspace version must map cleanly, whatever it currently is.
real="$("$script" --semver)" || { echo "FAIL: cannot read the real workspace version"; failures=$((failures + 1)); }
if [[ -z "${real:-}" ]] || ! "$script" --deb >/dev/null; then
    echo "FAIL: real version has no deb mapping"
    failures=$((failures + 1))
fi

if ((failures > 0)); then
    echo "$failures failure(s)"
    exit 1
fi
echo "version_test: all checks passed"
