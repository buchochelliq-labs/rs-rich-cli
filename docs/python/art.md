# Art: images, FIGlet, GIFs and image diffs

```python
from rs_rich import art
```

`rs_rich.art` is the [`rs-rich-art`](../guide/art/index.md) crate
from Python. Rich has no counterpart, so the API follows the Rust one: each
builder method is a keyword argument and each enum a lowercase name (the
`rich` command line's spelling). Everything renders in Rust, and every class
here is a renderable: print it, or put it in a `Table` cell or a `Panel`.

The submodules mirror the crate's modules: `rs_rich.art.image_art`,
`ascii`, `block`, `braille`, `quadrant`, `sixel`, `figlet`, `gif`, `stage`
and `imagediff`. `rs_rich.art` itself exports everything.

## Images

An image argument is any of:

- a path (`str` or `os.PathLike`) to a PNG, JPEG or GIF file;
- the encoded bytes (`bytes`, `bytearray` or `memoryview`);
- an [`ArtImage`](#artimage), a decoded image;
- a Pillow image. Pillow is not a dependency: `rs_rich` reads the image's
  RGBA bytes (`image.convert("RGBA").tobytes()`), so any object with
  `mode`, `size`, `convert` and `tobytes` works.

A missing file raises `FileNotFoundError` (or another `OSError`), and bytes
that are not an image raise `ImageDecodeError`.

The examples on this page draw a gradient built from raw pixels:

```python
from rs_rich import art
from rs_rich.console import Console

def gradient(w, h):
    pixels = bytearray()
    for y in range(h):
        for x in range(w):
            pixels += bytes((x * 255 // (w - 1), y * 255 // (h - 1), 128, 255))
    return art.ArtImage.frombytes("RGBA", (w, h), pixels)

image = gradient(32, 16)
console = Console(width=40, color_system=None)
console.print(art.ImageArt(image, mode="ascii", width=32))
```

```text
          ............'''',,,,;;
   ............'''',,,,;;;::::cc
.......''',,,,;;;;::::cccclllloo
'',,,,;;;;::::ccccllllooooddddxx
;;::::ccccllllooooddddxxxxkkkkOO
ccllllooooddddxxxxkkkOOOO0000KKK
oddddxxxxkkkkOOOO0000KKKKXXXXNNN
xxkkkkOOOO0000KKKKXXXXNNNNWWWWMM
```

### ImageArt

```text
ImageArt(image, *, mode="auto", width=None, height=None, fit=None,
         anchor="center", background=None, color=False,
         color_mode="truecolor", dither=None, color_distance="rgb",
         rotate=0, flip_horizontal=False, flip_vertical=False,
         grayscale=False, brightness=1.0, contrast=1.0, gamma=1.0,
         max_width=None, max_height=None, options=None)
```

| Argument | Values |
|---|---|
| `mode` | `"auto"` (ASCII without colour, half-blocks with it, Sixel where the terminal looks able), `"ascii"`, `"blocks"` (also `"half-block"`), `"braille"`, `"quadrants"` or `"sixel"`. |
| `width`, `height` | Columns and rows (default: the console width, and the image's aspect). |
| `fit` | `None`, `"contain"`, `"cover"` or `"stretch"` (these need `width` and `height`), or `"native"`: the image's own pixels at the mode's density, only ever shrunk. |
| `anchor` | What `"cover"` keeps: `"center"`, `"top"`, `"bottom"`, `"left"`, `"right"`, `"top-left"`, `"top-right"`, `"bottom-left"`, `"bottom-right"`. |
| `background` | What transparency becomes: `None`, a colour (`"#336699"`, `"red"`, `(r, g, b)`), `"default"` (the terminal's own background) or `"checkerboard"`. |
| `color` | Colour ASCII cells with their pixel. |
| `color_mode` | `"truecolor"`, `"ansi256"`, `"ansi16"` or `"grayscale"`. |
| `dither` | `None`, `"floyd-steinberg"`, `"bayer4x4"` or `"atkinson"`; needs a reduced `color_mode`. |
| `color_distance` | `"rgb"` or `"oklab"`; needs a reduced `color_mode`. |
| `rotate` | Clockwise degrees: a multiple of 90. |
| `flip_horizontal`, `flip_vertical`, `grayscale` | Transforms applied before fitting. |
| `brightness`, `contrast`, `gamma` | Adjustments; `1.0` leaves the image alone. |
| `max_width`, `max_height` | Caps in columns and rows. |
| `options` | An `ImageOptions(mode, width, height, color)`, which replaces those four arguments. |

Each argument is also a read-only attribute (`art.mode`, `art.fit`, ...).
Unknown names raise `ValueError`.

```python
console.print(art.ImageArt(image, mode="braille", width=16))
console.print(art.ImageArt(image, mode="ascii", fit="cover", width=12, height=3, anchor="left"))
```

```text
⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
⠀⠀⠀⠀⠀⠀⠀⠀⠀⣀⣀⣤⣤⣶⣶⣿
⠀⣀⣀⣤⣤⣶⣶⣿⣿⣿⣿⣿⣿⣿⣿⣿
⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿
   ....',;:c
',;:cllodxkO
odxkO0KXXNWM
```

`resolve_mode` says what `"auto"` picks for a console's capabilities, and
`native_grid` the cells `fit="native"` takes:

```python
auto = art.ImageArt(image)
print(auto.resolve_mode(art.RenderCapabilities(color=True)))
print(auto.resolve_mode(art.RenderCapabilities.from_console(console)))
print(auto.native_grid("braille", 80))
```

```text
blocks
ascii
(16, 4)
```

### Errors

What cannot be rendered raises `ImageArtError`, whose `kind` says why:
`"invalid_adjustment"`, `"unsupported_color_options"` and
`"invalid_fit_dimensions"` when the `ImageArt` is built;
`"non_terminal_destination"`, `"sixel_too_large"` and
`"sixel_encode_failed"` when it is printed. Printing is strict, like the
crate's `ImageArt::render`: Sixel to a file is an error, not a silent ASCII
fallback.

```python
try:
    console.print(art.ImageArt(image, mode="sixel"))
except art.ImageArtError as error:
    print(error.kind)
try:
    art.ImageArt(image, dither="bayer4x4")
except art.ImageArtError as error:
    print(error)
```

```text
non_terminal_destination
Braille images are monochrome, so they take no color mode; dithering and color distance require ansi256, ansi16 or grayscale
```

### Single backends

The crate's backends are renderables of their own, with fewer options:

| Class | Arguments and methods |
|---|---|
| `AsciiArt(image, *, width, height, ramp, invert, color, normalize=True)` | `columns(available)`, `to_text(width)`; `DEFAULT_RAMP` is the default ramp. |
| `BlockArt(image, *, width, height)` | Half-blocks. |
| `BrailleArt(image, *, width, height)` | `to_text(width)`. |
| `QuadrantArt(image, *, width, height)` | Quadrant blocks. |
| `SixelArt(image, *, width, height, cell_px=(8, 16), max_colors=256)` | `encode(available)` returns the escape sequence (or `None`). `SIXEL_MAX_PIXELS` caps its raster. |

```python
print(art.AsciiArt(image, height=2, ramp=" .:#").to_text(16))
```

```text
     ..........:
..:::::::::#####
```

`sixel_is_probably_supported()` is the crate's guess from `RICH_GRAPHICS`,
`RICH_SIXEL` and the terminal's name.

### ArtImage

A decoded image: `ArtImage(source)` takes any image argument, and
`ArtImage.open(path)`, `ArtImage.from_bytes(data)`, `ArtImage.from_pil(image)`
and `ArtImage.frombytes(mode, size, data)` (mode `"RGBA"`, `"RGB"`, `"LA"`
or `"L"`) name the source. It has `width`, `height`, `size` and `mode`, and
gives its pixels back with `tobytes()` (its own mode), `to_rgba()`,
`to_png()`, `save(path)` (PNG, JPEG or GIF by extension) and `to_pil()`
(needs Pillow).

## FIGlet

```text
Figlet(text, *, font=None, justify="left", style=None, width=None)
```

A banner in a FIGfont, laid out to the console width (or `width`), wrapping
onto more banner rows as `figlet` does. `FigletFont()` is the bundled
`standard` font; `FigletFont.parse(source)` and `FigletFont.from_path(path)`
read `.flf` files (a bad one raises `FigletFontError`).
`figlet_render(text, font=None, width=80, justify="left")` and
`Figlet.to_text(width)` give the plain text.

```python
console.print(art.Figlet("rs", justify="center"))
```

```text
                         
                _ __ ___ 
               | '__/ __|
               | |  \__ \
               |_|  |___/
                         
```

## GIFs

```text
AnimatedArt(source, *, width=None, height=None, ramp=None, invert=False,
            color=False, blocks=False, color_mode="truecolor", dither=None,
            color_distance="rgb", repeat=None, max_fps=None)
```

`source` is a GIF's path or bytes. `repeat` is a number of passes (`None`: once) or
`"forever"`. Printed, an animation shows its first frame. It has
`frame_count` (and `len()`), `duration` and `frame_delay(i)` in seconds,
`frame(i)` (as `AsciiArt`) and `render_frame(i)` (a `GifFrame`: half-blocks
when `blocks=True` on a colour terminal, else ASCII).

`play(console=None)` animates it in place on a terminal (on a file it prints
the first frame once), blocking until it is done; Ctrl-C stops it and shows
the cursor again. To drive a display yourself, `frames()` gives each
`(GifFrame, delay)`:

```py
import time
from rs_rich.live import Live

animation = art.AnimatedArt("spinner.gif", width=30, color=True, blocks=True)
with Live(animation.render_frame(0)) as live:
    for frame, delay in animation.frames():
        live.update(frame)
        time.sleep(delay)
```

`Stage(*arts, gap=2, until=None)` plays several animations side by side,
each on its own clock; `add(art)` appends one, and `until` is a number of
seconds (`None`: until all have finished). `stage.play(console=None)` plays
it. `show_cursor_sequence()` is the escape sequence that shows the cursor,
for a signal handler of your own.

## Image diffs

`image_diff(before, after, settings=None, *, blur=6.0, threshold=60.0,
open_kernel=11, min_region=400, top=3)` compares two images of the same size
perceptually (CIELAB ΔE, blurred and denoised) and returns a `DiffReport`:
`changed_fraction`, `naive_changed_fraction`, `mean_delta_e`,
`max_delta_e`, `delta_e` (one value per pixel), the ranked `regions`
(`DiffRegion`: `x`, `y`, `width`, `height`, `area_px`, `share_of_change`,
`mean_delta_e`), and `heatmap()` and `highlight(after)` as `ArtImage`s.
`DiffSettings` holds the same settings. Images of different sizes raise
`ImageDiffError` (`kind == "size_mismatch"`).

```python
before = gradient(48, 40)
pixels = bytearray(before.to_rgba())
for y in range(10, 20):
    for x in range(12, 24):
        pixels[(y * 48 + x) * 4 : (y * 48 + x) * 4 + 3] = b"\xff\x00\x00"
after = art.ArtImage.frombytes("RGBA", (48, 40), pixels)
report = art.image_diff(before, after, blur=2.0, threshold=30.0, min_region=20, open_kernel=3)
print(f"{report.changed_fraction:.3f}", len(report.regions))
region = report.regions[0]
print(region.x, region.y, region.width, region.height)
```

```text
0.081 1
11 9 14 12
```
