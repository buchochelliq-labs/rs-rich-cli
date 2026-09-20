# Rich-art videos

## Run the suite in your terminal

CLI 0.0.8 preparation includes `rich --demo`: a single guided pass through the
suite with three-second section pauses. Use `--demo-delay 5` to slow it down;
Ctrl+C stops cleanly. See [the tour instructions](cli.md#take-the-guided-tour).

<video controls playsinline preload="none" poster="../assets/demos/v8-demo-tour.png" style="width:100%;max-width:1100px" aria-label="Guided tour of the rich CLI suite">
  <source src="../assets/demos/v8-demo-tour.mp4" type="video/mp4">
  Your browser cannot play this video. Use the download below.
</video>

[Tour MP4](assets/demos/v8-demo-tour.mp4) · [Tour GIF](assets/demos/v8-demo-tour.gif)

This recording runs `rich --demo --demo-delay 0.5` in a real terminal, faster
than the default three-second pace. Playback and command output are captured
from the optimized build, with a caption added and no audio. Reproduce with
`python scripts/capture_demo_tour.py --binary target/release/rich` after installing
the docs-media dependencies listed below.

## CLI 0.0.8 workflows

These clips replay actual output captured from an optimized `rich (rs-rich-cli)
0.0.8` binary in an 88 × 28 PTY. Command captions are added, each completed
result is held for two seconds, and there is no audio.

### All nine crop anchors

A deterministic asymmetric source makes the crop position visible. Every anchor
is recorded in a wide and a tall viewport: cover preserves the source aspect
ratio, so a particular viewport crops only one axis. Positions on the other
axis therefore produce the same crop.

<video controls playsinline preload="none" poster="../assets/demos/v8-crop-anchors.png" style="width:100%;max-width:1100px" aria-label="Nine crop anchors in wide and tall viewports">
  <source src="../assets/demos/v8-crop-anchors.mp4" type="video/mp4">
  Your browser cannot play this video. Use the download below.
</video>

[Download MP4](assets/demos/v8-crop-anchors.mp4) · [Looping GIF](assets/demos/v8-crop-anchors.gif)

### Plan a batch, then export in parallel

The first command uses `--dry-run`; the capture script checks that the destination
is still empty. The second exports both documents with `--jobs 2`, checks the
resulting HTML files, and records the real JSON completion report.

<video controls playsinline preload="none" poster="../assets/demos/v8-batch.png" style="width:100%;max-width:1100px" aria-label="Batch dry-run followed by parallel HTML export">
  <source src="../assets/demos/v8-batch.mp4" type="video/mp4">
  Your browser cannot play this video. Use the download below.
</video>

[Download MP4](assets/demos/v8-batch.mp4) · [Looping GIF](assets/demos/v8-batch.gif)

### Inspect configuration and overrides

`config validate`, default settings, the `preview` profile, and explicit CLI
width/color/pager overrides are shown in sequence. The fixture deliberately
places the profile before defaults to demonstrate that TOML document order does
not change precedence.

<video controls playsinline preload="none" poster="../assets/demos/v8-config.png" style="width:100%;max-width:1100px" aria-label="Configuration validation, profile selection and explicit overrides">
  <source src="../assets/demos/v8-config.mp4" type="video/mp4">
  Your browser cannot play this video. Use the download below.
</video>

[Download MP4](assets/demos/v8-config.mp4) · [Looping GIF](assets/demos/v8-config.gif)

[Capture evidence and benchmark results](https://github.com/buchochelliq-labs/rs-rich-cli/tree/main/.github/evidence/0.0.8)
include source/binary hashes, exact arguments, exit codes, raw ANSI/asciinema
recordings, fixtures and exported HTML. Reproduce from the repository root:

```bash
cargo build --release -p rs-rich-cli --locked
python -m pip install -r scripts/requirements-docs-media.txt
python scripts/capture_v8_demos.py
python scripts/bench_batch.py
```

The benchmark compares jobs 1, 2 and 4 using generated JSON inputs and fresh
HTML destinations. It checks that all exports are byte-identical across job
counts. Timings include process startup and disk writes; results depend on host,
input size and filesystem caches, and do not promise a parallel speedup. PTY
capture requires POSIX, FFmpeg and DejaVu fonts; the benchmark uses Python's
standard library only.

## CLI 0.0.7 demos

These clips were captured from the built `rich (rs-rich-cli) 0.0.7` binary (`target/release/rich`, default features).
They replay its actual PTY output; captions are added, still results are held
for two seconds, and watch output retains event timing sampled at 10 fps.
There is no audio.


### Image render modes

The same bundled cat image rendered as half-blocks, Braille and ASCII.

<video controls playsinline preload="none" poster="../assets/demos/release-image-modes.png" style="width:100%;max-width:1100px" aria-label="Image render modes">
  <source src="../assets/demos/release-image-modes.mp4" type="video/mp4">
  Your browser cannot play this video. Use the download below.
</video>

[Download MP4](assets/demos/release-image-modes.mp4)

### Fit, crop and transparency

Contain, cover, then a translucent input composited over a purple background.

<video controls playsinline preload="none" poster="../assets/demos/release-image-fit.png" style="width:100%;max-width:1100px" aria-label="Fit, crop and transparency">
  <source src="../assets/demos/release-image-fit.mp4" type="video/mp4">
  Your browser cannot play this video. Use the download below.
</video>

[Download MP4](assets/demos/release-image-fit.mp4)

### Watch and batch workflows

A file changes, becomes invalid and recovers; batch then exports two HTML files.

<video controls playsinline preload="none" poster="../assets/demos/release-workflows.png" style="width:100%;max-width:1100px" aria-label="Watch and batch workflows">
  <source src="../assets/demos/release-workflows.mp4" type="video/mp4">
  Your browser cannot play this video. Use the download below.
</video>

[Download MP4](assets/demos/release-workflows.mp4)

[Commands and source/binary hashes](https://github.com/buchochelliq-labs/rs-rich-cli/tree/main/.github/evidence/release-finish)
are committed with raw ANSI/asciinema recordings and the exported HTML files.
The alpha fixture is generated deterministically by the capture script.
Run `cargo build --release -p rs-rich-cli --locked`, install the media requirements below, then
`python scripts/capture_release_demos.py` to capture and encode again.
The capture needs a POSIX PTY, FFmpeg and the DejaVu fonts.
See [workflow recipes](recipes.md) and [image fitting options](cli.md#fit-crop-and-transparent-backgrounds).

## Archived GIF playback

The same bouncing-ball GIF rendered with truecolor half-blocks and colored ASCII.
These clips replay real output from our archived CLI recordings. They have no audio.

## Compare the renderers

<video controls playsinline preload="none" poster="../assets/demos/rich-art-comparison.png" style="width:100%;max-width:980px" aria-label="Bouncing ball rendered as half-blocks and ASCII">
  <source src="../assets/demos/rich-art-comparison.mp4" type="video/mp4">
  Your browser cannot play this video. Use the MP4 download below.
</video>

[Download comparison MP4](assets/demos/rich-art-comparison.mp4) ·
[View looping GIF](assets/demos/rich-art-comparison.gif)

The left side uses upper-half block characters to represent two vertical pixels
per terminal cell. The right side uses ASCII characters; both retain color.

## Half-block rendering

<video controls playsinline preload="none" poster="../assets/demos/rich-art-truecolor.png" style="width:100%;max-width:480px" aria-label="Half-block bouncing-ball animation">
  <source src="../assets/demos/rich-art-truecolor.mp4" type="video/mp4">
  Your browser cannot play this video. Use the MP4 download below.
</video>

[Download half-block MP4](assets/demos/rich-art-truecolor.mp4)

From a repository checkout, in a truecolor terminal:

```bash
cargo run -p rs-rich-cli -- --gif crates/rich-art/examples/assets/ball.gif --gif-mode blocks --width 32 --loop 1
```

## ASCII rendering

<video controls playsinline preload="none" poster="../assets/demos/rich-art-ascii.png" style="width:100%;max-width:480px" aria-label="Colored ASCII bouncing-ball animation">
  <source src="../assets/demos/rich-art-ascii.mp4" type="video/mp4">
  Your browser cannot play this video. Use the MP4 download below.
</video>

[Download ASCII MP4](assets/demos/rich-art-ascii.mp4)

```bash
cargo run -p rs-rich-cli -- --gif crates/rich-art/examples/assets/ball.gif --gif-mode ascii --width 32 --loop 1
```

Animation needs an interactive terminal. Redirected output renders one frame;
color support depends on terminal capabilities. See the [CLI guide](cli.md).

## How these videos were made

The source is the project's **v0.0.4 PTY evidence**, captured in an 80 × 24 terminal
with a 32-column image. These are replays of that recorded output, not fresh
recordings of the current checkout. The generator verifies the saved ANSI SHA-256
hashes and checks that the asciinema output matches the ANSI stream.

The replay keeps complete frames, crops unused terminal space, uses DejaVu Sans
Mono, and displays each frame for 100 ms. MP4s repeat the sequence three times;
the preview GIF loops continuously. This presentation timing differs from the
original recording. Titles and comparison layout are added by the media script.

- [Original recordings and capture script](https://github.com/buchochelliq-labs/rs-rich-cli/tree/main/.github/evidence/v0.0.4-gif)
- [Media generator](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/scripts/build_docs_media.py)

To rebuild the media from the repository root (requires FFmpeg, Cairo and the
DejaVu Sans Mono font installed on the system):

```bash
python -m pip install -r scripts/requirements-docs-media.txt
python scripts/build_docs_media.py
```

The progress and spinner GIFs elsewhere in the docs are conversions of committed
SVG frame sequences exported by the library. They are not terminal recordings.
