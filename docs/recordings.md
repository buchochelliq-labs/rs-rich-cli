# Terminal recordings

Every recording on this page is a real terminal session. A script called a
**tape** types into `bash` on a pseudo-terminal, runs the `rich` binary built
from this repository, and waits for the screen before each step. Nothing is
drawn by hand. Press play, pause anywhere, and select the text: the recordings
are [asciinema](https://asciinema.org) casts, not videos.

The screenshots under each recording are taken from the same run. CI re-runs
every tape and fails when any screenshot's text no longer matches, so this page
cannot drift from what the CLI does. [How it works](#how-it-works) is at the
end.

## Gallery

Point at a card, or move to it with Tab, to play its recording. With reduced
motion turned on in your system settings the cards stay still.

<div class="tape-gallery" markdown>

[![A CSV file rendered as a table](media/tapes/hero/table.png){ data-gif="hero.gif" loading=lazy }<span class="tape-card-title">One binary, every format</span>](#one-binary-every-format){ .tape-card }
[![rich --watch after an edit](media/tapes/watch/after.png){ data-gif="watch.gif" loading=lazy }<span class="tape-card-title">Watch a file</span>](#watch-a-file){ .tape-card }
[![Search matches highlighted in less](media/tapes/pager/search.png){ data-gif="pager.gif" loading=lazy }<span class="tape-card-title">Page and search</span>](#page-and-search){ .tape-card }
[![rich input refusing an empty answer](media/tapes/input/required.png){ data-gif="input.gif" loading=lazy }<span class="tape-card-title">Ask for a line</span>](#ask-for-a-line){ .tape-card }
[![A fuzzy file picker with a preview](media/tapes/components/select.png){ data-gif="components.gif" loading=lazy }<span class="tape-card-title">Components</span>](#components){ .tape-card }
[![rich file previewing a file before picking it](media/tapes/file/preview.png){ data-gif="file.gif" loading=lazy }<span class="tape-card-title">Ask for more in a script</span>](#ask-for-more-in-a-script){ .tape-card }
[![A Markdown file paged in a viewport](media/tapes/viewport/paged.png){ data-gif="viewport.gif" loading=lazy }<span class="tape-card-title">An interactive component</span>](#an-interactive-component){ .tape-card }
[![The guided tour, inspecting structured data](media/tapes/tour/inspect.png){ data-gif="tour.gif" loading=lazy }<span class="tape-card-title">The guided tour</span>](demos.md#run-the-suite-in-your-terminal){ .tape-card }
[![rich choose in the 0.0.13 release recording](media/tapes/release-0.0.13/choose.png){ data-gif="release-0.0.13.gif" loading=lazy }<span class="tape-card-title">The 0.0.13 release</span>](releases/0.0.13.md){ .tape-card }

</div>

## One binary, every format

Markdown, CSV and source code, each rendered from a plain `rich FILE`.

<div class="tape-player" data-cast="../media/tapes/hero/hero.cast" data-poster="npt:0:4">
  <img src="../media/tapes/hero/hero.gif" alt="rich rendering a Markdown file, a CSV table and Python source">
</div>

| Markdown | CSV | Code |
|---|---|---|
| ![Markdown rendered by rich](media/tapes/hero/markdown.png) | ![A CSV file as a table](media/tapes/hero/table.png) | ![Python highlighted by rich](media/tapes/hero/code.png) |

[Tape](tapes/hero.tape) · [Cast](media/tapes/hero/hero.cast) · [GIF](media/tapes/hero/hero.gif) · [Page](media/tapes/hero/hero.html)

## Watch a file

`rich --watch` renders a file and renders it again on every save. Here the
tape edits `service.json` from outside the terminal while `rich` watches it.

<div class="tape-player" data-cast="../media/tapes/watch/watch.cast" data-poster="npt:0:3">
  <img src="../media/tapes/watch/watch.gif" alt="rich --watch re-rendering a JSON file after an edit">
</div>

| Before the edit | After the edit |
|---|---|
| ![replicas is 2](media/tapes/watch/before.png) | ![replicas is 5](media/tapes/watch/after.png) |

[Tape](tapes/watch.tape) · [Cast](media/tapes/watch/watch.cast) · [GIF](media/tapes/watch/watch.gif) · [Page](media/tapes/watch/watch.html)

## Page and search

`rich view --pager` numbers and highlights a file, then hands it to `less`:
page down, then `/characters` to search.

<div class="tape-player" data-cast="../media/tapes/pager/pager.cast" data-poster="npt:0:3">
  <img src="../media/tapes/pager/pager.gif" alt="rich view paging a Rust file through less and searching it">
</div>

| Opened | Paged down | Searched |
|---|---|---|
| ![The top of rule.rs](media/tapes/pager/top.png) | ![One page further](media/tapes/pager/scrolled.png) | ![Matches of characters highlighted](media/tapes/pager/search.png) |

[Tape](tapes/pager.tape) · [Cast](media/tapes/pager/pager.cast) · [GIF](media/tapes/pager/pager.gif) · [Page](media/tapes/pager/pager.html)

## Ask for a line

`rich input` reads one line for a script. `--placeholder` shows a hint while
the line is empty, and `--required` refuses an empty answer with an error under
the line. The answer is captured with `$(…)`.

<div class="tape-player" data-cast="../media/tapes/input/input.cast" data-poster="npt:0:4">
  <img src="../media/tapes/input/input.gif" alt="rich input showing a placeholder, refusing an empty answer, then taking one">
</div>

| Placeholder | Required | Answered |
|---|---|---|
| ![The prompt with its placeholder](media/tapes/input/placeholder.png) | ![An answer is required](media/tapes/input/required.png) | ![The answer echoed by the script](media/tapes/input/answer.png) |

[Tape](tapes/input.tape) · [Cast](media/tapes/input/input.cast) · [GIF](media/tapes/input/input.gif) · [Page](media/tapes/input/input.html)

## Components

Every `rs-rich-interact` component, one after another, recorded in a small
fixture project:

- a fuzzy file picker with a highlighted preview;
- a multi-select;
- an input with suggestions and validation;
- a confirmation sheet with a diff and four choices;
- a form that reports an error under its field;
- a pager that searches.

See [Interactive components](guide/interact/index.md#ready-made-components).

<div class="tape-player" data-cast="../media/tapes/components/components.cast" data-poster="npt:0:5">
  <img src="../media/tapes/components/components.gif" alt="rs-rich-interact's components, one after another">
</div>

| Select | Input | Confirm |
|---|---|---|
| ![A fuzzy pick with a preview](media/tapes/components/select.png) | ![Suggestions as you type](media/tapes/components/input-suggestions.png) | ![A confirmation sheet](media/tapes/components/confirm.png) |
| **Form** | **MultiSelect** | **Pager** |
| ![A form with an error](media/tapes/components/form-error.png) | ![Marking several](media/tapes/components/multi.png) | ![Searching](media/tapes/components/pager.png) |

[Tape](tapes/components.tape) · [Cast](media/tapes/components/components.cast) · [GIF](media/tapes/components/components.gif) · [Page](media/tapes/components/components.html)

## Ask for more in a script

`rich write`, `rich file`, `rich color` and `rich asset`, each captured into
a shell variable with `$(…)`: several lines of a commit message, a path
picked from the fixture project with its preview, a colour from the
names and the palette, and an emoji and a box style used together. See
[Ask in a script](cli.md#ask-in-a-script).

<div class="tape-player" data-cast="../media/tapes/file/file.cast" data-poster="npt:0:3">
  <img src="../media/tapes/file/file.gif" alt="rich file browsing a project and picking a file">
</div>

| rich write | rich file | rich file |
|---|---|---|
| ![Several lines typed](media/tapes/write/editing.png) | ![A directory listed](media/tapes/file/browse.png) | ![A file previewed](media/tapes/file/preview.png) |
| **rich color** | **rich color** | **rich asset** |
| ![Named colours with a swatch](media/tapes/color/names.png) | ![The 256 palette](media/tapes/color/palette.png) | ![Box styles previewed](media/tapes/asset/box.png) |

[write tape](tapes/write.tape) · [cast](media/tapes/write/write.cast) ·
[file tape](tapes/file.tape) · [cast](media/tapes/file/file.cast) ·
[color tape](tapes/color.tape) · [cast](media/tapes/color/color.cast) ·
[asset tape](tapes/asset.tape) · [cast](media/tapes/asset/asset.cast)

## An interactive component

`rs-rich-interact`'s `Viewport`, run as its example pager, pages a Markdown
file on the alternate screen. Each key repaints only the cells that changed.
Enter gives the terminal back, as it was, with the line it was left at. See
[Interactive components](guide/interact/index.md).

<div class="tape-player" data-cast="../media/tapes/viewport/viewport.cast" data-poster="npt:0:3">
  <img src="../media/tapes/viewport/viewport.gif" alt="A Markdown file paged in rich_interact's Viewport">
</div>

| Opened | Paged down | Given back |
|---|---|---|
| ![The top of the README](media/tapes/viewport/top.png) | ![One page further](media/tapes/viewport/paged.png) | ![The shell again, with the line it was left at](media/tapes/viewport/returned.png) |

[Tape](tapes/viewport.tape) · [Cast](media/tapes/viewport/viewport.cast) · [GIF](media/tapes/viewport/viewport.gif) · [Page](media/tapes/viewport/viewport.html)

## How it works

A tape is a short script, one step per line. This is the watch tape:

```text
--8<-- "docs/tapes/watch.tape"
```

`rich record` runs it: the CLI's own recorder, built on the
[`rs-rich-record`](https://docs.rs/rs-rich-record) crate. It starts a shell
(`bash` unless the tape sets another) on a pseudo-terminal with a pinned
environment (a temporary home and working directory, `TERM=xterm-256color`,
truecolor, UTF-8, UTC), puts the binaries under test first on `PATH`, and
follows the screen with a terminal emulator. `Wait` blocks until the screen
shows the given text or matches a `/regex/`, so a tape never depends on a
fixed delay.

From one run it writes, under `docs/media/tapes/<tape>/`:

- for each `Screenshot`: a PNG, an SVG with selectable text, and a plain-text
  grid of the screen. The SVG and the text grid come from the same
  [frame](guide/ext/live-and-layout.md#frames) as the rest of rich's export, so a screenshot
  looks like any other rich SVG;
- the whole session as an asciinema cast, with the keys pressed;
- a GIF with the keys shown as they are pressed, and an MP4 when FFmpeg is
  installed;
- an HTML page (`<tape>.html`, the **Page** links above): a small player and
  the screenshots, all inline, with text you can select. It fetches nothing
  and plays only when asked;
- `provenance.json`: the tape's fingerprint, the recorder and `rich`
  versions, the commit, and the screenshots written, so the next run removes
  only the ones the tape no longer takes.

Box-drawing characters are drawn as lines rather than font glyphs, so table
borders join between rows, and block, quadrant and braille characters are
drawn as shapes, so images and progress bars have no seams. Text uses DejaVu
Sans Mono and emoji use Twemoji, in colour, both embedded in the recorder, so
a PNG or GIF looks the same on every machine; `--font FILE` chooses another
text font. An emoji cluster such as 👩‍👧, 👍🏽, ❤️ or 🇺🇸 takes one cell as
wide as rich measures it, so the columns after it line up.

| Step | Meaning |
|---|---|
| `Set Size 100x28`, `Set Title "…"`, `Set TypingDelay 40ms`, `Set Timeout 15s`, `Set Env NAME value` | Configure the session |
| `Set WindowFrame off`, `Set Caption "…"`, `Set KeyOverlay off` | Presentation: leave out the window frame (title bar and buttons) around PNGs, SVGs and video; add a line of text under them and under the page's player; leave out the keys shown in video and the player. The frame and the overlay are on by default |
| `Output gif png`, `Output demo.html` | Write only these formats (`png`, `svg`, `cast`, `gif`, `mp4`, `html`; text grids always), or name the file a per-tape format goes to, as in VHS. Several words or lines add up; `--format` can still narrow them |
| `Set Shell zsh` | Run in `bash` (the default), `zsh`, `fish` or `sh`, each without your profile or rc files and with the same `❯` prompt. CI records in all four |
| `Write FILE "text"`, `Exec "command"` | Prepare or change files, outside the terminal |
| `Type "text"` | Type into the terminal, one character at a time |
| `Enter`, `Tab`, `Space`, `Backspace`, `Escape`, arrows, `Home`, `End`, `PageUp`, `PageDown`, `Ctrl+C` | Press a key; a number after it repeats it |
| `Wait "text"`, `Wait /regex/` | Wait until the screen shows it, or has since the previous step began (so fast output that scrolls past is not missed) |
| `Sleep 500ms` | Pause the recording |
| `Screenshot NAME` | Save the screen |
| `Hide`, `Show` | Leave steps out of the recording |
| `Resize 80x24` | Resize the terminal |
| `Mask /regex/ "text"` | Replace matches in the text grids `--check` compares, for output that differs on every run such as temporary paths or timings; images keep what was shown |

### Shells and emoji

A tape can type and print emoji, and the recorder draws them as the program
measures them: rich, and modern terminals, give 👩‍👧 two cells. A shell's
line editor may not agree. bash, zsh and fish take character widths from the
C library, which counts 👩‍👧 as four, so moving the cursor back across a
joined emoji at the prompt (`Left`, `Home`, `Backspace`) redraws the line in
the wrong place, as it does in a real terminal. The command that runs is
still right; only the echoed line is garbled. Keep such text out of line
editing: put it in a file with `Write` and run that, or have the program
print it.

The docs' tapes use `bash`, as CI does. macOS ships bash 3.2, whose line
editing is older than CI's; `rich record` warns when it finds a bash older
than 4, and a newer one (`brew install bash`) first on `PATH` gives
recordings that match.

Regenerate everything, or check it as CI does:

```bash
cargo build -p rs-rich-cli -p rs-rich-interact --bins --examples
rich=target/debug/rich
$rich record --bin-dir target/debug --output docs/media/tapes docs/tapes/*.tape
$rich record --check --bin-dir target/debug --output docs/media/tapes docs/tapes/*.tape
```

## Record your own

`rich record` is not limited to `rich`: a tape can drive any program you can
start from a shell, in `bash`, `zsh`, `fish` or `sh` (`Set Shell`). Write a
tape, then:

```bash
rich record demo.tape                       # writes recordings/demo/
rich record --format gif,png demo.tape      # only the GIF and the PNGs
rich record --format html demo.tape         # the page: a player and the screenshots
rich record --window-frame off --caption 'Save to redraw' demo.tape
rich record --check demo.tape               # fail if a screenshot changed
```

| Option | Meaning |
|---|---|
| `--output DIR` | Write to `DIR/<tape>/` (default `recordings`) |
| `--check` | Compare each screenshot's text with `DIR/<tape>/<name>.txt` instead of writing, and report screenshots the last write listed that the tape no longer takes; exits non-zero on any difference |
| `--format LIST` | Any of `png`, `svg`, `cast`, `gif`, `mp4`, `html`, or `all` (the default). Text grids are always written. A tape's `Output` narrows it further |
| `--window-frame on\|off`, `--caption TEXT`, `--key-overlay on\|off` | Override the tape's `Set WindowFrame`, `Set Caption` and `Set KeyOverlay` |
| `--no-video` | Skip the GIF and MP4 |
| `--bin-dir DIR` | Put `DIR` first on the session's `PATH` (default: the directory of the running `rich`) |
| `--font FILE` | Draw PNG and GIF text in another font |

`$REPO` in a tape is the directory `rich record` was started in, so a tape
can copy fixtures (`Exec "cp $REPO/fixtures/data.json ."`). Linux and macOS
are supported; on Windows it builds through ConPTY but needs the tape's shell
on `PATH`, and is experimental. Use the recorder from Rust through the
`rs-rich-record` crate: `tape::parse`, `record::record`, then
`record::write` or `record::check`.

### Limits, and what a tape can do

**A tape is code: record only tapes you trust.** `Exec` runs its command with
`sh`, and everything `Type` sends runs in the shell, with your user's
permissions. The recorder pins the environment and works in a temporary
directory so recordings repeat, not to contain the tape.

What the recorder does limit, so a mistake fails clearly rather than filling
the disk or memory:

| Limit | |
|---|---|
| Terminal size | `Set Size` and `Resize` from 2x2 to 500x200 |
| Durations | `Sleep`, `Wait` and `Set Timeout` at most `3600s` |
| `Write` | A relative path inside the working directory: no `/…`, no `..` |
| `Exec` | 60 seconds; the first 64 KiB of its error output is reported |
| Tape names | A tape's name is its output directory: `...tape` (named `..`) is refused |
| Stale screenshots | Removed only when `provenance.json` lists them from an earlier run; other files in the output directory are never touched |
| Frames | At most 12 a second; a burst over 32 KiB is kept as a repaint of the screen |
| Video | The first 5 minutes; a GIF or MP4 of a longer recording is an error (use `--no-video`) |
| Images | At most 100 million pixels; a GIF at most 65535 pixels a side |
