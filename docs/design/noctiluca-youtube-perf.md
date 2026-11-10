# Noctiluca's video and readings, measured

**Status:** measured and partly fixed, 2026-11-10, on `main` at
`b8ce24e2` (after #877, #878 and #886) as the baseline. This replaces the
audit of 2026-11-05 made on `b0bfdc4c`, before those three pull requests
rewrote the tab, the DevTools backend and the video path; its findings
were checked again on today's code and its fixes re-made on it. What was
fixed is in [What changed](#what-changed); what is left is in
[What is left](#what-is-left).

## How it was measured

- **The binary:** release builds (`cargo build --release -p noctiluca`),
  one per step below, on a 4-core Linux container; Chromium 153 headless.
  Another agent's job kept the load average at 1.7 to 3.3 during the runs:
  numbers within about 10% of each other are the same.
- **The harness:** a Python PTY harness (kept outside the repository): it
  sets the window to 120x40, answers the terminal's queries, records each
  byte with its time, counts the graphics escapes it receives (Kitty
  `a=T`, Sixel DCS, iTerm2 `1337;File`) and the painter's writes, samples
  the CPU of Noctiluca and of Chrome's processes from `/proc` over a 15 s
  window after 10 s of warm-up, and times a key from its press to the first
  output after which a line of the page has moved.
- **The trace:** `NOCTILUCA_TRACE=FILE` (added here) writes JSON lines:
  each reading (snapshot, AX and build times, why it ran), each screencast
  take and its size, each frame handed to the app, each encode, each change
  of the screencast's area, the media path's stages, and once a second the
  painter's frames, bytes and draw, paint and write times.
- **Pages:** a local **watch-page fixture** (a 1280x720, 30 fps H.264 test
  card in a player whose clock and progress bar a script updates on
  `timeupdate`, as a site's player does; a title, comments, related
  videos), since YouTube's player shows a bot wall from this network;
  **Twitch**'s always-on channel (a real site's player, 640x360 live); and
  YouTube's search results for scrolling.

## What main did

On the fixture, main drew **one video frame a second** in every protocol
(0.9 in Kitty, 0.7 to 0.9 painter writes a second in Sixel, half blocks
and iTerm2). The trace says why:

- The player's clock pings a page change four times a second. Each ping
  was read 100 ms later (a DOMSnapshot, the AX tree and the model: 155 ms
  on the fixture), so the tab's thread spent **60%** of its time reading
  (3.8 readings a second).
- Since #877 no video frame is cut while a change waits to be read (a
  frame must not be cut with an old model). With a ping every 250 ms and a
  reading of 155 ms, a change was waiting nearly all the time: the frames
  went out between readings, about once a second.
- Every frame was a JPEG of the whole viewport (960x648), for a region of
  502x283; and in Kitty an uncompressed RGB frame (400 kB a frame).

Twitch's page pings less (1 reading a second): main drew 13 frames a
second there, with Chrome at 52% of a core.

## What changed

Each step is a commit with a test that fails without it, measured again
with the same harness (120x40, fixture unless named). Frames a second are
what reached the terminal: Kitty transmissions, else the painter's writes
(the trace's frames handed to the app in brackets).

| Case | main | + screencast area | + pinged readings throttled | + Kitty frames compressed |
|---|---|---|---|---|
| Kitty: frames/s | 0.9 | 0.3 (0.3) | **24.3** (24.4) | 24.3 (24.4) |
| Kitty: kB/s to the terminal | 349 | 155 | 9,877 | **2,672** |
| Kitty: CPU Noctiluca / Chrome | 36% / 37% | 29% / 29% | 39% / 32% | 37% / 28% |
| Sixel: painter writes/s | 0.9 | 1.1 (0.9) | 17.6 (24.2) | 17.7 (23.9) |
| Sixel: kB/s | 79 | 95 | 1,735 | 1,813 |
| Sixel: CPU | 40% / 41% | 40% / 28% | 79% / 27% | 80% / 27% |
| Half blocks: writes/s | 0.7 | 0.7 (0.7) | **24.7** (24.2) | 25.2 (24.0) |
| Half blocks: kB/s | 13 | 15 | 405 | 408 |
| Half blocks: CPU | 36% / 37% | 36% / 26% | 16% / 24% | 15% / 25% |
| iTerm2: writes/s | 0.4 | 0.6 (0.6) | 21.2 (24.3) | 21.2 (24.0) |
| iTerm2: kB/s | 25 | 55 | 1,746 | 1,710 |
| iTerm2: CPU | 38% / 42% | 39% / 32% | 73% / 26% | 74% / 27% |
| Readings a second (share of the tab's thread) | 3.8 (60%) | 3.8 (60%) | **0.87 (13%)** | 0.87 (13%) |
| Longest gap between frames | — | 1.3 to 4 s | 0.20 to 0.21 s | 0.20 to 0.23 s |
| Twitch, Kitty: frames/s | 13.2 | 16.4 | 19.6 | 19.2 |
| Twitch, Kitty: kB/s | 5,805 | 6,782 | 8,027 | **4,774** |
| Twitch, Kitty: CPU | 36% / 52% | 38% / **31%** | 32% / 30% | 37% / 35% |

Key latency (a `j` or `k` until the page's text moved) stayed within 10
to 60 ms in every case but a few single samples of 100 to 160 ms while
frames were streaming; with one sample per key per run it is noise at
this resolution, and the rows above do not show a trend.

1. **`NOCTILUCA_TRACE`** (crate-private), and one trace: the media
   path's `RICH_MEDIA_TRACE` lines, which went to stderr (the terminal,
   while Noctiluca draws fullscreen) from Noctiluca and from rs-rich-art
   and rs-rich-ext, now go to the same file, and the library crates print
   nothing.
2. **Screencast only the playing region.** rs-rich-embed's
   `ChromeEngine::set_screencast_area` gives `Emulation.setDeviceMetricsOverride`
   a `viewport` (the visible area, which the page does not see): Chrome
   paints and encodes the region's 502x283 pixels, not the 960x648
   viewport. Checked live: the page's scroll position and inner size, a
   click's page coordinates and `Page.captureScreenshot` clips are as
   without it. Frames carry the area's size and are placed at its top
   left; a frame Chrome sends before it answers the restarted screencast
   is dropped whatever its size (the old branch recognised an area's frames
   by size only, so a stale frame of an earlier area of the same size would
   have been cut in the wrong place). The tab casts the one moving region
   wholly inside Chrome's view, the viewport otherwise and in the pixel
   view. Chrome's CPU while a video plays: 37% to 29% on the fixture, 52%
   to 31% on Twitch.
3. **Pinged readings wait their turn, and frames go on meanwhile.** A
   reading the page pinged for waits three times the last reading's cost
   (at least 100 ms), and at least a second while the screencast runs; a
   reading for an action of ours, a navigation, a load or a settling
   document never waits. While a ping waits, frames are cut with the
   current model (the engine still checks each frame's document and
   viewport); not while a reading is asked for or was refused. This is the
   step that took the fixture from one frame a second to 24.
4. **Kitty video frames are zlib-compressed** (`o=z`, fastest): 9.9 MB to
   2.7 MB a second on the test card, 8.0 to 4.8 MB on Twitch.
5. **The delivery mailbox coalesces pages**: a newer page, state or
   metrics update replaces the waiting one (frames already did), so a UI
   thread that falls behind finds one page, not a pile.

Also fixed on the way, from review: a scroll view inside a `layers` stack
kept old cells in its own buffer (Noctiluca's reader since its root went
into `layers`: `[d][ ]nSend news`); a skip link parked above a fixed bar
was drawn in it (L1 below); and buttons drawn under WrapActionLabels left
their row (`[Send]` after `[Unavailable]`, "Like this video" over three
rows).

## Already on main

- **Kitty pictures that only moved are not sent again on scroll**
  (rs-rich-ext's `frame::plan`, since `fc1c8520`): scrolling YouTube's
  search results one row in Kitty sends 26 to 28 kB (1.1 MB before the
  first audit), a page down 380 to 850 kB (the pictures newly in view).
- **Sixel and iTerm2 still redraw every picture that moved**: 475 to 590
  kB a row on the same page, a page down 340 to 580 kB, settled in 15 to 51
  ms. That is #884's remaining cost (below).

## iTerm2 (#883)

The f309073 qualification's own workload (`combined_frame_pipeline`, the
ignored test in `app.rs`; `NOCTILUCA_MEASURE_PROTOCOL=iterm` now selects
iTerm2, as that qualification's temporary patch did) was run three times a
protocol, release, with its in-test frame and input latencies and the test
process's CPU; its external recorder and decoder are not in the
repository, so these are not the same measurement as the report's.

| Run | p95 ms | p99 ms | input max ms | CPU s | Gate |
|---|---:|---:|---:|---:|---|
| main, iTerm2 1 / 2 / 3 | 12.45 / 11.75 / 11.11 | 13.30 / 11.98 / 12.11 | 5.16 / 2.46 / 3.51 | 0.63 / 0.68 / 0.59 | pass 3/3 |
| this branch, iTerm2 1 / 2 / 3 | 10.86 / 12.27 / 11.50 | 12.14 / 13.70 / 11.62 | 2.99 / 3.44 / 3.05 | 0.64 / 0.71 / 0.66 | pass 3/3 |
| main, ANSI 1 / 2 / 3 | 3.77 / 3.89 / 4.49 | 4.79 / 4.01 / 4.87 | 2.24 / 2.05 / 2.18 | 0.22 / 0.24 / 0.25 | pass 3/3 |
| this branch, ANSI 1 / 2 / 3 | 4.08 / 4.26 / 4.00 | 4.21 / 4.51 / 4.35 | 1.67 / 2.76 / 1.86 | 0.25 / 0.27 / 0.23 | pass 3/3 |

A second session on a quieter but slower host (load average under 1,
nothing else running) gave this branch: **iTerm2 0/3** (p95 12.8 to 16.1
ms, p99 14.0 to 17.6, input max 2.7 to 3.3, CPU 0.79 to 0.85 s: p95, p99
and CPU over the gate), **Kitty 3/3** (p95 4.2 to 4.5, p99 4.3 to 4.8,
input max 2.4 to 3.5, CPU 0.28 s) and **ANSI 3/3** (p95 4.2 to 4.5, p99
4.3 to 5.3, input max 2.4 to 3.2). The Kitty run needed its inline-picture
check repaired: since #877/#878 Kitty's inline pictures are the image
presentation's own resources, not cells tagged with the picture's id, so
the check failed on main before the window.

Where an iTerm2 frame's time went, measured inside rs-rich-art on the
encoder's thread: fitting the 480x270 frame to the region's 328x224 pixels
(Lanczos3, then the crop `Cover` asks for) was **4.5 ms of 4.6**; the PNG
encode is 0.1 ms on this workload's flat frames. A micro-benchmark of the
choices: as it was 4.96 ms, resampling as RGB 4.75 (no gain once the
conversion is paid), cropping before resampling 4.10, a bilinear filter
3.80 (which changes the picture).

### Fixed

What fixed it, on this branch after `47905250`, in four commits:

1. **rs-rich-art's own resampler** (`resample`, crate-private; `3bac4f20`).
   It does `image`'s arithmetic in `image`'s order: the same `f32`
   weights computed the same way, a vertical pass then a horizontal one,
   each sum built tap by tap from zero, rounded half away from zero. So
   it gives `image`'s pixels **exactly**, for Lanczos3 and Triangle,
   RGB and RGBA. It is faster because it reads contiguous rows the
   compiler vectorises, sums eight output rows side by side, and works
   out only the pixels a `Cover` crop keeps and the columns they read.
   The frame's fit went from 4.6 ms to 1.2 to 1.5 ms; the cells' Triangle
   fit from 0.6 to 0.15 ms. `fast_image_resize` (MIT or Apache-2.0, MSRV
   1.87) was the brief's first choice. It was not used: its fixed-point
   arithmetic does not give `image`'s pixels, so every picture, golden
   and tape would have changed, and it would have been a new dependency
   in five lockfiles. Resampling in bands on threads was not needed
   either.
2. **The painter** (rs-rich-ext; `a627ca78`). A picture's cells under
   iTerm2 or Sixel are tagged with its metadata. To the painter each
   tagged style was new, so it worked out each one's SGR through
   `Style::ansi_codes`, about 10,000 instructions a cell. It now writes a
   colour pair tagged only with metadata from its colours, as it writes a
   plain pair. The bytes are the same. The iTerm2 run's `paint_encode_ms`
   went from 65 to 71 down to 22 to 29.
3. **Copies on the app's thread** (`5f2a4cf3`): `Pixels::to_rgba` wrote a
   byte at a time (0.9 ms a frame), `Picture::lines` cloned the frame, and
   `ImageArt` copied an RGBA image into a new one.
4. **Dev builds** (`aca95eac`): rs-rich-art is optimised in them, as
   `image` already was. Both resamplers are generic code instantiated in
   rs-rich-art, so unoptimised, a frame's fit took 17 ms (`image`) or 63
   ms (the resampler) in a debug build. The media-grid tape, which runs
   a debug build, showed its last screen before `G` had scrolled it.

The gate after these, on a quiet host (load average under 0.5, nothing
else running), the same release test binary each run. CPU is the test
process's user and system time (`getrusage` of the child). Input max is
the largest of the eight key latencies. Each run, from `crates/noctiluca`,
after `cargo test --release -p noctiluca --lib --no-run`:

```bash
NOCTILUCA_MEASURE_PROTOCOL=iterm ../../target/release/deps/noctiluca-<hash> \
  app::update_measurements::combined_frame_pipeline --exact --ignored --nocapture
```

(`kitty` and `ansi` for the others; the frame and input latencies are on
the `pipeline:` line it prints.)

| Protocol / run | load avg (1 min) | p95 ms | p99 ms | input max ms | CPU s | Gate |
|---|---:|---:|---:|---:|---:|---|
| iTerm2 1 | 0.48 | 8.85 | 9.46 | 4.14 | 0.564 | pass |
| iTerm2 2 | 0.38 | 8.65 | 8.85 | 3.10 | 0.537 | pass |
| iTerm2 3 | 0.32 | 8.86 | 9.49 | 3.92 | 0.539 | pass |
| Kitty 1 | 0.25 | 3.80 | 4.33 | 3.08 | 0.247 | pass |
| Kitty 2 | 0.27 | 3.95 | 4.02 | 2.64 | 0.246 | pass |
| Kitty 3 | 0.21 | 3.48 | 3.80 | 2.22 | 0.250 | pass |
| ANSI 1 | 0.16 | 3.43 | 3.90 | 1.86 | 0.253 | pass |
| ANSI 2 | 0.12 | 3.60 | 3.80 | 1.98 | 0.242 | pass |
| ANSI 3 | 0.18 | 3.56 | 3.57 | 2.81 | 0.264 | pass |

The same harness on `47905250`, before the fix, on the same host. iTerm2's
six runs scraped by: one over p99, one 0.005 ms under p95, CPU 0.71 to
0.78 s.

| Run | load avg (1 min) | p95 ms | p99 ms | input max ms | CPU s | Gate |
|---|---:|---:|---:|---:|---:|---|
| iTerm2 1 | 1.43 | 13.67 | 15.51 | 2.75 | 0.713 | pass (p95 by 0.005) |
| iTerm2 2 | 1.31 | 12.84 | 13.47 | 2.71 | 0.755 | pass |
| iTerm2 3 | 1.11 | 13.19 | 13.95 | 2.99 | 0.750 | pass |
| iTerm2 4 | 0.86 | 12.82 | 13.47 | 4.05 | 0.746 | pass |
| iTerm2 5 | 0.73 | 13.59 | 15.90 | 2.98 | 0.775 | **fail**: p99 |
| iTerm2 6 | 0.62 | 13.42 | 14.52 | 3.61 | 0.741 | pass |

The earlier session's 0/3 above had CPU at 0.79 to 0.85 s. It was a
slower host, or CPU counted with more than the test process; this
session did not reproduce it. What is left in an iTerm2 frame on the
app's thread (about 2.6 ms a render, against under 1 ms in Kitty) is
its tagged cells: one style per cell, whose metadata is cloned through
the draw. That is not needed for the gate.

## Layout

Compared at 120x40 with Chrome's own layout of reduced fixtures. Of the
first audit's eleven defects:

| # | Defect | Now |
|---|---|---|
| L1 | A skip link parked above a fixed bar drawn as `[Skip navigation]` | **Fixed**: a control wholly above a pinned or docked strip is not drawn (fixture `masthead.html`) |
| L2 | A bar's items, centred in its 56 px, spread over two or three rows | Still on main. Putting one-line items on the row their middles share moved the search field out of its row in today's run fitting: not ported |
| L5, L10 | A word or a number split across lines in a narrow column (`Subscriptio` / `ns`, `28` / `K`) | Not reproduced on today's projection of the reduced fixture: the label overflows its column instead |
| L6 | `23M` / `11y ago` on two rows | Still on main for the first result row of the fixture |
| L8 | A crowded right-edge menu button wrapped (`[Action` / `menu]`) | Still on main: the button is a wrapped item, outside the run fitting the old branch's `fit_run` extended; not ported |
| L3, L4, L7, L9, L11 | Combobox as select, icon glyphs, avatar over text, button faces, thumbnail size | Not looked at again |

## What is left

1. **A reader thread.** A reading (155 ms on the fixture, 250 to 350 ms on
   YouTube) still runs on the tab's thread: the frames stop for its
   length about once a second while a video plays (the 200 ms gaps above).
   The old branch moved readings to a thread of their own; on today's code
   that needs the snapshot's capture scope (`ScreencastScope`) checked and
   adopted off the engine, and the source model's build moved with it.
2. **Sixel and iTerm2 while scrolling** (#884): show a moving picture's
   cells and draw its pixels once the view rests, instead of 0.5 MB a row.
3. **Sixel's painter** costs 80% of a core at 24 frames a second here.
4. **Layout** L2, L6, L8 above, and those not looked at again.
5. Kitty frames through shared memory when the terminal is local, cheaper
   readings of large pages (a partial AX tree), sound to the terminal's
   machine over SSH: unchanged from the first audit's plan.
