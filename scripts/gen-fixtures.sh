#!/usr/bin/env bash
# Regenerate the tiny synthetic media fixtures in tests/fixtures/media.
#
# Needs ffmpeg with libx264 and libvpx-vp9. The Lucerna runtime never needs ffmpeg; this script is
# only for maintainers. Every clip is a 64x64, 1 second, low-rate synthetic test pattern generated
# from lavfi `testsrc`, so there is no third-party content and no copyright: CC0.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
out="$here/../tests/fixtures/media"
mkdir -p "$out"

src=(-f lavfi -i "testsrc=size=64x64:rate=10:duration=1")
common=(-y -hide_banner -loglevel error -an -map_metadata -1 -fflags +bitexact -flags:v +bitexact)

ffmpeg "${src[@]}" "${common[@]}" -c:v libx264 -preset veryslow -crf 40 -pix_fmt yuv420p -movflags +faststart "$out/sample.mp4"
ffmpeg "${src[@]}" "${common[@]}" -c:v libvpx-vp9 -b:v 0 -crf 50 -pix_fmt yuv420p "$out/sample.webm"
ffmpeg "${src[@]}" "${common[@]}" -c:v libx264 -preset veryslow -crf 40 -pix_fmt yuv420p "$out/sample.mkv"
ffmpeg "${src[@]}" "${common[@]}" -vf "fps=10,palettegen=max_colors=16" -frames:v 1 -update 1 "$out/.palette.png"
ffmpeg "${src[@]}" -i "$out/.palette.png" "${common[@]}" -lavfi "fps=10[x];[x][1:v]paletteuse" -loop 0 "$out/sample.gif"
rm -f "$out/.palette.png"

# A file with a video extension that is not a video at all: mpv must fail on it gracefully.
head -c 4096 /dev/urandom >"$out/corrupt.mp4"

ls -l "$out"
