# rs-rich-micro

Micro assets for [rs-rich](https://github.com/buchochelliq-labs/rs-rich-cli),
the Rust port of Python's `rich`: emoji-sized inline images and animations,
written `Deploying :micro:rocket:`. This crate is an rs-rich addition, not a
port: `rich` has no micro assets. Core is untouched; `:micro:name:` is not an
emoji code, so it passes through core as plain text.

**0.0.1** holds the model, the package format, the layered registry, the
markup and the drawing: `MicroGraphics` picks Kitty, iTerm2, Sixel,
half-blocks or the emoji or text fallback for the terminal (`RICH_MICRO`
overrides it; a pipe always gets the text), animates where it can, and
carries images through `rich-ext`'s live regions and the interactive
painter.

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

## Assets

A `MicroAsset` has a name (`[a-z0-9_-]` segments joined by `/`, like
`status/success`), a kind (static or animated), a size in cells (2×1 by
default, the footprint of an emoji; 1×1 allowed; one row only), images,
**mandatory alt text**, an emoji and/or text fallback that must fit its
columns, aliases, and where it came from (layer, pack, version, licence,
author).

## Packages

A `.richmicro` package is a zip archive or a directory:

```text
success.richmicro/
├── manifest.json      {"schema_version": 1, "name": "status/success", "alt": "…",
│                       "size": "2x1", "fallback": {"emoji": "✅", "text": "OK"},
│                       "static": "static.png", "animation": "animation.gif"}
├── static.png
└── animation.gif      optional: GIF, APNG or WebP; or "frames": [...]
```

A pack is a directory or archive with a `pack.json` listing its packages.
Packages are untrusted input: hard `Limits` bound archive and file sizes,
entry counts, the manifest, pixel dimensions, frame counts and the bytes an
asset takes decoded, all checked from headers without decoding; paths that
are absolute, use `..`, or are links out of the package are refused.

## Registry

Layers resolve built-in < user (`~/.config/rich/micro/`) < trusted project
(`.rich/micro/`) < inline. A project's assets load only when the caller says
the project is trusted. Aliases resolve within their layer; two claims on a
name in one layer are reported as a collision, and the first in file-name
order wins. `MicroRegistry::explain(name)` shows the chain.

## Markup and API

- `:micro:name:` is the one syntax; `\:micro:` escapes it. Unknown names stay
  as written and come back as diagnostics.
- `render_markup` / `markup_text` / `PreparedMarkup` swap tokens out before
  core parses the markup, so `:micro:ship::fire:` gets both.
- `MicroExt` on `Text` (`append_micro`, `expand_micro`), the `MicroAssetRef`
  renderable, and `MicroTransform` / `MicroPlugin` for transform pipelines.

## Placeholders

An asset in a `Text` is exactly its columns of ordinary text (its fallback,
padded with U+2800) under a style whose metadata holds `rich.micro =
[name, cols, rows, id]`. So it measures, wraps, crops and sits in tables and
panels at exactly its width, and prints as plain fallback in pipes, logs and
exports. `MicroView` offers each complete placement to `MicroRenderer`s,
which may swap it for other cells of the same width.

Licensed MIT.
