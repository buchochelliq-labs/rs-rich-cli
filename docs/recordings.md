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

## One binary, every format

Markdown, CSV and source code, each rendered from a plain `rich FILE`.

<div class="tape-player" data-cast="../media/tapes/hero/hero.cast" data-poster="npt:0:4">
  <img src="../media/tapes/hero/hero.gif" alt="rich rendering a Markdown file, a CSV table and Python source">
</div>

| Markdown | CSV | Code |
|---|---|---|
| ![Markdown rendered by rich](media/tapes/hero/markdown.png) | ![A CSV file as a table](media/tapes/hero/table.png) | ![Python highlighted by rich](media/tapes/hero/code.png) |

[Tape](tapes/hero.tape) · [Cast](media/tapes/hero/hero.cast) · [GIF](media/tapes/hero/hero.gif)

## Watch a file

`rich --watch` renders a file and renders it again on every save. Here the
tape edits `service.json` from outside the terminal while `rich` watches it.

<div class="tape-player" data-cast="../media/tapes/watch/watch.cast" data-poster="npt:0:3">
  <img src="../media/tapes/watch/watch.gif" alt="rich --watch re-rendering a JSON file after an edit">
</div>

| Before the edit | After the edit |
|---|---|
| ![replicas is 2](media/tapes/watch/before.png) | ![replicas is 5](media/tapes/watch/after.png) |

[Tape](tapes/watch.tape) · [Cast](media/tapes/watch/watch.cast) · [GIF](media/tapes/watch/watch.gif)

## Page and search

`rich view --pager` numbers and highlights a file, then hands it to `less`:
page down, then `/characters` to search.

<div class="tape-player" data-cast="../media/tapes/pager/pager.cast" data-poster="npt:0:3">
  <img src="../media/tapes/pager/pager.gif" alt="rich view paging a Rust file through less and searching it">
</div>

| Opened | Paged down | Searched |
|---|---|---|
| ![The top of rule.rs](media/tapes/pager/top.png) | ![One page further](media/tapes/pager/scrolled.png) | ![Matches of characters highlighted](media/tapes/pager/search.png) |

[Tape](tapes/pager.tape) · [Cast](media/tapes/pager/pager.cast) · [GIF](media/tapes/pager/pager.gif)

## How it works

A tape is a short script, one step per line. This is the watch tape:

```text
--8<-- "docs/tapes/watch.tape"
```

`scripts/tape.py` runs it. It starts `bash` on a pseudo-terminal with a pinned
environment (a temporary home and working directory, `TERM=xterm-256color`,
truecolor, UTF-8, UTC), puts the binaries under test first on `PATH`, and
follows the screen with a terminal emulator. `Wait` blocks until the screen
shows the given text or matches a `/regex/`, so a tape never depends on a
fixed delay.

From one run it writes, under `docs/media/tapes/<tape>/`:

- for each `Screenshot`: a PNG, an SVG with selectable text, and a plain-text
  grid of the screen;
- the whole session as an asciinema cast, with the keys pressed;
- a GIF with the keys shown as they are pressed, and an MP4 when FFmpeg is
  installed;
- `provenance.json`: the tape's hash, the `rich --version` and the commit.

| Step | Meaning |
|---|---|
| `Set Size 100x28`, `Set Title "…"`, `Set TypingDelay 40ms`, `Set Timeout 15s`, `Set Env NAME value` | Configure the session |
| `Write FILE "text"`, `Exec "command"` | Prepare or change files, outside the terminal |
| `Type "text"` | Type into the terminal, one character at a time |
| `Enter`, `Tab`, `Space`, `Backspace`, `Escape`, arrows, `Home`, `End`, `PageUp`, `PageDown`, `Ctrl+C` | Press a key; a number after it repeats it |
| `Wait "text"`, `Wait /regex/` | Wait until the screen shows it |
| `Sleep 500ms` | Pause the recording |
| `Screenshot NAME` | Save the screen |
| `Hide`, `Show` | Leave steps out of the recording |
| `Resize 80x24` | Resize the terminal |

Regenerate everything, or check it as CI does:

```bash
cargo build -p rs-rich-cli
pip install -r scripts/requirements-docs-media.txt
python3 scripts/tape.py docs/tapes/*.tape          # write the media
python3 scripts/tape.py --check docs/tapes/*.tape  # compare the screenshots
```

The same tape format becomes a `rich record` command in a later release
([#599](https://github.com/buchochelliq-labs/rs-rich-cli/issues/599)), so you
will be able to record your own programs the same way.
