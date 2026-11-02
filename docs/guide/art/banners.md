# Text banners

`Figlet` draws text in large letters made of ordinary characters, the way the
classic `figlet` program does. Use it for a title screen, a release banner or a
section heading that has to stand out in a wall of log output.

Banners need no Cargo features and no image decoders.

## The smallest example

```rust
--8<-- "crates/rich-art/examples/guide_banners.rs:quickstart"
```

![A FIGlet banner reading Hello](../../media/guide/guide_banners-quickstart.svg)

`console.print` takes any renderable; a `Figlet` is one. The output is
byte-identical to `pyfiglet` for the bundled `standard` font.

## Style

`.style()` paints every row of the banner with a `rich` style:

```rust
--8<-- "crates/rich-art/examples/guide_banners.rs:styled"
```

![A bold magenta banner](../../media/guide/guide_banners-styled.svg)

## Justification

`.justify()` positions the banner within the available width. The default is
`Justify::Left`.

```rust
--8<-- "crates/rich-art/examples/guide_banners.rs:justify"
```

![Left, centred and right-justified banners](../../media/guide/guide_banners-justify.svg)

## Width and wrapping

A banner lays out to the console's width. `.width(n)` overrides it. When a line
would not fit, the banner wraps onto another row of big letters, breaking at
spaces as `figlet` does:

```rust
--8<-- "crates/rich-art/examples/guide_banners.rs:wrap"
```

![A banner wrapped onto two rows at forty columns](../../media/guide/guide_banners-wrap.svg)

## Compose it with other renderables

A banner is a renderable like a `Text` or a `Table`, so it can go inside a
`Panel`, a `Table` cell or a `Layout`:

```rust
--8<-- "crates/rich-art/examples/guide_banners.rs:panel"
```

![A banner inside a rounded green panel](../../media/guide/guide_banners-panel.svg)

## Plain text

`to_text(width)` returns the banner as a `String`, with no console and no
styling. Use it for a file header, a `--version` message or a test:

```rust
--8<-- "crates/rich-art/examples/guide_banners.rs:to-text"
```

## Fonts

Fonts are FIGfont (`.flf`) files. The crate bundles one, `standard`, and uses
it by default. `FigletFont::parse` reads any other FIGfont from its text:

```rust
--8<-- "crates/rich-art/examples/guide_banners.rs:font"
```

![The Hi banner in the standard font](../../media/guide/guide_banners-font.svg)

The parser supports the FIGfont header, comment block, the required character
set and code-tagged characters (decimal, `0x` hex and octal). Layout supports
full width, kerning, controlled smushing rules 1–6, universal overlapping and
hardblanks. A malformed font is a `FontError`, not a panic.

## Animated banners

`rich_art::animate` (0.0.24) turns a banner into frames: `AnimatedBanner`
lays the text out once, and `frame(t)` gives the frame at time `t`, a grid
of characters with true-colour foregrounds. It is a pure function of the
banner and the time, with no clock and no terminal: a CLI prints frames on
a timer, a TUI draws them on its clock, a test snapshots any instant. At
and after its `duration` (two seconds by default) every frame is the still
one, `still()`, which is also what to show with no colour, with reduced
motion, or when output is not a terminal.

```rust
--8<-- "crates/rich-art/examples/guide_banners.rs:animated"
```

![A banner halfway through its glow, then its still frame, each over a wave with specks of light](../../media/guide/guide_banners-animated.svg)

The effects: `Glow` (dark letters, a soft band of blue-green light
travelling left to right through them and leaving them lit, like plankton
glowing where the water moves), `Shimmer` (a narrow highlight sweeping
across lit letters), `Typewriter` (a column at a time), `Fade` (dark to
lit) and `Still`. Colours come from a `BannerPalette`; through a console
they drop to 256 or 16 colours, or none, as its colour system needs.
`pulse(t)` gives a brightness from 0 to 1 that pulses while the
animation runs and stays at 1 once still, for a light drawn beside the
banner. intuiTUIve plays it on its clock with
[`banner::banner` and `banner::Play`](../intuituive/timers.md#an-animated-banner),
and `rich banner --effect`, `--wave` and `--specks` play it in the CLI.

## Gotchas

- Only the `standard` font ships with the crate. Other fonts are separate
  files; check their licences before you redistribute them.
- Banners are wide. At 80 columns the `standard` font fits roughly a
  dozen characters per row, so keep banner text short or let it wrap.
- `style` colours the whole banner. For a gradient or per-letter colours,
  build several banners or post-process `to_text`.

## See also

- [rich-art overview](index.md)
- [Images](images.md) — pictures rather than letters
- The `banner` example: `cargo run -p rs-rich-art --example banner -- "your text"`
