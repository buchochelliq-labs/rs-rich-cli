# rich-art

ASCII-art renderables for [`rich`](../rich) — currently **FIGlet-style text
banners**, in the spirit of `figlet(6)` / `pyfiglet`.

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

## Example

```bash
cargo run -p rich-art --example banner -- "your text"
```
