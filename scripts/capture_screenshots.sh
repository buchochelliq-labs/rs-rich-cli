#!/bin/sh
# Regenerate every image used in the documentation.
#
# The images are produced by rendering through the real library and exporting
# SVG (`Console::export_svg`), so a screenshot cannot drift from the behaviour it
# claims to show — if rendering changes, the image changes with it, and the diff
# shows up in review.
#
# SVG rather than PNG on purpose: it stays crisp at any zoom, the text is
# selectable and searchable, it diffs as text, and it needs no image tooling.
#
#     sh scripts/capture_screenshots.sh
#
# Animated images are built from numbered frames by scripts/build_animation.py,
# which this script invokes.
set -e

cd "$(dirname "$0")/.."
OUT=docs/assets
mkdir -p "$OUT"

echo "building the renderer…"
cargo build -q -p rs-rich --example screenshot

shot() {
    cargo run -q -p rs-rich --example screenshot -- "$1" "$OUT/$1.svg" 2>/dev/null
    printf '  %s\n' "$OUT/$1.svg"
}

echo "rendering stills…"
for demo in $(cargo run -q -p rs-rich --example screenshot -- --list); do
    shot "$demo"
done

echo "rendering animation frames…"
# A repo-relative directory, not mktemp: on Windows the shell's /tmp is not
# visible to the native cargo binary, so frames would be written nowhere.
FRAMES=target/doc-frames
rm -rf "$FRAMES"
mkdir -p "$FRAMES"
i=0
while [ "$i" -le 10 ]; do
    cargo run -q -p rs-rich --example screenshot -- \
        progress "$FRAMES/progress-$i.svg" --frame "$i" 2>/dev/null
    i=$((i + 1))
done

i=0
while [ "$i" -lt 12 ]; do
    cargo run -q -p rs-rich --example screenshot -- \
        spinner "$FRAMES/spinner-$i.svg" --frame "$i" 2>/dev/null
    i=$((i + 1))
done

echo "stitching animations…"
python scripts/build_animation.py "$FRAMES/progress-*.svg" "$OUT/progress-animated.svg" --duration 3.0
python scripts/build_animation.py "$FRAMES/spinner-*.svg"  "$OUT/spinner-animated.svg"  --duration 1.0

rm -rf "$FRAMES"
echo "done — $(find "$OUT" -name '*.svg' | wc -l) images in $OUT"
