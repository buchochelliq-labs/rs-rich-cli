# Divergences from upstream

Every intentional deviation of `crates/rich` from Python `rich` is recorded here,
with a justification. The default build of `crates/rich` must otherwise behave
exactly like upstream. Anything that is purely *our own* feature belongs in
`rich-ext` and is **not** a divergence — it doesn't go here.

Format: what differs · why · how to remove it (if temporary).

## Current divergences

### 1. Cell-width table delegated to `unicode-width`
- **Differs:** upstream ships a generated width table (`_cell_widths.py`); we call
  the `unicode-width` crate.
- **Why:** both implement the same Unicode East Asian Width rules; reusing the
  crate avoids vendoring a large generated table.
- **Remove:** only if a concrete codepoint mismatch is found — then vendor the
  upstream table into `cells.rs`. Tracked with the `cells` porting issue.

### 2. Markup: error is swallowed at the print boundary; partial backslash rules
- **Differs:** `markup::render` now matches upstream — a `[…]` is only a tag when
  it starts with `[a-z#/@]`, an unmatched closing tag returns `RichError::Markup`,
  and unclosed *opening* tags auto-close (as upstream does). Two smaller gaps
  remain: (a) the `Console` print path swallows a markup error and falls back to
  printing the raw text, rather than propagating it; and (b) the parser handles
  the common `\[` escape but not the full backslash-run doubling semantics of
  `_parse` (the public `markup::escape` *does* implement the full rule).
- **Why:** swallowing at the boundary keeps the demo/CLI robust on arbitrary
  input; the exotic backslash-run cases are rare.
- **Remove:** add a strict print variant that surfaces `MarkupError`, and port
  the backslash-run branch of `_parse`, under the markup/text issue (#2).

### 3. Byte offsets in `Text` spans
- **Differs:** upstream `Text` uses code-point offsets for spans; our `Text` uses
  byte offsets internally.
- **Why:** simpler and faster in Rust; observable behavior is identical for the
  ported operations (append/stylize/render), and ASCII-only callers such as the
  example highlighter are unaffected.
- **Remove:** not planned unless a public API needs code-point indexing; if so,
  expose a char-index helper without changing the internal representation.

### 5. Over-long-word fold suppresses one empty chunk vs upstream
- **Differs:** `Text` word-wrapping (`_wrap.divide_line`) and the over-long-word
  fold (`cells.chop_cells`) are ported and are **byte-parity** with upstream —
  current rich's `chop_cells` is itself char-based (not grapheme-based), and
  0-width combining marks stay attached to their base char in both (verified with
  a decomposed `base + U+0301` fold). The only residual difference: when a *single
  character is wider than the entire fold width* (e.g. folding a 2-cell CJK char
  to width 1), upstream emits an empty leading chunk (`["", "宽", …]`) and we
  suppress it (`["宽", …]`).
- **Why:** the empty leading chunk is an upstream quirk that only appears when
  folding below one character's width — a case that doesn't occur in normal
  layout; suppressing it is cleaner and never observable in realistic output.
- **Remove:** drop the `!line.is_empty()` guard in `chop_cells` to match
  upstream's empty chunk exactly, if strict parity on that edge is ever needed.

### 6. Box substitution is opt-in (no legacy-terminal auto-detection)
- **Differs:** `Box.substitute` **is** ported — `Box::substitute` maps the fancy
  boxes (`ROUNDED`/`HEAVY`/`HEAVY_HEAD`) to `SQUARE` when `legacy_windows` is set,
  and any non-ASCII box to `ASCII` when `ascii_only` is set; `Panel`/`Table`
  apply it. The `legacy_windows`/`safe_box`/`ascii_only` console flags exist. What
  differs: those flags default **off** and are not auto-detected from the runtime
  terminal (upstream auto-detects legacy Windows / a non-UTF-8 encoding), so the
  default build always emits the requested glyphs.
- **Why:** keeps default output deterministic (and golden fixtures, captured in
  UTF-8 non-legacy mode, unaffected); runtime terminal detection is platform code.
- **Remove:** auto-detect `legacy_windows` (WINDOWS && no VT support) and
  `ascii_only` (non-UTF-8 encoding) at `Console` build time, under the
  Windows-console issue (#12).

### 7. `Table` — a few advanced options remain
- **Differs:** sizing (content, shrink-to-fit, `expand`), per-column justify,
  explicit per-column **width**, per-column **style**, **`no_wrap`** (crop to one
  line with ellipsis), **ellipsis overflow** (the table default), title, caption,
  and `show_lines` are all ported. Not yet ported: explicit per-column
  `ratio`/`min_width`/`max_width`. Also, a *wrapping* column squeezed to width 0
  by a greedy `no_wrap` neighbor still renders its cell padding (upstream drops
  it) — a rare over-constrained case.
- **Why:** the remaining options are less common and build on the same width
  machinery; the width-0 padding edge only appears when a table is narrower than
  its no_wrap content plus one other column.
- **Remove:** add the per-column ratio/min/max fields, and drop padding on
  zero-width columns, with the Table issue (#5).

### 8. `Json` does not escape non-ASCII (`ensure_ascii`)
- **Differs:** Python's `json.dumps` defaults to `ensure_ascii=True`, escaping
  non-ASCII as `\uXXXX`; our `Json` (via `serde_json`) emits UTF-8 directly.
  Exotic float formatting may also differ from CPython's `repr`.
- **Why:** the first `Json` slice targets byte-parity on ASCII documents (the
  common case); matching CPython's escaping/float formatting exactly is a
  follow-up.
- **Remove:** post-process the serialized string to `\u`-escape non-ASCII, and
  reconcile float formatting, under the JSON issue (#10).

### 9. `Markdown` covers most elements (code blocks are non-parity)
- **Differs:** paragraphs, ATX headings (h1–h6), bullet + ordered lists, block
  quotes, thematic breaks (hr), and inline strong/emphasis/code are rendered
  **byte-parity**. Fenced/indented **code blocks** now render via the `Syntax`
  renderable — so they're highlighted but **not** byte-identical to upstream
  (syntect ≠ Pygments; see #18). **Links** (need OSC hyperlinks / `Text` link
  support) and **tables** (need several `Table` features — see #9 on GitHub) are
  still deferred. Also, a document that *ends* with a thematic break omits a
  trailing blank line that upstream emits.
- **Why:** these are the common elements; links/tables each need more machinery.
- **Remove:** add link + table elements under the Markdown issue (#9).

### 10. `AnsiDecoder` skips OSC hyperlinks
- **Differs:** upstream's decoder reads OSC `8;…` sequences and attaches the URL
  as a `Style` link; we recognize and skip the OSC string (the surrounding text
  still decodes normally, just without the link).
- **Why:** `Style` links/meta are not yet ported (see #3-adjacent notes); SGR
  styling — the common case — is fully handled.
- **Remove:** attach the link once `Style` grows link support, under the
  Console/text-completeness issues (#1/#2).

### 11. `Layout` — empty-leaf placeholder
- **Differs:** an empty `Layout` leaf renders as blank space, not upstream's
  interactive `_Placeholder` panel (which shows the layout name/size).
- **Why:** the split/sizing/tiling core is the valuable part; the placeholder is
  a debugging aid.
- **Remove:** add a placeholder renderable under the Layout issue (#7).
- **Resolved:** height-aware leaves — `Panel` now consumes `options.height` and
  expands to fill its region (byte-parity), via `Console::render_lines`'s height
  handling. Other containers can adopt the same pattern as needed.

### 12. `Screen` keeps its trailing row separator when printed
- **Differs:** printing a full-height `Screen` emits a trailing newline after the
  last row (like every other renderable in this port), whereas upstream's
  line-oriented print pipeline omits the final separator for a `Screen` that
  exactly fills the console height.
- **Why:** our `print` uniformly appends one newline after a renderable; matching
  upstream's per-renderable trailing-newline suppression is a print-pipeline
  concern that would affect the shared path.
- **Remove:** model upstream's line-based print (crop/emit rows without a trailing
  separator when a renderable fills the height) under the Console issue (#1).

### 13. `ISO8601Highlighter` covers standard *extended* formats only
- **Differs:** upstream ships ~13 ISO 8601 patterns, including compact/basic forms
  and ones using PCRE conditionals (`(?(hyphen)…)`). `fancy-regex` doesn't support
  conditionals, so we ship the 3 standard **extended** patterns — date, time, and
  date-time, each with optional timezone. Compact forms (`20230615`, week dates,
  ordinal dates) aren't highlighted.
- **Why:** the extended `YYYY-MM-DDThh:mm:ss` family is the common case; the
  conditional patterns can't compile as-is.
- **Remove:** rewrite the compact/conditional patterns into fancy-regex-compatible
  alternations under the highlighter/theme issue (#3).

### 14. Highlighting resolves to fixed styles, not theme-driven names
- **Differs:** upstream stores highlight spans as style *names* (`repr.number`)
  resolved against the console theme at render time; our highlighters resolve to
  concrete `Style`s from a built-in `default_styles` subset. So a custom
  `RegexHighlighter` with a novel `base_style` can create span boundaries but not
  colors (its names aren't in the built-in map), and re-theming built-in highlight
  colors isn't possible yet.
- **Why:** `Text` spans hold concrete styles in this port; name-based resolution
  needs the theme stack.
- **Remove:** store style names on spans and resolve at render via the theme, under
  the highlighter/theme issue (#3).

### 15. HTML export done (both forms); SVG export not ported
- **Differs:** `Console::export_html` (inline styles) and
  `Console::export_html_classes` (the default `.r1 {…}` stylesheet form) are both
  ported with byte-parity. `export_svg` is not. Custom `TerminalTheme`s work for
  color resolution, but only the default theme is byte-parity-verified.
- **Why:** SVG is a separate renderer (font metrics, `<rect>`/`<text>` layout).
- **Remove:** add `export_svg` (+ the SVG template) under the Console issue (#1).

### 16. `Progress` renders the three default deterministic columns only
- **Differs:** `Progress` renders a static grid of the three default *deterministic*
  columns — description, a flexing bar, and percentage — with byte-parity. Upstream
  composes arbitrary `ProgressColumn`s (custom text, spinner, download, transfer-
  speed, and the time-remaining/elapsed columns), and drives an in-place `Live`
  refresh. Custom columns, the time/rate columns (non-deterministic), and the
  refresh loop are not ported.
- **Why:** the default description+bar+percentage layout is the common case and is
  deterministic (testable); the time/rate columns depend on wall-clock elapsed and
  the refresh loop needs `Live`.
- **Remove:** generalize to a `ProgressColumn` list (over renderable table cells)
  and add the `Live` loop, under the Live/progress issue (#6).

### 17. `Live` is the manual-refresh core only (no auto-refresh thread)
- **Differs:** `Live` implements the deterministic `start`/`update`/`refresh`/`stop`
  flow, writing a byte stream that is byte-parity with upstream's
  `auto_refresh=False`, `transient=False` Live. Not ported: the background
  auto-refresh thread (`refresh_per_second`), `transient`/alt-screen modes,
  stdout/stderr redirection, and the console render-hook integration. It also
  renders to a generic `Write` sink rather than through `Console`'s own file.
- **Why:** the manual path captures the mechanism and stays testable; the
  auto-refresh thread is timing-dependent and the render-hook plumbing is large.
- **Remove:** add a refresh thread + `transient`/alt-screen handling, and route
  through `Console`, under the Live/progress issue (#6).

### 18. Syntax highlighting uses `syntect`, not Pygments (non-byte-parity)
- **Differs:** `Syntax` (`syntax.rs`) highlights code with the `syntect` crate,
  whereas upstream uses Pygments. The two ship *different grammars and themes*, so
  the token colors are **not byte-identical** to Python rich — this is the one
  renderable whose output is functional rather than golden-tested. The default
  theme is `base16-ocean.dark` (a syntect built-in), not rich's `ansi_dark`/
  `monokai`. Line numbers, the `Syntax.from_path` loader, word-wrap/`line_range`,
  and background-highlight ranges are not yet ported.
- **Why:** Rust has no Pygments; `syntect` is the standard Rust equivalent
  (mirrors how `cells` delegates East-Asian-width to `unicode-width`). Byte-parity
  is impossible across highlighter engines.
- **Remove:** not removable while using a different engine; the divergence is
  inherent. Future work can add line numbers, themes matching rich's names, and
  the path/loader conveniences.

### 19. Python-object modules are reimagined for Rust
- **Differs:** `pretty.py`/`repr.py`/`_inspect.py`, `traceback.py`, and the
  `logging` handler render *Python objects, exceptions, and log records* via
  runtime reflection — which Rust doesn't have. So these are **reimagined**, not
  faithfully ported:
  - `Pretty` (`pretty.rs`) formats a value with its [`Debug`] impl (`{:#?}` /
    `{:?}`) and colorizes the result with `ReprHighlighter`. Because the coloring
    targets Python-repr spellings, Rust-specific tokens differ — `true`/`false`
    (vs `True`/`False`) are left unstyled — and there is no field/attribute
    introspection (`inspect`). No golden test; verified functionally.
  - `Traceback` (`traceback.rs`) renders an error's message and its
    `Error::source()` chain (`Caused by:`) in a red-bordered panel. There are no
    stack frames or source snippets — Rust errors don't carry them (pair with
    `std::backtrace::Backtrace` at the call site if you want a frame list).
  - `LogRender` (`log_render.rs`) formats one log record — optional time, a
    severity-colored level, message, optional path — into a styled line, using the
    same column styles (`log.time`, `logging.level.*`, `log.path`). It takes a
    `LogLevel` enum + strings rather than depending on `log`/`tracing`; wiring a
    `log::Log` handler on top is a `rich-ext` follow-up.
- **Why:** a 1:1 port isn't possible without reflection; the Rust-native analogs
  deliver the same *utility* (colorized value/error/log rendering).
- **Remove:** inherent to the language difference; not removable.

## Feature-flagged divergences

*None yet.* If a future feature can only be built by changing core behavior, it
must be behind a Cargo `feature` that is **off by default**, and listed here.
