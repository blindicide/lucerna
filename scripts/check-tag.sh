#!/usr/bin/env bash
# Release gate (directive §31, §40 steps 2-4): the tag must be annotated, must equal the Cargo
# workspace version, and the version must have a CHANGELOG section. Package metadata is never
# rewritten to hide a mismatch.
#
#   check-tag.sh vX.Y.Z
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
tag="${1:-}"
[[ -n "$tag" ]] || { echo "usage: check-tag.sh <tag>" >&2; exit 2; }

kind="$(git cat-file -t "refs/tags/$tag" 2>/dev/null || true)"
if [[ "$kind" != "tag" ]]; then
    echo "Release tags must be annotated: $tag is '${kind:-missing}'. Create it with: git tag -a $tag -m \"Lucerna $tag\"" >&2
    exit 1
fi

cargo_version="$("$here/version.sh" --semver)"
if [[ "${tag#v}" != "$cargo_version" ]]; then
    echo "Tag $tag does not match Cargo workspace version $cargo_version." >&2
    exit 1
fi

if [[ -z "$("$here/changelog-section.sh" "$cargo_version")" ]]; then
    echo "CHANGELOG.md has no section (or an empty one) for $cargo_version." >&2
    exit 1
fi

echo "tag $tag: annotated, matches the workspace version, changelog present"
