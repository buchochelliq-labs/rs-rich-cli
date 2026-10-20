# intuiTUIve's elephant

A purple elephant, drawn in the mark's language: the mark's deep purple
(`#7c4dff`) with a darker rim (`#5a32d6`), a light inside (`#b9a1ff`), ink
(`#1c1a24`) and a near-white lit cell (`#f4f1ff`). Its eyes are block
cursors, its toes are three light cells, and in its trunk it holds up the lit
cell from the mark: the one cell a signal redraws.

## Names

- **Memo**: an elephant never forgets, and `memo` is the signal that
  remembers a derived value until what it read changes. First choice.
- **Trunk**: the root of the retained tree is a trunk, and so is what it
  waves with.
- **Tui** (said "too-ee"): the middle of intuiTUIve, short enough for a
  tape caption.

## Personality

Memo is patient and precise: it remembers everything, redraws nothing it
does not have to, and carries one lit cell around as if it were a lantern.
It is friendly rather than goofy: it waves people in at the tutorial, holds
up a sign with a prompt on it for tips, scratches its chin at
troubleshooting, and throws confetti cells for a release. It never talks in
the docs; it shows up beside the text that does.

## The files

| File | What it is | For |
|---|---|---|
| `elephant-hero.svg` | Waving, with the lit cell and a soft floor | The intuiTUIve overview page |
| `elephant-wave.svg` | Waving | Welcome, getting started, the tutorial |
| `elephant-sign.svg` | Holding up a sign with a prompt | Tips and notes |
| `elephant-think.svg` | Trunk to its chin, a question above | Troubleshooting |
| `elephant-celebrate.svg` | Eyes closed, trunk up, confetti cells | Release notes |
| `elephant-32.svg` | Silhouette drawn for 32 px | Avatars, small badges |
| `elephant-16.svg` | Silhouette drawn for 16 px | Favicons, inline |
| `elephant-pixel.png` | 32x32 pixels, one pixel a cell | Source for a micro asset or a sprite |
| `elephant-pixel.txt` | The same grid as characters, with the palette | Source for a `pixels` buffer or a test |
| `elephant-pixel.svg` | The same grid as SVG runs | The pixel version in docs |
| `preview.html` | Every file on dark and light | Reviewing changes |

Every SVG works on the dark and the light scheme: the body is the accent,
and the light lit cell has a dark rim.

## In a terminal

The pixel version is 32x32 pixels with five colours and no
anti-aliasing. Drawn in half blocks (two pixels stacked in one cell), it is
32 columns by 16 rows. It reads at that size because the silhouette (trunk,
ear, legs) has gaps of background between its parts. Two ways to draw it
in intuiTUIve today, with no new code:

- `pixels(32, 32, |buf| ..)` fills a pixel buffer from
  `elephant-pixel.txt` or the PNG.
- `sprite_sheet_bytes(include_bytes!("elephant-pixel.png"), 32, 32)`
  (`art`) makes it a sprite for a `stage`.

## As a micro asset

A micro asset may be up to 4x2 cells, and at the usual 8x16-pixel cells a
4x2 asset is 32x32 pixels, which is exactly this image: it needs no
scaling. To make one:

```sh
rich micro create docs/assets/intuituive/mascot/elephant-pixel.png \
    --name intuituive/elephant --size 4x2 --fit stretch \
    --alt "purple elephant holding up a small white square with its trunk" \
    --emoji 🐘 --text Memo --license MIT --add
```

The name is namespaced (`intuituive/elephant`) because a bare `elephant` is
the emoji code `:elephant:`. The text fallback must fit the asset's columns
(four for 4x2). For the default 2x1 size (16x16 pixels), draw a 16x16 grid
by hand from `elephant-16.svg`: shrinking the 32x32 grid would lose the eyes.

To ship it with the project, there are two ways, and both need code
changes that have not been made yet:

1. **As a built-in asset.** Built-in assets are drawn in code in
   `crates/rich-micro/examples/gen_builtin.rs`, as shapes on a grid of 8x16
   units a cell. Add an `intuituive` pack (or add the elephant to `fun`)
   with this grid as its drawing, then run
   `cargo run -p rs-rich-micro --example gen_builtin`. The tests that check
   the committed files and the metadata (alt text, emoji, text, licence, no
   emoji-code name) then cover it.
2. **As a pack in the repo.** Put the package (`manifest.json` and
   `static.png`, as the command above writes) and a `pack.json` under, for
   example, `assets/micro/intuituive/`. Users install it with
   `rich micro install`. An intuiTUIve app then loads it with
   `micro_sprite(&asset)` (`micro`).

Either way, `:micro:intuituive/elephant:` would then work in console
markup, and a recording could show the elephant through the micro tapes.
