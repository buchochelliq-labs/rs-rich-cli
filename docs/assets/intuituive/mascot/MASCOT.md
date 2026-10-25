# intuiTUIve's elephants: Memo and Tui

Memo is a purple elephant, drawn in the mark's language: the mark's deep purple
(`#7c4dff`) with a darker rim (`#5a32d6`), a light inside (`#b9a1ff`), ink
(`#1c1a24`) and a near-white lit cell (`#f4f1ff`). Her eyes are block
cursors, her toes are three light cells, and in her trunk she holds up the lit
cell from the mark: the one cell a signal redraws.

## Names

- **Memo** (she): an elephant never forgets, and `memo` is the signal that
  remembers a derived value until what it read changes.
- **Tui** (said "too-ee"), Memo's calf: the middle of intuiTUIve, and
  short enough for a tape caption.

## Personality

Memo is patient and precise: she remembers everything, redraws nothing she
does not have to, and carries one lit cell around as if it were a lantern.
She is friendly rather than goofy: she waves people in at the tutorial, holds
up a sign with a prompt on it for tips, scratches her chin at
troubleshooting, and throws confetti cells for a release. She never talks
in the docs; she shows up beside the text that does.

Tui is Memo's calf: a bigger head, shorter legs, two toes a foot, and
eyes a size too large. Where Memo holds up a lit cell, Tui carries a cursor
(an underscore): the cell that is not filled in yet. Tui suits beginners'
pages, first steps and small wins, and in the pair's picture follows Memo
holding her tail in a curled trunk, as calves do.

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
| `tui.svg` | Tui, holding a cursor | Beginners' pages, first steps |
| `memo-and-tui.svg` | Memo waving, Tui holding her tail (224x128) | The tutorial's start, the guide's front page |
| `elephant-pixel.png` | 32x32 pixels, one pixel a cell | Source for a micro asset or a sprite |
| `elephant-pixel.txt` | The same grid as characters, with the palette | Source for a `pixels` buffer or a test |
| `elephant-pixel.svg` | The same grid as SVG runs | The pixel version in docs |
| `crates/*.svg` | One spot per crate (160x128), see [Crate spots](#crate-spots) | Crate READMEs and crate lists |
| `preview.html` | Every file on dark and light | Reviewing changes |

Every SVG works on the dark and the light scheme: the body is the accent,
and the light lit cell has a dark rim.

## Crate spots

`crates/` holds one small spot illustration per crate: Memo in the waving
pose, at the right of a 160x128 view box, with a prop at the left that says
what the crate does. They are meant for 96 to 160 px beside a heading (each
crate's README shows its spot at 96 px), not as hero pictures. They carry no
text: the caption belongs to the page. Terminals are ink with a light rim,
so they stay visible on a dark page, and a prompt's cursor is the lit cell.
Besides the house palette, props use four accents: mint `#2dd4bf`, amber
`#fbbf24`, coral `#fb7185` and blue `#60a5fa`.

| File | Crate | What the prop says |
|---|---|---|
| `rs-rich.svg` | `rs-rich` | She holds up a terminal with a `>_` prompt: styled output in the terminal |
| `rs-rich-cli.svg` | `rs-rich-cli` | Brush in her trunk, a palette at her feet, a table and progress bars on the terminal: the `rich` command paints files and data |
| `rs-rich-ext.svg` | `rs-rich-ext` | She fits a puzzle piece into a terminal: extensions slot into the core |
| `rs-rich-plugin-api.svg` | `rs-rich-plugin-api` | She joins a plug to a socket, eyes shut at the spark: the plugin contract |
| `rs-rich-macros.svg` | `rs-rich-macros` | A rubber stamp over `{ }` and a tick: proc-macros stamp out checked code |
| `rs-rich-art.svg` | `rs-rich-art` | She paints a pixel heart on an easel: images and FIGlet art |
| `rs-rich-mermaid.svg` | `rs-rich-mermaid` | A flowchart of three boxes, and a fish tail on her trunk tip: Mermaid diagrams |
| `rs-rich-diagram.svg` | `rs-rich-diagram` | A graph of small boxes and straight edges inside a terminal: graphs laid out in box drawing |
| `rs-rich-lumis.svg` | `rs-rich-lumis` | A torch lights coloured lines of code: syntax highlighting |
| `rs-rich-record.svg` | `rs-rich-record` | A video camera aimed at a terminal with a red record dot: tapes |
| `rs-rich-interact.svg` | `rs-rich-interact` | She ticks one checkbox in a list: interactive components |
| `rs-rich-micro.svg` | `rs-rich-micro` | Tweezers place a tiny flower over a cell grid: micro assets |
| `rs-rich-data.svg` | `rs-rich-data` | She places the last cell of a coloured table: tabular data |
| `rs-rich-ratatui.svg` | `rs-rich-ratatui` | She stands on a bridge between two terminals: ratatui interop |
| `rs-rich-intuituive.svg` | `rs-rich-intuituive` | Memo and Tui beside a bar chart and a donut panel: the TUI framework |
| `rs-rich-web.svg` | `rs-rich-web` | One small terminal sent out to three browser windows: serving to a browser |
| `rs-rich-embed.svg` | `rs-rich-embed` | She slots a terminal pane into a bigger app window: a terminal inside an app |
| `rs-rich-marketplace.svg` | `rs-rich-marketplace` | A stall of plugin boxes, one with a seal: the signed plugin index |
| `rs-rich-py.svg` | `rs-rich` on PyPI (`crates/rich-py`) | A friendly two-colour snake, and the lit cell between them: Python bindings |

Two pairs are easy to confuse, so they are drawn apart on purpose. Mermaid
is coloured flowchart boxes in the open with a fish tail; the diagram is a
box-drawing graph inside a terminal. The web spot sends one terminal out to
several browser windows; the embed spot puts one pane into one app window.

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
