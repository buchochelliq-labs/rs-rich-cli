# rich-art

Art renderables for [`rich`](https://crates.io/crates/rs-rich): **FIGlet-style
text banners** (`figlet(6)` / `pyfiglet`), **images as ASCII, Braille, half
blocks, quadrant blocks or Sixel graphics** (`jp2a` and beyond), **animated GIF
playback** and **perceptual image diffs** in the terminal.

| Feature | Default | Pulls in |
| --- | --- | --- |
| FIGlet banners | ✅ always | no optional dependencies — just `rs-rich` |
| `image` — images as ASCII, Braille, half blocks or quadrants | off | `image` (png + jpeg decoders) |
| `gif` — animated GIF playback | off | `image` + its gif decoder |
| `sixel` — real pixels through the Sixel graphics protocol | off | `image` + `icy_sixel` |

The default build has **one direct dependency**, `rs-rich` (imported as `rich`).
Its transitive dependency graph is determined by `rs-rich`; banners add no
optional image dependencies unless you enable them.

```rust
use rich::Console;
use rich_art::Figlet;

let console = Console::builder().build();
console.print(&Figlet::new("Hello"));
```

```
 _   _      _ _       
| | | | ___| | | ___  
| |_| |/ _ \ | |/ _ \ 
|  _  |  __/ | | (_) |
|_| |_|\___|_|_|\___/ 
```

## Why it's a separate crate

`rich` upstream has no banner support, so this is **our own feature, not a
port**. The repository's rule (see `AGENTS.md`) is that local features never
touch the faithful `rich` mirror — otherwise every upstream sync becomes a
merge conflict.

This crate is deliberately self-contained so it can be lifted into its own
repository unchanged:

- its default build depends directly only on `rs-rich`, imported as `rich`, for the
  `Renderable` trait;
- like every crate in this repository, it follows an **independent SemVer** that
  started at `0.0.1`; its version is bumped only when `rs-rich-art` is selected
  for a release and never mirrors the Python projects' release numbers;
- the parser and layout engine are original code; the single vendored asset is
  one FIGfont, documented in [`fonts/README.md`](fonts/README.md).

## What it does

- Parses the FIGfont (`.flf`) format: header, comment block, the required
  character set, and code-tagged characters (decimal / `0x` hex / octal tags).
- Implements FIGlet's layout: full-width, kerning, and controlled smushing
  (rules 1–6), plus universal overlapping and hardblank handling.
- Wraps at the console width, with left / centre / right justification.
- Renders as a `rich` `Renderable`, so a banner composes with everything else —
  put it in a `Panel`, style it, export it to HTML or SVG.

Banner output is **byte-parity with `pyfiglet`** for the bundled font, verified
by `tests/figlet_parity.rs` against captured golden output.

## Using other fonts

```rust
use rich_art::{Figlet, FigletFont};

let font = FigletFont::parse(&std::fs::read_to_string("slant.flf")?)?;
println!("{}", Figlet::new("hello").font(font).to_text(80));
```

## Images and animated GIFs

```rust
use rich_art::AsciiArt;

// jp2a-style: a density ramp by luminance, optionally in colour.
let art = AsciiArt::from_path("photo.png")?.color(true);
println!("{}", art.to_text(80));
```

Cells are corrected for terminal aspect (they're about twice as tall as wide),
so images aren't stretched. `invert()` suits light-on-dark terminals, and
`ramp()` takes a custom density ramp.

`ImageArt` is the reusable capability-aware facade for CLI and application
code. It selects ASCII, half-block, quadrant, Braille, or Sixel rendering from
`ImageOptions` and `RenderCapabilities`. By default, sizing and alpha handling
remain in the individual renderers. Optional fitting and background compositing
preprocess still images consistently across backends:

```rust
use rich_art::{ImageAnchor, ImageArt, ImageFit, ImageMode};

let art = ImageArt::from_path("photo.png")?
    .mode(ImageMode::Blocks)
    .width(40)
    .height(12)
    .fit(ImageFit::Cover)
    .anchor(ImageAnchor::TopLeft)
    .background([32, 40, 48]);
```

`ImageFit::Contain` centers the entire image with background padding;
`ImageFit::Cover` fills the rectangle and crops around the selected anchor.
`ImageAnchor` offers `Center` (default), `Top`, `Bottom`, `Left`, `Right`,
`TopLeft`, `TopRight`, `BottomLeft` and `BottomRight`. Anchor selection affects
only cover fitting; contain and unfitted images retain their existing behavior.
Calling `options()` preserves fit, anchor and background settings.

Fitting requires positive explicit width and height, clamps width to the
available terminal columns, and preserves aspect ratio assuming cells are twice
as tall as wide. Output and intermediate rasters are limited to 16 megapixels.
`background()` composites transparency before resizing and colors contain
padding; fitting without a background uses black. `background_mode()` takes an
`ImageBackground`: `Color([r, g, b])` (what `background()` sets),
`TerminalDefault` (leave pixels under half opacity unpainted so the terminal's
own background shows through) or `Checkerboard` (a two-tone gray checkerboard).
Use `render()` to receive validation errors. These APIs require the `image`
feature; crop anchors have been available since art 0.0.6.

Optional palette reduction and dithering:

```rust
use rich_art::{Dither, ImageArt, ImageColorMode, ImageMode};

let art = ImageArt::from_path("photo.png")?
    .mode(ImageMode::Blocks)
    .width(60)
    .color_mode(ImageColorMode::Ansi256)
    .dither(Dither::FloydSteinberg);
```

`ImageColorMode::TrueColor` and `Dither::None` remain the defaults. The reduced
palettes (`Ansi256`, `Ansi16`, `Grayscale`) and the three dithers
(Floyd–Steinberg, Bayer 4×4, Atkinson) work with the ASCII, half-block, quadrant
and Sixel backends and with GIF frames; dithering needs a reduced palette, and
Sixel then encodes exactly the palette's colours. Braille is monochrome and
rejects a reduced palette, as does an Auto mode that resolves to Braille, with
a validation error from `render()`. The builders preserve the public
`ImageOptions` struct shape and require the `image` feature. The
[images guide](https://buchochelliq-labs.github.io/rs-rich-cli/guide/art/images/) shows every mode and option.

Palette reduction runs after fitting, background compositing and final sampling,
before glyph selection. ANSI256 uses fixed entries 16–255 (terminal-dependent
entries 0–15 are excluded). By default the nearest colour is the one at the
smallest squared encoded-RGB distance, with lowest-index ties;
`color_distance(ColorDistance::Oklab)` measures perceptually in OKLab instead,
which keeps hues truer on small palettes. Diffusion scans left-to-right,
top-to-bottom and discards error at image boundaries; the output is
deterministic.

The image APIs intentionally live in `rich-art`, the repository's dedicated
art crate, rather than `rich-ext`: `rich-ext` provides console/plugin
extensions, while `rich-art` owns image decoding, raster renderers, GIFs, and
terminal graphics. The CLI depends on this public crate and contains only
argument mapping and output policy.

Animated GIFs play in place, driven by `rich`'s `Live` display and honouring
each frame's own delay. Frame disposal is handled by the decoder, so frames
never smear.

```rust
use rich::Console;
use rich_art::gif::{AnimatedArt, Repeat};

AnimatedArt::from_path("cat.gif")?
    .color(true)
    .max_fps(20.0)              // colour art is byte-heavy; cap the rate
    .repeat(Repeat::Forever)
    .play_stdout(Console::builder().build())?;
```

`play` hides the cursor and restores it on return. An interrupt (Ctrl-C) kills
the process without unwinding, so a caller that traps signals should emit
`rich_art::gif::show_cursor_sequence()` on the way out.

## Examples

```bash
cargo run -p rs-rich-art --example banner -- "your text"

# Draw a waving cat, then play it in the terminal.
cargo run -p rs-rich-art --features gif --example make_demo_gif
cargo run -p rs-rich-art --features gif --example gif -- cat.gif 3
```

Explicit destinations can call `ImageArt::render_with_environment` with the
shared `rich::protocol::RenderEnvironment`. Nested ImageArt renderables also
consume a context attached to their Console. Only legacy consoles without a
context detect terminal support. Capture/export contexts reject explicit Sixel;
the infallible Renderable path falls back to safe text.

`ImageArt::transforms(ImageTransforms { .. })` adds clockwise quarter-turns,
horizontal then vertical flips, and grayscale before fitting and final sampling.
Geometry-only transforms preserve RGBA; grayscale composites against the chosen
background and converts that background for contain padding too. Unselected
transforms keep the existing shared-image fast path and ImageOptions stays unchanged.

### Still-image transformations

`ImageArt::transforms(ImageTransforms { rotation: Rotation::Clockwise90,
flip_horizontal: true, flip_vertical: false, grayscale: true })` applies clockwise
rotation, horizontal flip, vertical flip, alpha compositing and grayscale, then
fit/anchor, final sampling, palette quantization and glyph selection. Geometry
alone retains alpha. Grayscale uses `(77R + 150G + 29B + 128) >> 8` after
compositing; contain padding uses the same grayscale background.

`Dither::Bayer4x4` is an opt-in, origin-anchored ordered dither, and
`Dither::Atkinson` an error diffusion that spreads six eighths of the error.
Like Floyd–Steinberg, both need a reduced palette; truecolor and Braille reject
them. Exhaustive matches on `Dither` must handle every variant. Defaults retain the
previous output. Still-image transforms do not apply to animation or image diff.

Explicit-context Auto selects ASCII for `unicode=false`. Explicit Blocks/Braille
remain caller opt-ins and may emit Unicode; choose ASCII for restricted sinks.
