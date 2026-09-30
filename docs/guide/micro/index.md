# Micro assets (rs-rich-micro)

`rs-rich-micro` (`rich_micro`, new in 0.0.14) holds micro assets:
emoji-sized inline images and animations, written `Deploying :micro:rocket:`.
`rich` has no micro assets, so this crate is an rs-rich addition, not a
port, and core is unchanged: `:micro:name:` is not an emoji code, so it
passes through core as plain text.

Version 0.0.1 is the foundation: the model, the `.richmicro` package format,
the layered registry, and the markup. Every asset renders as its emoji or
text fallback. Kitty, iTerm2 and Sixel drawing, animation, a built-in
library and a `rich micro` command follow in later releases.

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
width. A placement that was cropped or split is never offered. This release
ships `FallbackRenderer` (emoji, text, or alt text); terminal graphics
renderers plug in at the same seam.

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

1. built-in (empty in 0.0.1);
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
