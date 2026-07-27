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

### 5. Wrapping folds by cell width, not full grapheme clusters
- **Differs:** `Text` word-wrapping is implemented (port of `_wrap.divide_line`),
  but the over-long-word fold (`cells.chop_cells`) breaks on cell boundaries per
  `char`, whereas upstream splits on grapheme clusters for combining sequences.
- **Why:** avoids vendoring the grapheme/`_unicode_data` tables for the first
  wrapping slice; identical for non-combining text (the common case).
- **Remove:** port grapheme segmentation with the `_unicode_data` utilities issue.

### 6. No platform box substitution
- **Differs:** upstream's `Box.substitute` swaps box-drawing glyphs for ASCII (or
  a safe subset) on legacy Windows / non-UTF-8 terminals. We always emit the
  requested glyphs.
- **Why:** keeps output deterministic and platform-independent for the first
  layout slice (and for golden fixtures, which are captured in UTF-8 mode).
- **Remove:** port `Box.substitute` + the `legacy_windows`/`ascii` console flags
  with the Windows-console issue (#12).

### 7. `Table` — a few advanced options remain
- **Differs:** sizing (content, shrink-to-fit, `expand`), per-column justify,
  explicit per-column **width**, per-column **style**, **ellipsis overflow**
  (the table default), title, caption, and `show_lines` are all ported. Not yet
  ported: explicit per-column `ratio`/`min_width`/`max_width` and `no_wrap`.
- **Why:** the remaining options are less common and build on the same width
  machinery.
- **Remove:** add the per-column ratio/min/max and `no_wrap` fields with the
  Table issue (#5).

### 8. `Json` does not escape non-ASCII (`ensure_ascii`)
- **Differs:** Python's `json.dumps` defaults to `ensure_ascii=True`, escaping
  non-ASCII as `\uXXXX`; our `Json` (via `serde_json`) emits UTF-8 directly.
  Exotic float formatting may also differ from CPython's `repr`.
- **Why:** the first `Json` slice targets byte-parity on ASCII documents (the
  common case); matching CPython's escaping/float formatting exactly is a
  follow-up.
- **Remove:** post-process the serialized string to `\u`-escape non-ASCII, and
  reconcile float formatting, under the JSON issue (#10).

### 9. `Markdown` covers a subset of elements
- **Differs:** paragraphs, ATX headings (h1–h6), bullet + ordered lists, block
  quotes, thematic breaks (hr), and inline strong/emphasis/code are rendered
  (byte-parity); code blocks, links, and tables are not yet handled. Also, a
  document that *ends* with a thematic break omits a trailing blank line that
  upstream emits.
- **Why:** these are the common elements; the rest each need their own renderer.
- **Remove:** port the remaining `markdown.py` element types under the Markdown
  issue (#9).

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

### 15. HTML export is inline-styles only (no CSS classes, no SVG)
- **Differs:** `Console::export_html` implements upstream's `inline_styles=True`
  path (each span carries its own `style="…"`) with byte-parity. The default
  CSS-class variant (`.r1 {…}` stylesheet + `class="r1"`) and `export_svg` are
  not ported. Custom `TerminalTheme`s work for color resolution, but only the
  default theme is byte-parity-verified.
- **Why:** inline styles are the self-contained common case; the class variant is
  a mechanical follow-up and SVG is a separate renderer.
- **Remove:** add the CSS-class dedup + stylesheet assembly, and an
  `export_svg`, under the Console issue (#1).

## Feature-flagged divergences

*None yet.* If a future feature can only be built by changing core behavior, it
must be behind a Cargo `feature` that is **off by default**, and listed here.
