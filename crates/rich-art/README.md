# rich-art

ASCII-art renderables for [`rich`](../rich): **FIGlet-style text banners**
(`figlet(6)` / `pyfiglet`), **image → ASCII/ANSI art** (`jp2a`), and **animated
GIF playback** in the terminal.

| Feature | Default | Pulls in |
| --- | --- | --- |
| FIGlet banners | ✅ always | nothing — just `rich` |
| `image` — image → ASCII/ANSI art | off | `image` (png + jpeg decoders) |
| `gif` — animated GIF playback | off | `image` + its gif decoder |

The default build has **exactly one dependency** (`rich`), so banners cost you
nothing extra.

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

- its only dependency is `rich`, and only for the `Renderable` trait;
- it carries its **own SemVer** (`0.1.0`), never version-locked to
  `rich`/`rich-cli`, which mirror upstream releases;
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
cargo run -p rich-art --example banner -- "your text"

# Draw a waving cat, then play it in the terminal.
cargo run -p rich-art --features gif --example make_demo_gif
cargo run -p rich-art --features gif --example gif -- cat.gif 3
```
