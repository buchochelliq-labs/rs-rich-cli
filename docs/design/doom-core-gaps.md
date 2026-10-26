# What rs-rich-doom needed from rs-rich: gaps for core

Copied on 2026-10-25 from rs-rich-doom's `docs/core-gaps.md` (its branch
`claude/next-priority-items-3c5p8e`, at `2ca5783`). The maintainer moved
these gaps to [0.0.23](../plans/0.0.23.md), except gap 4, which lands in
[0.0.22](../plans/0.0.22.md) with workstream 2. This copy is what the
plans cite; Doom's own copy may move on.

Doom is a demanding, repeatable workload for rs-rich: 35 new frames a second
of 320x200 pixels, keys held and released, a clock to keep, and sound. This is
what it needed and had to build itself, written against the published 0.0.21
crates (rs-rich 0.0.11, rs-rich-ext 0.0.18, rs-rich-art 0.0.14,
rs-rich-interact 0.0.9, rs-rich-intuituive 0.0.6). The integration branch
`claude/next-priority-items-3c5p8e-wip-integ3` (2135f4b) was checked too:
rs-rich-doom builds and passes on it, and none of these gaps is closed there.
Each gap below now opens with its status in 0.0.23.

Each entry follows rs-rich's rule that core holds generic pieces only. Each
proposal names a second, non-Doom use, and nothing Doom-specific (WAD, DMX,
MUS, OPL, Doom's keys or pan law) is proposed for core. Those stay here.

What already landed and is used here: pixel cells (`Canvas::put_rgb` and the
`pixels_fit` node), style-table compaction, held keys (`held_keys`,
`all_keys`, `synthesize_releases`), `fixed_step` with catch-up, `rate`,
`StatsHandle`, `DrawCx::rect()` and the audio output (`audio::output`, volume).

## The review's three findings, verified on 0.0.21

| Finding | Verdict on 0.0.21 |
|---|---|
| A live in-memory frame source for intuiTUIve's `art` image nodes | **Still open** (gap 1). `image_bytes_of` takes *encoded* bytes, decodes each change on a worker thread and shows "loading…" until it is done. That's fine for a cover picture, but at 35 frames a second it means a PNG encode and decode per frame plus a flicker. Each new picture gets a new Kitty id, and they are freed only when the app ends. Sixel always goes through the adaptive quantizer at `CellPixels::DEFAULT` rather than the measured cell size. rs-rich-doom keeps its own `PlacementSource`. |
| A public indexed-plus-palette Sixel encoder in rs-rich-art | **Still open** (gap 2). `sixel::encode_indexed` is `pub(crate)` and maps indices through the fixed ANSI palette. rs-rich-doom keeps `graphics::sixel_indexed`. |
| Style-table compaction over a long run | **Fixed and measured.** `tests/app.rs` runs the real engine under the headless driver at 80x24 in Braille (averaged colours, the most styles) for 120 s of game time, walking and turning. Styles kept: 1,337 after the first second, peak 2,766, peak in the last minute 2,605, 2,056 at the end. That is bounded by twice the screen's cells (3,840), as the compaction rule says. Resident memory went from 24.9 MB to 25.3 MB. The test asserts the bound. |

## Gaps

### 1. A live frame image node (intuiTUIve, `art` plugin)

**Status: closed in 0.0.23** (wave 1, `ad84120f`). intuiTUIve `art`'s
`picture_of(picture: impl Fn() -> Option<Picture>, view: ImageView)` draws
each new frame at once, with no worker and no "loading". rs-rich-art's
`Picture::from_rgba` and `Picture::next_rgba` make a frame from raw RGBA,
and a frame of the same picture keeps its id: Kitty gets it again under
that id (raw pixels, `f=32`), and `image_graphics` frees it once. The
pictures are fitted to the cells as the cells were drawn. Not taken
from the proposal: `ImageView::aspect` (a picture shown at a ratio other
than its pixels'; gap 12's `PixelBuf::draw_fitted` does it for pixel
cells, and an app can resample a frame before handing it over),
`ImageView::cell_px` and `keep_last_row`. Tests: `image.rs`'s
`live_frames_keep_one_kitty_id_and_are_freed_once`,
`sixel_frames_are_drawn_again_and_counted` and
`an_overlay_picture_partly_covered_is_not_drawn_over_what_covers_it`, and
`picture_of`'s doc-test (half blocks, no worker).

- **What Doom needed:** show a new in-memory RGBA frame 35 times a second as
  real pixels (Kitty, Sixel, iTerm2), fitted to the node and kept to an aspect
  ratio, with a cell fallback and cleanup.
- **Workaround:** `src/graphics.rs` (`Pictures`, a `PlacementSource`, and
  `Picture`, a `Graphic`) and `app::ImageView`, a widget that draws Kitty
  placeholders and hands the frame and its screen rectangle
  (`DrawCx::rect()`) to `Pictures`. It also keeps one Kitty image id, reserves
  Sixel's last row, centres a capped Sixel footprint, and sizes images from
  the measured cell size.
- **Proposed API (rs-rich-intuituive, `art`):**
  `image_frames(frame: impl Fn() -> Option<Arc<RgbaFrame>> + 'static, view: ImageView) -> Node`
  where `RgbaFrame { width, height, pixels: Vec<[u8; 4]>, palette_exact: bool }`
  (or a `PixelBuf`). It draws synchronously, with no worker and no "loading".
  One protocol image id per node, reused for every frame and deleted on
  unmount. `ImageView::cell_px` (default: the measured `GraphicsEnvironment`
  size). `ImageView::aspect(Option<f64>)` for a picture shown at a ratio other
  than its pixels' (Doom's 320x200 is shown at 4:3). `ImageView::keep_last_row`
  for Sixel. With `palette_exact` and 256 colours or fewer, Sixel uses gap 2's
  encoder.
- **Why generic:** an emulator's screen (NES, Game Boy, CHIP-8), a video or
  webcam preview, a live plot rendered to pixels, a remote desktop.
- **Acceptance tests:** under the driver, a node fed 100 frames shows each
  frame's placement with one Kitty id (the escape stream holds one `a=d`, on
  unmount). Resizing the node moves and refits the placement. Removing the node
  repaints its cells. The cell fallback (`Pixels::Cells`) draws half blocks
  with no worker. A Sixel image capped by width is centred. A frame at 4:3 in
  80x24 cells with 1:2 cells covers 64x24.

### 2. A public Sixel encoder for indices and a palette (rs-rich-art)

**Status: closed in 0.0.23.** `rich_art::sixel::encode_palette(indices,
palette, width, height) -> Option<String>` and
`rich_art::sixel::index_exact(rgba) -> Option<(indices, palette)>`, behind
`sixel`. The crate's own fixed-ANSI encoder is now `encode_palette` over
the 256-colour palette, byte for byte as before. Its bytes match
rs-rich-doom's encoder but for P1, which the raster attributes override
(9 here, as in every encoder in the crate; 0 there). Tests:
`a_palette_of_ones_own_encodes_exactly_and_validates` (rs-rich-doom's
bytes for a 3x7 image, `icy_sixel` decodes the size and `#123456` within
Sixel's precision, `None` left unset, a long run split, past the palette,
short or past `MAX_PIXELS` refused) and
`exact_indexing_keeps_up_to_256_colours`. Not done: `Picture`'s Sixel path
still quantizes; using `encode_palette` there for frames of 256 colours or
fewer would change the Sixel bytes of pictures already in tapes.

- **What Doom needed:** encode a frame that already has at most 256 colours
  with exactly those colours, quickly: no quantizing, no resampling.
- **Workaround:** `graphics::sixel_indexed` (about 90 lines), with a fallback
  to `SixelArt::encode_cells` (adaptive) past 256 colours.
- **Proposed API (rs-rich-art, `sixel`):**
  `pub fn encode_palette(indices: &[Option<u8>], palette: &[[u8; 3]], width: usize, height: usize) -> Option<String>`
  (`None` is transparent). It is the existing `encode_indexed` with the
  palette passed in, keeping its repeat-count splitting and `MAX_PIXELS`
  check. Plus `pub fn index_exact(rgba: &[[u8; 4]]) -> Option<(Vec<Option<u8>>, Vec<[u8; 3]>)>`
  for images of 256 colours or fewer.
- **Why generic:** pixel art and sprite editors, emulators with fixed
  palettes, indexed PNG and GIF frames, terminal plots in a few colours.
- **Acceptance tests:** a 3x7 two-colour image encodes to the same bytes
  rs-rich-doom's encoder makes. Decoding with `icy_sixel` gives the size and
  every palette colour back within Sixel's 1% precision, including non-ANSI
  colours (`#123456`). `None` pixels are left unset (P2=1). More than
  `MAX_PIXELS` returns `None`. A run longer than 65,535 is split. A palette
  index past the palette's end returns `None`.

### 3. Kitty raw-pixel transmission and placement ids (rs-rich-art)

**Status: closed in 0.0.23.** Wave 1 added `kitty::transmit_rgba`
(`f=32`, chunked), which `Picture` uses for raw frames. This gap's work
added the rest: `kitty::transmit_rgb` (`f=24`) and
`kitty::transmit_raw(&RawImage, &Transmit)`, where `Transmit` carries the
image id, a placement id (`p`), the cells and zlib compression (`o=z`,
through flate2, which png already brings). Tests: rs-rich-art's
`raw_pixels_are_chunked_and_round_trip_compressed_or_not` (keys, chunks
of at most 4096 bytes, payloads read back, compressed or not), and
rs-rich-record's `tests/kitty_raw.rs`, where the emulator decodes RGB and
RGBA, compressed or not, to the pixels sent.

- **What Doom needed:** send a new frame each tic without a PNG encode.
- **Workaround:** PNG through `graphics::encode_png` each frame, about 180 kB
  a frame and the main cost of Kitty mode.
- **Proposed API (rs-rich-art, `kitty`):**
  `transmit_rgba(id, placement: Option<u32>, rgba: &[u8], width, height, cols, rows, compress: bool) -> String`
  (`f=32`, `o=z` when compressing, chunked), and `transmit_rgb` (`f=24`).
- **Why generic:** video, emulators and live plots in Kitty, and any image
  that changes often.
- **Acceptance tests:** escapes parse as the Kitty protocol (`f=32`, `s`,
  `v`, `o=z`, `m=1` chunks of at most 4096 bytes). rs-rich-record's Kitty
  emulator decodes them to the same pixels. Compressing is optional and
  round-trips.

### 4. Made-up releases with a first-press hold and a repeat hold (intuiTUIve)

**Status: closed in 0.0.22** (`e7651f81`): `App::synthesize_releases_with(Holds::new(first, repeat))`.
Tests: `tests/input_timing.rs`, with `Holds::new(400 ms, 120 ms)`.

- **What Doom needed:** on terminals without release reports, hold a key
  across the keyboard's repeat delay after the first press, but let it go
  soon after the last repeat. Before, rs-rich-doom used 400 ms after the first
  press and 120 ms after each repeat.
- **Workaround:** none that keeps using held keys. `synthesize_releases` takes
  one hold, so rs-rich-doom passes `--hold-ms` (400). After a key is let go on
  a legacy terminal, the player keeps moving for up to 400 ms (before:
  120 ms).
- **Proposed API (rs-rich-intuituive):**
  `App::synthesize_releases_with(Holds { first: Duration, repeat: Duration })`,
  where `synthesize_releases(Some(d))` stays `Holds { first: d, repeat: d }`.
  Optionally learn `repeat` from the measured gap between repeats.
- **Why generic:** any game or held-key control on a legacy terminal: a
  block puzzle's soft drop, a raycaster's movement, a scrubber held on a
  timeline.
- **Acceptance tests:** under the driver with `Releases::None` and
  `Holds { first: 400 ms, repeat: 120 ms }`: a press at 0 is held at 399 ms
  and released at 400 ms. Repeats at 450, 480 and 510 ms keep it held, and it
  is released at 630 ms. A key whose release is reported never times out.

### 5. A worker on the app's clock, in lockstep (intuiTUIve)

**Status: closed in 0.0.23.** `stepped_worker(period, max_catch_up, |steps:
Steps<T>| ...) -> Signal<Option<T>>`, and `stepped_worker_with(Lockstep,
work) -> (Signal<Option<T>>, Timer)` for the timeout (250 ms by default)
and the timer. `Steps::wait() -> Option<u64>` blocks for ticks and returns
`None` once the app has gone; `Steps::publish(T)` hands output over. It is
a `fixed_step` underneath. Tests: `tests/lockstep.rs` (a second at 35 a
second is 35 ticks and tick 35's output; a 300 ms stall hands over
`max_catch_up`; a worker that ends makes each step return at once and the
app quits; a worker asleep for a second times out and catches up), and
`stepped_worker`'s doc-test.

- **What Doom needed:** run a simulation that must live on its own thread
  (the C engine is global and blocking) so that it advances exactly as the
  app's clock does: real time in a terminal, exact time under the driver.
  The app takes its output once the simulation has caught up.
- **Workaround:** `engine::Pace` and `engine::Pacer`. A `fixed_step` sends the
  tics given so far. The engine's sleep reports "idle at tic n" and blocks for
  more. The step waits (at most 250 ms) for the report, then takes the newest
  frame from a shared slot. Exit drops the channel, so the step learns at once.
- **Proposed API (rs-rich-intuituive):**
  `stepped_worker(period, max_catch_up, |steps: Steps| { /* worker thread */ })`
  returns a `Signal<Option<T>>` of the worker's output. `Steps::wait()` blocks
  until the app gives ticks and returns how many. `Steps::publish(T)` hands
  output over. The app's step waits for the worker to finish the ticks, with a
  timeout. Generic over `T`; no Doom.
- **Why generic:** an emulator core, a physics or cellular-automaton
  simulation, a WASM or script VM, a deterministic replay. Anything whose
  stepping blocks and must be tested exactly under the driver.
- **Acceptance tests:** under the driver, `advance(1 s)` at a 1/35 s period
  gives the worker exactly 35 ticks, and the signal holds tick 35's output
  when `advance` returns. A stall of 300 ms hands over at most `max_catch_up`
  ticks. A worker that ends makes the next step return at once (no timeout)
  and the app can quit. A slow worker (sleeps 1 s) lets the step time out and
  catch up later.

### 6. Frame timings in `FrameStats` (intuiTUIve)

**Status: closed in 0.0.23** (wave 1, `7d285128`). `FrameStats` has
`draw`, `paint`, `write`, `painted`, and the totals `total_bytes`,
`total_draw`, `total_paint`, `total_write`, `frames` and `skipped`;
`App::timedemo` runs a script at full speed and reports each frame.
Tests: `tests/timedemo.rs` (a widget that sleeps 5 ms shows `draw` of
5 ms or more; `total_bytes` is the sum of each render's bytes).

- **What Doom needed:** `--fps` and `--stats` used to show the time a draw
  took and the bytes over a run.
- **Workaround:** dropped. `--stats` reports frames, tics, the newest engine
  frame, frames held back and the last frame's bytes. Per-node timings exist
  only through the inspector (`Driver::inspector_timings`).
- **Proposed API (rs-rich-intuituive):** `FrameStats` gains `draw: Duration`
  (widgets' draw), `paint: Duration` (diff and escape writing), and
  cumulative `total_bytes: u64` and `total_draw: Duration`.
- **Why generic:** any app's performance overlay, benchmarks under the
  driver, regression gates like the ratatui comparison.
- **Acceptance tests:** a widget that sleeps 5 ms in `draw` shows `draw`
  ≥ 5 ms. `total_bytes` is the sum of each render's output length.
  Durations are zero under a fake clock if the driver says so.

### 7. A voice mixer for sound effects (intuiTUIve, `pcm`/`audio`)

**Status: closed in 0.0.23.** `audio::Mixer` (with `pcm`):
`play(channel, clip: Arc<Clip>, gain, pan) -> Voice` (a sound on a
channel stops the one there), `Voice::set(gain, pan)`, `stop`,
`is_playing`, `Mixer::source(Box<dyn Source>)` for a stream, and
`stop_all`. Clips and streams at any rate are resampled linearly. The pan
is a balance: 0 plays the sound at its gain on both sides, -1 the left
only; Doom's own pan law stays in Doom. `Mixer::new(&Output)` (with
`audio`) mixes 256-frame blocks into the output on its own thread;
`Mixer::detached(rate, channels)` and `Mixer::mix(&mut [f32])` mix by
hand, from a renderer (gap 8) or a test. Tests: `tests/audio_mixer.rs`
(two clips sum sample for sample; 11,025 Hz into 44,100 Hz lasts four
times as many frames; pan -1 is left only; `stop` silences the next
block; `is_playing` ends within a block of the clip's end; a stream under
the voices; the thread into a null output, read through a tap), and
`Mixer`'s doc-test.

- **What Doom needed:** play many short sounds at once on numbered channels,
  each at its own sample rate, gain and pan, stopped or changed while
  playing, mixed into the output.
- **Workaround:** `sound::Mixer` and `sound::Voice`, plus a mixer thread that
  writes 256-frame blocks to `audio::Output`. The DMX lump format and
  Doom's pan law (`gains`) are Doom's own and stay.
- **Proposed API (rs-rich-intuituive, `audio`):**
  `audio::Mixer::new(output: &Output) -> Mixer` with
  `play(channel: usize, clip: Arc<Clip>, gain: f32, pan: f32) -> Voice`,
  `Voice::set(gain, pan)`, `stop`, `is_playing`, and `Mixer::source(Box<dyn Source>)`
  to add a stream (music). Clips at any rate are resampled linearly; it mixes
  on its own thread into the output.
- **Why generic:** sound effects for any game (the block-puzzle and raycaster
  clones), UI sounds, a sampler.
- **Acceptance tests:** with a test sink (gap 9), two clips started together
  sum sample for sample. A clip at 11,025 Hz into 44,100 Hz lasts four times
  as many frames. Pan −1 is left only. `stop` silences from the next block.
  `is_playing` turns false within one block of the clip's end.

### 8. A render callback output for synthesizers (intuiTUIve, `audio`)

- **What Doom needed:** the OPL synthesizer produces samples on demand, and
  its song timing runs off the samples it makes.
- **Workaround:** the mixer thread pulls music into each block and writes it
  to the output's queue. The queue adds 60 ms of latency to effects, the price
  of a push model.
- **Proposed API (rs-rich-intuituive, `audio`):**
  `audio::output_with(options, render: impl FnMut(&mut [f32], channels) + Send + 'static) -> Output`:
  the device's callback calls `render` directly. Volume, mute and `failed`
  work as for `output`.
- **Why generic:** synthesizers, trackers and chiptune players, tone
  generators, anything that makes sound rather than playing a file.
- **Acceptance tests:** with a test sink, a 440 Hz sine renderer produces
  440 zero-crossing pairs a second. Volume 0 gives silence without stopping
  the renderer. A panicking renderer reports `failed()` rather than taking
  down the app.

### 9. A paced test sink and capture for audio output (intuiTUIve, `audio`)

**Status: closed in 0.0.23.** `OutputOptions::null()` opens an `Output`
with no device (44,100 Hz in two channels unless set) that takes samples
at its rate on the real clock, never more than its queue ahead.
`Output::tap(keep: Duration) -> Tap` records what any output's device
takes, after the volume: the newest `keep` of it (`Tap::samples`), the
frames in all (`Tap::frames`), and `Tap::to_wav(path)` (32-bit float)
with `wav`. The null output runs on the real clock only, not the app's:
under the driver a test waits real time for it. Tests:
`tests/audio_null.rs` (44,100 frames a second within 2%, a writer held to
the queue; volume 0 taps only zeros; a 1 s tap keeps 1 s; hound reads the
WAV back the same), and `OutputOptions::null`'s doc-test.

- **What Doom needed:** prove that sound reaches the device (sample counts,
  silence on mute, volume) on a machine with no sound card, and record the
  game's sound for a video.
- **Workaround:** a temporary `~/.asoundrc` with ALSA's `file` plugin over
  `null`, writing into a FIFO that a reader drains at the sample rate
  (`tests/audio_device.rs`; `docs/media/play/paced_tap.py` for the video).
  Unpaced, the null device takes samples as fast as the CPU allows. An
  earlier version of the test wrote gigabytes to `/tmp` that way.
- **Proposed API (rs-rich-intuituive, `audio`):**
  `OutputOptions::null()` opens a device-less output that consumes at its
  rate on a clock (real or the app's). `Output::tap() -> Tap` records what
  the device takes, bounded (`Tap::samples()`, `Tap::to_wav(path)` with
  `wav`).
- **Why generic:** every audio feature's tests; recording an app's sound for
  a demo or a bug report; CI machines without sound cards.
- **Acceptance tests:** a null output at 44,100 Hz takes about 44,100 frames
  a second (±2%) and never more than its queue ahead. With volume 0 the tap
  holds only zeros. A tap capped at 1 s keeps 1 s. `to_wav` writes a file
  hound reads back with the same samples.

### 10. ALSA's messages kept off the screen for the stream's life (intuiTUIve, `audio`)

- **What Doom needed:** ALSA's complaints (an underrun) must never land on
  the game's screen.
- **Workaround:** `csrc/sound.c`'s `rs_doom_quiet_alsa` installs a
  process-wide no-op handler (`snd_lib_error_set_handler`). intuiTUIve's
  handler is thread-local and in force only while devices are listed and
  opened.
- **Proposed API (rs-rich-intuituive, `audio`):** while any output or capture
  is open, install the keeping handler process-wide and restore the previous
  one when the last closes. `audio::messages()` returns them as today.
- **Why generic:** any TUI that plays or records sound on Linux (the music
  player, the scope).
- **Acceptance tests:** with an output on a device that reports an error
  after opening (ALSA's `file` plugin onto an unwritable path), nothing is
  written to stderr and `messages()` holds the line.

### 11. A palette-keeping two-colour rule for Braille and quadrants (intuiTUIve, `pixels`)

**Status: closed in 0.0.23.** `Node::pixel_colors(PixelColors::Average |
PixelColors::Nearest)` for `pixels` and `pixels_fit` nodes, `Average` the
default. With `Nearest`, each side's colour is the one of its pixels
nearest the side's average. Tests: `pixels.rs`'s
`nearest_colours_keep_to_the_cells_own` (a 2x4 cell of red, dark red and
black, in Braille and quadrants), `averaged_colours_are_the_default` and
`cells_written_directly_match_the_segments_they_replaced` (unchanged), and
`pixel_colors`'s doc-test.

- **What Doom needed:** Braille cells in the frame's own colours. Before,
  rs-rich-doom chose each side's colour as the member pixel nearest that
  side's mean, never a blend.
- **Workaround:** none. On pixel cells, Braille now uses the core's averaged
  colours: more distinct styles (see the compaction measurement) and colours
  the palette doesn't have.
- **Proposed API (rs-rich-intuituive):**
  `Node::pixel_colors(PixelColors::Average | PixelColors::Nearest)` for
  `pixels`/`pixels_fit` in `Quadrants` and `Braille`, with `Average` the
  default.
- **Why generic:** pixel art and palette-limited graphics (emulators, 16- or
  256-colour art) keep their palette. It is also cheaper on the style table.
- **Acceptance tests:** a 2x4 cell of red, dark red and black in Braille with
  `Nearest` draws only colours present in the cell. With `Average`, output is
  as today, byte for byte.

### 12. An aspect-fitted pixel buffer (intuiTUIve, `pixels`)

**Status: closed in 0.0.23.** `PixelBuf::draw_fitted(src, width, height,
&Fitted) -> Rect`, where `Fitted::new(mode)` takes `.aspect(f64)`,
`.cell_aspect(f64)` (default 0.5), `.fit(PixelFit::Contain | Cover |
Stretch)` and `.filter(Filter::Nearest | Average)`; it returns the cells
used. One change from the proposal: the fit is intuiTUIve's own
`PixelFit`, not rs-rich-art's `ImageFit`, because `pixels` is core and
rs-rich-art is behind the `art` plugin; and `Fitted` names the cell mode,
which says how many pixels a cell holds. Tests:
`a_picture_is_fitted_at_its_ratio_in_whole_cells` (320x200 at 4:3 into
80x24 cells of 1:2 is 64x24 at column 8; `aspect` unset fills the buffer;
`Nearest` keeps the source's colours; margins stay `CLEAR`; Braille; cover)
and `the_average_filter_blends_what_each_pixel_covers`, and
`draw_fitted`'s doc-test.

- **What Doom needed:** a 320x200 frame shown at 4:3 in whatever cells the
  node has, from the cell's real aspect ratio, centred, the margins
  transparent.
- **Workaround:** `render::Fit` and `render::draw_into` fit in whole cells
  and resample nearest-neighbour into the `pixels_fit` buffer.
- **Proposed API (rs-rich-intuituive):**
  `PixelBuf::draw_fitted(src: &[Rgba], width, height, fit: ImageFit, aspect: Option<f64>, cell_aspect: f64, filter: Filter)`
  (`Filter::Nearest | Average`), returning the cell rectangle used.
  `ImageFit` and `ImageAnchor` are rs-rich-art's.
- **Why generic:** emulators (8:7 or 4:3 pixels), video, any source picture
  in a pixel node.
- **Acceptance tests:** 320x200 at 4:3 into 80x24 cells of 1:2 is 64x24 at
  column 8. With `aspect: None` it fills the buffer. `Nearest` keeps the
  source's colours exactly; margins stay `CLEAR`.

### 13. Sound in recordings (rs-rich-record)

- **What Doom needed:** a video of the game with its sound.
- **Workaround:** `docs/media/play/record.sh`: xterm on Xvfb captured by
  ffmpeg, the sound through gap 9's ALSA-FIFO tap, muxed with ffmpeg.
  `rich record` makes GIFs, APNGs and videos without sound.
- **Proposed API (rs-rich-record):** `Set Audio on` in a tape, or
  `rich record --audio`. The recorded program's sound goes through an
  environment the recorder sets (an ALSA or Pulse device it provides, or
  intuiTUIve's `Output::tap` when the program asks for it). It is muxed as
  AAC into MP4 output, in sync with the frames' timestamps.
- **Why generic:** demos of any app with sound (the music player, games),
  bug reports.
- **Acceptance tests:** a tape running a program that plays a 1 s 440 Hz
  tone from its second frame produces an MP4 with an AAC track whose tone
  starts within one frame of that frame and lasts 1 s ±1 frame.

## Not gaps

- **Overlays in `layers`:** a text layer fills its rectangle (and covered the
  picture) unless placed. `.place(Align::Start, Align::Start)` is the
  documented way, and works.
- **`fixed_step` itself, held keys, `rate`, `StatsHandle`, `put_rgb`,
  compaction:** used as they are, no changes needed.
