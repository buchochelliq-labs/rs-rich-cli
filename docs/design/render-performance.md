# Rendering performance: viewports, infinite scroll and paint strategies

**Status:** proposed on 2026-10-25, for 0.0.23. Written from three
surveys: Noctiluca and rs-rich-web's render paths, intuiTUIve's pipeline,
and how other projects do it (sources at the end). The painter's part
(frame stats, the timedemo, the benchmark, the faster painter and the
paint strategies) is built on the `023-paint` branch: see [What was
built, measured](#what-was-built-measured). The viewport and Noctiluca
are another branch's. The maintainer asked for three things:

1. **Forever scroll:** pages and lists that keep loading as you scroll,
   with only what is on screen rendered. Possibly a core optimisation.
2. **Faster frames under heavy damage.**
3. **A configurable paint strategy**, with a `best` option that picks per
   frame from the damage, chosen by benchmarks rather than guesses.

## Where things stand

### intuiTUIve's pipeline

- **Damage is per node, as rectangles.** Only dirty or moved nodes draw
  (`node.rs:3105`). The `Screen` is kept between frames, so a clean node's
  cells stay. A non-retained widget clears and redraws its whole rectangle.
- **The painter compares cells inside the damage only**
  (`rich-ext/src/surface/screen.rs:906`, `changed` at :997). There are no
  row hashes and no full-buffer compare.
- **The output is plain.**
  - Every move is an absolute CUP (:799).
  - Every style change is a full reset plus the new attributes:
    `\x1b[0;…m` (:1090).
  - There is no EL, ECH or REP. There is no scroll region (DECSTBM) and no
    SU or SD.
  - Nothing switches to a full repaint when most of the screen changed.
- **What we already do well:**
  - frames go out in synchronized output (DEC 2026) where the terminal
    answers for it;
  - styles skip their SGR when unchanged;
  - links open and close only on change;
  - the style table compacts.
- **Scrolling rewrites the pane.**
  - `Canvas::scroll_up` shifts the buffer, but damages the whole
    rectangle (`widget.rs:1228`), so every shifted cell is sent again.
  - That includes `LogView`, which already renders only its new lines.
- **The scroll view is not virtual.**
  - `scroll()` lays its child out at full height, capped at
    `MAX_SCROLL_ROWS = 4000` (`builtin.rs:28`). Rows past it cannot be
    reached.
  - It draws offscreen and copies out only the rows in view, so a scroll
    renders nothing new. A change, though, draws the whole content.
- **Virtual lists exist**, but only as list and table widgets:
  `list_lazy`, `virtual_list`, `virtual_table`, `tree_lazy`. They draw a
  screenful whatever their length.
- **No "near the end" hook exists** anywhere.

### Measured against ratatui

At 80x24, in release, on the benchmark gate (`tests/versus_ratatui.rs`):

| Scene | ratatui | intuiTUIve |
|---|---:|---:|
| Status tick | 95 µs, 37 B | 15 µs, 18 B |
| Selection move | 97 µs, 184 B | 39 µs, 118 B |
| Log append | 94 µs, 443 B | 47 µs, 428 B |
| Churn, every cell | 326 µs, 62 kB | 3,042 µs, 66 kB |
| Chart redrawn | 81 µs, 4.2 kB | 463 µs, 3.1 kB |

Small updates are already cheaper than ratatui's. Heavy damage is where we
lose: churn is about 9 times slower and the chart about 6 times. The log
append sends nearly a whole pane for one new line.

### Where the time and bytes go (measured)

Measured on 2026-10-25 with the paint benchmark (`tests/paint_bench.rs`,
below) and the frame stats it reads, before any optimisation: release
build, the `cells` strategy, best of three runs, a frame's mean. "Draw"
is the widgets drawing into the screen (rich rendering, then
`Screen::write_lines`); "paint" is the painter turning changes into
bytes. callgrind on 30 frames of each heavy scene splits them further.

| Size | Scene | frame µs | draw µs | paint µs | B/frame | moves B | styles B | text B |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| 80x24 | Blink | 7.6 | 5.3 | 1.8 | 16 | 7 | 7 | 2 |
| 80x24 | Typing | 18.5 | 15.0 | 2.4 | 27 | 7 | 4 | 1 |
| 80x24 | StatusClock | 32.7 | 26.3 | 5.4 | 31 | 15 | 14 | 2 |
| 80x24 | LogAppend | 224.4 | 114.7 | 107.7 | 4,013 | 1,329 | 1,132 | 1,551 |
| 80x24 | ListPage | 253.6 | 205.4 | 47.2 | 958 | 384 | 304 | 269 |
| 80x24 | ListRow | 279.4 | 199.0 | 79.2 | 3,933 | 1,318 | 1,100 | 1,514 |
| 80x24 | TableSort | 1,360.3 | 1,281.8 | 75.9 | 2,132 | 690 | 541 | 901 |
| 80x24 | Churn | 1,715.4 | 1,549.5 | 160.7 | 65,740 | 305 | 59,735 | 5,700 |
| 80x24 | Chart | 807.3 | 696.0 | 108.0 | 3,139 | 1,120 | 1,275 | 742 |
| 200x60 | Blink | 21.1 | 13.1 | 7.1 | 16 | 7 | 7 | 2 |
| 200x60 | Typing | 47.6 | 33.8 | 11.7 | 27 | 7 | 4 | 1 |
| 200x60 | StatusClock | 50.7 | 36.7 | 12.8 | 31 | 15 | 14 | 2 |
| 200x60 | LogAppend | 774.3 | 253.7 | 516.4 | 24,706 | 8,070 | 6,095 | 10,539 |
| 200x60 | ListPage | 1,542.2 | 1,242.2 | 295.7 | 3,830 | 1,745 | 1,011 | 1,073 |
| 200x60 | ListRow | 1,707.4 | 1,188.0 | 514.8 | 24,470 | 8,053 | 5,980 | 10,436 |
| 200x60 | TableSort | 3,908.6 | 3,515.8 | 388.2 | 13,024 | 4,271 | 3,333 | 5,420 |
| 200x60 | Churn | 11,351.7 | 10,403.9 | 928.2 | 398,824 | 2,488 | 361,871 | 34,464 |
| 200x60 | Chart | 2,994.1 | 2,525.4 | 461.5 | 11,267 | 5,054 | 4,081 | 2,130 |

What it says:

- **The draw, not the painter, is most of a heavy frame.** Churn spends
  90% of its frame drawing and 9% painting; the chart 86% and 13%; the
  table 94% and 6%. The guess in the first draft, that the heap string per
  cell makes the painter slow, is wrong for these scenes: the painter's
  cost is small next to what feeds it.
- **Inside the draw** (callgrind, share of all instructions):
  - **Churn:** `Screen::write_lines` 52% (`Frame::from_segments` 15%,
    `StyleTable::intern` 16%, a rich `Style` hashed per cell), the
    scene building a `Style` and `Segment` per cell 31% (most of it
    `format!` inside rich's colour constructors), and malloc and free
    about 30% of everything across both. The painter is 10%.
  - **Chart:** rich rendering the chart 37%, `write_lines` 38%, the
    painter 14%.
  - **Table and lists:** rich's text rendering (wrapping, `divide_line`)
    41 to 46%; `write_lines` 10%; the painter 5%.
  - **Log append:** the painter 40% (it rewrites the whole pane),
    `Screen::scroll_up` 16% (it clones every cell's string), rendering
    the one new line 9%.
- **Bytes:** cursor moves are a third of every scene that writes rows
  (LogAppend, ListRow, TableSort, Chart): each run starts with an
  absolute CUP. Churn is 91% SGR: two full true-colour codes per cell,
  always after a reset. The log append and the one-row scroll send a
  whole pane for what a terminal scroll does in a few bytes.
- **Small scenes are already cheap** (8 to 50 µs, 16 to 31 bytes):
  whatever is added must not cost them.

So the order of work follows the costs, not the list of ideas:

1. **`write_lines` and the cell store** (data layout, no per-frame
   allocation): write segments into cells without building a `Frame`,
   memoise the style intern for runs of one style, keep cells flat and
   `Copy` so scrolling and copying are moves, not clones. Largest share in
   churn and the chart.
2. **Scrolling in the terminal** for the log and lists (the `scroll`
   strategy): the largest byte and paint cost after churn.
3. **The encoder** (state-change minimisation): cheaper moves and SGR
   deltas, a third of the bytes in row-writing scenes.
4. **rich's rendering** (wrapping, `Style` building) is the largest cost
   left in text-heavy scenes, but it is the faithful mirror's: changes go
   upstream-first or into rs-rich-ext's own fast paths (`put_rgb` for
   pictures already skips it).

### What was built, measured

Everything below is from the paint benchmark (`tests/paint_bench.rs`) in
release, best of five interleaved rounds, a frame's mean; the first table
is the `cells` strategy at 80x24 after each step, so each step's effect
shows on its own. Each step was kept only because the numbers moved, and
every scene's bytes still replay through vt100 to the screen a full
repaint draws.

| Step | Blink | LogAppend | ListRow | TableSort | Churn | Chart |
|---|---:|---:|---:|---:|---:|---:|
| Before (µs; B) | 7.6; 16 | 224; 4,013 | 279; 3,933 | 1,360; 2,132 | 1,715; 65,740 | 807; 3,139 |
| Lines straight into cells | 6.9 | 141 | 161 | 1,359 | 880 | 377 |
| Cells of 12 bytes, `Copy` | 4.2 | 88 | 90 | 1,252 | 810 | 299 |
| The encoder | 4.0; 15 | 97; 3,081 | 96; 3,014 | 1,143; 1,634 | 758; 61,370 | 310; 2,845 |
| `best` | 3.4; 15 | 50; 147 | 54; 143 | 1,090; 1,634 | 752; 61,377 | 293; 2,845 |

At 200x60, `best` against the start: the log append 774 µs and 24,706
bytes to 101 µs and 325 bytes; the one-row scroll 1,707 µs and 24,470
bytes to 235 µs and 320; churn 11,352 µs to 4,925; the chart 2,994 µs
and 11,267 bytes to 1,145 µs and 8,664.

**What each step was:**

- **Lines straight into cells** (`Screen::write_lines`): the profile's
  largest share in churn and the chart was building a `Frame` (a text
  arena, runs, a `Vec` of cells a row) only to copy it into the screen.
  Segments now go into cells directly, a character a cell where each
  character is its own grapheme, and through rich's grapheme splitting
  only for the segments that need it (combining marks, joined emoji),
  with the Frame path's rules; a random test checks the two cell for
  cell. Only a line break inside a segment, or a region tag, still takes
  the Frame path.
- **Data-oriented cells:** a `Cell` was `{ text: String, width, style }`.
  It is now 12 bytes and `Copy`: a grapheme of up to four bytes inline,
  a longer one as an index into a shared, append-only table. Comparing,
  copying, scrolling and keeping the sent copy are plain moves; paint
  time halved on every scene. The text is read with `Cell::text()`.
- **The encoder** (state changes, as a game batches them): the shortest
  of CUP (its column left out when it is the first), a relative move, a
  carriage return and a relative move, `\r\n`, or writing through up to
  eight unchanged cells when that is shorter, style changes counted; a
  colour change sent as the change when the attributes stay (true-colour
  pairs straight from the colours, no parsing); ECH and EL for blank runs
  on the default background, starting where the pen is; numbers written
  without `format!`; one buffer a frame (`Painter::paint_into`). Bytes
  fell 6 to 25% on every scene that writes rows. A grapheme with a ZWJ
  or VS16 now leaves the cursor unknown, so the next write moves there:
  terminals disagree with rich's table on those widths.
- **The strategies**, below.

**Tried and dropped, because the benchmark said so:**

- *Interning a style by its colour pair* (skipping the full hash for a
  true-colour style): 7.6% more instructions on churn. Reverted.
- *Row hashes kept as rows are written*, for finding scrolled rows:
  comparing rows as slices of the new cells is cheaper, since a row that
  differs stops at its first cell (the scroll scene at 200x60: 140 µs
  with hashes, 99 with compares). Removed; `Screen::scrolled` keeps the
  scrolls `scroll_up` made, which the painter tries first.
- *REP*: vt100, our check, has no REP, and once ECH and EL take the blank
  runs no scene has repeated runs worth it. Left out.

#### The strategies, measured

Every strategy on every scene (µs a frame; bytes a frame):

| Size | Scene | cells | rows | full | scroll | best |
|---|---|---:|---:|---:|---:|---:|
| 80x24 | Blink | 3.4; 15 | 3.4; 14 | 50.2; 2,536 | 3.5; 15 | 3.4; 15 |
| 80x24 | Typing | 13.2; 27 | 13.2; 27 | 62.7; 2,574 | 12.7; 27 | 13.3; 27 |
| 80x24 | StatusClock | 11.6; 31 | 10.9; 31 | 61.9; 2,465 | 11.6; 31 | 12.0; 31 |
| 80x24 | LogAppend | 96.6; 3,081 | 104.0; 3,321 | 105.6; 3,327 | 50.6; 147 | 49.9; 147 |
| 80x24 | ListPage | 60.1; 857 | 52.9; 905 | 107.1; 3,286 | 60.6; 857 | 62.2; 857 |
| 80x24 | ListRow | 96.1; 3,014 | 103.9; 3,244 | 107.3; 3,287 | 53.9; 143 | 53.9; 143 |
| 80x24 | TableSort | 1,119; 1,634 | 1,132; 1,732 | 1,136; 2,361 | 1,167; 1,634 | 1,090; 1,634 |
| 80x24 | Churn | 771; 61,369 | 723; 61,371 | 760; 61,377 | 707; 61,369 | 752; 61,377 |
| 80x24 | Chart | 291; 2,845 | 284; 3,146 | 303; 3,272 | 295; 2,845 | 293; 2,845 |
| 80x24 | Unicode | 472; 3,690 | 491; 3,630 | 469; 3,736 | 435; 486 | 424; 486 |
| 200x60 | Blink | 4.6; 15 | 4.4; 14 | 204; 9,552 | 4.8; 15 | 4.6; 15 |
| 200x60 | Typing | 18.6; 27 | 18.4; 27 | 237; 9,590 | 17.7; 27 | 20.0; 27 |
| 200x60 | StatusClock | 13.7; 31 | 13.7; 31 | 208; 9,312 | 13.0; 31 | 13.1; 31 |
| 200x60 | LogAppend | 410; 18,143 | 414; 18,721 | 412; 18,726 | 97; 325 | 101; 325 |
| 200x60 | ListPage | 271; 2,941 | 253; 3,116 | 531; 18,591 | 255; 2,941 | 262; 2,941 |
| 200x60 | ListRow | 520; 17,949 | 525; 18,503 | 525; 18,593 | 241; 320 | 235; 320 |
| 200x60 | TableSort | 2,762; 9,762 | 2,707; 10,244 | 2,773; 11,714 | 2,871; 9,762 | 2,826; 9,762 |
| 200x60 | Churn | 4,943; 368,098 | 4,942; 368,769 | 4,785; 368,844 | 4,709; 368,098 | 4,925; 368,844 |
| 200x60 | Chart | 1,116; 8,664 | 1,121; 8,610 | 1,146; 8,861 | 1,158; 8,664 | 1,145; 8,664 |
| 200x60 | Unicode | 1,314; 9,216 | 1,286; 9,066 | 1,233; 9,334 | 1,135; 486 | 1,157; 486 |

Timings move by about 10% between runs on a shared machine; bytes are
exact. What the table decided:

- **`scroll`** is the one strategy that changes the order of things: a
  log line or a one-row scroll costs about one row of bytes, 20 to 75
  times fewer, and half the time. It finds the move by comparing the
  first changed rows with the rows sent near them (32 either way), lets
  them vote for a shift, and takes the run of rows the best of three
  shifts puts right. Rows found in more than eight places (blank ones)
  do not vote. The move is kept only if it puts more rows right than it
  opens. It resets SGR before scrolling, scrolls the whole screen with
  SU or SD or a band inside DECSTBM margins, and takes the cursor as
  lost after. It is off on Windows, under tmux, in an inline app, and
  where a picture sits in the moved rows.
- **`rows`** is within a few percent of `cells` everywhere once the cell
  encoder writes through short gaps, never better by more than 1% in
  bytes. `best` does not try it.
- **`full`** is only worth it when nearly every cell changed; otherwise
  it sends many times the bytes.
- **`best`** therefore: `full` when the whole screen is damaged and a
  sample of every seventh cell shows nine in ten changed (full motion);
  else `scroll` when a move is found; else `cells`. It encodes nothing
  twice, so choosing costs only the search, which gives up after four
  changed rows found nowhere. It is within 5% of the best fixed strategy
  in bytes and in time on every scene (the CI gate, with a 10 µs floor
  for timer noise and a second look at a failing scene).

#### The draw stage, profiled after the fast path

callgrind on 40 frames of each heavy scene, share of all instructions:

- **TableSort and the lists:** rich rendering the markup is nearly all
  of it: `TextNode::lines` 93%, of which markup parsing 37%, `cell_len`
  27% (`char_cell_width`'s table search 22%), wrapping (`divide_line`)
  26%, the emoji regex 12%. All in `crates/rich`, the mirror.
- **Churn:** the scene building a rich `Style` per cell, 50% (most of it
  `format!` inside rich's colour constructors), `StyleTable::intern` 10%,
  `write_lines` 13%, the painter 19%.
- **The chart:** rs-rich-ext's chart rendering 61% (`Chart::grid` 32%,
  `Line::push` 25%, the raster 18%), malloc and free about a third of
  everything. Measuring chart text without the width table took 13% off
  its draw.
- **Unicode text:** `TextNode::lines` 62% (markup 27%, `cell_len` 21%),
  `write_lines` 28% before it stopped building a Frame (`Frame::cells`
  13%, `split_graphemes` 11%, `from_segments` 8%); the draw fell from
  1,524 to about 1,160 µs at 200x60.

- **The Frame path itself** (still taken by lines with a line break
  inside a segment or region tags, and by the graphics plan, which builds
  a Frame of the whole screen each frame an app shows pictures):
  `tests/frame_path.rs` builds a 200x60 screen of styled text into a
  Frame and splits it into cells. It took about 670 µs; rich's grapheme
  splitting per run (a `Vec` of characters each time) and its width
  table were most of it. `Frame::cells` now writes a run whose
  characters are each their own grapheme without splitting, and widths
  of ASCII, Latin letters, box drawing, blocks, Braille, CJK ideographs
  and Hangul come without the table search (a test checks them against
  rich's): about 200 µs.

So what is left of a heavy frame is mostly rich's own rendering. Those
fixes belong upstream-first: an ASCII fast path in `cell_len` and
`char_cell_width` (upstream measures a string of single-width
characters by its length), and colour constructors that do not format a
name. Neither is made here; `crates/rich` is the faithful mirror.

#### Allocations a frame

In a steady state, under `best` (the regression baseline,
`tests/paint_bench_baseline.txt`):

| Scene | 80x24 | 200x60 |
|---|---:|---:|
| Blink | 65 | 65 |
| Typing | 364 | 364 |
| StatusClock | 162 | 162 |
| LogAppend | 505 | 619 |
| ListPage | 510 | 2,044 |
| ListRow | 425 | 1,778 |
| TableSort | 12,022 | 29,198 |
| Churn | 9,711 | 60,223 |
| Chart | 2,879 | 7,309 |
| Unicode | 4,618 | 11,372 |

Typing made 5,172 a keystroke before `Keymap` learnt to look a key up
without building every binding's id when nothing is rebound. Zero a
frame, the research's target for the tick and the chart, is not reached:
most of what is left is rich's segments and styles, built afresh each
frame.

#### Against ratatui

`tests/versus_ratatui.rs`, release, like for like (µs a frame, bytes a
frame); its `Feed` now hands out one key a frame, as noted above. The
gated scenes use the default `cells`; `best` is shown where it differs.

| Size | Scene | ratatui | intuiTUIve before | intuiTUIve now |
|---|---|---:|---:|---:|
| 80x24 | Status tick | 92 µs, 37 B | 18 µs, 18 B | 12 µs, 18 B |
| 80x24 | Selection move | 83 µs, 184 B | 49 µs, 118 B | 33 µs, 118 B |
| 80x24 | Log append | 95 µs, 443 B | 52 µs, 428 B | 23 µs, 325 B |
| 200x60 | Status tick | 309 µs, 37 B | 24 µs, 18 B | 12 µs, 18 B |
| 200x60 | Selection move | 313 µs, 280 B | 66 µs, 118 B | 35 µs, 119 B |
| 200x60 | Log append | 342 µs, 1,480 B | 235 µs, 1,465 B | 48 µs, 1,056 B (`best`: 410 B) |
| 80x24 | Churn, every cell | 327 µs, 62.0 kB | 1,629 µs, 65.7 kB | 788 µs, 61.4 kB |
| 80x24 | Chart redrawn | 83 µs, 4.2 kB | 477 µs, 3.1 kB | 299 µs, 2.8 kB |
| 200x60 | Churn, every cell | 2,037 µs, 376 kB | 10,885 µs, 399 kB | 5,499 µs, 368 kB |
| 200x60 | Chart redrawn | 348 µs, 14.7 kB | 1,997 µs, 11.3 kB | 1,036 µs, 8.7 kB |

Small updates went from about 2 to 3 times faster than ratatui to 2.5 to
25 times, with fewer bytes everywhere. Churn is now 2.4 times ratatui's
time (it was 5), the chart 3.6 times (it was 5.7), both with fewer bytes
than ratatui. The research's target for churn, within 1.5 times
ratatui's, is not reached: most of the remaining frame is the scene
building a rich `Style` per cell and rich's own rendering (above). Fast
paths in rich since brought churn to 1.6 times ratatui's time at 80x24:
see [rich made faster, measured](#rich-made-faster-measured).

### rich's own share, profiled

The maintainer then allowed performance work in the mirror, as long as
the output stays byte-identical (AGENTS.md). Before changing anything,
callgrind on 20 frames of each heavy scene (`profile_one_scene`, which
now takes `PAINT_BENCH_SIZE`; the `cells` strategy), share of all
instructions, inclusive:

| Scene | Ir | rich render | markup | `cell_len` | `char_cell_width` | `divide_line` | `truncate_line` | emoji | `Style::normalize` | `format!` | `intern` | painter | malloc | free |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 80x24 Churn | 225M | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 49 | 10 | 19 | 12 | 7 |
| 80x24 Chart | 51M | 67 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 1 | 15 | 16 | 15 |
| 80x24 TableSort | 264M | 53 | 37 | 26 | 22 | 25 | 12 | 6 | 6 | 2 | 0 | 3 | 9 | 10 |
| 80x24 ListPage | 500M | 53 | 40 | 27 | 22 | 26 | 12 | 6 | 6 | 2 | 0 | 1 | 9 | 10 |
| 80x24 Unicode | 111M | 39 | 31 | 18 | 18 | 19 | 9 | 5 | 5 | 0 | 1 | 11 | 7 | 9 |
| 200x60 Churn | 1,375M | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 50 | 10 | 19 | 16 | 7 |
| 200x60 Chart | 161M | 65 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 1 | 14 | 11 | 12 |
| 200x60 TableSort | 679M | 49 | 36 | 25 | 21 | 24 | 11 | 6 | 6 | 2 | 0 | 7 | 8 | 10 |
| 200x60 ListPage | 548M | 47 | 37 | 24 | 20 | 24 | 11 | 5 | 6 | 1 | 1 | 5 | 10 | 10 |
| 200x60 Unicode | 280M | 39 | 30 | 18 | 18 | 18 | 9 | 5 | 5 | 0 | 1 | 13 | 7 | 9 |

"rich render" is `Console::render_lines`, which for the chart is mostly
rs-rich-ext's own chart code; "markup" is parsing the markup into a
`Text`, before rendering. ListPage's profile is mostly its first frame,
which renders the thousand rows it then scrolls through.

What it says, by where the instructions go:

- **Churn: `format!` is half of everything.** `Color::from_rgb` names a
  colour by its hex (`#rrggbb`), through `format!` and three `{:02x}`,
  twice a cell. `StyleTable::intern` (rs-rich-ext) is 10%, the painter
  19%, malloc and free a fifth.
- **The text scenes (TableSort, the lists, Unicode): rich is 70 to 90%.**
  - `char_cell_width` alone is 18 to 22%, nearly all of `cell_len`: a
    binary search of the width table for every character, ASCII
    included, behind a `OnceLock` read.
  - `divide_line` 18 to 26%: most of it `cell_len` again, and a
    `Vec<char>` and a `String` per word.
  - Markup 30 to 40%: the tag regex (fancy-regex) about 12%, the emoji
    scan 6%, `Style::normalize` 6% (a full parse and `definition()` per
    tag), malloc.
  - `truncate_line` 9 to 12%, again mostly `cell_len`.
- **The chart:** rich's share is small (`Segment::split_lines` 9%,
  `Style` clones 5%); the rest is rs-rich-ext's chart and malloc.

### rich made faster, measured

Each change below was made alone, then checked: the goldens and parity
tables (`cargo test -p rs-rich`), the differential fuzzer against Python
rich 15.0.0 (its corpus and 3,000 generated cases each for seeds 11 and
23), and the release `diff_render` byte for byte against the binary from
before any change, on 30,081 cases (the corpus and 1,500 generated cases
for each of 20 seeds) under the newest Unicode table and under 4.1.0.
It was kept only if callgrind's instruction count fell; wall-clock time
moves by 10 to 20% between runs on this machine, so it confirms but does
not decide. Every fast path is in docs/PORTING.md's "Performance notes"
with the test that pins it.

Instructions for 20 frames at 80x24, millions, after each kept step:

Churn: 6.01M at the start (rich's path). Drawn with `Canvas::put_rgb`,
each true-colour pair's SGR encoded once per style id and kept whole (so
a change of both colours is one copy): 1.58M.

The chart, one row a kept step:

| Step | Chart |
|---|---:|
| Start (rich's path) | 2.14M |
| `CellGrid` of plain cells and a `StyleTable`; drawn with `Draw` and `write_runs`; `bresenham` an iterator | 1.23M |
| The grid's cells copied straight into the screen (`write_cell_grid`); `Op` specs `Arc<str>` | 0.92M |
| A cell of one character written without a UTF-8 check (`Cell::single_char`) | 0.85M |
| The pen names a style's cached SGR (`Held`) rather than copying it | 0.79M |
| Braille lines set their dots directly; `Bounds::clip` skips Liang-Barsky inside the bounds (same arithmetic); `DotCanvas` layers `u32` | 0.71M |
| `raster_into` (no second grid and blit); `Layer::finish` matches its pixels once; a spec looked up once a run of steps | 0.64M |
| The painter passes unchanged cells a run at a time | 0.61M |
| The painter keeps the SGR of each change between two cached styles (`SgrChanges`) | 0.58M |

The last step is 4% of the instructions but most of the chart's paint
time (69 to 21 µs at 80x24): it replaces string compares and copies a
cell with one lookup.

At 200x60: churn 36.5M to 9.5M a frame, the chart 7.1M to 1.8M.

#### rich's strings made light

The maintainer also decided (2026-10-25) to make rich's per-cell strings
lighter in rich itself, a breaking change to `rs-rich` 0.0.x with the
output unchanged. `Segment.text` and `Color.name` became
`compact_str::CompactString` (re-exported as `rich::CompactString`).

Why `CompactString` and not `Arc<str>` or `Cow<'static, str>`: the
profile's cost was making the strings, not copying them. Every frame
builds its segments' text and its colours' names anew (`Color::from_rgb`
names a colour `#rrggbb`), so `Arc<str>` would still allocate once each,
and `Cow<'static, str>` only saves for text known at compile time, which
neither a run nor a colour's name is. `CompactString` keeps up to 24
bytes inline: a colour's name (7 bytes) and most runs never allocate.
Measured, rich's path, allocations a frame (the baseline gate):

| Scene, 80x24 | before | after |
|---|---:|---:|
| ChurnRich (a `Segment` and a `Style` a cell) | 5,871 | 111 |
| ChartRich | 1,410 | 598 |
| TableSort | 3,288 | 2,179 |
| Unicode | 1,410 | 1,068 |
| ListPage | 510 | 124 |
| LogAppend | 260 | 200 |

and instructions a frame: ChurnRich 5.52M to 3.73M, TableSort 3.98M to
3.54M, ListPage 0.59M to 0.46M, Unicode 2.23M to 2.14M, ChartRich 1.36M
to 1.29M. One thing it cost first: `CompactString::push` a character at a
time is slower than the allocation it saves (ChurnRich went to 6.35M), so
`ColorTriplet::hex` builds the name on the stack and copies it in once.

Verified as for every change to the mirror: `cargo test -p rs-rich` (the
goldens and the parity tables), `diff_render` byte for byte against the
starting binary on 30,081 cases under the newest Unicode table and under
4.1.0, and the differential fuzzer against Python rich 15.0.0 (its corpus
and 3,000 generated cases each for seeds 11 and 23).

#### The two paths, side by side

The paint benchmark, `cells`, best of three interleaved runs of the
starting binary and the final one (µs a frame; bytes unchanged
everywhere):

| Size | Scene | start | rich's path now | fast path now |
|---|---|---:|---:|---:|
| 80x24 | Churn | 718 | 244 | 94 |
| 80x24 | Chart | 302 | 138 | 55 |
| 200x60 | Churn | 4,430 | 1,704 | 697 |
| 200x60 | Chart | 1,059 | 404 | 139 |

rich's path gained from the light strings and from everything the two
paths share (the chart's grid, the painter); the fast path is still 2.5
to 3 times faster, as it builds no segment or style at all. In
instructions a frame at 80x24: rich's path is 2.4 times the fast path's
on churn (3.73M against 1.58M; it was 3.5 times before the light
strings) and 2.2 times on the chart (1.29M against 0.58M).

The other scenes, through rich's path as before (start, now): TableSort
454 to 379 µs, Unicode 235 to 203, ListPage 63 to 37, ListRow 102 to 55,
LogAppend 79 to 48 at 80x24; at 200x60 TableSort 1,247 to 1,061, Unicode
615 to 515, ListPage 303 to 181, LogAppend 398 to 207. The small scenes
(blink, typing, the clock) are unchanged within noise (3.5 to 13 µs).

#### Against ratatui, now

`tests/versus_ratatui.rs`, release, best of three runs (ratatui's time is
its best across them):

| Size | Scene | ratatui | intuiTUIve | / ratatui | before |
|---|---|---:|---:|---:|---:|
| 80x24 | Status tick | 88 µs, 37 B | 8 µs, 18 B | 0.10 | 0.10 |
| 80x24 | Selection move | 88 µs, 184 B | 23 µs, 118 B | 0.26 | 0.24 |
| 80x24 | Log append | 93 µs, 443 B | 16 µs, 325 B | 0.17 | 0.21 |
| 80x24 | Churn, every cell | 340 µs, 62.0 kB | 104 µs, 61.4 kB | 0.31 | 1.58 |
| 80x24 | Chart redrawn | 84 µs, 4.2 kB | 60 µs, 2.8 kB | 0.71 | 3.5 |
| 200x60 | Churn, every cell | 2,049 µs, 376 kB | 762 µs, 368 kB | 0.37 | 1.84 |
| 200x60 | Chart redrawn | 353 µs, 14.7 kB | 166 µs, 8.7 kB | 0.47 | 3.2 |

The target, within 1.5 times ratatui's time on churn and the chart, is
met with room: both are now faster than ratatui's, with fewer bytes.

#### Tried and dropped

- *Formatting a true-colour pair's parameters on the stack and copying
  them in with one `push_str`*: the UTF-8 check (`str::from_utf8`, not
  inlined) cost more than the byte pushes it saved (churn's painter went
  up). Caching the encoded pair per style id (`RgbCodes`) replaced it.
- *Recycling the pen's `Sgr` buffers between frames*: fewer allocations,
  but the copies stayed; naming the cached parameters by style id
  (`Held`) removed both.

#### What is left

- **The painter** is now most of a frame that changes a lot (churn's 60
  of 94 µs at 80x24): moving the pen (`goto_into`, `move_pen`) and the
  bookkeeping per written cell.
- **rich's path** still builds a `Style` per segment (colour names now
  inline, but each `Style` is a struct of two `Color`s and an
  `Option<String>` link) and interns it (`style_key` is 5% of churn
  through rich's path); text scenes are still mostly markup and
  wrapping (above).
- **More widgets on the fast path:** `LevelMeter`, `Distribution`,
  `SmallMultiples` and the bullet chart keep rich's path (their
  `rich_render` returns early in places); lists and tables in intuiTUIve
  draw rich text, which the profile has not yet shown worth a fast path.

### Noctiluca

- **Chrome never scrolls.**
  - The design note says Chrome's scroll follows ours, "so lazy loading
    and infinite scroll still fire" (`browser.md:358`), but nothing sends
    the scroll command (`Command::Scroll` exists in `tab.rs:76` and is
    never used).
  - Feeds never load more, and `loading=lazy` images below the fold never
    load.
- **Every read is the whole document.**
  - `DOMSnapshot.captureSnapshot` has no clip, viewport or incremental
    mode, so this is a protocol limit, not ours.
  - `build()` walks every node.
  - `projection()` lays out every block into a full grid.
- **Scrolling is cheap, but some keys are not.** The scroll view copies
  rows out. But `scroll.down`, `page.down`, `bottom` and find all call
  `view_lines()`, which lays the whole page out again on every keypress.
  With find active, that happens once per text piece on each draw.
- **Hard limits that fail silently:**
  - the 4,000-row scroll cap;
  - `MAX_BLOCKS = 20,000`, whose `truncated` flag nothing reads.
- **Images:** a Sixel or iTerm2 picture shows as half blocks unless its
  whole height is on screen (`image.rs:611`). Kitty works row by row.
- **Wasted work:** rs-rich-embed starts Chrome's screencast and decodes
  every JPEG frame even under Noctiluca, which never shows them. The
  design note measured this starving snapshot calls.

### rs-rich-web

- **The DOM renderer sends only the rows that changed**, but finds them by
  scanning every cell each frame. It ignores the damage list.
- **It rebuilds the accessibility tree's JSON each frame.**
- **There is no scrollback.** The page holds exactly the screen's rows, and
  the wheel goes to the app.

## How others do it

- **Textual:**
  - Finds what is visible through a spatial map of 100x20 tiles, so
    scrolling 1,000 widgets costs about as much as 8.
  - Merges dirty regions into per-line spans.
  - Long content uses a line API (`render_line(y)` over a `virtual_size`),
    which renders only the rows asked for.
- **Bubble Tea v2 on ultraviolet** is the closest to our cell screen, and
  the best model:
  - **Scrolling:** it hashes rows, finds runs that moved by one shift, and
    scrolls them in the terminal (whole-screen SU/SD, then DECSTBM margins,
    then IL/DL) before diffing the rest.
  - **Shorter runs:** ECH and REP only where they are shorter than the run,
    and EL where it is cheaper than blanks.
  - **Cursor moves:** it picks the shortest of several encodings.
  - **Safety:** it turns scrolling off on Windows.
- **notcurses** counts what it elided and emitted per frame (cells,
  colours, bytes, raster and write time). Our stats should do the same
  before we tune anything.
- **Carbonyl** is fast because Chromium paints only damaged pixels, and
  only the viewport. A DevTools client cannot get that. Its output side is
  a plain cell diff with one write per frame.
- **Browsh:**
  - Parses only a window of about six screens either side.
  - Watches the DOM with a MutationObserver, debounced at 100 ms on the
    leading edge.
  - Turns `fixed` and `sticky` elements into `absolute`, so they do not
    stutter.
- **Virtual lists on the web:**
  - **react-window and TanStack Virtual:** overscan, estimated then
    measured heights, and stable keys.
  - **Anchoring:** keep the top visible item still when content is
    inserted above.
  - **Infinite loaders:** a threshold of rows before the end, a minimum
    batch, and one load in flight as back-pressure.
- **Terminal caveats:**
  - Scrolled-in lines may take the current background, so reset SGR
    before scrolling and repaint the exposed rows.
  - DECSTBM homes the cursor.
  - tmux redraws scroll regions itself, so the saving is smaller there.
  - Kitty has no background-colour erase.
  - Generic xterm, screen and the Linux console have no REP.

## Proposal

Three pieces, in order. The core pieces are generic. Noctiluca uses them
but owns nothing they need.

### 1. A virtual scroll view in intuiTUIve core

`scroll_virtual(len, height_of, row)`: a scroll view over content
measured in rows that lays out and draws only what is in view plus
overscan.

- **Items, not a child tree:** `len` items, each with an estimated height
  that is corrected once drawn.
- **Builds only what is visible:** the items in view plus an overscan of
  one screen above and below. They are kept by key while they stay near
  the view and dropped when they leave it.
- **No row cap:** the offset is a `u64` of rows, and the scrollbar is
  computed from the estimated heights.
- **Anchoring:** when items above the view change height or are inserted,
  the top visible item keeps its row.
- **`on_near_end(threshold, f)`:** called once when the view comes within
  `threshold` rows of the end, and not again until `len` grows. That is
  the forever-scroll hook, with one load in flight.
- **The existing widgets keep their behaviour:**
  - `scroll()` stays as it is, for small content;
  - `list_lazy` and `virtual_table` move onto the same viewport code;
  - `MAX_SCROLL_ROWS` stays only for `scroll()`, documented, and warns in
    debug builds when it cuts content.

Second users besides Noctiluca: log viewers, chat, file lists, search
results, `rich pager` on huge files.

### 2. Paint strategies in the painter, with `best`

`App::paint_strategy(PaintStrategy)` (also `RICH_PAINT_STRATEGY`). The
choice sits at `Driver::paint` and inside `Painter::paint`, the one place
a frame becomes bytes.

- **`Cells`:** today's per-cell diff inside the damage. The default until
  `best` has its numbers.
- **`Rows`:** compare whole rows by hash, and rewrite changed rows from
  their first change to their last.
- **`Full`:** repaint everything: home, then each row, then clear the rest.
- **`Scroll`:** find runs of rows that moved by one shift, scroll them in
  the terminal, then diff what is left.
  - It tries whole-screen SU/SD first, then DECSTBM margins.
  - It resets SGR first, repaints the exposed rows, and treats the cursor
    as lost afterwards.
  - It is off on Windows consoles, and it is skipped when a Sixel or
    iTerm2 image is in the moved region.
- **`Best`:** per frame, encode the candidates that apply and send the
  shortest. It is cheap because every candidate works from the same damage
  and row hashes:
  - always `Cells`;
  - `Scroll` when the row hashes show a shift;
  - `Full` when more than a measured share of the screen changed.

  It is bounded so it never costs more than it saves: past a cell count it
  stops encoding and picks by the damaged share alone. Its thresholds come
  from the benchmarks below.

Every strategy also gets these encoder improvements, each checked against
the terminal's capabilities:

- **Style changes:** send the change from the current style, not a full
  reset, where that is shorter.
- **Cursor moves:** the cheapest of CUP, a relative move, CR plus a
  relative move, or writing through one or two unchanged cells.
- **Shorter runs:**
  - ECH or EL for a run of blanks, only with a reset background (kitty
    has no background-colour erase);
  - REP for an ASCII run, only on terminals known to have it.
- **One write per frame**, built in one buffer. It already goes out in one
  synchronized-output block.

The work stays generic: everything is in rs-rich-ext's painter and
intuiTUIve's driver. rs-rich-web's DOM renderer moves to the damage list
too, sending the rows the damage touched instead of scanning every cell.

### 3. Noctiluca on both

- **Scroll Chrome along with us.**
  - When our offset moves, send a debounced `Input.dispatchMouseEvent`
    (`mouseWheel`), so the page's own IntersectionObserver sentinels fire.
  - Use `window.scrollTo` for jumps such as `bottom` and find.
  - Read `Page.getLayoutMetrics` back for the content height.
- **Near the end, ask Chrome to scroll on.** When our view comes within a
  screen of the projection's end, scroll Chrome to its bottom too. Re-read
  on the page's change ping, with one read in flight.
- **Project only what is near the view.**
  - Index blocks by their y.
  - Lay out only the blocks within the view plus overscan (two screens),
    with prefetch beyond that.
  - Keep the page's own coordinates for the rest, so the scrollbar and
    jumps stay right.
- **Anchor by node.** On a re-read, keep the top visible block (by
  `backendNodeId`) on the same row, and reconcile with Chrome's own scroll
  anchoring.
- **Cache the layout.** `view_lines()` stops laying out the whole page per
  keypress. The memo is shared, and find works from a text index.
- **Say when the page was cut.** Show `truncated` in the status line.
- **Stop the unused screencast.** rs-rich-embed's screencast runs only for
  a raster view; the design note's "screencast on demand".
- **Images:** draw a Sixel or iTerm2 picture once it is partly in view,
  clipped to the rows shown. That needs a per-region still cut to the
  visible part.

A full snapshot per read stays, since the protocol has no other kind.
Prototyping a JavaScript walker that serialises only elements near the
viewport is listed below, to be measured on a long feed before it
replaces anything.

## Ideas from game engines

Games have spent decades on "draw a lot, every frame, within a budget".
Most of their ideas carry over to a cell screen. In order of expected
payoff:

1. **Data-oriented cells.** A `Cell` is `{ text: String, width, style }`
   (`rich-ext/src/surface/screen.rs:203`): a heap string per cell.
   - Comparing, copying and keeping the "last sent" screen all chase a
     pointer per cell. That is the likeliest reason churn costs 3.0 ms
     against ratatui's 0.33.
   - Games lay hot data out flat (structure of arrays, no per-item
     allocation). The same here: a cell becomes a packed `u32` code point,
     or an index into a grapheme table for the rare cluster, plus width
     and style, in one `Vec`.
   - Comparing a row is then comparing a slice, and hashing it is cheap.
2. **A frame budget with adaptive quality** (dynamic resolution).
   - Measure each frame's build and write time. When a frame runs over
     budget, or the terminal is slow to take our writes, step quality
     down for the regions that move:
     - real pixels drop to half blocks, then to placeholders;
     - a region's frame rate drops.
   - Step quality back up when things settle.
   - Noctiluca's media "byte budget" is this idea; it belongs in core.
3. **Level of detail while scrolling fast.** During a fast scroll or a
   held key, draw cheap stand-ins: text without images, and skeleton rows
   for items not built yet. Draw full detail once the scroll rests for a
   few frames. Lists, Noctiluca and image-heavy views all gain.
4. **A render thread with back-pressure** (triple buffering).
   - Build frame N+1 while frame N is still being written.
   - If the terminal is still busy when a new frame is ready, drop the
     stale one and send only the newest. Games drop frames rather than
     queue them; today a slow terminal queues our writes.
   - The frame rate follows how fast the terminal really takes bytes.
5. **Culling.**
   - **Frustum culling is the viewport:** the virtual scroll view above.
   - **Occlusion culling:** a node wholly covered by an opaque layer (a
     modal, a full-screen overlay) need not draw at all.
   - **Spatial partitioning:** a coarse tile grid, like Textual's 100x20
     map or a game's quadtree, makes both cheap, and also hit-testing.
6. **Tiles with hashes.** Split the screen into tiles and keep a hash per
   tile and per row. A frame compares hashes first, and cells only inside
   tiles that differ. The `rows` and `scroll` strategies need row hashes
   anyway.
7. **Fewer state changes** (draw-call batching). GPUs sort draws to
   change state less. A terminal's state is the cursor and the SGR pen,
   so the encoder writes each row's runs in an order that changes the pen
   least, and sends style deltas instead of resets. This is the encoder
   work above, framed as state sorting.
8. **Parallel encoding** (job systems). Rows encode independently once
   the pen state at each row start is fixed, so a big damaged frame can
   encode rows on several threads and join them. Worth it only past a
   size the benchmark finds.
9. **Asset atlases** for images. Kitty can keep an image under an id and
   place it many times, or move a placement, without sending it again.
   - Like a texture atlas, we upload once and place by reference.
   - A scrolled picture then costs a placement, not a re-upload.
   - Sprites and micro assets reuse uploaded frames, and Kitty's own
     animation frames play a GIF with no bytes from us per frame.
10. **Arenas and pools.** Reuse per-frame buffers (damage lists, span
    lists, the output buffer) instead of allocating them each frame.
11. **A frame-time overlay** (a game's performance HUD). The inspector
    shows per-frame build, encode and write time, bytes, cells elided and
    strategy chosen, as a small graph. That makes regressions visible
    while using an app, not only in CI.
12. **Network ideas, for rs-rich-web:**
    - **Interest management:** send only what a client can see.
    - **Delta snapshots with acknowledgements**, so a slow client gets
      the newest state and not a backlog.
    - **Client-side prediction of typed characters:** local echo,
      corrected by the next frame.

13. **A timedemo.**
    - Doom's `-timedemo` replays a recorded demo (inputs only) as fast as
      the machine can draw it, and reports frames per second. Every Doom
      port has been compared that way for thirty years.
    - We have the pieces: rs-rich-record's tapes record input, and the
      headless driver runs on a virtual clock.
    - A `timedemo` mode replays a tape against an app with no waits and
      reports frames, time, bytes and the strategy chosen per frame.
    - Any app becomes a benchmark scene with no code, and a user's slow
      case becomes a reproducible report.
14. **Lookup tables, as Doom's colormaps are.** Doom never computes a
    shade; it looks it up. A frame of pixels in a fixed palette can keep
    one precomputed SGR string per palette pair, instead of formatting
    `38;2;r;g;b` per run.
15. **Exact damage for slow-changing parts.** Doom's status bar redraws a
    number only when it changes, while the 3D view redraws whole. Mixed
    screens (a full-motion region beside a few static widgets) are the
    case `best` must get right: full repaint for the moving region, cell
    diff for the rest, per region rather than per frame.
16. **Sample input as late as possible** (late latching). Games read
    input just before rendering to cut latency. We drain events before a
    frame (`DRAIN_EVENTS = 64`). A frame that is built slowly should
    re-drain before it is encoded, so a key pressed during the build
    shows in that frame rather than the next.

### What rs-rich-doom taught us already

It is our heaviest real workload: 35 full-screen frames a second, with
sound. Its measurements and [core gaps](doom-core-gaps.md) belong here.

- **Throughput:**
  - about 34 fps in a real PTY at 80x24 and 160x50, in release, in every
    mode;
  - in debug, Sixel at 160x50 falls to 7.7 fps;
  - the style table stays bounded (peak 2,766 at 80x24 Braille).
- **Its gaps overlap this plan:**
  - **gap 1, a live frame image node:** today each frame is encoded to
    PNG, decoded on a worker and shown after a "loading" step;
  - **gap 3, Kitty raw-pixel transmission and reused ids:** this is
    idea 9;
  - **gap 6, frame timings in `FrameStats`:** this is the stats work in
    step 1.

  They are scheduled for 0.0.23 already; this plan takes gaps 1, 3 and 6
  into its steps rather than doing them twice.
- **It makes the churn scene real.** Doom at 35 fps in half blocks is
  the benchmark's "every cell changes" case, played from a recorded demo
  as a timedemo (idea 13), with Freedoom's data downloaded in CI as its
  own tests do.

Each goes through the benchmark below before it is kept. Ideas 1, 2, 4, 6
and 7 change the core painter. Idea 3 is the virtual scroll view's.
Ideas 9 and 12 are the image layer's and rs-rich-web's.

## Lessons from games, researched

A research pass on 2026-10-25 into how games optimise. Some primary pages
were blocked; those parts are marked unverified in the pass, and nothing
below rests on them alone.

### How games work, not just what they do

- **Budgets, not averages.**
  - A frame has a budget in milliseconds, and each subsystem has a share
    of it.
  - Frames are judged by their time percentiles (p50, p99, max), because
    average frames per second hides hitches.
  - [Android's frame-pacing guide](https://developer.android.com/games/sdk/frame-pacing)
    adds that a steady cadence matters as much as speed: a frame that
    comes early, then a late one, reads as stutter.
- **Profile each stage, per frame.** Tracy, PIX and Unreal Insights slice
  time by frame into named stages, so the stage that blew the budget is
  visible.
- **Fixed input, repeatable runs.** The Quake and Doom timedemos replay
  recorded input as fast as possible. CI gates fail a build on p95 or p99
  against a baseline, with a noise floor.
- **Data first.** Mike Acton's
  [CppCon 2014 talk](https://github.com/CppCon/CppCon2014): reading a
  64-byte cache line to use 4 to 8 bytes wastes about 90% of it, and
  fitting a loop's data to the line gave a 10x speedup. Strings are his
  example of the problem.
- **Change state rarely.** Draw calls are sorted by a key so that
  consecutive draws share state. Skia's Graphite still orders draws to
  "minimize pipeline transitions".
- **Send the latest state, never a backlog.**
  - [Mosh](https://github.com/mobile-shell/mosh/blob/master/src/terminal/terminaldisplay.cc)
    synchronises screen state, not the byte stream. It skips
    intermediate frames and paces itself to the round trip.
  - VNC sends an update only when the client asks for one.
  - Wayland's frame callback says when to draw next, and is withheld
    from hidden surfaces.
  - Quake 3 sends deltas against the last state the client
    acknowledged.
- **Move what is there, then draw the seam.** VNC's CopyRect and the NES's
  scroll register move pixels the display already has, so only the newly
  exposed row or column is written.

### What it says about our painter

Read against `rich-ext/src/surface/screen.rs`:

- **The cell layout.** `Cell { text: String, .. }` is about 32 bytes with
  its glyph behind a pointer. The diff dereferences two heap strings per
  cell. ratatui keeps the symbol inline (`CompactString`).
- **Allocation in the output.** `goto` builds every cursor move with
  `format!`, and `paint` returns a fresh `String` each frame.
- **Colour codes.** `set_pen` sends a full reset on every style change.
  ratatui diffs the foreground, the background and the attributes
  separately. True colour codes are not cached.
- **Cursor moves.**
  - Mosh's `put_row` picks the cheapest move.
  - It reprints a short gap of unchanged cells rather than jump over it.
  - It uses ECH for blank runs longer than 4 and EL where the background
    allows.
  - It detects scrolled blocks of rows and scrolls them.

Mosh's `terminaldisplay.cc` is close to a reference for the painter we
want.

### Ranked by expected payoff

The columns are (a) full-screen churn, (b) scrolling, (c) small updates,
and (d) slow or remote terminals and rs-rich-web.

| Change | (a) | (b) | (c) | (d) |
|---|---|---|---|---|
| Cells without a heap string: inline or in planes | High | Med | Low | Low |
| Style deltas instead of resets; cache true colour codes | High | Med | Low | High |
| No allocation per frame: persistent buffers, no `format!` per move | Med | Med | Med | Low |
| Cost-based moves; reprint short gaps; ECH and EL | Med | Low | Low | High |
| Scroll in the terminal and draw the seam; render only the visible window | Low | Very high | none | Very high |
| A coalescing scheduler: latest state only, wait for the write to drain, read input late | Med | Med | Med | Very high |
| Skip unchanged rows inside the damage | Med | Low | Low | Low |
| A byte budget with priorities, and lower quality when over it | Low | Low | Low | High |
| Mosh-style deltas against an acknowledged frame, and local echo, for rs-rich-web | none | Med | High (feel) | Very high |

### How we will know

- **The rig first.** A stage breakdown for every scene. Frame-time
  percentiles, bytes and allocations per frame. A CI gate that a
  deliberate 20% regression fails.
- **Cells without the heap:**
  - full-screen at 80x24 within 1.5 times ratatui's 0.33 ms (1.0 ms on
    the way there);
  - cache misses per frame well down;
  - the status tick still 15 µs or less.
- **Style deltas:** the chart redraw under 150 µs, and its bytes down 30%
  or more, with the emulator's screen identical.
- **No allocation per frame:** zero, in steady state, on the tick and
  chart scenes.
- **Cost-based moves:** bytes down 20% or more on the churn and form
  scenes.
- **Scrolling in the terminal:**
  - a one-row scroll of a full pane costs about one row of bytes plus
    the control sequences;
  - no tearing inside synchronized output;
  - images placed again correctly.
- **The coalescing scheduler:** under a throttled link, the time from
  input to the latest state shown stays bounded, and the frame queue
  stays at one or less.

## Compression

Measured on 2026-10-25, on the docs tapes' recorded output: 931 frames of
200 bytes or more, 4.16 MB in all, compress to 0.60 MB with zlib at level 1
(14%). That runs at about 86 MB/s even from Python, about 52 µs a frame.
Terminal output is very repetitive: escape sequences, runs of blanks, the
same styles again and again.

Where it can and cannot help:

- **A local terminal: not at all.** A terminal reads raw bytes and cannot
  decompress. On a local PTY, compressing only adds work. Here the gain
  comes from saying less: the encoder work above (style deltas, cheaper
  moves, ECH, EL and REP, scrolling) is compression in the terminal's own
  vocabulary.
- **SSH:** `ssh -C` (or `Compression yes`) already compresses the whole
  stream with zlib. That is worth saying in the docs for slow links. Mosh
  goes further: it sends state differences and echoes keys locally.
- **rs-rich-web over a network: yes.** That is where frames cross a real
  link.
  - tungstenite 0.30 has no permessage-deflate, so it would be ours to
    add.
  - The cleanest option is binary frames compressed with deflate
    (`flate2`, already in the workspace), and decompressed in the page
    with the browser's built-in `DecompressionStream("deflate-raw")`. That
    needs no JavaScript library.
  - It applies to both the xterm.js stream and the DOM renderer's JSON.
    The DOM renderer would also gain from a binary encoding before any
    compression.
  - It is turned on only above a size threshold, and only where the
    measured link is slower than the compressor.
- **Kitty images:** the graphics protocol takes zlib-compressed pixels
  (`o=z`), which matters for raw RGBA frames (Doom's gap 3).
- **Recordings:** casts and tapes can be stored compressed, but they are
  small already.

The benchmark gains a simulated link (bytes per second plus a fixed
latency per write), so each strategy and compression setting is judged by
time to the screen, not only by bytes.

## Benchmarks

A benchmark comes first and decides the defaults. Each scene runs at
80x24 and 200x60, under every strategy. Each reports bytes and time per
frame, plus counts like notcurses': cells compared, cells written, cells
elided, and bytes by kind (moves, styles, text). Every scene's output is
replayed through vt100 and must reproduce the screen exactly, so a
strategy can never be faster by being wrong.

| Scene | What it stresses |
|---|---|
| One cell blinks | The smallest change; cost of the strategy choice itself |
| Typing in a field | A short run, cursor moves |
| A status line and a clock | Two small rectangles far apart |
| Log append | One new line at the bottom of a full pane: the scroll case |
| A list scrolled a page | Every row moves |
| A list scrolled one row | Every row moves by one: the best scroll case |
| A table re-sorted | Every row changes, styles stay |
| Churn, every cell | The worst case: random colours every frame |
| A chart redrawn | Dense braille, few styles |
| Text of many scripts | Combining marks, wide CJK and Hangul: the grapheme-splitting path |
| Noctiluca scrolling a long page | The browser's case |
| rs-rich-doom's demo as a timedemo | Real full-motion churn beside a static status bar |

There are two harnesses:

- **In process:** the gate's `Feed` backend, extended to every strategy.
  It is fast enough for CI.
- **Through a PTY:** to count the real write calls and the time to the
  terminal, using `portable-pty` and `vt100` as `tests/backends.rs` does.

The results table goes in this note, and `best`'s thresholds are set from
it. CI then fails if `best` is ever more than 5% worse, in bytes or time,
than the best fixed strategy on any scene. The existing ratatui gate keeps
its assertions.

**Built:** the in-process harness, on the timedemo (`App::timedemo`), with
the first ten scenes, the vt100 check (it runs in every `cargo test`, at
80x24, and in the benchmark at both sizes), the per-stage times and
counts, and the `paint-bench` CI job. Besides the 5% gate, a second gate
holds `best`'s bytes and allocations a frame to a checked-in baseline,
10% over failing, so a 20% regression fails on any machine.
`PAINT_BENCH_SCENE`, `PAINT_BENCH_STRATEGIES` and `PAINT_BENCH_FRAMES`
narrow a run, and the `profile_one_scene` test runs one scene for a
profiler. **Not yet:** the PTY harness, and the Noctiluca and rs-rich-doom
scenes (Doom lives in its own repository; its demo can run through
`App::timedemo` there).

## Order of work

1. **The benchmark harness and stats:** the scenes, the per-frame counts,
   and the vt100 check, with today's painter as the baseline.
2. **Data-oriented cells and row hashes** (ideas 1 and 6), then **the
   encoder improvements:** shorter style changes and cursor moves,
   ECH, EL and REP, and one buffer.
3. **The strategies:** `Rows`, `Full` and `Scroll`, then `Best` with
   thresholds from step 1's numbers.
4. **The virtual scroll view, with `on_near_end`,** and `list_lazy` and
   `virtual_table` moved onto its viewport code.
5. **Noctiluca:**
   - Chrome's scroll following ours;
   - near-end loading;
   - projection near the view only;
   - anchoring;
   - the cached layout;
   - the truncated notice;
   - no screencast.
6. **rs-rich-web's DOM renderer** on the damage list.

Steps 1 to 3 are independent of 4 to 6, and can run alongside them.
Steps 1 to 3 are done on the `023-paint` branch, measured first and in
the order the profile gave: the draw's line writing, the cell layout, the
encoder, then the strategies.

## Open questions

- **Decided on 2026-10-25:** `best` ships opt-in for one release, then
  becomes the default once it beats `cells` on every scene. Work starts
  now on two branches: the painter, and the viewport with Noctiluca.
- **Decided on 2026-10-25:** the game ideas are lessons, not features.
  We learn how games optimise (measure first, profile each stage, then fix
  the biggest measured cost) and apply that to our pipeline. Each change
  is kept only if the benchmark shows a win.
- **Terminals with unknown capabilities:**
  - scrolling stays off under tmux unless the benchmark shows a gain
    there;
  - REP stays off unless the terminal is known to have it.
- **The JavaScript viewport walker:** prototype and measure on a long
  feed page before deciding.

## Sources

- **Textual:**
  - [the 1.0 post](https://raw.githubusercontent.com/Textualize/textual/main/docs/blog/posts/release1.0.0.md)
  - [`_compositor.py`](https://raw.githubusercontent.com/Textualize/textual/main/src/textual/_compositor.py)
- **Bubble Tea and ultraviolet:**
  - [the v2 renderer](https://raw.githubusercontent.com/charmbracelet/bubbletea/main/cursed_renderer.go)
  - [ultraviolet's `terminal_renderer.go`](https://raw.githubusercontent.com/charmbracelet/ultraviolet/main/terminal_renderer.go)
  - [ultraviolet's `terminal_renderer_hardscroll.go`](https://raw.githubusercontent.com/charmbracelet/ultraviolet/main/terminal_renderer_hardscroll.go)
- **ratatui:**
  [`buffer.rs`](https://raw.githubusercontent.com/ratatui/ratatui/main/ratatui-core/src/buffer/buffer.rs)
- **notcurses:**
  - [render](https://raw.githubusercontent.com/dankamongmen/notcurses/master/doc/man/man3/notcurses_render.3.md)
  - [stats](https://raw.githubusercontent.com/dankamongmen/notcurses/master/doc/man/man3/notcurses_stats.3.md)
  - [`TERMINALS.md`](https://raw.githubusercontent.com/dankamongmen/notcurses/master/TERMINALS.md)
- **Synchronized output:**
  [the specification](https://github.com/contour-terminal/vt-extensions/blob/master/synchronized-output.md)
- **Carbonyl:**
  - [the project](https://github.com/fathyb/carbonyl)
  - [its renderer](https://raw.githubusercontent.com/fathyb/carbonyl/main/src/output/renderer.rs)
- **Browsh:**
  - [`dimensions.js`](https://raw.githubusercontent.com/browsh-org/browsh/master/webext/src/dom/dimensions.js)
  - [`manager.js`](https://raw.githubusercontent.com/browsh-org/browsh/master/webext/src/dom/manager.js)
- **Chrome DevTools protocol:**
  - [DOMSnapshot](https://raw.githubusercontent.com/ChromeDevTools/devtools-protocol/master/pdl/domains/DOMSnapshot.pdl)
  - [Input](https://raw.githubusercontent.com/ChromeDevTools/devtools-protocol/master/pdl/domains/Input.pdl)
  - [Page](https://raw.githubusercontent.com/ChromeDevTools/devtools-protocol/master/pdl/domains/Page.pdl)
- **Virtual lists and infinite loading:**
  - [TanStack Virtual](https://raw.githubusercontent.com/TanStack/virtual/main/docs/api/virtualizer.md)
  - [react-window-infinite-loader](https://raw.githubusercontent.com/bvaughn/react-window-infinite-loader/master/README.md)
- **The web platform:**
  - [IntersectionObserver](https://raw.githubusercontent.com/w3c/IntersectionObserver/main/index.bs)
  - [CSS scroll anchoring](https://raw.githubusercontent.com/w3c/csswg-drafts/main/css-scroll-anchoring-1/Overview.bs)

Unverified:

- xterm's support for synchronized output;
- how each terminal moves Sixel images under DECSTBM;
- `captureSnapshot`'s cost on large pages, since nothing is published.
