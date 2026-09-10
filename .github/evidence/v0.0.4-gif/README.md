# GIF playback evidence

Real CLI PTY capture, 80 × 24 terminal, GIF width 32, shipped `ball.gif` fixture.
`pty-results.json` records output hashes, exit status, wall time and cursor state.
`recordings.zip` contains the original ANSI streams and asciinema v2 recordings
for truecolor, 256-color, 16-color, no-color, ASCII, two loops and interruption.
Extract the ZIP and replay a `.cast` file with an asciinema-compatible player.

```bash
python3 .github/evidence/v0.0.4-gif/capture.py \
  --binary target/debug/rich --fixture crates/rich-art/examples/assets/ball.gif
```

The script emits gzip-compressed raw streams and recordings; the checked-in ZIP
contains their decompressed contents. UTF-8 is decoded incrementally across PTY
read boundaries. Normal loops restore the cursor. SIGINT exits promptly but can
leave the cursor hidden, as documented for existing ASCII playback.

`frame-0.svg` and `frame-4.svg` visualize complete frames 1 and 5 extracted from
the recorded truecolor CLI stream at Live cursor-reset boundaries. Python Rich
15.0.0's `Text.from_ansi` and `Console.save_svg` display the captured characters
and colors; this is a transcript visualization, not CLI GIF export support.
The browser screenshots capture those two actual sequential CLI frames.

Independent review found no blockers. Added the requested mixed ASCII/block
Stage test for height-capped widths and shorter-neighbor padding, and clarified
16-color fidelity in the API docs. The original ASCII frame API is preserved.

GitHub review follow-up: options/parsing/help/defaults now live in
`rich-ext::cli`, with generic CLI delegation. ASCII-only consoles fall back to
ASCII, and decoded images are shared with `Arc` across frame renderers/stages
instead of being deep-copied. Regression tests cover both capabilities and
sharing. Independent sub-agent review confirmed all three fixes.
