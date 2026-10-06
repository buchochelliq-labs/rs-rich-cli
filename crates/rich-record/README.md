# rs-rich-record

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
- a GIF with a key overlay, and an MP4 when FFmpeg is installed;
- a self-contained HTML page: a small player (no CDN, no terminal emulator)
  and the screenshots as selectable text.

`Set WindowFrame off`, `Set Caption "…"` and `Set KeyOverlay off` change how
stills and video are presented, and `Output gif png` (or `Output demo.html`)
chooses what a tape writes.

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
rs-rich-record = "0.0.4"
```

```rust,no_run
use rich_record::record::{self, Formats, Options};
use rich_record::render::raster::Fonts;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let tape = rich_record::tape::parse(&std::fs::read_to_string("demo.tape")?)?;
    let recording = record::record(&tape, "demo", &Options::default())?;
    // The tape's `Output` formats (all of them if it has none); `Formats::ALL`
    // adds no further limit. `record::write` instead takes formats as given.
    let written = record::write_selected(
        &recording,
        "media/demo".as_ref(),
        "demo",
        Formats::ALL,
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

Linux and macOS are supported. Windows builds through ConPTY but needs `bash`
on `PATH`, and is experimental.

Text in PNG and GIF output uses DejaVu Sans Mono, embedded in the crate under
the Bitstream Vera licence (see `fonts/LICENSE-DejaVu`). Emoji are drawn in
colour from Twemoji, also embedded, under CC BY 4.0 (see
`fonts/LICENSE-Twemoji`). Each emoji cluster (ZWJ sequences, skin tones,
flags, ❤️, keycaps) takes one cell as wide as rich measures it, and box-drawing, block and braille characters are drawn as
geometry, so borders join and block images have no seams.
