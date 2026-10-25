# rs-rich-record

<img src="https://raw.githubusercontent.com/buchochelliq-labs/rs-rich-cli/main/docs/assets/intuituive/mascot/crates/rs-rich-record.svg" width="96" align="right" alt="Memo the elephant filming a terminal with a video camera">

Record scripted terminal sessions (**tapes**) into screenshots, asciinema
casts, GIF and MP4. Part of [rs-rich](https://github.com/buchochelliq-labs/rs-rich-cli),
the Rust port of Python's `rich`. This crate is an rs-rich addition, not a port.

A tape scripts a session, one step per line:

```text
Set Size 90x22
Set Title "rich --watch · re-render on save"
Write service.json '{"replicas": 2}'
Type "rich --watch service.json"
Enter
Wait /"replicas": 2/
Screenshot before
Exec "sed -i 's/2/5/' service.json"
Wait /"replicas": 5/
Screenshot after
Ctrl+C
```

The recorder runs it in `bash` on a real PTY with a pinned environment, follows
the screen with a VT emulator, and writes:

- per `Screenshot`: a PNG in a window frame, an SVG with selectable text (drawn
  by rs-rich-ext's frame exporter, like rich's other SVG output), and a text
  grid;
- an asciinema v2 cast, with input events and the palette in its header;
- a GIF with a key overlay, an MP4 when FFmpeg is installed, and an
  animated PNG (APNG) when asked: every settled screen update, dropped to
  the tape's frame rate (`Set Framerate 35`, `--fps`) and with identical
  frames merged;
- a self-contained HTML page: a small player (no CDN, no terminal emulator)
  and the screenshots as selectable text.

`Set WindowFrame off`, `Set Caption "…"` and `Set KeyOverlay off` change how
stills and video are presented, and `Output gif png` (or `Output demo.html`)
chooses what a tape writes; a tape without an `Output` line writes every
format but the APNG, which is written only when an `Output` or `--format`
names it.

## Games and graphics

The emulator follows the images a program draws, on the cell grid: the Kitty
graphics protocol (PNG or raw RGB/RGBA, zlib or not, chunked; placements at
the cursor and Unicode placeholder cells, deletes), Sixel and iTerm2 inline
images (PNG, GIF). PNG, GIF, APNG and video frames draw them over their cells;
text grids and SVGs show the cells under them. A recording keeps every frame
packed (about 12 bytes a cell, unchanged rows shared, images kept compressed):
30 s at 35 frames a second at 100x30 is a few tens of megabytes.

A tape can hold keys, for games:

```text
Set Framerate 35           # video frames a second (12)
Set FontSize 10            # text size of video frames (16)
Set KittyKeyboard on       # tell the program the kitty keyboard protocol is there
Set KeyRepeat 200ms 33ms   # a held key's autorepeat
KeyDown Up                 # hold a key (or one character: KeyDown w) ...
Sleep 1s
KeyUp Up                   # ... and let it go
Hold f 2s                  # both, with a wait between
Output gif apng
```

A held key autorepeats, as a keyboard does. With `Set KittyKeyboard on` the
emulator answers the protocol's query and follows the flags the program
pushes: presses, repeats and releases are then sent as the protocol's escapes
(`CSI 1;1:3A` for a released Up), so a game hears exactly how long a key is
held; without it, a release sends nothing, as on a legacy terminal.

`check` compares a new run's text grids with committed ones, so documentation
media cannot drift from the program it shows.

```rust,no_run
use rich_record::{record, tape};

let tape = tape::parse(&std::fs::read_to_string("demo.tape")?)?;
let recording = record::record(&tape, "demo", &record::Options::default())?;
let problems = record::check(&recording, "media/demo".as_ref());
assert!(problems.is_empty(), "{problems:?}");
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Using the recorder without the CLI

The `rich record` command ships with `rs-rich-cli`; this crate is the library
behind it, and needs nothing else. A program that records a tape and writes
the files its `Output` lines ask for:

```toml
[dependencies]
rs-rich-record = "0.0.8"
```

```rust,no_run
use rich_record::record::{self, Options};
use rich_record::render::raster::Fonts;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let tape = rich_record::tape::parse(&std::fs::read_to_string("demo.tape")?)?;
    let recording = record::record(&tape, "demo", &Options::default())?;
    // The tape's `Output` formats, or every format but the APNG if it has
    // none. `Formats::ALL` would add the APNG to a tape without `Output`.
    // `record::write` instead takes formats as given.
    let written = record::write_selected(
        &recording,
        "media/demo".as_ref(),
        "demo",
        recording.default_formats(),
        &Fonts::embedded(),
        &Default::default(),
        None,
    )?;
    for path in &written.paths {
        println!("wrote {}", path.display());
    }
    // An MP4 needs FFmpeg: without it the write succeeds and lists the video
    // here, so check before relying on it.
    if !written.is_complete() {
        for skipped in &written.skipped {
            eprintln!("skipped {}: {}", skipped.path.display(), skipped.reason);
        }
        std::process::exit(1);
    }
    Ok(())
}
```

From the shell, the `rich` CLI (`rs-rich-cli`) has `rich record TAPE…`.

## A PNG of rendered output

The rasteriser also draws output that never ran on a PTY: the segments a
`Console` recorded, or a rs-rich-ext `Frame`, in the screenshots' font,
theme and window frame. The same output gives the same bytes everywhere.
The `rich` CLI's `--export-png PATH` is this.

```rust,no_run
use rich::{Console, Text};
use rich_record::render::png::{segments_png, PngOptions};
use rich_record::render::raster::Fonts;

let console = Console::builder().width(40).build();
let segments = console.record_output(|c| c.print(&Text::new("hello")));
let options = PngOptions { title: "hello", columns: Some(40), ..PngOptions::default() };
let png = segments_png(&segments, &Fonts::embedded(), &options)?;
std::fs::write("hello.png", png)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

Linux and macOS are supported. Windows builds through ConPTY but needs `bash`
on `PATH`, and is experimental.

Text in PNG and GIF output uses DejaVu Sans Mono, embedded in the crate under
the Bitstream Vera licence (see `fonts/LICENSE-DejaVu`). Emoji are drawn in
colour from Twemoji, also embedded, under CC BY 4.0 (see
`fonts/LICENSE-Twemoji`). Each emoji cluster (ZWJ sequences, skin tones,
flags, ❤️, keycaps) takes one cell as wide as rich measures it, and box-drawing, block and braille characters are drawn as
geometry, so borders join and block images have no seams.
