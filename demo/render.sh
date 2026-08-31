#!/usr/bin/env bash
# demo/render.sh — regenerate demo/alint.gif from demo/alint.tape.
#
# Use this, not a bare `vhs demo/alint.tape`. The tape alone produces a GIF that
# plays 2.4x too fast, for a reason worth understanding before you "fix" it:
#
#   VHS records by screenshotting a headless browser IN REAL TIME. At the 2x
#   canvas we need for sharp text (1760x1520), it cannot screenshot fast enough,
#   so it silently DROPS frames while still tagging the output 25fps. The result
#   is a complete but comically fast demo (343 frames tagged 25fps = 13.7s for a
#   demo that really takes 33.3s). VHS gives no warning.
#
# So: render, then re-time the GIF back to the true wall-clock duration.
#
# WHY 2x AT ALL: most people read GitHub on a HiDPI display. A 1x (880px) asset
# gets upscaled 2x by the browser, and small antialiased terminal glyphs turn to
# mush -- that was the original "the letters are blurry" bug. Rendering at 1760px
# and displaying it at 880 CSS px means HiDPI maps it 1:1 to device pixels (sharp)
# and 1x displays downscale it exactly 2:1 (clean). The README <img> therefore
# carries width="880" and MUST keep it: without it the browser would show the
# 1760px source at full width and blow out the column.
set -euo pipefail

cd "$(dirname "$0")/.."

# Target GIF duration. NOT the real wall-clock length (that is ~33.3s, measured
# from an 880x760 render where VHS captured every frame). We deliberately play it
# a touch faster: the first published demo ran at 29.16s because VHS was
# frame-dropping at 1100px, and that snappier pace read better, so we reproduce it
# on purpose here instead of leaving it to a capture artifact. setpts scales the
# complete 2x-captured frames to this duration -- uniform speedup of typing and
# pauses alike, which is exactly what the frame-drop happened to do. Nudge this if
# the pace feels off; it is a pure playback-speed knob, no re-render needed.
TRUE_DURATION=29.16

command -v vhs >/dev/null    || { echo "need vhs (charmbracelet/vhs)"; exit 1; }
command -v ffmpeg >/dev/null || { echo "need ffmpeg"; exit 1; }
[ -x target/release/alint ]  || cargo build --release -p alint

export PATH="$PWD/target/release:$PATH"
export ALINT_DEMO_FIXTURE="$PWD/demo/fixture"

echo "==> vhs demo/alint.tape"
vhs demo/alint.tape

raw=$(mktemp --suffix=.gif)
trap 'rm -f "$raw"' EXIT
mv demo/alint.gif "$raw"

cur=$(ffprobe -v error -show_entries format=duration -of csv=p=0 "$raw")
factor=$(python3 -c "print(f'{$TRUE_DURATION/$cur:.5f}')")
echo "==> re-timing: ${cur}s -> ${TRUE_DURATION}s (x${factor}); VHS dropped frames at the 2x canvas"

# dither=none keeps glyph edges hard; Floyd-Steinberg would smear them.
ffmpeg -v error -i "$raw" -filter_complex \
  "[0:v]setpts=PTS*${factor}[v];[v]split[a][b];\
   [a]palettegen=max_colors=256:stats_mode=full[p];\
   [b][p]paletteuse=dither=none:diff_mode=rectangle" \
  -y demo/alint.gif

w=$(ffprobe -v error -select_streams v:0 -show_entries stream=width -of csv=p=0 demo/alint.gif)
d=$(ffprobe -v error -show_entries format=duration -of csv=p=0 demo/alint.gif)
echo "==> demo/alint.gif: ${w}px wide, ${d}s, $(du -h demo/alint.gif | cut -f1)"
echo "    README displays it at width=\"880\" (half of $w) so HiDPI maps it 1:1."
echo
echo "Now re-run the drift gate, which asserts the demo still tells the truth:"
echo "    ci/scripts/demo-drift.sh"
