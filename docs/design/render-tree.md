# Render tree spike (#226)

**Status:** design note for review (0.0.12 workstream 5). Nothing in the
crates changes. The prototype lives in
[`docs/design/render-tree/prototype`](https://github.com/buchochelliq-labs/rs-rich-cli/tree/main/docs/design/render-tree/prototype),
a standalone crate that is not a workspace member, not published and not built
by CI.

## Summary

- **Recommendation: yes, but as styled runs, not a cell grid, and in
  `rich-ext`, not core.** A frame of rows of styled runs, with styles
  interned into a table, keeps the segment stream's shape. On the benchmark
  cases it retains **2–7× less heap** than today's `Vec<Segment>`, encodes
  no slower, and reproduces today's output **byte for byte**.
- A per-grapheme cell grid is the wrong default. It costs more memory than
  segments on text-heavy output (1.3× on syntax, 1.4× on Markdown, 4× on wide
  text), and the consumers that need cells can derive them from runs on
  demand.
- Cells pay off for live repaint: a one-cell change in a 50-row table
  repaints **25 bytes** as a cell diff, against **91** for today's row diff
  and **4,385** for a full repaint.
- Semantic roles (heading, table cell, link) cannot come from the segment
  stream. They need renderables to report them, through an opt-in trait.
- Proposed milestone: **0.0.13, "Frames"**, in the three phases under
  [Migration](#migration).

## What happens today

Every renderable returns `Vec<Segment>` from `Renderable::rich_render`
(`crates/rich/src/protocol.rs`). `Console` crops the stream
(`Segment::crop_lines`) and encodes it in `segments_to_string`
(`console.rs`), which calls `Style::render` on each styled segment. There is
no SGR state between segments: each styled segment is wrapped in its own
`ESC[…m … ESC[0m`, and an OSC 8 link is opened and closed around each
segment. That is upstream's `_render_buffer`, and the goldens depend on it.

`Segment` is 136 bytes: a `String`, an `Option<Style>` and a flag. `Style`
(104 bytes) holds two `Color`s, each with a name `String`, plus a link
`String`. Nothing is interned, so every segment deep-copies its style.

Several consumers rebuild rows or cells from the stream on their own:

| Consumer | What it rebuilds | Where |
|---|---|---|
| SVG export | lines, padding, x positions from `cell_len` | `crates/rich/src/svg.rs` (`split_and_crop_lines`, `export_svg`) |
| `LiveCoordinator` | rows, by re-wrapping each region's segments into `Text`, then a per-row string diff | `crates/rich-ext/src/live/mod.rs` (`rows`, `paint`), `layout/overflow.rs` (`fit_segments`) |
| QA stress, matrix, lint, explain | plain lines and widths | `crates/rich-ext/src/qa/mod.rs` (`plain_lines`), `qa/lint.rs` |
| `qa::profile` frame cost | a `w×h` shape via `Segment::set_shape` | `crates/rich-ext/src/qa/profile.rs` |
| Accessibility | table columns and tree depth, by recognising box and guide glyphs in a plain render | `crates/rich-ext/src/a11y/semantic.rs` |
| Test terminal emulator | a cell grid, from the ANSI bytes | `crates/rich-ext/tests/support/screen.rs` |

Information is also lost at the segment boundary:

- **Link identity.** A link split across segments or wrapped lines becomes
  several unrelated OSC 8 pairs. HTML and SVG export drop links entirely.
- **Structure.** Which cells are a table header, a tree guide or a heading is
  known inside `Table`, `Tree` and `Markdown` and gone afterwards. The a11y
  module says so, and lists the layouts its heuristics misread.
- **Snapshot stability.** `rich_ext::testing::RenderSnapshot` stores
  segments, so a change in how text is split into segments shows as a diff
  even when every cell looks the same.

## What a frame would give

| Area | Today | With a frame |
|---|---|---|
| **Export** | SVG re-derives the grid. HTML is a flat span stream without links. | One layout feeds text, HTML and SVG. Links can become `<a>` elements, because a run knows its link. |
| **Snapshots** | Segment-level JSON. Harmless resegmentation shows as a diff. | Compare runs after merging, or cells: what shows, not how it was split. |
| **Accessibility** | Structure guessed from glyphs. | Regions with roles, reported by the renderables that know them. |
| **Live and TUI** | Re-wrap plus row diff, repainting whole changed rows. | Cell diff against the previous frame, with cursor-addressed runs. |
| **Recording and replay** | Record the ANSI stream. | Record frames or frame diffs, and replay at any colour depth. |
| **Encoding** | One SGR pair per segment. | Default: identical bytes. Opt-in: merged runs, up to 3.9× fewer bytes. |

## Options

1. **Status quo plus helpers.** Keep `Vec<Segment>` and share a
   `split_rows` helper among the consumers above. Cheap, but nothing becomes
   smaller, links and structure stay lost, and each consumer still decodes
   styles on its own.
2. **Styled runs (recommended).** Rows of `Run { text: u32, len: u32, cells:
   u32, style: u32 }` (16 bytes) over one text arena, with a style table.
   Cells are computed on demand.
3. **Cell grid.** Rows of `Cell { text: u32, len: u16, width: u8, style: u32 }`
   (12 bytes), one per grapheme, with a continuation cell after a wide
   grapheme. This is what a TUI needs for diffing, but it is expensive to keep
   for scrollback-sized output.
4. **A new core render protocol**: `rich_render` returns a frame. Rejected.
   It rewrites every renderable, breaks the mirror rule in `AGENTS.md`, and
   makes every future upstream sync a translation.

## Measurements

The prototype renders each case once through core, as `Console::print` does
(`rich_render` then `crop_lines`), and builds every representation from that
one stream. Before timing anything it asserts that:

- the cell grid and merged runs encode to the same ANSI as the merged
  (`Segment::simplify`d) stream, line by line;
- exact runs encode to **the same bytes as `segments_to_string`**;
- plain text is identical.

These are the `library_bench` cases, plus the `bench_cli.py` files and a wide-
character case. Width 100, truecolor. Medians of 15 runs in milliseconds, on a
4-core Intel Xeon (2.8 GHz), release build. Heap figures are retained bytes,
from a counting allocator.

| Case | Segments | Runs | Cells | Styles | Render | → runs | → cells | Encode segments | Encode runs | Encode cells |
|---|---|---|---|---|---|---|---|---|---|---|
| justified text | 2,017 | 64 | 6,396 | 2 | 1.95 | 0.18 | 0.48 | 0.62 | 0.03 | 0.08 |
| table, 500 rows | 7,019 | 510 | 33,264 | 3 | 10.83 | 0.81 | 1.88 | 0.38 | 0.03 | 0.36 |
| panel of markup | 629 | 533 | 2,100 | 4 | 0.81 | 0.54 | 0.63 | 0.11 | 0.11 | 0.13 |
| Markdown, `DIVERGENCES.md` | 2,554 | 1,736 | 37,501 | 16 | 6.70 | 2.34 | 3.52 | 0.41 | 0.37 | 0.74 |
| syntax, `text.rs` (46 KB) | 15,791 | 7,865 | 178,500 | 10 | 302.76 | 16.84 | 24.30 | 12.74 | 5.77 | 9.87 |
| JSON, 400 records | 26,402 | 20,801 | 69,609 | 8 | 24.01 | 10.56 | 12.94 | 2.91 | 2.71 | 3.45 |
| wide (CJK and emoji) | 149 | 75 | 7,000 | 2 | 0.64 | 0.10 | 0.19 | 0.03 | 0.03 | 0.08 |

"Runs" counts merged runs. Exact runs number about as many as segments.

| Case | Segments heap | Runs heap | Cells heap | ANSI bytes: today → merged runs |
|---|---|---|---|---|
| justified text | 282 KB | 39 KB | 102 KB | 27,953 → 7,163 |
| table, 500 rows | 969 KB | 152 KB | 700 KB | 38,243 → 38,195 |
| panel of markup | 87 KB | 14 KB | 34 KB | 6,692 → 6,692 |
| Markdown | 382 KB | 89 KB | 528 KB | 50,299 → 49,315 |
| syntax | 2,454 KB | 446 KB | 3,160 KB | 668,127 → 440,873 |
| JSON | 3,610 KB | 553 KB | 1,378 KB | 162,426 → 162,426 |
| wide | 32 KB | 16 KB | 126 KB | 12,949 → 12,949 |

Runs are allocated with one slot per segment, so exact runs retain the same
heap as merged runs.

**What the numbers say:**

- **Building a frame costs 6–67% of rendering.** It is cheapest where
  rendering is expensive (syntax: 17 ms on 303 ms), and relatively dearest for
  small outputs. Built on demand from the existing stream, it adds nothing to
  a plain `print`.
- **Runs are 2–7× smaller than segments** in every case (2× only on the wide-character case, whose segments are already long). Most of that comes
  from not cloning a 104-byte `Style` with heap strings into every segment.
  Style interning alone would recover much of it.
- **Cells are 12 bytes per grapheme**, so they lose to segments wherever a
  segment carries many characters (syntax, Markdown, wide text), and win only
  where segments are short (JSON, tables, justified text). They never beat
  runs.
- **Encoding is never slower from runs**, and 2–20× faster where runs merge.
  The merged bytes are fewer, but they are not upstream's bytes; see
  [Constraints](#constraints).
- **Live repaint:** changing one table cell's style changes 1 row and 7 cells.
  A cell diff writes 25 bytes, today's row diff 91, and a full repaint 4,385.
  A layout change (a wider column) changes every row and gains nothing.

Reproduce with:

```bash
cd docs/design/render-tree/prototype
cargo run --release
```

Run it from that directory: its `.cargo/config.toml` puts the build under the
repository's `target/`, not inside `docs/`.

## Constraints

- **Default output stays byte-identical.** The goldens and the differential
  corpus compare bytes, and upstream writes one SGR pair per segment. A frame
  must be able to encode exactly that. The prototype's exact runs do.
  Merging is an opt-in encoding for places that are not parity surfaces,
  such as live repaint, recordings and our own exports.
- **Core stays a mirror.** A frame type is not upstream, so it belongs in
  `rich-ext`, built from the public segment stream. The only core changes
  worth considering are additive and behaviour-free, such as deriving `Hash`
  on `Style` and `Color` so interning does not need the prototype's `Debug`
  string key. Each would be recorded in PORTING and DIVERGENCES.
- **Graphemes follow core.** Cells use `rich::cells::split_graphemes` and its
  width tables, so a frame's columns match what core measured and cropped.
- **Control segments are not content.** The frame drops them. Whatever
  paints the frame (a live region, a TUI) owns cursor movement.

## Migration

Existing renderables do not change. A frame is built from what they already
return.

**Phase 1: frames in `rich-ext` (no core change).**

- `rich_ext::frame::{Frame, Run, StyleTable}`, with
  `Frame::from_segments`, exact and merged ANSI encoders, plain text, cells on
  demand (`Frame::cells(row)`) and `Frame::diff`.
- `RenderTarget::frame(renderable)` beside `segments()`, with the same
  control and link filtering.
- Move the re-deriving consumers onto it, one PR each, each proving
  unchanged output:
  - `LiveCoordinator`: cell diff, with row diff as a fallback when rows move;
  - QA stress, matrix and lint: rows and widths from the frame;
  - `RenderSnapshot` schema 2: runs after merging, so resegmentation stops
    showing as a diff (schema 1 stays readable);
  - an ext SVG/HTML exporter from frames, which can keep links.

**Phase 2: semantic regions (opt-in).**

- An extension-point trait in `protocol.rs`, with an empty default so core's
  behaviour is unchanged. For example:
  `fn regions(&self, console, options) -> Vec<Region>`, where a `Region` is
  rows × columns plus a role (heading level, table header, table cell,
  link, decoration).
- Ext renderables (`TableData`, `StreamingTable`, diagnostics, badges)
  implement it first, because their layout is ours.
- For core `Table`, `Tree`, `Panel` and `Markdown`, the regions would come
  from ext wrappers that re-derive structure from public data. Upstream
  modules themselves are not touched. Where that is not possible, the a11y
  heuristics stay.
- The a11y module and HTML export consume regions when present.

**Phase 3: optional core interning.**

- Only if Phase 1 shows the frame build to be the bottleneck: derive `Hash`
  for `Style` and `Color`, then consider an interned style id on `Segment`
  behind an off-by-default feature (AGENTS.md rule 3).

**Not proposed:** changing `Renderable::rich_render`, or a frame-returning
twin method on every core renderable.

## Proposed milestone: 0.0.13 "Frames"

| Item | Crate | Acceptance |
|---|---|---|
| `rich_ext::frame` (runs, style table, exact and merged encoders, cells on demand, diff) | ext | Exact encoding is byte-identical to `segments_to_string` on the goldens and the differential corpus |
| `RenderTarget::frame` | ext | Same filtering as `segments()`, tested per target kind |
| `LiveCoordinator` cell diff | ext | Bytes written drop on the live tests, and the terminal-emulator tests pass unchanged |
| Snapshot schema 2 | ext | Resegmented but identical output compares equal; schema 1 fixtures still load |
| Frame-based HTML/SVG export with links | ext | Links survive as `<a>`; the existing export tests stay green |
| Semantic regions trait and ext implementations | core seam + ext | Default output unchanged; the a11y output for `TableData` no longer depends on glyphs |

Each item is its own PR, and none changes default output.

## Open questions

1. Should merged-run encoding become the default for the ext live and export
   paths? It is smaller, but a diff against an upstream-rendered file would
   then show SGR differences with no visible effect.
2. Interning key: derive `Hash` in core (tiny, additive) or hash in ext from
   `Style`'s public accessors?
3. Wide-grapheme continuation cells: store them (simple diffs), or skip them
   and index by column (smaller)? The prototype stores them.
4. Whether a future TUI crate should own `Frame::diff` and cursor addressing,
   leaving ext with the data type only.
