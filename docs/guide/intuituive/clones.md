# Eleven apps, rebuilt

intuiTUIve is tested against real programs. We take a well-known terminal
app, most of them ratatui apps, and rebuild it on intuiTUIve from the
framework's components only. This page shows the eleven rebuilds: what each
one clones, a recording of it, how to run it, the components it is built
from, its size, and the report written while building it.

Each clone is an example in
[`crates/rich-intuituive/examples/`](https://github.com/buchochelliq-labs/rs-rich-cli/tree/main/crates/rich-intuituive/examples),
with a test file of its own in
[`tests/`](https://github.com/buchochelliq-labs/rs-rich-cli/tree/main/crates/rich-intuituive/tests)
that drives it headless. None reuses the original's code: the layout, keys
and strings are rebuilt, and each example's header credits the original
and its licence. To port an app of your own, read
[Porting a ratatui app](porting.md).

## The demo loop

The clones are a loop, and each turn of it is a round:

1. **Clone** a real app from intuiTUIve's components only. Nothing in the
   framework changes during the round, and the example draws no widget of
   its own to stand in for a missing one.
2. **Write down the gaps.** What the clone needs and cannot get goes in its
   report: missing components, bugs, friction with the existing APIs, and
   the components that would make it shorter or more faithful.
3. **Add them.** Gaps chosen from the reports are built into the framework.
4. **Rebuild** each clone on what was added, and report again.

The loop goes on until a clone uses intuiTUIve's components only; then we
pick more apps.

One rule holds throughout: **core components are generic.** A core
component must be reusable by other components, with no domain types,
names or parameters (no paths, files, samples or one app's shapes).
Anything only one domain could use is a plugin, an off-by-default feature
such as `files`, `pcm`, `wav` or `art`, built on a generic core piece where
there is one. The Yazi clone's Miller columns are the `files` plugin's
`files::miller`, over the core's `miller_of`. The rule, and every round's
gaps, are in
[what the demos are missing](../../design/intuituive-demo-gaps.md).

| Clone | Original | Example | Lines | Tests | Report |
|---|---|---|---|---|---|
| [files](#files-yazi) | Yazi | `files.rs` | 1,198 | 47 | [rounds 1–4](../../design/intuituive-demo-gaps.md#round-4-files-yazi) |
| [scope](#scope-scope-tui) | scope-tui | `scope.rs` | 1,274 | 31 | [rounds 1–4](../../design/intuituive-demo-gaps.md#round-4-scope-tui) |
| [git](#git-gitui) | gitui | `git.rs` | 2,204 | 23 | [git](../../design/gaps/git.md) |
| [top](#top-bottom) | bottom | `top.rs` | 2,415 | 25 | [top](../../design/gaps/top.md) |
| [http](#http-slumber) | Slumber | `http.rs` | 2,026 | 22 | [http](../../design/gaps/http.md) |
| [blocks](#blocks-rata-tetris) | rata-tetris | `blocks.rs` | 959 | 18 | [blocks](../../design/gaps/blocks.md) |
| [csv](#csv-csvlens) | csvlens | `csv.rs` | 1,375 | 14 | [csv](../../design/gaps/csv.md) |
| [disk](#disk-diskonaut) | diskonaut | `disk.rs` | 1,192 | 21 | [disk](../../design/gaps/disk.md) |
| [music](#music-termusic) | termusic | `music.rs` | 1,512 | 20 | [music](../../design/gaps/music.md) |
| [tasks](#tasks-taskwarrior-tui) | taskwarrior-tui | `tasks.rs` | 2,412 | 27 | [tasks](../../design/gaps/tasks.md) |
| [raycast](#raycast) | (an original game) | `raycast/` | 2,891 | 28 | [raycast](../../design/gaps/raycast.md) |

Lines are the example's own, comments included. Every recording on this
page is a [tape](../../recordings.md#how-it-works) in `docs/tapes/`, run
against the example by `rich record`; CI replays each one and compares its
screenshots' text with the committed ones.

## files: Yazi

A file manager in the style of [Yazi](https://github.com/sxyazi/yazi):
the parent, current and preview columns, Yazi's keymap and status line,
tabs, a selection, yank and paste, a filter, search, bulk rename, the
finder (`z`), frecency (`Z`), and a task manager for file jobs.

![The file manager in a fixture project](../../media/tapes/files/files.gif)

| Which-key after `,` | An image preview | The task manager |
|---|---|---|
| ![The which-key pop-up after `,`](../../media/tapes/files/files-which.png) | ![An image preview, with the art feature](../../media/tapes/files/files-image.png) | ![The task manager with a failed move](../../media/tapes/files/files-tasks.png) |

```bash
cargo run -p rs-rich-intuituive --features files --example files -- DIR
cargo run -p rs-rich-intuituive --features files,archive,watch,art --example files
```

It needs `files`; `archive` adds archive previews, `watch` follows the
disk, and `art` draws images.

**Built from:** the `files` plugin's `miller` (on the core's `miller_of`),
`tabs` (on `tab_set` and `history`), `selection` (on `multi_select`),
`Yank`, `finder_with` (on `finder_of`), `frecency`, `spot` (on `facts`),
`bulk_rename`, `shell` and `search`, and its tasks (on the core's `jobs`);
the core's `cursor_by`, `list_search`, `seg_gauge`, `fuzzy_list`,
`tabs(..).fit()` and `help_with`. The porting guide walks through it:
[a Yazi-style file manager](porting.md#worked-example-a-yazi-style-file-manager).

## scope: scope-tui

An audio scope in the style of
[scope-tui](https://github.com/alemidev/scope-tui): an oscilloscope, a
vectorscope and a spectroscope over a test signal, raw PCM from a file or
a pipe, or a sound card.

![The test signal in the three scopes](../../media/tapes/scope/scope.gif)

| Oscilloscope | Vectorscope | Spectroscope |
|---|---|---|
| ![The oscilloscope, paused](../../media/tapes/scope/oscilloscope.png) | ![The vectorscope](../../media/tapes/scope/vectorscope.png) | ![The spectroscope](../../media/tapes/scope/spectroscope.png) |

```bash
cargo run -p rs-rich-intuituive --example scope --features pcm              # a test signal
cargo run -p rs-rich-intuituive --example scope --features pcm -- file PATH # raw PCM
cargo run -p rs-rich-intuituive --example scope --features audio -- audio   # a sound card
```

**Built from:** rs-rich-ext's `Chart` (Braille lines, a layer a grid
line); the `pcm` plugin's `Capture` (a simulated one for the test
signal), `latest_with_state` and `audio::play`; the core's `modes` (each
scope's keys bound and listed only while it shows), `bind_toggle`,
`bind_step`, `bind_scaled`, `recent` and the `Proxy` that wakes the loop
from another thread. The porting guide walks through it:
[scope-tui, an oscilloscope](porting.md#worked-example-scope-tui-an-oscilloscope).

## git: gitui

A git client in the style of [gitui](https://github.com/extrawurst/gitui):
its tabs, the unstaged and staged trees beside the diff, staging by file,
hunk and line, the commit popup, the log with branch and tag markers, a
commit's details, branches and stashes. It runs the `git` on the `PATH`.

![gitui's status, a file staged, the log and a commit's details](../../media/tapes/git/git.gif)

| Status | Staged | Log | Details |
|---|---|---|---|
| ![Unstaged changes beside the diff](../../media/tapes/git/status.png) | ![A new file staged, its diff](../../media/tapes/git/staged.png) | ![The log with a branch and a tag](../../media/tapes/git/log.png) | ![A commit's details and files](../../media/tapes/git/details.png) |

```bash
cargo run -p rs-rich-intuituive --example git -- DIR
```

**Built from:** `tree_keyed` with status prefixes (`TreeItem::prefix`),
`list_rows(..).range(..)` for the diff's line selection, `shown_when` and
`size_with` for gitui's focus-driven layout, `on_focus`, `key_hints_with`
for gitui's key bar, and `keyed` for the Files tab's viewer.

## top: bottom

A system monitor in the style of
[bottom](https://github.com/ClementTsang/bottom) (`btm`): CPU, memory and
network graphs, temperatures, disks, and a process table to sort, search,
group, show as a tree and kill from. It reads Linux's `/proc` and `/sys`.

![bottom's dashboard, its help, the sort menu and an expanded widget](../../media/tapes/top/top.gif)

The stills are of the help, since the dashboard is this machine's live
data:

| The help | Its process keys |
|---|---|
| ![bottom's help over the dashboard](../../media/tapes/top/help.png) | ![The help scrolled to the process widget's keys](../../media/tapes/top/help-processes.png) |

```bash
cargo run -p rs-rich-intuituive --example top
cargo run -p rs-rich-intuituive --example top -- --basic
```

**Built from:** each widget a `focus_scope` moved between by
`bind_directional_focus` (on `focus::neighbour`), `shown_when` for the
expanded widget, `table_with` with `columns_with` and `Column::priority`,
a `query::Grammar` of bottom's search keywords, `title_with` for the
load averages, and rs-rich-ext's `Chart` for the graphs.

## http: Slumber

A terminal HTTP client in the style of
[Slumber](https://github.com/LucasPickering/slumber): a YAML collection of
profiles and recipes, templates, the request's query, headers and body,
and the response, filtered by JSONPath or a shell command. Requests go
over plain HTTP/1.1 (no TLS).

![A request sent, its body filtered, and the profiles](../../media/tapes/http/http.gif)

| Query | Response | Filtered | Profiles |
|---|---|---|---|
| ![A recipe's query parameters](../../media/tapes/http/recipe.png) | ![A JSON response](../../media/tapes/http/response.png) | ![The body filtered by a JSONPath](../../media/tapes/http/filtered.png) | ![The profile list](../../media/tapes/http/profiles.png) |

```bash
cargo run -p rs-rich-intuituive --example http \
    --features rich-ext/yaml,rich-ext/jsonpath,watch -- slumber.yml
```

It needs rs-rich-ext's `yaml` and `jsonpath`; with `watch` the collection
is read again when it changes.

**Built from:** one tree laid out by `shown_when`, `hidden_when` and
`size_with`; `focus_within` to focus a pane a `switch` has not built yet;
`focus_border` for Slumber's double border; titles from state
(`panel_with`, `add_title_with`); `tree_keyed` and `tree_reveal` for the
recipes; a `Timer` paused while no request runs; input history.

## blocks: rata-tetris

The falling-block game [rata-tetris](https://github.com/nw-wood/rata-tetris),
NES-style: the seven pieces from a shuffled bag, gravity in NES frames,
levels, a ghost piece, statistics and a top score, plus a hold panel.

![A seeded game from the title to the game over](../../media/tapes/blocks/blocks.gif)

| Title | Playing | Game over |
|---|---|---|
| ![The title](../../media/tapes/blocks/title.png) | ![A game with a held piece](../../media/tapes/blocks/playing.png) | ![The game over](../../media/tapes/blocks/game-over.png) |

```bash
cargo run -p rs-rich-intuituive --example blocks
cargo run -p rs-rich-intuituive --example blocks -- --seed 7
```

**Built from:** a `fixed_step` of one NES frame with catch-up
(`Timer::max_catch_up`), paused off the playing screen; `Node::repeat` for
the NES's DAS and ARR; `App::min_size`; a composited `layers` backdrop;
`state_file` and `write_atomic` for the top score; and the falling piece
and its ghost as sprites on a stage (`sprite::stage_with`).

## csv: csvlens

A CSV viewer in the style of [csvlens](https://github.com/YS-L/csvlens): a
table over a CSV or TSV file (or stdin), read lazily, with csvlens's keys
to find, filter, sort, select cells, columns or rows, freeze columns and
wrap long cells.

![A CSV file found in, filtered and its help](../../media/tapes/csv/csv.gif)

| Table | Find | Filter | Help |
|---|---|---|---|
| ![The table and csvlens's status line](../../media/tapes/csv/table.png) | ![Matches of a find](../../media/tapes/csv/find.png) | ![The rows a filter keeps](../../media/tapes/csv/filter.png) | ![csvlens's keys](../../media/tapes/csv/help.png) |

```bash
cargo run -p rs-rich-intuituive --example csv -- FILE.csv
some-command | cargo run -p rs-rich-intuituive --example csv
```

**Built from:** the file as a `TableSource`, with `view_of` to keep, find
and sort its rows on a background task; one reactive table with
`columns_with`, `freeze_columns_with`, `multiline_when` and
`Column::max_percent`; `TableLayout`; a `command_line` with a shared
history for the prompts; and core's `Text::markup()`.

## disk: diskonaut

A disk-space treemap in the style of
[diskonaut](https://github.com/imsnif/diskonaut): the folder's children as
tiles sized by the space they take, drawn while the scan runs, then
browsed, zoomed and pruned. Deleting is real.

![A folder scanned, browsed and a delete asked](../../media/tapes/disk/disk.gif)

| Map | Selected | Inside a folder | Delete? |
|---|---|---|---|
| ![The treemap](../../media/tapes/disk/map.png) | ![A tile selected](../../media/tapes/disk/selected.png) | ![The target folder](../../media/tapes/disk/folder.png) | ![The question before a delete](../../media/tapes/disk/delete.png) |

```bash
cargo run -p rs-rich-intuituive --example disk -- DIR
cargo run -p rs-rich-intuituive --example disk -- DIR --apparent-size
```

**Built from:** `layout::squarify` for the tiles, `focus::neighbour` to
move between them, `fit_text` and `truncate_middle` for every line that
picks the longest form that fits, `keyed` for the map, `App::min_size`,
and a `grid` with `collapse_borders` to join the tiles' lines.

## music: termusic

A music player in the style of
[termusic](https://github.com/tramhao/termusic): the library tree, the
playlist, the progress gauge, lyrics from a `.lrc` file, the album cover
and the key footer. There is no sound: playback is a play head on the
app's clock, and WAV is the one format it reads.

![A library added to the playlist and a track played](../../media/tapes/music/music.gif)

| Library | Playing | Help |
|---|---|---|
| ![The library tree](../../media/tapes/music/library.png) | ![A track paused at 00:05, with its lyrics and cover](../../media/tapes/music/playing.png) | ![The help, from the playlist](../../media/tapes/music/help.png) |

```bash
cargo run -p rs-rich-intuituive --example music --features wav,art -- DIR
```

It needs `wav` (WAV files through hound) and `art` (the cover).

**Built from:** a `transport` play head (play, seek, speed, the track's
end); `panel_with` titles from state; `tree_keyed` and `tree_reveal` for
the library; `TableOptions::symbol_for` for the playlist's markers;
`keyed` and `shown_when` for the cover; `HelpOptions::all_panes`.

## tasks: taskwarrior-tui

A Taskwarrior front end in the style of
[taskwarrior-tui](https://github.com/kdheepak/taskwarrior-tui): the report
sorted by urgency over the selected task's details, Taskwarrior's filter
syntax, the command bar for new tasks and changes, marks, undo, contexts,
the Projects view and a calendar. The tasks are kept as JSON lines in a
directory, as `task export` writes them.

![A filter, a new task with completion, and the Projects view](../../media/tapes/tasks/tasks.gif)

| Report | Filter | Completion | Projects |
|---|---|---|---|
| ![The report and the details](../../media/tapes/tasks/report.png) | ![A filter with `or`](../../media/tapes/tasks/filter.png) | ![A project completed in a pop-up](../../media/tapes/tasks/add.png) | ![The Projects view](../../media/tapes/tasks/projects.png) |

```bash
cargo run -p rs-rich-intuituive --example tasks --features rich-ext/data -- DIR
```

`DIR` defaults to `$TMPDIR/intuituive-tasks`, filled with a few tasks the
first time.

**Built from:** a `query::Grammar` of Taskwarrior's filter syntax over a
`Record`; `dates::parse_with` and rs-rich-interact's `Zone`;
`command_line_with` with completion (`Mode::complete`) and a history per
action; one reactive table with `columns_with`, `cursor_by` and
`symbol_for`; `year_view_with` for the calendar.

## raycast

Not a clone but an original: a first-person ray caster in the style of the
early-90s shooters, with doors, guards, pickups and two levels, drawn in
half blocks, quadrants, Braille, or real pixels through Kitty's or iTerm2's
protocol. Its levels, textures, sprites and face are made in code.

| Title | Start | Map |
|---|---|---|
| ![The title over the first level](../../media/tapes/raycast/title.png) | ![The first level](../../media/tapes/raycast/start.png) | ![The map](../../media/tapes/raycast/map.png) |

[A walk-through (GIF, 7.7 MB)](../../media/tapes/raycast/raycast.gif)

```bash
cargo run -p rs-rich-intuituive --example raycast --features art --release
cargo run -p rs-rich-intuituive --example raycast --features art --release -- --mode braille
```

**Built from:** a `fixed_step` of a tic (35 a second) with catch-up;
`on_key_up`, `Ctx::releases_reported` and `App::all_keys` for held keys;
`pixels_fit` and `pixels` for the view and the face (`DrawCx::rect` to
place the image); `keyed` with `on_unmount` to swap cells for pixels.

## What the rounds added

Rounds 1 to 4 rebuilt the Yazi and scope-tui clones four times. Each time
their reports led to new pieces: keys and pop-ups, list and table rows,
the `files` plugin, the `pcm`, `audio` and `viz` plugins, and runtime
fixes. Round 5 cloned the nine apps above it and gathered the gaps they
shared; round 6 built them (tables, trees and lists that follow state,
layout and focus from state, keyed nodes, a command line with completion,
queries and dates, fixed steps and held keys, sprites); and round 7
rebuilt the nine on round 6. The table of what replaced what, the bugs
found, and the gaps still open are in
[what the demos are missing](../../design/intuituive-demo-gaps.md#round-7-the-nine-apps-rebuilt-on-round-6).

## Real Doom

[rs-rich-doom](https://github.com/buchochelliq-labs/rs-rich-doom) runs
Doom itself (doomgeneric) in the terminal, rendered through intuiTUIve: the
engine runs on a `latest` thread, and its 320x200 frames are drawn in half
blocks, Braille, or Kitty, Sixel or iTerm2 pixels, at 33 to 35 frames a
second, with sound effects through cpal. It is a separate repository,
under the GPL (the engine is GPL), and it depends on this one. What it
needed from rs-rich is listed in
[Real Doom (rs-rich-doom)](../../design/intuituive-demo-gaps.md#real-doom-rs-rich-doom).
