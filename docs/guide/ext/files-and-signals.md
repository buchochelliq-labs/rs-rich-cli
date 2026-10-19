# Files, spectra and warm-ups

Modules for terminal apps that show the outside world: a file manager's
icons, MIME types, times and owners, listings, openers, file operations,
trash, git status and archives; the spectrum of a sampled signal; and
syntax highlighting warmed up before the first frame needs it.
intuiTUIve's Yazi and scope-tui rebuilds (and its `files` components) use
them, but none needs intuiTUIve.

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
- **In the background:** `mime_batch(&paths)` sniffs many files at once
  (at most 512 bytes each), for a listing that colours its rows by type
  once a worker thread has read them, as Yazi's fetchers do.
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

## Directory listings and walks

`rich_ext::listing` (no feature, no dependencies) reads a directory as a
file manager shows it:

```rust
use rich_ext::listing::{read_dir, readable_size, Sort, SortBy};

for entry in read_dir(dir, false, Sort::by(SortBy::Natural))? {
    println!("{:>6} {}", readable_size(entry.size), entry.name);
}
```

- **`Entry`** has the name, whether it is a directory (a link to one
  counts), its size, mode, owner, times, and where a link points (and
  whether its target is missing). `Entry::read(dir, name)` reads one.
- **`Sort`** is Yazi's `sort_by`: alphabetical (the default), natural
  (`a2` before `a10`), modified, created, extension, size or random,
  reversed or not, directories first unless `dirs_first` is off. Hidden
  files (a leading dot) are left out unless asked for.
- **Line-mode cells:** `readable_size` (`1.4K`), `permissions`
  (`drwxr-xr-x`), `owner` (`user:group`); `find_ranges(name, query)` is
  where a smart-case search matches a name.
- **`walk(root)`** goes through a tree depth first as an iterator, reading
  each directory only when it gets there, so a finder shows its first
  results at once: `.hidden(true)`, `.max_depth(n)`,
  `.skip_dir(".git")`. Links are listed, never followed.

## Opening files

`rich_ext::open` is Yazi's `[opener]` and `[open]` tables as data. An
`Opener` says how (`Launch::Editor`, `System`, `Reveal`, `Shell(run)`,
`Extract`) and whether it takes the terminal (`block`); a `Rule` picks
openers by MIME type or name glob (`text/*`, `{audio,video}/*`, `*/` for a
directory); the first rule that matches decides.

```rust
use rich_ext::open::{Launch, Opener, Rule, Rules};

let rules = Rules::yazi()
    .opener(Opener::new("view", "View", Launch::Shell("less \"$@\"".into())).block(true))
    .prepend(Rule::name("*.log", &["view", "edit"]));
let openers = rules.matching(path, mime); // the first is what Enter uses
let commands = openers[0].commands(&[path.to_path_buf()]);
```

It builds `Command`s and runs nothing: `editor()` is `$EDITOR` (else
`vi`), `system_opener()` is `$OPENER` (else `xdg-open`, `open`, or `cmd /C
start`), and a shell command runs in `sh -c` with the files as `"$@"`.
`rules.shared(&files)` is what every file of a selection can be opened
with. intuiTUIve's `files::open_with` runs one from an app.

## File operations

`rich_ext::file_ops::run(&op, &cancel, &mut progress)` copies, moves,
deletes or trashes, calling `progress` with bytes and items done (after
every 256 KiB) and stopping at the next of those once the
[`CancelToken`](workflows.md#cancellation) is cancelled. A file half copied
is removed; what was done before stays.

```rust
use rich_ext::cancel::CancelToken;
use rich_ext::file_ops::{run, Conflict, Op};

let op = Op::Copy { from: paths, into: dir, conflict: Conflict::Rename };
let report = run(&op, &CancelToken::new(), &mut |p| println!("{:.0}%", p.fraction() * 100.0));
```

- A name that is taken gets `_1`, `_2`… before its extension
  (`Conflict::Rename`, Yazi's default), or is replaced (`Overwrite`) or
  left (`Skip`); `unique(path)` is the rule on its own.
- A directory is never pasted into itself; a cut pasted where it is stays.
- A move is a rename where it can be, else a copy and a delete.
- `Op::title()` names it for a task list (`Copy 3 items to docs`);
  `under(dir, typed)` turns a typed name into a path that cannot climb out
  of `dir`.

## The trash

`rich_ext::trash` keeps the
[FreeDesktop.org trash](https://specifications.freedesktop.org/trash-spec/latest/),
the one GNOME Files, Dolphin, Thunar and `trash-cli` share:

```rust
let item = rich_ext::trash::put(path)?;   // moved, with where and when
for item in rich_ext::trash::list()? { println!("{}", item.original.display()); }
item.restore()?;                           // back where it was
```

A file on the home directory's file system goes to `~/.local/share/Trash`;
one on another mount to that mount's `.Trash/$uid` or `.Trash-$uid`, so
nothing is copied across devices. On macOS and Windows, whose bins the
Finder and the shell keep, `put` and `list` return an `Unsupported` error
that says so. `Trash::at(dir)` is a trash directory you name, anywhere
(the tests use one).

## Git status

`rich_ext::git::status(dir)` runs `git status --porcelain=v1 -z
--ignored=matching` (no git library) and returns a `GitStatus`, or `None`
outside a work tree. `status.of(path)` says what happened to a path:
`Change::Modified`, `Added`, `Deleted`, `Untracked`, `Ignored` or
`Conflicted`; a directory takes the greatest change inside it, and a file
in an untracked or ignored directory takes the directory's. `letter()`,
`glyph()` and `style()` are the sign and colour Yazi's git plugin shows.
`parse_porcelain(root, bytes)` reads output taken some other way.

## Archives

`rich_ext::archive` (the `archive` feature: `zip` with deflate only, and
`flate2`'s pure-Rust backend) reads tar, tar.gz and zip files:

```rust
use rich_ext::archive::{default_dest, entries, extract};

for entry in entries(path)? { println!("{} {}", entry.size, entry.path); }
let done = extract(path, &default_dest(path))?; // `docs.tar.gz` → `docs/`
```

Extraction is careful with what an archive says: an entry that is absolute
or climbs out with `..` is refused, links are not made, nothing is
overwritten and set-id bits are dropped; `done.skipped` lists what was
left out and why. `extract_with` reports progress and can be cancelled,
as `file_ops::run` does. Tar is read by the module itself (ustar, GNU long
names, pax paths).

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
  440 Hz (sharps `#`, flats `b`, octaves -1 to 9, 4 when left out).
  **`NoteTable::ScopeTui.hz("E2")`** reads it as scope-tui's table does:
  octave 0's pitches rounded to the hundredth and doubled, octave 0 when
  left out, so a clone's `--tune` buffer sizes match scope-tui's sample
  for sample (E2 at 48 kHz is 583 samples there, 582 in equal
  temperament).
- **`peak_hz(spectrum)`** is the loudest bin's frequency above 0 Hz,
  refined between bins, and **`nearest_note(hz)`** the note nearest a
  frequency, as `("A", 4, cents)`: a tuner's reading.
- **`bands(spectrum, n, lo_hz, hi_hz)`** gathers a spectrum into `n`
  log-spaced bands, as cava's bars, and **`Falloff::gravity(g)`** keeps
  levels that rise at once and fall faster and faster (`update(&levels)`
  once a frame); `chart::BarSpectrum` draws them.

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
