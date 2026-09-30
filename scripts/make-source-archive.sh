#!/usr/bin/env bash
# Create the source archive dist/lucerna-<semver>.tar.gz from a Git ref (default HEAD).
#
#   make-source-archive.sh [ref]
#
# The archive is what `git archive` produces for the ref, with a lucerna-<semver>/ prefix and
# `gzip -n` (no timestamp), so the same commit always yields the same bytes.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$here/.."
ref="${1:-HEAD}"
stem="$("$here/version.sh" --tarball-stem)"

mkdir -p "$root/dist"
git -C "$root" archive --format=tar --prefix="$stem/" "$ref" | gzip -n >"$root/dist/$stem.tar.gz"
echo "$root/dist/$stem.tar.gz"
