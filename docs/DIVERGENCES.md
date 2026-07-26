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

### 2. Markup parser is lenient about unbalanced tags
- **Differs:** upstream raises `MarkupError` on mismatched `[/]`; our slice parser
  auto-closes open tags at end of input and ignores stray closes.
- **Why:** keeps the first slice's demo/CLI robust on arbitrary input.
- **Remove:** restore strict `MarkupError` behavior when `markup.py` is fully
  ported; add golden/negative tests. Tracked with the markup/text porting issue.

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
  title, caption, and `show_lines` are all ported. Not yet ported: explicit
  per-column `ratio`/`width`/`min_width`/`max_width`, `no_wrap`, and per-column
  style.
- **Why:** the remaining options are less common and build on the same width
  machinery.
- **Remove:** add the per-column width/no_wrap/style fields with the Table
  issue (#5).

### 8. `Json` does not escape non-ASCII (`ensure_ascii`)
- **Differs:** Python's `json.dumps` defaults to `ensure_ascii=True`, escaping
  non-ASCII as `\uXXXX`; our `Json` (via `serde_json`) emits UTF-8 directly.
  Exotic float formatting may also differ from CPython's `repr`.
- **Why:** the first `Json` slice targets byte-parity on ASCII documents (the
  common case); matching CPython's escaping/float formatting exactly is a
  follow-up.
- **Remove:** post-process the serialized string to `\u`-escape non-ASCII, and
  reconcile float formatting, under the JSON issue (#10).

## Feature-flagged divergences

*None yet.* If a future feature can only be built by changing core behavior, it
must be behind a Cargo `feature` that is **off by default**, and listed here.
