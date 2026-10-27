# Noctiluca's iconography: the bloom and the lanternfish

*Noctiluca scintillans* is the "sea sparkle" plankton that makes waves glow
at night. Noctiluca the browser draws the web in a terminal, so its logo is
a bloom of small terminal windows, linked like cells in a colony and
glowing in dark water. Its mascot is a lanternfish whose lamp is a small
terminal window held out on its lure.

## Palette

Blue and cyan, the colours of bioluminescence. They sit beside
intuiTUIve's purple (`#7c4dff`) without fighting it: Memo and the
lanternfish share a picture in `memo-amazed.svg`.

| Name | Colour | Used for |
|---|---|---|
| water | `#0b1a2b` | the logo's tile, the fish's eye, ink |
| screen | `#10304d` | a terminal window's inside |
| body | `#2a7fd8` | the fish's body |
| rim | `#1d5fae` | the fish's back, fins, tail and lure stalk |
| light | `#9fe6ff` | the fish's belly and eye ring, plain windows' rims |
| glow | `#3ff0e0` | the lit window's rim, the photophores, sparkle |
| lit | `#eafffd` | the cursor cell, the eye's highlight |

## Drawing rules

- Every terminal window has a rim, a title bar with three dots where it is
  big enough, and a prompt cell: a block cursor, sometimes after a `>`.
- One window is lit: the logo's centre one, the fish's lure. It has the
  glow rim and a soft halo; the others have the light rim.
- The fish's eye is a block cursor in a light ring, as Memo's eyes are
  block cursors. Its photophores are five glow cells along the belly, as
  Memo's toes are three light cells.
- It faces left, like Memo, and never talks.

## Dark and light

The logo carries its own dark water, so it reads the same on both
schemes. The fish's body is a mid blue that keeps contrast on white and
on the slate scheme; its lit cells (the photophores and the 16 px lure)
have a rim in the rim colour, so they stay visible on white.

## Names

- **Noctiluca**: the plankton, and the browser.
- **The lanternfish**: unnamed for now. A lanternfish (a myctophid) lives
  in the deep and makes its own light; this one's light is a terminal.

## The files

| File | What it is | For |
|---|---|---|
| `logo.svg` | The bloom: a lit terminal in the centre, six around it, four small ones further out, linked, on dark water (128x128) | The docs page, the README, release notes |
| `favicon.svg` | The bloom drawn for 16 px: one window and four linked cells | Favicons, inline badges |
| `lanternfish-hero.svg` | The lanternfish, its lure a terminal with a prompt | The docs page's introduction |
| `lanternfish-tip.svg` | Holding its lure up, a prompt and a line of text on it, looking at it | Tips and notes |
| `lanternfish-lost.svg` | Puzzled, its lure drooping and flickering a question mark | Troubleshooting |
| `lanternfish-32.svg` | Silhouette drawn for 32 px | Avatars, small badges |
| `lanternfish-16.svg` | Silhouette drawn for 16 px: the lure is one glowing cell | Favicons, inline |
| `memo-amazed.svg` | Memo, intuiTUIve's elephant, wide-eyed at a web page drawn in terminal cells, ears up, trunk raised, the lanternfish swimming out of the screen towards her (256x128) | The docs page's introduction |
| `lanternfish-pixel.png` | 32x32 pixels, one pixel a cell | Source for a micro asset or a sprite |
| `lanternfish-pixel.txt` | The same grid as characters, with the palette | Source for a `pixels` buffer or a test |
| `lanternfish-pixel.svg` | The same grid as SVG runs | The pixel version in docs |
| `preview.html` | Every file on dark and light | Reviewing changes |
| `promo.mp4` | Noctiluca's promo: the bloom on a night sea, glowing plankton, a network of terminal windows, the title. AI-generated, made by the maintainer for rs-rich, MIT. 960x540, H.264 and AAC, 36 s | The docs page's introduction, the READMEs |
| `promo-poster.jpg` | The promo's title frame (33 s) | The video's poster |

The promo's five seconds of the bloom are also Noctiluca's video fixture
`crates/noctiluca/tests/fixtures/media/bloom.webm`, cut by `make_bloom.py`
there.

Memo in `memo-amazed.svg` is drawn from her own files in
`docs/assets/intuituive/mascot/`, with her palette: her hero's body, her
celebrating pose's raised trunk, and wider eyes. When Memo changes there,
change her here too.

## In a terminal

The pixel version is 32x32 pixels with six colours and no anti-aliasing.
Drawn in half blocks (two pixels stacked in one cell), it is 32 columns by
16 rows. Its lure is a 9x7 window with a 2x2 cursor, the smallest that
still reads as a terminal. Two ways to draw it in intuiTUIve today, with
no new code:

- `pixels(32, 32, |buf| ..)` fills a pixel buffer from
  `lanternfish-pixel.txt` or the PNG.
- `sprite_sheet_bytes(include_bytes!("lanternfish-pixel.png"), 32, 32)`
  (`art`) makes it a sprite for a `stage`.

As a micro asset it is 4x2 cells at 8x16-pixel cells, like Memo's:

```sh
rich micro create docs/assets/noctiluca/lanternfish-pixel.png \
    --name noctiluca/lanternfish --size 4x2 --fit stretch \
    --alt "blue lanternfish holding out a small glowing terminal window on its lure" \
    --emoji 🐟 --text fish --license MIT --add
```

The bloom has no pixel version yet. The design note's
[Name and mascot](../../design/browser.md#name-and-mascot) says how it
could be drawn with rs-rich-diagram and animated as a micro asset.
