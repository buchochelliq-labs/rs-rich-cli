# Changelog

Notable changes to the port. Because the mirror crates track upstream versions,
entries here note which upstream release was absorbed and what our own crates did.

The format loosely follows [Keep a Changelog](https://keepachangelog.com/).

## [Unreleased]

### Fixed
- **`Color::downgrade` was wrong for 8-bit and standard targets** (`color.rs`) —
  found by the new colour-system goldens, which is exactly the coverage gap that
  let it survive:
  - **8-bit:** we searched the 256 palette for the nearest colour. Upstream
    instead maps into the 6×6×6 cube *by formula* (with a greyscale-ramp branch
    under 15% saturation), which picks different entries: `#00ff00` is cube index
    46, not the exactly-matching system green 10. The axis is also non-linear
    (first step 0–95, then 40 per step), and Python's banker's rounding is
    reproduced with `round_ties_even`.
  - **standard:** we matched against the 128-based ANSI table. Upstream matches
    against a *separate* 170/85-based `STANDARD_PALETTE`, so `#ff0000` gives 1
    (maroon), not 9. The two tables are now distinct: `ANSI_BASE_PALETTE` (the
    first 16 of the 8-bit palette, and the default terminal theme's ANSI set)
    versus `STANDARD_PALETTE` (matching only).

  A unit test had been asserting the buggy values; it now asserts upstream's,
  captured from real rich 15.0.0.

### Fixed
- **Four more defects**, from a second adversarial review (of the change above):
  - **Tabs were never expanded on the render path.** `Text::expand_tabs` existed
    but nothing called it, so a tab occupied one cell in every layout calculation
    and eight on the terminal. `Text::measurement` now measures the *expanded*
    text too — upstream gets away with measuring raw (a tab counts as zero cells
    there) only because its `print` never narrows to the measurement.
  - **A bare `link` was accepted** and emitted an empty OSC 8 hyperlink; upstream
    raises `"URL expected after 'link'"`. This one was mine, from the `link`
    keyword added in the previous commit.
  - **Colour names kept their original case**, so `on #FF0000` did not normalize
    to `on #ff0000` and a mixed-case open/close tag pair raised.
  - **`Text::new` did not strip control codes** (BEL, backspace, vertical tab,
    form feed, carriage return), which upstream removes on construction.
- **Six markup and `Style::parse` defects**, all found by an adversarial review of
  the named-span change and each verified against real rich 15.0.0:
  - **The highlighter beat markup.** `[green]123[/]` rendered `repr.number` cyan
    instead of green, because markup spans were pushed first and the highlighter
    decorated that `Text` in place. Upstream highlights the plain string and
    appends the markup spans last, so an explicit tag wins.
  - **Two tags over the exact same range combined backwards**, so `[red][blue]x[/][/]`
    came out red. Upstream's `sorted(spans[::-1], key=start)` needs both the
    reversal *and* a start-only key; sorting by `(start, end desc)` makes an
    identical range compare equal and hands the win to the outer tag.
  - **`expand_tabs` kept one span** where upstream splits per tab-part, so a span
    crossing several tabs rendered as one segment instead of several — same
    colours, different bytes.
  - **Tag names weren't normalized.** `Style::normalize` is now ported (with
    `Style::definition`, the port of `Style.__str__`), so `[b]` opens what
    `[/bold]` closes and a mixed-case name reaches the theme lowercased.
  - **`[link=url]` was unusable**: parameters weren't split from the tag name, so
    `[/link]` raised a markup error. `Style::parse` also had no `link` keyword,
    which meant such a style resolved to null and dropped the hyperlink.
  - **`not BOLD` was accepted** and silently cancelled an enclosing bold;
    upstream's `not` operand is case-sensitive and raises.

### Changed
- **Style names on spans now resolve at render time, against the rendering
  console's theme** (`StyleType` in `style.rs`, `Theme::get_style`,
  `Console::get_style`; closes the main half of #14). Previously a span held a
  concrete `Style`, resolved eagerly — highlighters looked names up in a
  *process-global* default theme, so a console's own theme could not restyle
  their output at all, and an unknown markup tag was an error rather than the
  no-op upstream produces.
  - `Span.style` and `Text`'s base style are now `StyleType { Name | Style }`,
    the port of upstream's `Union[str, "Style"]`. The render path resolves them
    once per render into a vector parallel to the spans (upstream's `style_map`).
  - Two user-visible behaviour changes, both verified against real rich 15.0.0
    before shipping: highlight colours follow the console's theme, and an unknown
    markup **tag name** renders as a no-op (a genuine *syntax* error still fails).
  - `Text::highlight_regex` gains its named-group half — each group is styled
    with `{style_prefix}{name}` as a *name*, retiring the deferral noted when it
    was first ported. This is also how `RegexHighlighter` now works, so a custom
    highlighter with novel group names finally gets colours rather than just span
    boundaries.
  - `Text::stylize` takes its arguments in upstream's order
    (`stylize(style, start, end)`), and the render family takes a `&Theme`.
    `Text::render_wrapped` and `render_joined_justified` are gone (no callers,
    not upstream). `Text::render` drops its unused `ColorSystem` argument.
  - Covered by `golden/highlight.tsv` — the **only** fixture set rendered with
    `highlight=true`, which is why this path had no byte-parity net before. All
    12 pre-existing fixture sets are byte-identical.
  - Still deferred: upstream's theme *stack* (`push_theme`/`pop_theme`). See
    `docs/DIVERGENCES.md` §14 for why it needs its own design pass.

### Added
- **Prompts** (`prompt.rs`, closes the `prompt.py` half of #11): `Prompt`,
  `Confirm`, `IntPrompt`, `FloatPrompt` — a styled question, a read, and a
  re-ask until the answer validates.
  - Reading sits behind an `InputSource` trait (upstream's `stream=` argument),
    so the whole loop — including the re-ask path and the empty-answer-takes-the-
    default rule — is testable without a terminal. `ScriptedInput` is provided
    for that; `StdinInput` is the default.
  - The question line is byte-parity tested (13 fixtures in `golden/prompts.tsv`)
    across choices, defaults, `show_choices`/`show_default`, markup prompts and
    `Confirm`'s `(y)`/`(n)` rendering.
  - Case-insensitive choices return the choice as spelled in the list, not as
    typed, which upstream is explicit about.
  - Diverges in one place: exhausted input returns the default instead of
    looping forever (Python raises `EOFError`).
- **The rest of the `Text` manipulation API** (`text.rs`): `divide`, `split`,
  `pad`/`pad_left`/`pad_right`, `right_crop`, `rstrip`, `rstrip_end`,
  `expand_tabs`, `join`, `blank_copy`, `highlight_words` and `highlight_regex`.
  All carry spans correctly through the transformation — re-based and clipped by
  `divide`, shifted by `pad_left`, extended over the spaces a tab expands into.
  - `highlight_regex` is ported in its **explicit-style** form only. Upstream also
    styles each named group with the group's *name* as a style name, which needs
    a span that can hold an unresolved name; that waits on the named-span
    refactor (#14). `RegexHighlighter` already covers the named-group case by
    resolving against a theme as it goes.
  - Covered by 24 new byte-parity fixtures (`golden/text_ops.tsv`). Each case
    renders its result, so one comparison pins the plain text, the span
    boundaries and the styles together.
- **Text overflow** (`Overflow` in `console.rs`, `text.rs`, `segment.rs`). `Text`
  now carries `overflow` and `no_wrap`, as do `ConsoleOptions`, resolved with
  upstream's `self.x or options.x or DEFAULT` precedence. The wrap pipeline is
  now a full port of `Text.wrap`: split on newlines, wrap (folding an unbreakable
  word **only** under `fold`), justify, then truncate each line.
  - `Overflow::Ignore` neither wraps nor truncates, which upstream keeps
    survivable via `Console.print(crop=True)` — ported as `Segment::crop_lines`
    and applied on the print path. Without it, `ignore` would run off the side of
    the terminal rather than being cut at the edge.
  - `Text::truncate(max_width, overflow, pad)` is exposed directly.
  - Covered by 22 new byte-parity fixtures (`golden/overflow.tsv`) spanning every
    method, `no_wrap`, double-width characters straddling the cut, and the three
    positions an ellipsis can land relative to a span — the last of these caught
    a real bug, since the marker inherits the style of the run the cut *entered*,
    not the one it left.
- **Paging** (`pager.rs` + `Console::page`, `rich-cli --pager`). A `Pager` trait
  with a `SystemPager` that
  ports `pydoc.get_pager`'s selection order (`MANPAGER`, then `PAGER` — split
  into program + args — then `less -R`/`more`), including its guards: no tty on
  stdin/stdout or `TERM=dumb`/`emacs` falls back to writing straight to stdout,
  as does a pager that can't be spawned. `Console::page(styles, f)` buffers a
  closure's output and shows it, stripping styles unless `styles` is set
  (matching `Console.pager(styles=False)`); `page_with` takes a custom `Pager`,
  upstream's `Console.pager(pager=…)` seam. `rich --pager` pages *styled*
  output.
- **`rich-cli` URL fetch** (`rich-cli/main.rs`, completes the common CLI surface):
  `rich <url>` fetches an `http(s)` resource and renders it — the render mode
  comes from a flag, else the URL extension, else the response `Content-Type`
  (markdown/json/csv), else syntax-highlighting (with a lexer guessed from the
  Content-Type; upstream's behavior). Fetching uses `ureq` (rustls +
  bundled webpki roots — self-contained TLS), bounded by a 30 s timeout and a
  16 MiB **decoded**-body limit, behind a **default `fetch` feature**
  (`--no-default-features` gives a lean, network-free build). Like
  `requests.get`, a non-2xx status still renders the response body. The size
  limit is applied to the post-decompression stream (ureq's own `limit()` sits
  *under* the gzip decoder and would only bound compressed bytes, leaving a
  decompression-bomb hole); oversized and non-UTF-8 bodies get their own clear
  errors. The only deliberate divergences from upstream are the timeout and size
  bound, which upstream lacks.
- **`Live` auto-refresh thread** (`live.rs`, advances DIVERGENCES #17):
  `Live::spawn(renderable, console, writer, refresh_per_second)` returns an
  `AutoLive` handle whose background thread redraws on an interval (and on each
  `update`) — a port of upstream's `refresh_per_second`. The thread constructs and
  owns the `Live` internally (only `Send` inputs cross the boundary), so
  `Console` is now `Send` (highlighter boxes are `dyn Highlighter + Send`; the
  rich-ext registry factory matches). Deterministically tested to emit the same
  byte stream through the thread; `Drop` finalizes if `stop()` is skipped.
- **`rich-cli` `.ipynb` (Jupyter notebook) rendering** (`rich-cli/main.rs`, port
  of rich-cli's `render_ipynb`): `--ipynb` (and `.ipynb` auto-detect) renders a
  notebook — markdown cells as `Markdown`, code cells as an `In [n]:` label + a
  dim `Panel` of `Syntax`, and cell outputs (stream / error traceback /
  execute_result `text/plain`) decoded via `AnsiDecoder`, blank-line separated.
  Adds a `serde_json` dependency for notebook parsing. (Rich outputs — images,
  HTML — are deferred; text is handled.)
- **`rich-cli` panel `--title` / `--caption` / `--style` + `none` box**
  (`rich-cli/main.rs`, `box.rs`): completes the `--panel` decorator to full
  rich-cli fidelity — the panel gets a title (top border), caption (bottom
  border), and border style (e.g. `--style "bold red"`); `--panel none` uses the
  new blank `box.NONE`. Composes the already-byte-parity `Panel` builders.
- **`rich-cli` `--panel` / `--padding` decorators** (`rich-cli/main.rs`): wrap any
  render mode's output in a `Panel` (`--panel ascii|ascii2|square|rounded|heavy|
  double`) and/or `Padding` (`--padding` with 1, 2, or 4 comma-separated ints,
  unpacked like upstream's `Padding.unpack`), composing the byte-parity Panel/
  Padding renderables. Port of rich-cli's decorator flow (padding inside, panel
  outside). `Console::build_text` is now public so the `--print` markup can be
  wrapped. (`none` box + `--title`/`--caption`/`--style` are follow-ups.)
- **`rich-cli` `--export-svg`** (`rich-cli/main.rs`): exposes the new SVG export
  through the CLI (any render mode → a self-contained SVG), alongside the existing
  `--export-html`. The two are mutually exclusive; the SVG title is the resource's
  basename (else `rich`) and it uses a fixed `unique_id` (the default can't be
  byte-parity — DIVERGENCES #15). `emit` refactored to an `Export` enum.
- **`Console::export_svg`** (`svg.rs`, resolves DIVERGENCES #15): exports recorded
  output as a self-contained SVG image of a terminal window (Fira Code font-face,
  window chrome + traffic-light circles, per-line clip-paths, a generated CSS
  class table, and the styled text matrix), using `SVG_EXPORT_THEME`.
  **Byte-parity** with real rich 15.0.0 (golden `tests/golden/svg_export.svg`).
  Built on the groundwork primitives `SVG_EXPORT_THEME`, `blend_rgb`, and
  `Style::get_svg_style`, plus new `Style::attr` / `Color::is_default` accessors.
  Takes an **explicit `unique_id`** (upstream's default hashes Python `repr()`
  output, not reproducible in Rust — the auto-default is the only residual, #15).
- **`rich-cli` CSV/TSV rendering** (`rich-cli/main.rs`, advances the CLI port):
  `--csv` (and `.csv`/`.tsv` auto-detection) renders a delimited file as a table,
  a port of rich-cli's `render_csv` — blue-bordered `HEAVY_HEAD` table with the
  first row as the header and any all-numeric column right-justified + bold-green
  (body & header). Includes an RFC-4180-ish parser (double-quoted fields with
  `""` escaping, embedded delimiters/newlines, `\r\n`, leading-BOM stripping) and
  the numeric-column heuristic. **Byte-parity** with the Table real rich-cli
  builds (unit-tested). Deferred: `csv.Sniffer` dialect/has-header heuristics and
  title/caption. Demo shows a CSV table.
- **`Table` `border_style` + per-column header cell fill** (`table.rs`): a
  `border_style` builder (tints the box edges/dividers, composed over the table
  style) and `column_header_fill` (a per-column header *cell* style — content +
  padding — combined over the table `header_style`, distinct from the existing
  content-only span). Together they reproduce **rich-cli's `render_csv` styling**
  byte-parity (HEAVY_HEAD, blue border, numeric columns right-justified + bold
  green), verified by the new golden `table_csv_style`. Groundwork for the CLI's
  CSV rendering.
- **`Progress` `Download` column + `filesize::pick_unit_and_suffix`** (`progress.rs`,
  `filesize.rs`, advances DIVERGENCES #16): the `DownloadColumn` renders
  `completed`/`total` in a shared SI byte unit (`0.5/1.0 kB`), byte-parity with
  rich 15.0.0. Added `pick_unit_and_suffix` (the unit/suffix picker upstream's
  download/transfer columns share). This completes the *deterministic* Progress
  columns; only the wall-clock columns (spinner/speed/time) + the `Live` loop
  remain.
- **`Progress` custom columns** (`progress.rs`, advances DIVERGENCES #16):
  generalized from a hard-coded 3-column layout to a configurable
  `ProgressColumn` list (`Progress::columns`). Deterministic columns ported
  byte-parity — description, static text, bar, percentage, and **M-of-N**
  (`{completed}/{total}`, `progress.download` green). The inline grid matches
  upstream's `Table.grid(padding=(0,1))`. Non-deterministic columns (spinner,
  speed, time) and the download byte-unit formatting stay deferred (need the
  `Live` loop / filesize units). Default layout unchanged (byte-parity).
- **`Table` per-column `ratio` / `min_width` / `max_width`** (`table.rs`, the
  substantive half of DIVERGENCES #7): `column_ratio`, `column_min_width`, and
  `column_max_width` builders. `min_width`/`max_width` clamp the measured content
  width (over-wide cells wrap); `ratio` columns share the free width in proportion
  when the table is `expand`ed, via a faithful port of upstream's flexible/fixed
  split (`ratio_distribute` now takes per-slot minimums). Byte-parity goldens
  `table_ratio`, `table_min_width`, `table_max_width`. Only the rare width-0
  padding edge remains open on #7.
- **`ISO8601Highlighter` covers the full upstream pattern set** (`highlighter.rs`,
  resolves DIVERGENCES #13): added compact/basic calendar dates (`20230615`),
  ordinal dates, week dates, basic times, standalone timezones, and space-separated
  date-times. Upstream's one PCRE-conditional pattern is rewritten as two
  non-conditional alternatives (all-hyphen/colon + all-basic) that match the same
  strings. Byte-parity with rich 15.0.0 (unit-tested); the extended forms are
  unchanged.
- **`AnsiDecoder` decodes OSC 8 hyperlinks** (`ansi.rs`, resolves DIVERGENCES
  #10): `\x1b]8;<params>;<url>\x1b\` sequences now attach the URL to the running
  `Style` (via the new `Style::update_link`), with the empty closing sequence
  clearing it and `id=`/other params ignored — matching upstream. Re-rendering is
  byte-identical to rich except the random `id=` field we omit for determinism
  (same deviation as #20). Round-trip unit tests added.

### Fixed / clarified
- **`chop_cells` over-long-word fold is byte-parity** (`cells.rs`, resolves
  DIVERGENCES #5): dropped the `!line.is_empty()` guard so folding a character
  wider than the fold width (e.g. a 2-cell CJK char to width 1) emits upstream's
  empty leading chunk (`["", "宽", …]`), and `_wrap.divide_line` therefore yields
  the same break positions (and the same empty line) as rich. Normal text is
  unaffected (`cw <= width` never triggers the empty push). Unit-tested against
  real rich 15.0.0. Only a rare multi-codepoint-grapheme residual remains (needs a
  grapheme table).
- **Markdown trailing thematic-break blank line** (`markdown.rs`): a document that
  *ends* with a `---` rule now emits the extra trailing blank line upstream does
  (upstream's hr element yields a trailing break, observable only when the rule is
  the last block — a mid-document rule merges with the normal block separator).
  Byte-parity golden `markdown_hr_end` + unit test; the mid-document case
  (`markdown_quote_hr`) is unchanged. Closes the last Markdown-block divergence.
- **`Json` non-ASCII is byte-parity** (`json.rs`): confirmed and locked in with a
  golden (`json_unicode`) + unit test. `rich.json.JSON` defaults to
  `ensure_ascii=False`, so our UTF-8 output already matched upstream (accented
  characters/symbols render literally, not `\uXXXX`), and `serde_json`'s
  `preserve_order` keeps input key order. Corrected the stale DIVERGENCES #8,
  which wrongly claimed an escaping mismatch; the only remaining JSON caveat is
  exotic **number formatting** (exponent notation like `1e+20`/`1e-07`, and
  integers beyond i64/u64) differing from CPython's `repr`.

### Added
- **Markdown tables** (`markdown.rs`, port of upstream's `TableElement`): GFM
  tables now parse (`pulldown-cmark` `ENABLE_TABLES`) and render via [`Table`]
  built exactly as upstream — `box=SIMPLE`, `pad_edge=false`,
  `collapse_padding=true`, `markdown.table.border` (cyan) table style, and
  `markdown.table.header` (`not bold cyan`) header-content styling — with
  per-column justify read from the alignment row. **Byte-parity** with real rich
  15.0.0 (new golden `markdown_table`). Inline styling *within* a table cell is a
  documented follow-up. Enabled by a new Table capability: a per-column
  **header-content style span** (`Table::column_header_style`) that styles the
  visible header characters while the header padding keeps `header_style`.
- **Table-level `style`** (`table.rs`, port of `Table(style=…)`): a default style
  for the whole table, composed as the base of the border style
  (`border_style = style + border_style`) so it tints the box glyphs and dividers
  while cell content keeps its own styles — matching upstream exactly. Byte-parity
  golden `table_style`. With `pad_edge`/`show_edge`/`collapse_padding`/per-column
  justify already in place, the only remaining Markdown-tables step is wiring the
  `TableElement` (#9).
- **Table `collapse_padding`** (`table.rs`, port of `_get_padding_width`):
  `Table::collapse_padding(true)` merges adjacent cell padding — an interior
  column's left pad is reduced by the previous column's right pad
  (`max(0, pad_left - pad_right)`), so a default-padded grid collapses to a
  single space between columns. Byte-parity-tested (new golden `table_collapse`).
  The last per-cell Table feature Markdown tables need before wiring the element.
- **Table `pad_edge` + `show_edge`** (`table.rs`, port of upstream's flags):
  `Table::pad_edge(false)` drops the first column's left pad and the last
  column's right pad; `Table::show_edge(false)` removes the outer box edges
  (top/bottom borders + left/right glyphs), leaving only the internal dividers +
  content. `Box::get_top/get_row/get_bottom` gained an `edge` parameter.
  Byte-parity-tested (new goldens `table_pad_edge`, `table_no_edge`). These are
  two of the features Markdown tables need (`SIMPLE` box + `pad_edge=False`).
- **OSC 8 hyperlinks** (`Style::with_link`): a `Style` can now carry a link URL,
  rendered as an OSC 8 hyperlink around its styled text. **Markdown links**
  (`[text](url)`) render as the `markdown.link_url` style (underline blue) + the
  hyperlink. Byte-identical to real rich 15.0.0 **except** upstream's random `id=`
  field, which we omit for determinism (DIVERGENCES #20).

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
