#!/usr/bin/env bash
# Headless smoke test of an INSTALLED Lucerna package (directive §34, §35).
#
#   smoke-test-package.sh <deb|rpm> --expect-version <semver>
#
# Run it in a clean container after `apt-get install ./lucerna_*.deb` or `dnf install ./*.rpm`.
# No display, no Wayland, no session bus: exactly the situation of a build server. It checks that
# the files are installed, the three programs answer --version/--help, `lucerna` explains itself
# without a display, `lucernactl doctor` sees a compatible mpv, and that removing the package
# leaves the user's configuration alone.
#
# It CANNOT show a wallpaper, and says so: that needs a real desktop (docs/MANUAL-ACCEPTANCE.md).
set -uo pipefail

kind="${1:-}"
[[ "$kind" == "deb" || "$kind" == "rpm" ]] || { echo "usage: $0 <deb|rpm> --expect-version <semver>" >&2; exit 2; }
shift
expected=""
while (($# > 0)); do
    case "$1" in
        --expect-version) expected="$2"; shift 2 ;;
        *) echo "unknown option $1" >&2; exit 2 ;;
    esac
done
[[ -n "$expected" ]] || { echo "--expect-version is required" >&2; exit 2; }

failures=0
pass() { echo "  ok    $*"; }
fail() { echo "  FAIL  $*"; failures=$((failures + 1)); }
check() { # description, command...
    local description="$1"
    shift
    if "$@" >/dev/null 2>&1; then pass "$description"; else fail "$description"; fi
}

clean_env=(env -u DISPLAY -u WAYLAND_DISPLAY -u DBUS_SESSION_BUS_ADDRESS -u XDG_RUNTIME_DIR)

echo "== installed files"
for bin in lucerna lucernad lucernactl; do
    check "/usr/bin/$bin is executable" test -x "/usr/bin/$bin"
done
check "desktop entry" test -f /usr/share/applications/org.lucerna.Lucerna.desktop
check "icon" test -f /usr/share/icons/hicolor/scalable/apps/org.lucerna.Lucerna.svg
check "AppStream metainfo" test -f /usr/share/metainfo/org.lucerna.Lucerna.metainfo.xml
if grep -q "release version=\"$expected\"" /usr/share/metainfo/org.lucerna.Lucerna.metainfo.xml; then
    pass "metainfo release entry is $expected"
else
    fail "metainfo release entry is not $expected"
fi
if command -v desktop-file-validate >/dev/null 2>&1; then
    check "desktop entry validates" desktop-file-validate /usr/share/applications/org.lucerna.Lucerna.desktop
fi
if [[ "$kind" == "deb" ]]; then
    check "copyright file" test -f /usr/share/doc/lucerna/copyright
else
    check "license file" test -f /usr/share/licenses/lucerna/LICENSE
fi

echo "== --version and --help without a display"
for bin in lucerna lucernad lucernactl; do
    got="$("${clean_env[@]}" "$bin" --version 2>&1)"
    if [[ "$got" == "$bin $expected" ]]; then pass "$bin --version is '$got'"; else fail "$bin --version printed '$got', expected '$bin $expected'"; fi
    check "$bin --help exits 0" "${clean_env[@]}" "$bin" --help
done

echo "== lucerna without a display"
out="$("${clean_env[@]}" lucerna 2>&1)"
code=$?
if [[ $code -eq 1 ]]; then pass "exits 1"; else fail "exited $code, expected 1"; fi
if [[ "$out" == *"could not connect to a graphical display"* ]]; then pass "explains the problem"; else fail "message was: $out"; fi
if [[ "$out" == *panicked* ]]; then fail "panicked"; else pass "no panic"; fi

echo "== lucernactl"
"${clean_env[@]}" lucernactl status >/dev/null 2>&1
code=$?
if [[ $code -eq 3 ]]; then pass "status exits 3 when there is no daemon"; else fail "status exited $code, expected 3"; fi
report="$("${clean_env[@]}" lucernactl doctor --json 2>&1)"
code=$?
if [[ $code -eq 0 ]]; then pass "doctor --json exits 0"; else fail "doctor exited $code"; fi
if command -v python3 >/dev/null 2>&1; then
    check "doctor: mpv found" python3 -c 'import json,sys; sys.exit(0 if json.loads(sys.argv[1])["mpv"]["found"] is True else 1)' "$report"
    check "doctor: this distribution's mpv accepts every option Lucerna passes" \
        python3 -c 'import json,sys; sys.exit(0 if json.loads(sys.argv[1])["mpv"]["options_compatible"] is True else 1)' "$report"
elif command -v jq >/dev/null 2>&1; then
    check "doctor: mpv found" jq -e '.mpv.found == true' <<<"$report"
    check "doctor: mpv options compatible" jq -e '.mpv.options_compatible == true' <<<"$report"
else
    echo "  skip  doctor field checks (neither python3 nor jq is installed)"
fi

echo "== removal keeps the user's data"
config_dir="${XDG_CONFIG_HOME:-$HOME/.config}/lucerna"
mkdir -p "$config_dir"
echo 'schema_version = 1' >"$config_dir/config.toml"
if [[ "$kind" == "deb" ]]; then
    apt-get remove -y lucerna >/dev/null 2>&1
else
    dnf remove -y lucerna >/dev/null 2>&1
fi
for bin in lucerna lucernad lucernactl; do
    check "/usr/bin/$bin removed" test ! -e "/usr/bin/$bin"
done
check "configuration still there" test -f "$config_dir/config.toml"

echo
echo "NOTE: this checked installation and headless behaviour only. No wallpaper was shown or can be"
echo "shown here; that needs a real desktop (docs/MANUAL-ACCEPTANCE.md)."
if ((failures > 0)); then
    echo "$failures check(s) FAILED"
    exit 1
fi
echo "all package smoke checks passed"
