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

### 3. Partial `ANSI_COLOR_NAMES`
- **Differs:** only the 16 standard color names (+`grey`/`gray`) are recognized;
  upstream knows all 256 named colors.
- **Why:** the 256-name table is large and not needed for the first slice.
- **Remove:** port the full table into `color.rs`. Tracked with the color issue.

### 4. Byte offsets in `Text` spans
- **Differs:** upstream `Text` uses code-point offsets for spans; our `Text` uses
  byte offsets internally.
- **Why:** simpler and faster in Rust; observable behavior is identical for the
  ported operations (append/stylize/render), and ASCII-only callers such as the
  example highlighter are unaffected.
- **Remove:** not planned unless a public API needs code-point indexing; if so,
  expose a char-index helper without changing the internal representation.

### 5. No word-wrapping in `Text` yet
- **Differs:** upstream wraps `Text` to the available width; our `Text` does not
  yet wrap. Container renderables (`Panel`, `Padding`) pad/crop child lines to fit
  via `Console::render_lines`, so boxes stay intact, but over-long content is
  cropped rather than wrapped.
- **Why:** the first layout slice targets byte-parity on fitting content; faithful
  wrapping (`_wrap.py`) is a focused task of its own.
- **Remove:** implement wrapping in `text.rs` (issue #2), then have `Text` honor
  `options.max_width`.

### 6. No platform box substitution
- **Differs:** upstream's `Box.substitute` swaps box-drawing glyphs for ASCII (or
  a safe subset) on legacy Windows / non-UTF-8 terminals. We always emit the
  requested glyphs.
- **Why:** keeps output deterministic and platform-independent for the first
  layout slice (and for golden fixtures, which are captured in UTF-8 mode).
- **Remove:** port `Box.substitute` + the `legacy_windows`/`ascii` console flags
  with the Windows-console issue (#12).

## Feature-flagged divergences

*None yet.* If a future feature can only be built by changing core behavior, it
must be behind a Cargo `feature` that is **off by default**, and listed here.
