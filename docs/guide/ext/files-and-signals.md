# Files, spectra and warm-ups

Four small modules for terminal apps that show the outside world: a file
manager's icons, MIME types, times and owners; the spectrum of a sampled
signal; and syntax highlighting warmed up before the first frame needs it.
intuiTUIve's Yazi and scope-tui rebuilds use them, but none needs
intuiTUIve.

## File-type icons and MIME types

`rich_ext::filetype` (no feature, no dependencies) gives the glyph and
colour [Yazi](https://github.com/sxyazi/yazi) shows by default for a file:
its `theme-dark.toml` and `theme-light.toml` presets, whose glyphs and
colours come from nvim-web-devicons.

```rust
use std::path::Path;
use rich_ext::filetype::{icon_for, icon_with, Appearance, FileFlags};

let (glyph, colour) = icon_for(Path::new("main.rs"), false);
let line = format!("[{colour}]{glyph}[/] main.rs"); // console markup

// Links, devices, executables, the hovered folder, the light preset:
let flags = FileFlags::of(path)?;
let icon = icon_with(path, flags, Appearance::Light);
```

The lookup is Yazi's: a directory by its name; a file by its name, then its
last extension (`gz` for `a.tar.gz`), each exactly and then in lower case;
then the conditions (an orphan link, a link, a device, a directory, an
executable, any file).

- **Type colours:** `style_for(mime, flags)` is Yazi's `[filetype]` rules as
  a rich style: images `yellow`, audio and video `magenta`, archives `red`,
  documents `cyan`, an orphan link `on red`, an executable `green`, a
  directory `blue`.
- **MIME types:** `mime_for_path(path)` goes by the extension (about a
  hundred common ones), `mime_from_bytes(head)` by magic numbers (images,
  audio and video containers, archives, PDF, fonts, SQLite, WebAssembly,
  ELF, Mach-O and PE), and `mime_for(path)` reads at most 512 bytes and
  tries both, answering `inode/directory`, `inode/x-empty`, `text/plain` or
  `application/octet-stream` when nothing else fits.
- **Nerd Fonts:** the glyphs need one. No terminal reports its font, and the
  glyphs are in the Private Use Area, so a cursor-position probe measures
  them as one cell either way: nothing can *detect* the font. `nerd_font()`
  says what can be known: `RICH_NERD_FONT` (or `NERD_FONT`) set to `1` or
  `0`, else whether a Nerd Font file is installed in the usual font
  directories (which the terminal may not use, and which a session over SSH
  cannot see). `nerd_font().likely()` is the yes or no.

## Local times and owners

`rich_ext::fs` fills a listing's other columns.

```rust
use rich_ext::fs::{group_name, short_local_time, user_name};

let meta = std::fs::metadata(path)?;
let when = short_local_time(meta.modified()?); // `10/18 14:05`, or `10/18  2024`
#[cfg(unix)]
let owner = {
    use std::os::unix::fs::MetadataExt;
    format!("{}:{}", user_name(meta.uid()).unwrap_or_default(), group_name(meta.gid()).unwrap_or_default())
};
```

- `local_time(time, "%Y-%m-%d %H:%M")` formats with `strftime` codes in the
  local time zone; `short_local_time` is `ls -l`'s and Yazi's short form.
  Both need the off-by-default `local-time` feature (chrono with its
  `clock`, reading the zone through iana-time-zone).
- `user_name(uid)` and `group_name(gid)` look the system's user database up
  (`getpwuid_r`, `getgrgid_r`; no extra dependency) and remember each
  answer. Off Unix they return `None`.

## FFT and spectrum

`rich_ext::dsp` (no feature, no dependencies) has a fast Fourier transform
of any length (radix 2 for a power of two, Bluestein's chirp transform
otherwise), the Hann window, and a magnitude spectrum:

```rust
use rich_ext::dsp::{hann, spectrum};

// (Hz, magnitude) from 0 Hz to the Nyquist frequency, rate / n apart.
let bins = spectrum(&hann(&samples), 48_000.0);
```

`fft(&samples)` returns the complex bins as `(re, im)` pairs, and
`magnitude(bin)` their size. `spectrum_full(samples, rate)` gives every
bin, the mirrored half above the Nyquist frequency too, as scope-tui plots
them. intuiTUIve's `audio::input` (its `audio` feature) gives the samples
to feed it, a buffer at a time.

## Test signals, triggers and notes

The same module makes signals to test a scope with, and finds what one
shows:

```rust
use rich_ext::dsp::{mix, noise, note_hz, partials, tone, trigger, Edge};

let rate = 48_000.0;
let a4 = note_hz("A4").unwrap(); // 440.0
let mut signal = mix()
    .add(0.6, tone(a4, rate))
    .add(0.3, partials(a4 * 1.5, rate, &[1.0, 0.5, 0.25]))
    .add(0.05, noise(7).low_pass(300.0, rate));
let buffer: Vec<f64> = signal.by_ref().take(2048).collect();
// Where to start drawing, so the wave stands still.
let start = trigger(&buffer, 0.0, 4, Edge::Rising).unwrap_or(0);
```

- **`tone(hz, rate)`** is an endless sine (`.amplitude(a)`, `.phase(rad)`)
  whose phase carries on from buffer to buffer; **`partials(hz, rate,
  &amplitudes)`** a note with its overtones, those at or past the Nyquist
  frequency left out; **`noise(seed)`** white noise, the same for the same
  seed, and `.low_pass(hz, rate)` smooths it into a wander. **`mix()`**
  adds sources, each with a gain; all are iterators of samples.
- **`trigger(samples, level, depth, edge)`** finds where the samples
  cross `level` (`Edge::Rising` or `Falling`) and stay past it for
  `depth` samples: an oscilloscope's trigger.
- **`note_hz("C#3")`** is a note's frequency in equal temperament, A4 at
  440 Hz (sharps `#`, flats `b`, octaves -1 to 9, 4 when left out), and
  **`nearest_note(hz)`** the note nearest a frequency with how many cents
  off it is: a tuner's reading.

## Warming up syntax highlighting

Core's default highlighter loads syntect's grammars and themes the first
time anything is highlighted, and compiles a grammar's patterns the first
time they are tried. In a debug build that first highlight takes seconds,
so an interactive app's first Markdown preview stalls. `rich_ext::syntax`
(with the default `syntax` feature) does that work on a worker thread:

```rust
let warming = rich_ext::syntax::preload(&["rust", "markdown", "toml"]);
// … build the app; the first preview finds the grammars loaded.
```

`preload` returns at once with a `Preload` (`is_finished()`, `wait()`);
`preload_with(highlighter, languages, themes)` warms another
[highlighter](code-highlighters.md) and more themes. It works through the
highlighter's public API, so core is unchanged: the state it warms is the
highlighter's own, shared by every `Syntax` and `Markdown` in the process.
