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

### 2. Markup: lenient print path is opt-out, not opt-in; partial backslash rules
- **Differs:** `markup::render` matches upstream — a `[…]` is only a tag when it
  starts with `[a-z#/@]`, an unmatched closing tag returns `RichError::Markup`,
  unclosed *opening* tags auto-close, and (since tag names are resolved at render)
  an **unknown tag name is a silent no-op** rather than an error, as verified
  against real rich 15.0.0. Two gaps remain: (a) the infallible
  `Console::print_str`/`build_text` still fall back to printing the raw text
  where upstream would raise `MarkupError` — the strict behaviour is available as
  `try_print_str` / `try_print_justified` / `try_build_text`, and `rich --print`
  uses it, so user-supplied markup is reported rather than rendered literally;
  and (b) the parser handles the common `\[` escape but not the full
  backslash-run doubling semantics of `_parse` (the public `markup::escape`
  *does* implement the full rule).
- **Why:** the infallible signatures keep the ~30 internal call sites that pass
  literal markup free of `unwrap`/`?` noise, where a parse error is a bug rather
  than a runtime condition. The exotic backslash-run cases are rare.
- **Remove:** make the strict form the default (renaming the lenient one to
  `*_lossy`), and port the backslash-run branch of `_parse`, under #2.

### 3. Byte offsets in `Text` spans
- **Differs:** upstream `Text` uses code-point offsets for spans; our `Text` uses
  byte offsets internally.
- **Why:** simpler and faster in Rust; observable behavior is identical for the
  ported operations (append/stylize/render), and ASCII-only callers such as the
  example highlighter are unaffected.
- **Remove:** not planned unless a public API needs code-point indexing; if so,
  expose a char-index helper without changing the internal representation.

### 5. ~~Over-long-word fold suppresses one empty chunk vs upstream~~ (resolved)
- **Resolved:** `Text` word-wrapping (`_wrap.divide_line`) and the over-long-word
  fold (`cells.chop_cells`) are **byte-parity** with upstream. The `!line.is_empty()`
  guard has been dropped, so folding a character *wider than the fold width* (e.g.
  a 2-cell CJK char to width 1) now emits upstream's empty leading chunk
  (`["", "宽", …]`) — and `divide_line` therefore produces the same duplicate break
  position (hence the same empty line) as upstream. Verified against real rich
  15.0.0 (`chop_cells("宽宽", 1) == ["", "宽", "宽"]`, unit-tested), and unchanged
  for normal text (`cw <= width` never triggers the empty push). 0-width combining
  marks still stay attached to their base char (char-based, no grapheme table).
- **Residual (minor):** `chop_cells` is char-based, not grapheme-based, so it
  still differs from upstream for *multi-codepoint graphemes folded below their
  own width* (ZWJ emoji, regional-indicator flags) — vanishingly rare, and it
  would need a grapheme-segmentation dependency to close.

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

### 7. `Table` — one rare padding edge remains
- **Differs:** sizing (content, shrink-to-fit, `expand`), per-column justify,
  explicit per-column **width**, per-column **`ratio`/`min_width`/`max_width`**,
  per-column **style**, **`no_wrap`** (crop to one line with ellipsis), **ellipsis
  overflow** (the table default), a **table-level style**, `pad_edge`/`show_edge`/
  `collapse_padding`, title, caption, and `show_lines` are all ported and
  byte-parity. The only residual: a *wrapping* column squeezed to width 0 by a
  greedy `no_wrap` neighbor still renders its cell padding (upstream drops it) — a
  rare over-constrained case.
- **Why:** the width-0 padding edge only appears when a table is narrower than its
  no_wrap content plus one other column.
- **Remove:** drop padding on zero-width columns under the Table issue (#5).

### 8. `Json` — exotic number formatting differs from CPython
- **Differs:** non-ASCII strings and key order are **byte-parity** with upstream
  (golden `json_unicode`): `rich.json.JSON` defaults to `ensure_ascii=False`, so
  our UTF-8 output matches, and `serde_json`'s `preserve_order` keeps input key
  order — the earlier "we don't `\u`-escape" concern was a false alarm (upstream
  doesn't escape either). What *can* still differ is **number formatting** for
  exotic values: exponent notation (CPython renders `1e+20` / `1e-07`; ryu via
  `serde_json` renders `1e20` / `1e-7`, and the two use different thresholds for
  *when* to switch to exponent form), and integers beyond i64/u64 lose precision
  (parsed as f64) where CPython keeps them exact.
- **Why:** matching CPython exactly means replicating its `float_repr`
  (shortest-round-trip *and* its decimal/exponent threshold + `e[+-]NN` padding),
  which ryu formats differently; and exact big-integers need `serde_json`'s
  `arbitrary_precision`, which in turn stops normalizing numbers. Both are a
  rabbit hole disproportionate to how rarely JSON documents carry such values.
- **Remove:** port CPython's `float_repr` and enable `arbitrary_precision` (with
  its own normalization pass) under the JSON issue (#10).

### 9. `Markdown` covers most elements (code blocks are non-parity)
- **Differs:** paragraphs, ATX headings (h1–h6), bullet + ordered lists, block
  quotes, thematic breaks (hr), inline strong/emphasis/code, and **GFM tables**
  are rendered **byte-parity**. Fenced/indented **code blocks** now render via the
  `Syntax` renderable — so they're highlighted but **not** byte-identical to
  upstream (syntect ≠ Pygments; see #18). **Links** render as an OSC 8 hyperlink +
  the `markdown.link_url` style — byte-identical to upstream except the random
  `id=` field we omit (#20). The one remaining gap: **inline styling within a
  table cell** (e.g. `**bold**` inside a cell) is collected as plain text, since
  Table cells are strings, not `Text` renderables. (The trailing-blank-line quirk
  for a document ending in a thematic break is now matched — golden
  `markdown_hr_end`.)
- **Why:** these are the common elements; cell-level inline styling needs Table
  cells to become full renderables (a larger refactor).
- **Remove:** give Table cells styled `Text` content, then route inline markdown
  into table cells, under the Markdown issue (#9).

### 20. Hyperlinks omit upstream's random `id=` field
- **Differs:** `Style::with_link` renders an OSC 8 hyperlink as
  `\x1b]8;;{url}\x1b\…\x1b]8;;\x1b\`. Upstream adds a **random** `id=` field
  (`\x1b]8;id={random};{url}…`) so a terminal can group the segments of one link
  for hover highlighting. We omit it, making output deterministic (and
  golden-testable); the link still works, and byte output otherwise matches.
- **Why:** the random id is non-deterministic (breaks golden tests) and only
  affects hover-grouping of a multi-segment link.
- **Remove:** add a stable per-link id (e.g. a hash of the URL) if hover grouping
  is ever needed — but it still wouldn't match upstream's random value.

### 10. ~~`AnsiDecoder` skips OSC hyperlinks~~ (resolved)
- **Resolved:** the decoder now reads OSC 8 sequences (`\x1b]8;<params>;<url>\x1b\`)
  and attaches the URL to the running `Style` via `Style::update_link` (the empty
  closing sequence clears it; `id=`/other params are ignored). Re-rendering
  reproduces upstream byte-for-byte **except** the random `id=` field upstream
  adds, which we omit for determinism — the same, already-documented deviation as
  #20. Covered by round-trip unit tests (a golden isn't possible precisely because
  upstream's `id=` is random).

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

### 13. ~~`ISO8601Highlighter` covers standard *extended* formats only~~ (resolved)
- **Resolved:** all of upstream's ISO 8601 patterns are now ported, in the same
  order — compact/basic calendar dates (`20230615`), ordinal dates, week dates,
  basic times, standalone timezones, and the space-separated date-time forms.
  Upstream's single PCRE-conditional pattern (`(?(hyphen)…)`, which `fancy-regex`
  can't compile) is rewritten as two non-conditional alternatives — the
  all-hyphen/colon form and the all-basic form — which together match exactly the
  same strings the conditional does. Byte-parity with real rich 15.0.0 across
  compact/basic/ordinal/week/split forms (unit-tested).

### 14. No theme *stack* (`push_theme`/`pop_theme`)
- **Differs:** style names on spans now resolve against the rendering console's
  theme, as upstream does — that half is **done** (`StyleType`, `Theme::get_style`).
  What is missing is upstream's per-console theme *stack*: `Console.push_theme`,
  `pop_theme` and the `use_theme` context manager. Names resolve against the
  console's single current theme instead.
- **Why:** the stack forces a `&mut self`-vs-interior-mutability decision that
  this port should not make casually. An RAII guard borrowing the `Console`
  mutably makes `console.print(...)` *inside* the guard a borrow error — which is
  the entire use case — and a `RefCell` stack breaks `Console::theme() -> &Theme`
  and adds an `already borrowed` panic class on re-entrant renders. Upstream's
  stack is also thread-local, which sits awkwardly with a `Console` that gets
  *moved* between threads by `Live::spawn`.
- **Remove:** design the stack against those constraints, under its own issue.
  Late-bound span names are a strict prerequisite and are now in place.

### 14a. `Style::parse` results are not cached
- **Differs:** upstream LRU-caches style-definition parsing; we re-resolve names
  once per render (into a vector parallel to the spans, like upstream's
  `style_map`) with no cross-render cache.
- **Why:** the per-render resolution already collapses the repeated work inside a
  render, which is where it mattered; a global cache would need a lock or
  thread-local and has not been shown to be worth it.
- **Remove:** measure first; add only if a profile justifies it.

### 15. Exports done (HTML both forms + SVG); SVG needs an explicit `unique_id`
- **Differs:** `Console::export_html` (inline styles), `export_html_classes` (the
  default `.r1 {…}` stylesheet form), and **`export_svg`** are all ported with
  byte-parity (`svg.rs`, golden `tests/golden/svg_export.svg`). The only SVG
  residual: upstream's **default** `unique_id` is `adler32` over Python's `repr()`
  of each `Segment`, which Rust can't reproduce — so `Console::export_svg` takes an
  **explicit `unique_id`**, and output is byte-parity with
  `export_svg(title=…, unique_id=…)`. Same shape as the OSC8 `id=` deviation (#20).
- **Why:** the default id is non-deterministic (breaks golden tests) and only
  namespaces the CSS classes / element ids within one document.
- **Remove:** add an `adler32`-of-`repr` default id only if a caller needs the
  exact auto-generated ids (rare); the explicit-id form already round-trips.

### 16. `Progress` — deterministic columns done; time/rate/spinner + Live deferred
- **Differs:** `Progress` now renders a **configurable `ProgressColumn` list**
  (default: description, flexing bar, percentage), with the deterministic columns
  ported byte-parity — description, static text, the bar, percentage, **M-of-N**
  (`{completed}/{total}`), and **download** (`0.5/1.0 kB`, shared SI byte unit via
  `filesize::pick_unit_and_suffix`). The grid layout matches upstream's
  `Table.grid(padding=(0, 1))`: fixed columns take their widest cell, the bar
  flexes (capped at 40), single unstyled space between columns. Still deferred:
  the non-deterministic columns (spinner, transfer-speed, time-remaining/elapsed)
  and the in-place `Live` refresh loop.
- **Why:** the ported columns are deterministic (testable); the time/rate/spinner
  columns depend on wall-clock elapsed and the refresh loop needs `Live` (#17).
- **Remove:** add the time/rate/spinner columns (with the `Live` loop) under the
  Live/progress issue (#6).

### 17. `Live` — auto-refresh thread done; alt-screen/redirect deferred
- **Differs:** `Live` implements the deterministic `start`/`update`/`refresh`/`stop`
  flow (byte-parity with upstream's `auto_refresh=False`, `transient=False` Live),
  **and** a background **auto-refresh thread** — `Live::spawn(...)` returns an
  [`AutoLive`] handle that redraws every `1/refresh_per_second` (and on each
  `update`), a port of upstream's `refresh_per_second`. The thread constructs and
  owns the `Live` internally, so only `Send` inputs (renderable/console/writer)
  cross over — which made `Console` `Send` (its highlighter boxes are now
  `dyn Highlighter + Send`). Still deferred: `transient`/alt-screen modes,
  stdout/stderr redirection, and the console render-hook integration; `Live` also
  renders to a generic `Write` sink rather than through `Console`'s own file.
- **Why:** those remaining pieces are large plumbing; the refresh loop itself is
  ported and (with a long interval, so no timeout fires) even deterministically
  tested to emit the same stream through the thread.
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
