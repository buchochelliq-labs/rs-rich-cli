# Changelog

Notable changes to the port. Because the mirror crates track upstream versions,
entries here note which upstream release was absorbed and what our own crates did.

The format loosely follows [Keep a Changelog](https://keepachangelog.com/).

## [Unreleased]

### Added
- **Panel subtitle**: `Panel::subtitle` / `subtitle_align` (drawn into the bottom
  border, mirroring title alignment). Byte-parity-tested against real rich 15.0.0.
- **Spinner** (`spinner.rs`, port of `rich/spinner.py` + a subset of
  `_spinners.py`): `Spinner::render(time)` picks the animation frame for an
  elapsed time (dots/line/dots2/arrow/simpleDots). Frames verified against real
  rich 15.0.0. (Live-loop animation deferred.)
- **ProgressBar** (`progress_bar.rs`, port of `rich/progress_bar.py` static
  render): determinate bars with half-cell resolution and the default
  `bar.complete`/`bar.finished`/`bar.back` styles. Byte-parity-tested against
  real rich 15.0.0. (Indeterminate "pulse" deferred.)
- **Title alignment** for `Rule` and `Panel` (`HorizontalAlign::{Left,Center,Right}`,
  shared with `Align`): `Rule::align` and `Panel::title_align`. Byte-parity-tested
  against real rich 15.0.0.
- **Columns** (`columns.rs`, port of `rich/columns.py`): packs items into as many
  equal-gap columns as fit the width, filling row by row (ports the column-count
  fitting algorithm). Byte-parity-tested against real rich 15.0.0.
- **Constrain** (`constrain.rs`, port of `rich/constrain.py`): render a child
  within a reduced max width. Byte-parity-tested against real rich 15.0.0.
- **filesize** (`filesize.rs`, port of `rich/filesize.py`): `decimal()` SI byte
  formatting, unit-tested against real rich reference values.
- **Align** (`align.rs`, port of `rich/align.py` horizontal axis): left/center/
  right alignment of a child within the available width. Byte-parity-tested
  against real rich 15.0.0.
- **Tree** (`tree.rs`, port of `rich/tree.py`): renders a hierarchy with the
  `├──`/`└──` guide lines. Byte-parity-tested against real rich 15.0.0.
- **Table** (`table.rs`, port of `rich/table.py` core): columns, rows, box choice
  (default `HEAVY_HEAD`), per-cell padding, bold headers, and multi-line/wrapped
  cells. Byte-parity-tested against real rich 15.0.0 (SQUARE and HEAVY_HEAD).
  Added `Box::get_row`/`RowLevel`, `Segment::simplify`, and the `HEAVY_HEAD` box.
- **Word wrapping** (`wrap.rs`, port of `_wrap.divide_line` + `cells.chop_cells`):
  `Text` now wraps to the available width — breaking on words and folding
  over-long words — so `Panel`/`Padding` reflow long content instead of cropping.
  Byte-parity-tested against real rich 15.0.0.
- **Layout primitives**: width-aware render model (`ConsoleOptions`,
  `Console::render_lines`, `Segment::split_lines`/`adjust_line_length`) plus the
  first composite renderables — `box` (ROUNDED/SQUARE/HEAVY/DOUBLE/MINIMAL/ASCII),
  `Rule`, `Padding`, and `Panel` — all byte-parity-tested against real rich 15.0.0.
- Golden harness extended with a renderables fixture (`renderables.tsv`), captured
  in Python UTF-8 mode for deterministic box glyphs.

### Foundation
- **Workspace scaffold**: `rich` (core, mirroring upstream **15.0.0**),
  `rich-ext` (`0.1.0`), and `rich-cli` (mirroring upstream **1.8.1**).
- **First vertical slice** of the `rich` core, parity-tested against real
  `rich` 15.0.0: `color`, `style`, `cells`, `segment`, `markup`, `text`, `theme`,
  `console`, plus the `protocol` extension points (`Renderable`, `Highlighter`).
- **Internal plugin registry** in `rich-ext` (`ExtensionRegistry`, `ConsoleExt`)
  with an example `NumberHighlighter`, demonstrating the core/ext boundary.
- **`rich-cli` binary**: argument handling, plain-file printing, and a capability
  demo. Rich rendering subcommands are tracked as roadmap issues.
- **Governance**: `AGENTS.md`, `docs/{ARCHITECTURE,PORTING,PLUGINS,DIVERGENCES}.md`,
  the `sync-upstream` and `port-module` skills, `UPSTREAM.toml` pins, and the
  golden parity harness (`scripts/capture_golden.py`).
