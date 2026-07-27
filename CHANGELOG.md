# Changelog

Notable changes to the port. Because the mirror crates track upstream versions,
entries here note which upstream release was absorbed and what our own crates did.

The format loosely follows [Keep a Changelog](https://keepachangelog.com/).

## [Unreleased]

### Added (functional, non-byte-parity)
- **Markdown code blocks** (`markdown.rs`): fenced and indented code blocks now
  render via the `Syntax` renderable (language from the fence info string), so
  they're syntax-highlighted. Not byte-parity (syntect ≠ Pygments — DIVERGENCES
  #9/#18). Markdown now covers everything but links and tables.
- **`LogRender`** (`log_render.rs`, a Rust-native reimagining of `rich/_log_render.py`
  + `rich/logging.py`): formats a log record — optional time, a severity-colored
  `LogLevel`, message, optional path — into a styled line, using the same column
  styles (`log.time`, `logging.level.*`, `log.path`). Takes a `LogLevel` enum +
  strings, so the core stays dependency-light; a `log::Log` handler on top is a
  `rich-ext` follow-up (DIVERGENCES #19).
- **`Traceback`** (`traceback.rs`, a Rust-native reimagining of `rich/traceback.py`):
  `Traceback::new(&error)` walks an error's `Error::source()` chain and renders the
  message + `Caused by:` chain in a red-bordered `HEAVY` panel; `from_message` takes
  a plain string (e.g. a captured panic). No stack frames — Rust errors don't carry
  them (DIVERGENCES #19).
- **`Pretty`** (`pretty.rs`, a Rust-native reimagining of `rich/pretty.py`):
  `Pretty::new(&value)` / `Pretty::compact(&value)` format a value with its
  `Debug` impl (`{:#?}` / `{:?}`) and colorize the result with the built-in
  `ReprHighlighter` (numbers, strings, `None`/`Some`, paths, …). Rust has no
  reflection, so this replaces upstream's Python-object introspection; a few
  Rust spellings (`true`/`false`) are left unstyled (DIVERGENCES #19).
- **Syntax highlighting** (`syntax.rs`, port of `rich/syntax.py`'s renderable via
  the `syntect` crate): `Syntax::new(code, language)` highlights a code block into
  a solid colored panel (fg/bg + bold/italic/underline, each line padded to width
  with the theme background), with a configurable theme. Language is resolved by
  name or file extension; unknown languages render as plain text. **Not**
  byte-parity with upstream — `syntect` uses different grammars/themes than
  Pygments (DIVERGENCES #18); the first renderable that is functionally, not
  byte-, verified. `syntect` uses its pure-Rust `fancy-regex` backend (no C
  `onig`).

### Fixed / verified
- **Wrapping matches upstream for combining sequences** (DIVERGENCES #5 corrected):
  confirmed `cells::chop_cells` is byte-parity with real rich 15.0.0 — current
  rich folds over-long words char-by-char (not by grapheme), and 0-width combining
  marks stay attached to their base char in both. Added unit tests and a golden
  (`wrap_combining`, a decomposed `base + U+0301` fold). The old "we don't do
  grapheme wrapping" note was stale; the only residual difference is an empty
  leading chunk for a char wider than the fold width, which we suppress.

### Added
- **All built-in boxes** (`box.rs`): added the remaining upstream box constants —
  `ASCII2`, `ASCII_DOUBLE_HEAD`, `SQUARE_DOUBLE_HEAD`, `MINIMAL_HEAVY_HEAD`,
  `MINIMAL_DOUBLE_HEAD`, `SIMPLE`, `SIMPLE_HEAD`, `SIMPLE_HEAVY`, `HORIZONTALS`,
  `HEAVY_EDGE`, `DOUBLE_EDGE`, `MARKDOWN` — so `box.py` is complete. Byte-parity-
  tested (new goldens `table_simple`, `table_double_edge`). `SIMPLE`/`MARKDOWN`
  are the boxes Markdown tables will use.
- **Box substitution** (`Box::substitute`, port of `rich/box.py`'s `substitute`):
  on a legacy Windows console the fancy boxes (`ROUNDED`/`HEAVY`/`HEAVY_HEAD`) fall
  back to `SQUARE`, and on a non-UTF-8 terminal any non-ASCII box falls back to
  `ASCII`. Added the `legacy_windows`/`safe_box`/`ascii_only` `Console` flags
  (default off/on/off); `Panel` and `Table` apply the substitution. Byte-parity-
  tested against real rich 15.0.0. Resolves the mechanism half of DIVERGENCES #6
  (runtime auto-detection of legacy terminals is still deferred).
- **Table `no_wrap`** (`table.rs`, toward `rich/table.py`): `Table::column_no_wrap`
  marks a column whose cells crop to a single line with ellipsis instead of
  wrapping, and which doesn't shrink during collapse (only yielding via the
  last-resort even reduce). Byte-parity-tested (golden `table_nowrap`). Narrows
  DIVERGENCES #7 (only per-column ratio/min/max and the width-0 padding edge
  remain).
- **`Live`** (`live.rs`, port of `rich/live.py`'s manual-refresh core): drives a
  `LiveRender` over a generic `Write` sink — `start` hides the cursor and draws,
  `update`/`refresh` reposition and redraw in place, `stop` commits the final
  frame and shows the cursor. The emitted byte stream is byte-parity with real
  rich 15.0.0 (`auto_refresh=False`, `transient=False`). The background
  auto-refresh thread + transient/alt-screen modes are deferred (DIVERGENCES #17).
- **`LiveRender`** (`live_render.rs`, port of `rich/live_render.py`): wraps a
  renderable, records its rendered shape, and emits the `position_cursor` /
  `restore_cursor` control-code sequences (`\r` + erase-line + cursor-up×N) that
  redraw in place — the byte-parity-testable core of a `Live` display. Byte-parity
  vs real rich 15.0.0. (The full `Live` refresh loop + vertical-overflow cropping
  are deferred — issue #6.)
- **`Progress`** (`progress.rs`, port of `rich/progress.py`'s default display):
  `Progress::add_task(description, total, completed)` renders a static grid of
  tasks — each a description, a bar that flexes to fill the width, and a magenta
  percentage — reproducing the default `TextColumn`/`BarColumn`/`TaskProgressColumn`
  layout. Byte-parity-tested against real rich 15.0.0 (golden `progress_three`).
  Custom columns, the time/rate columns, and the `Live` refresh loop are deferred
  (DIVERGENCES #16 / issue #6).
- **`Status`** (`status.rs`, port of `rich/status.py`) + **styled spinner frames**:
  `Status::new("…")` renders a spinner (default `dots`, green) followed by a
  message; `Spinner::style` styles the frame while the trailing text stays plain.
  Byte-parity-tested against real rich 15.0.0 (static `t = 0` frame). Live-loop
  animation is still deferred (DIVERGENCES / issue #6).
- **HTML export** (`export.rs` + `terminal_theme.rs`, port of
  `Console.export_html` + `_export_format`): `Console::export_html(|c| …)`
  captures printed output and returns a self-contained HTML document with inline
  styles, resolving colors through a `TerminalTheme` (`DEFAULT_TERMINAL_THEME`).
  Added `Style::get_html_style` and `blend_rgb`. Byte-parity-tested against real
  rich 15.0.0 (full document + `get_html_style`). Also `export_html_classes`
  (the default `.r1 {…}` CSS-class stylesheet form, byte-parity). The `rich-cli`
  `--export-html` flag emits the class form (rich-cli's default). (SVG export
  deferred — DIVERGENCES #15.)
- **Generic `RegexHighlighter` base + `ISO8601Highlighter`** (`highlighter.rs`,
  toward `rich/highlighter.py`): extracted the shared regex-highlight loop into a
  public `RegexHighlighter` (base-style prefix + named-group patterns), refactored
  `ReprHighlighter` onto it, and added `ISO8601Highlighter` for standard extended
  date/time strings (`iso8601.date`/`time`/`timezone`). Highlighters now stylize
  **every** matched named group (unknown names → null style), reproducing
  upstream's per-field segment boundaries. Byte-parity-tested vs real rich 15.0.0
  (repr unchanged; ISO8601 new). See DIVERGENCES #13/#14 for the remaining gaps.
- **Table per-column width, style + ellipsis overflow** (`table.rs`, toward
  `rich/table.py`): `Table::column_width` pins a column's content width (content
  wraps to it instead of the column shrinking), `Table::column_style` styles a
  column's body cells, and table cells now wrap with **ellipsis overflow** (a
  word wider than the column is cropped with `…`), matching upstream's default.
  Added the last-resort even `ratio_reduce` when fixed columns overflow.
  Byte-parity-tested against real rich 15.0.0. (Per-column `ratio`/min/max and
  `no_wrap` still deferred — DIVERGENCES #7.)
- **Markup fidelity** (`markup.rs`, toward `rich/markup.py`): a `[…]` is now only
  a tag when it starts with `[a-z#/@]` (so `[Hello]`/`[42]` are literal text),
  unmatched closing tags return `RichError::Markup`, `@`-tags apply no visible
  style, and a public `markup::escape` implements upstream's backslash-doubling
  escape. Byte-parity-tested (literal brackets, meta tags) + negative tests for
  the error cases. Narrows DIVERGENCES #2.
- **`Styled`** (`styled.rs`, port of `rich/styled.py`) and **`Screen`**
  (`screen.rs`, port of `rich/screen.py`): `Styled` lays a `Style` under an
  entire renderable (each segment's own style still wins); `Screen` fills the
  console region (width × height) with a child, cropping/padding to an exact
  rectangle with an optional background style. Added `Segment::apply_style` and
  `Segment::set_shape` (now shared with `Layout`). `Styled` is byte-parity-tested
  against real rich 15.0.0; `Screen` has a known trailing-newline edge
  (DIVERGENCES #12).
- **`rich-cli` render modes** (`crates/rich-cli`): the binary now implements a
  slice of upstream rich-cli's flags — `-p/--print` (console markup),
  `-m/--markdown`, `-j/--json`, and `--rule` — plus `-w/--width`,
  `--left/--center/--right` justification, stdin (`-`), and file-extension
  auto-detection (`.md`/`.json`). Covered by CLI integration tests. (Syntax, CSV,
  panel, ipynb, URL fetch, and HTML export remain roadmap items.)
- **Height-aware `Panel`** (+ `Console::render_lines` height handling): a `Panel`
  now consumes `options.height`, expanding its content to fill the imposed height
  (`child_height = height − 2 − padding`) so a `Panel` used as a `Layout` leaf
  fills its region instead of being blank-padded. `render_lines` crops/pads any
  renderable to an explicit height. Byte-parity-tested against real rich 15.0.0
  (Panel-in-layout goldens). Resolves the height half of DIVERGENCES #11.
- **Layout** (`layout.rs` + `ratio.rs`, port of `rich/layout.py` +
  `_ratio.ratio_resolve`): split a region into ratioed/sized rows (`split_row`)
  and columns (`split_column`), recursively; each leaf is rendered to an exact
  `(width, height)` block (`Segment.set_shape`) and tiled. Added console
  `height` (builder + detection) and `ConsoleOptions::update_dimensions`.
  Byte-parity-tested against real rich 15.0.0 (column/row/nested, incl. `size`).
  (Empty-leaf placeholder renders blank; height-aware container leaves like
  `Panel` don't yet expand to fill — DIVERGENCES #11.)
- **Console capture + export** (`Console::capture` / `export_text`, port of
  `Console.capture()` + `export_text(styles=False)`): run a closure with output
  recorded to an internal segment buffer instead of stdout, returning either the
  rendered ANSI (`capture`) or plain style-stripped text (`export_text`). Captures
  nest. Byte-parity-tested against real rich 15.0.0. (HTML/SVG export deferred.)
- **ANSI decoder** (`ansi.rs`, port of `rich/ansi.py`): `AnsiDecoder` tokenizes a
  terminal string and turns SGR sequences back into styled `Text` — attributes,
  16/256/truecolor foreground + background, lenient parsing, and cross-line style
  persistence. Byte-parity-tested against real rich 15.0.0 (re-render round-trip).
  Added `Color::from_ansi`/`from_rgb` and `Style::from_color`. (OSC hyperlinks are
  skipped, pending `Style` link support — DIVERGENCES #10.)
- **Control codes** (`control.rs`, port of `rich/control.py` + `ControlType`):
  the `Control` renderable and typed `ControlType` sequences — screen clear,
  cursor home/move/move_to/move_to_column, show/hide cursor, alt-screen toggle,
  bell, erase-in-line — plus `Console::control`/`clear`/`show_cursor`/`bell`.
  Control segments are written only to a real terminal (matching upstream's
  `_render_buffer`). Byte-parity-tested against real rich 15.0.0.
- **Markdown** (`markdown.rs`, port of `rich/markdown.py` core): renders
  paragraphs, ATX headings (h1–h6, centered h1), **bullet + ordered lists**,
  **block quotes**, **thematic breaks (hr)**, and inline strong/emphasis/code via
  `pulldown-cmark`, as justified full-width blocks separated by blank lines.
  Byte-parity-tested against real rich 15.0.0. (Code blocks, links, and tables
  deferred — DIVERGENCES #9.)
- **Full spinner table**: all 73 built-in spinners are vendored
  (`spinner_data.rs`) from `_spinners.py`, replacing the curated subset.
- **ReprHighlighter** (`highlighter.rs`, port of `rich/highlighter.py`): the
  built-in highlighter that auto-colors numbers, bools, `None`, strings, paths,
  URLs, braces, calls, IPs, UUIDs, and tags. Patterns vendored verbatim
  (`repr_patterns.rs`) and compiled with `fancy-regex`; enabled via
  `ConsoleBuilder::highlight(true)`. Byte-parity-tested against real rich 15.0.0
  across many pattern types.
- **Full emoji table**: the complete ~3600-entry `_emoji_codes` table is vendored
  (`emoji_codes.rs`, binary-searched), so every `:shortcode:` resolves. Resolves
  the former curated-subset divergence.
- **Full 256-color names**: the complete `ANSI_COLOR_NAMES` table is vendored
  (`color_names.rs`), so markup/style names like `[orange1]`, `[grey37]`,
  `[deep_sky_blue1]` resolve to the correct 8-bit colors. Byte-parity-tested;
  resolves the former partial-names divergence.
- **Table title / caption / show_lines**: `Table::title` and `caption` render
  centered above/below the table (italic / dim-italic), and `show_lines` draws a
  separator between body rows. Byte-parity-tested against real rich 15.0.0.
- **Table `expand` + per-column justify**: `Table::expand` fills the available
  width (distributing leftover space via `ratio_distribute`), and
  `Table::add_column_justify` justifies a column's cells left/center/right.
  Byte-parity-tested against real rich 15.0.0.
- **Table flexible widths**: when a table's natural width exceeds the console
  width, the widest columns now shrink and their cells wrap to fit (port of
  `Table._calculate_column_widths` + `_collapse_widths` + `_ratio.ratio_reduce`,
  with banker's rounding). Byte-parity-tested against real rich 15.0.0.
- **Measurement** (`Renderable::measure` + top-level measurement-fit): the print
  path now shrinks the width to a renderable's measured content width when no
  explicit justify is set. This makes a bare `Text::justify(...)` shrink to its
  content (matching upstream, resolving the former DIVERGENCES #9), while
  `print(justify=…)` and container-embedded justify still pad to full width.
  Byte-parity-tested against real rich 15.0.0.
- **Bar** (`bar.rs`, port of `rich/bar.py`): a horizontal bar spanning
  `[begin, end]` within `[0, size]`, with eighth-block sub-cell edges. Byte-parity-
  tested against real rich 15.0.0.
- **Emoji** (`emoji.rs`, port of `_emoji_replace` + a curated `_emoji_codes`
  subset): `:name:` shortcodes (with `-emoji`/`-text` variants) expand in the
  `Console` print path (default on, `ConsoleBuilder::emoji`). Byte-parity-tested
  against real rich 15.0.0; replacement logic is complete, code table curated.
- **Text justify** (`Text::justify` + `Console::print_justified`): left/center/
  right justification, padded to the render width. Byte-parity-tested against
  real rich 15.0.0 inside a `Panel` and via `print(justify=…)`. (Bare top-level
  measurement-fit is deferred — see DIVERGENCES #9.)
- **JSON** (`json.rs`, port of `rich/json.py`): parses a JSON string (via
  `serde_json` with `preserve_order`) and pretty-prints it with 2-space indent
  and the default highlight colors (bold braces, bold-blue keys, green strings,
  bold-cyan numbers, italic bools/null). Byte-parity-tested against real rich
  15.0.0 for ASCII documents. First core third-party content dependency.
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

### Fixed
- **`Color::parse("color(N)")` for N < 16**: now returns a `Standard` color (SGR
  30–37/90–97) instead of an 8-bit palette color, matching `Color.parse`. This
  makes ANSI round-trips of the standard colors byte-identical to upstream.

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
