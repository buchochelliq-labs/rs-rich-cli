# Micro assets (rs-rich-micro)

`rs-rich-micro` (`rich_micro`, new in 0.0.14) holds micro assets:
emoji-sized inline images and animations, written `Deploying :micro:rocket:`.
`rich` has no micro assets, so this crate is an rs-rich addition, not a
port, and core is unchanged: `:micro:name:` is not an emoji code, so it
passes through core as plain text.

Version 0.0.1 holds the model, the `.richmicro` package format, the layered
registry, the markup, the drawing (Kitty, iTerm2 or Sixel images where the
terminal supports them, half-block cells or the emoji or text fallback
everywhere else, with animation), the image pipeline that makes packages,
and a built-in library (`status/`, `dev/` and `fun/` sets).

- [Authoring micro assets](authoring.md): the pipeline, `rich micro
  create`, the manifest, packs and the built-in library.
- [Micro assets in the `rich` CLI](cli.md): `rich micro`, `:micro:` in
  `--print --emoji`, `rich asset --kind micro`, `rich explore --icons`.
- [Terminal compatibility](terminals.md): what each terminal draws.
- From Python: [`rs_rich.micro`](https://buchochelliq-labs.github.io/rs-rich-cli/python/micro/).

The asset and markup samples below are the crate's doc tests, which CI runs.

## An asset

A `MicroAsset` has:

- a **name**: `[a-z0-9_-]` segments joined by `/` namespaces, such as
  `status/success`;
- a **kind**: static or animated;
- a **size in cells**: 2×1 by default (an emoji's footprint, since a cell is
  about twice as tall as it is wide), 1×1 allowed, at most 4 columns, and one
  row only in this release;
- **alt text**, which is mandatory: screen readers, logs and plain exports
  show it where nothing else fits;
- a **fallback**: an emoji, a short text, or both, each fitting the asset's
  columns with no whitespace;
- images (a static image, an animation, or a frame sequence), aliases, and
  its origin, version, licence and author.

```rust
use rich_micro::{CellSize, MicroAsset};

let asset = MicroAsset::new("status/ok", "green check mark")?
    .with_emoji("✅")?
    .with_text("OK")?;
assert_eq!(asset.size(), CellSize::default()); // 2x1
```

## Markup

`:micro:name:` is the one syntax (`[micro=…]` would collide with core's
markup tags). `\:micro:` escapes it. A name no layer has stays as written and
comes back as a diagnostic.

```rust
use rich::Console;
use rich_micro::{render_markup, FallbackPreference, Layer, MicroAsset, MicroRegistry};

let mut registry = MicroRegistry::new();
registry.add(Layer::Inline, MicroAsset::new("ship", "rocket")?.with_emoji("🚀")?)?;

let console = Console::builder().width(40).build();
let (text, diagnostics) =
    render_markup(&console, "Deploying :micro:ship: :fire:", &registry, FallbackPreference::Emoji);
assert_eq!(text.plain(), "Deploying 🚀 🔥");
assert!(diagnostics.is_empty());
```

`render_markup`, `markup_text` and `PreparedMarkup` swap tokens out before
core parses the markup, so a token directly followed by an emoji code
(`:micro:ship::fire:`) gets both. To expand tokens in a `Text` you already
have, use `expand`, `MicroExt::expand_micro`, or `MicroTransform` in a
transform pipeline (`MicroPlugin` registers it as `micro`). In code,
`text.append_micro(&registry, "ship")` appends one, and `MicroAssetRef` is a
renderable of its own.

## Width and layout

An asset in a `Text` is exactly its columns of ordinary text: its fallback,
padded with U+2800 (a blank that is not whitespace, so wrapping never splits
an asset), under a style whose metadata holds
`rich.micro = [name, cols, rows, id]`. It measures, wraps, crops, and sits in
tables and panels at exactly its width, and prints as plain fallback in
pipes, logs and exports.

`MicroView` wraps any renderable and offers each complete placement in its
output to `MicroRenderer`s, which may swap it for other cells of the same
width. A placement that was cropped or split is never offered.
`FallbackRenderer` redraws the emoji, text or alt text; the terminal graphics
renderers below plug in at the same seam.

## Drawing on a terminal

`MicroGraphics` holds one terminal's drawing state. `MicroGraphics::detect`
reads the terminal (through `rich_ext::graphics::GraphicsEnvironment`) and
selects one way to draw; `graphics.view(renderable)` draws every micro asset
in a renderable with it:

```rust
use std::sync::Arc;
use rich::Console;
use rich_micro::{render_markup, FallbackPreference, Layer, MicroAsset, MicroGraphics, MicroRegistry};

let mut registry = MicroRegistry::new();
registry.add(Layer::Inline, MicroAsset::new("ship", "rocket")?.with_emoji("🚀")?)?;
let registry = Arc::new(registry);

// Detects the terminal once: Kitty, iTerm2, Sixel, blocks or text.
let graphics = MicroGraphics::detect(Arc::clone(&registry));
let console = Console::new();
let (text, _) = render_markup(&console, "Deploying :micro:ship:", &registry, FallbackPreference::Emoji);
console.print(&graphics.view(text));
```

The selection goes, in order:

1. **Kitty** (`TERM=xterm-kitty` or `KITTY_WINDOW_ID`): each image is
   transmitted once per session under an id, with a virtual placement, and
   the asset's cells are Unicode placeholders (U+10EEEE with a row and a
   column diacritic, coloured with the id). The image lives in the cell grid:
   it scrolls with the text, survives redraws, and costs nothing to show
   again.
2. **iTerm2** (`TERM_PROGRAM` iTerm.app or WezTerm): an inline image of
   exactly the asset's cells, drawn over blank cells with the cursor saved
   before them and restored after, then moved past them.
3. **Sixel** (the same heuristic as `rich --image`), when the cell size in
   pixels is known: the image fitted to exactly its cells, with no line
   break, drawn the same way as iTerm2's.
4. **Half-blocks**, on a colour terminal: the asset's emoji or text if it has
   one, else its image as block cells at exactly its size, else its alt text.
5. **Text**: the emoji or text fallback, else the alt text.

`RICH_MICRO=kitty|iterm|sixel|blocks|text` overrides the choice on a
terminal (`blocks` puts the image before the emoji). Output that is not a
terminal, a pipe, a log file, or a console exporting text, HTML or SVG,
always gets the text fallback, byte for byte what it would be without
`MicroGraphics`, whatever the override says. Each renderer checks the
console it renders for, so an export from a console that is not a terminal
never holds an escape. `rich doctor` reports the choice and the reason.

Images are fitted to the cell size in pixels, from `RICH_CELL_PIXELS=WxH`,
the terminal's window size in pixels, or (for `MicroGraphics::detect` on an
interactive terminal that has a graphics protocol) a `CSI 16 t` query; 8×16
is assumed otherwise. The query is asked only by a foreground process and
only when no input is waiting, so it never stops a background job or eats
typeahead; set `RICH_CELL_PIXELS` to skip it altogether.

### Animation

Animated assets (GIF or APNG) are decoded under the package limits, fitted
to their cells, deduplicated (repeated frames merge) and rate-limited
(frames under 40 ms merge into the next). Kitty plays them with its own
frame protocol and iTerm2 as an animated GIF. Elsewhere, like a spinner, the
frame follows the clock: each redraw inside a live region or the interactive
event loop shows the frame that is due, and the graphics source tells the
event loop when the next one is. `RICH_A11Y=reduced-motion` and
`RICH_ANIMATION=0` show the still image, as does any output that is not
interactive.

### Live regions and interactive views

`Frame`s, `LiveCoordinator` and the interactive painter carry images beside
their cells, as a list of placements: `live.with_graphics(graphics.source())`
and `event_loop.graphics(graphics.source())`. Before a frame is built the
source swaps the fallback cells for the mode's cells (Kitty placeholders, or
blanks under an image); after the cell diff is written, images are drawn
where they are new, moved, on another frame, or where the cells under them
were repainted. Diffs stay exact because the cells hold the layout. When a
view closes, the Kitty images it no longer shows are deleted by id
(`graphics.close()` deletes every one for a program that is done).

### The cache

Decoded images are cached in memory, keyed by a hash of the source, the size
in cells and the cell size, within 32 MiB by default. `MicroGraphics::detect`
also keeps the fitted still images under the user cache directory
(`RICH_CACHE_DIR/micro`, else `$XDG_CACHE_HOME/rich/micro`,
`~/Library/Caches/rich/micro` or `%LOCALAPPDATA%\rich\cache\micro`), so a
later run reads a tiny PNG instead of decoding the source.

### Terminal compatibility

What each terminal draws, the overrides, and how to check yours are on
[their own page](terminals.md).

## Packages

A `.richmicro` package is a zip archive or a directory holding
`manifest.json` and its images:

```json
{
  "schema_version": 1,
  "name": "status/success",
  "alt": "green check mark",
  "size": "2x1",
  "fallback": { "emoji": "✅", "text": "OK" },
  "static": "static.png",
  "animation": "animation.gif",
  "aliases": ["success"],
  "version": "1.0.0",
  "license": "CC0-1.0",
  "author": "you"
}
```

The animation may be a GIF, APNG or WebP, or a `"frames"` list of still
images. A **pack** is a directory or zip archive with a `pack.json`
(`schema_version`, `name`, `version` and a `packages` list).

Packages are untrusted input. `Limits` bound the archive size, each file,
the manifest, the number of entries, the bytes read from one archive, pixel
dimensions, frame counts and the bytes an asset takes decoded. Images are
checked by their headers without being decoded, so a decompression bomb is
refused by its header. Paths that are absolute, use `..`, hold backslashes,
or are symbolic links (in an archive, any; in a directory, any that leads
out of it) are refused. Unknown manifest fields and any `schema_version` but
1 are errors.

## The registry

Layers resolve in this order, a later one winning:

1. built-in: the library's own set, compiled in (`MicroRegistry::builtin()`,
   or `MicroRoots::from_env`, which turns it on);
2. the user's, `~/.config/rich/micro/`;
3. a trusted project's, `.rich/micro/`;
4. inline, added in code.

A project's assets load only when the caller says the project is trusted,
under the same rule as project config, so a cloned repository cannot restyle
`success` or put images in your terminal. Aliases resolve within their own
layer. When two packages in one layer claim a name, the first in file-name
order keeps it and the other is reported as a collision; a package that
fails to load is reported and the rest still load.

```rust
use rich_micro::{Layer, Limits, MicroRegistry, MicroRoots};

let roots = MicroRoots::from_env(Some(std::path::Path::new("."))).trust_project(false);
let (registry, report) = MicroRegistry::load(&roots, &Limits::default());
for rejected in &report.rejected {
    eprintln!("{}: {}", rejected.path.display(), rejected.error);
}
if let Some(explanation) = registry.explain("status/success") {
    rich::Console::new().print(&explanation);
}
```

`explain` shows which layer won and what it overrides, through the
precedence view in `rich_ext::cli_doc`.
