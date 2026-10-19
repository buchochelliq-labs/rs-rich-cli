# disk: a disk-space treemap on intuiTUIve

## Round 1: diskonaut

The clone is [`examples/disk.rs`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/rich-intuituive/examples/disk.rs),
a rebuild of [diskonaut](https://github.com/imsnif/diskonaut) (Aram
Drevekenin, MIT; tui-rs with crossterm, jwalk for the scan). It is one screen
and a few boxes over it, so the round covers all of it.

What was built (1,395 lines after rustfmt: about 170 the folder tree and the
scan, 200 the treemap's layout, 50 moving between tiles, 100 sizes and text
fitting, 160 the app's state and actions, 490 the screen, 180 options and
the app; tests 710 lines in `tests/disk.rs`, 21 tests):

- **The map:** the current folder's children as tiles sized by the space they
  take, largest first, squarified with diskonaut's rules (a tile at least 8
  columns by 3 rows, a cell counted 2.5 times as tall as wide), each tile's
  name (a folder's with `/` and `(+n descendants)` or `(+n)`, cut in the
  middle as `long[...]name`) and size with its share (`78.1K (46%)`, shorter
  forms when narrow), the items too small for a tile grouped in a block of
  `x` at the bottom right with diskonaut's legend `(x = Small files)`, and
  "Folder is empty". The tiles are panels in rows and columns with
  `collapse_borders()`, so the lines join (`┬ ┴ ├ ┤`) as diskonaut's do. The
  selected tile is filled (blue for a folder, grey with magenta text for a
  file).
- **Drawn while scanning:** the scan runs on a thread (`spawn_stream`)
  walking with rs-rich-ext's `listing::walk` (hidden entries included, links
  not followed) and sends entries in batches; the map grows as they arrive,
  the title says `Scanning: 58.6K (2 files)` with a bold marker running along
  it, the status line shows the path read last, and the key legend leaves out
  delete. Sizes are blocks on disk (`du`'s) or, with `-a`, the length.
- **The title:** `Total: 171.5K (13 files), freed: 0 | /base/sub (78.1K, 2
  files) (+1 larger file(s), zoom out to show)`, each part as long as the
  width allows (diskonaut's "telescope"), `(failed to read n files)` and
  `(CAUTION: running as root)` in red, the total lit yellow for 250 ms after a
  delete, the path red for 250 ms after Esc at the top.
- **Keys, diskonaut's:** arrows, `hjkl` and Ctrl+F/B/N/P move to the tile on
  that side sharing the most edge (off the map: nothing selected; with
  nothing selected: the first); Enter enters the selected folder (the largest
  when nothing is selected); Esc goes up, the selection and zoom restored;
  `+`/`-`/`0` zoom past the largest items; Backspace (and Ctrl+D, which the
  original does not have) deletes after "Delete this file?" / "Delete folder
  with n children?" with the path and `(y/n)`, "Deleting" while it runs on a
  thread, an error box when it fails; during the scan it says "Can't delete
  while scanning" (the variant that fits, any key dismisses); `q` and Ctrl+C
  ask "Are you sure you want to quit?"; `-d` deletes without asking.
- **A small window** (under 50 x 15): "Terminal window is too small
  ¯\\\_(ツ)\_/¯", or a shorter form; only `q` and Ctrl+C work there.

Tests scan a temporary folder written with known sizes (apparent sizes) and
assert the tiles' labels, the joined frame, the small items' block and
legend, movement, entering and leaving, zoom, deleting (the file is gone and
`freed:` grows), the failed delete, the quit box and the small window. The
live scan is tested with a `Walker` the test feeds one entry at a time; the
model (squarify covering the map exactly once, shares, the small block,
equal empty files, movement on hand-made tiles) without the app.

**Bespoke:** no `Widget`. The squarify (about 120 lines) and the
directional movement (about 40) are the clone's own: there is no treemap
layout and no spatial navigation to use. Both are gaps below. The
title, status and key lines pick the longest form that fits in the clone's
code (`fit`, the title's parts), a third gap.

**Left out or changed:** diskonaut's parallel scan (jwalk on rayon; this
walk is single-threaded); its rule for the small items' block (a rectangle
from the first unrenderable tile's corner, which may cut into a strip; here
the block is what is left after the last strip that fits, with two cells
kept for it when items remain); diskonaut's exact loading indicator (a few
letters in bold running along the title, approximated); Windows' resize
handling. Fixed on the way: diskonaut pushes a history entry when Enter
lands on a file, so a later Esc restores the wrong selection; and it counts
a deleted folder's descendants but not the folder itself.

### New gaps and bugs

| Gap or bug | Needed for | API sketch or repro | Where |
|---|---|---|---|
| **A treemap layout.** Rectangles proportional to weights, squarified, with a minimum tile, a cell aspect and the leftovers grouped. The clone's `treemap` is 120 lines, and it then has to turn the strips into nested `row`/`column`s with `.fixed(len + 1)` per tile (a collapsed child's size counts the shared line), which took probing to find | the whole map | core, generic: `layout::squarify(rect, &weights, Squarify { min: (8, 3), aspect: 2.5 }) -> (Vec<Rect>, Option<(Rect, usize)>)`; and a node `treemap(move \|\| items: Vec<(K, f64)>, \|key, rect\| node).rest(\|count\| node).collapse_borders()` keyed by `K` | core |
| **A child rebuilt from state, dropped when replaced.** The map's structure changes with every scan batch, selection and zoom. `switch` keeps every page it built (its `HashMap` is never pruned: a live scan would keep thousands), and `each` stacks rows only. The clone uses a one-key `each(move \|\| vec![(map, selected)], ..)` with `.fixed(rows)` read from `viewport()`, since `each` gives a child one row unless told | live-updating layouts | `rebuild(move \|\| key, \|key\| node)` (one child, built again when the key changes, the old one unmounted, laid out as a flex child), or `switch(..).keep(Keep::Current)` | core |
| **Children placed at rectangles that follow state.** `layers` with `.inset(..)` places children where a treemap puts them, but insets are fixed when the node is made, and layered panels do not join their edges (collapsing works only in `row`/`column`/`grid`) | a treemap without rebuilding every tile; floating panels | `.inset_with(move \|\| (t, r, b, l))` or `place_at(move \|\| rect)`; `layers(..).collapse_borders()` joining edges that meet | core |
| **Spatial navigation.** Move to the neighbour on a side, the one sharing the most edge, by geometry (diskonaut's tiles, a dashboard of panels, a grid of cards, a keyboard on screen). The clone keeps `selected: Option<usize>` and its own `step` over tile rectangles | arrows and `hjkl` on the map | `nav::neighbour(rects, from, Dir) -> Option<usize>` (pure), and `App::spatial_focus(true)` / `.focus_by_arrows()` on a container so arrows move the focus among focusable children by their rectangles (browsers' spatial navigation, Textual's `focus_*` by position) | core |
| **Text that picks the longest form that fits.** diskonaut writes each line as variants (`SELECTED: docs (29.3K, 1 files)` → `SELECTED: docs (29.3K)` → `SELECTED: docs` → `docs`), the title as parts each with variants collapsing together, the keys long or short, the warning and the too-small message likewise. The clone reads `viewport()` in each `text` and picks; a modal's builder has no width of its own, so it guesses half the screen | title, status, keys, tile labels, every box | `text_fit(\|\| vec![long, medium, short])` choosing by the node's width; `fit_parts([(variants, style), ..])` for a line of collapsing parts | core |
| **Middle truncation.** `long_na[...]me.txt`: there is `.truncate_start()` and `Overflow::Ellipsis` at the end, not the middle | names and paths in tiles, the status line, the delete box | `.truncate_middle()` (with the marker to use), as `truncate_start` | core |
| **A batched stream with a time flush.** The scan sends batches of at most 1000 entries or every 50 ms, but the flush can only run when the walker hands it an entry: an idle producer (a slow network folder, the test's fed walker) holds a batch back. The test walker sleeps before each entry to make every one flush | live progress from any thread | `stream.batched(Duration::from_millis(50), 1000, \|batch, cx\| ..)` or `spawn_batched(work, every, max, deliver)`: the batching on the app's side, flushed by the app's clock | core |
| **A disk-usage scan.** diskonaut's model: a tree whose folders sum sizes and counts as entries stream in, sizes on disk or apparent, read errors counted, delete subtracting up the path. The clone's `Tree`/`Folder`/`disk_walker` are ~170 lines; `listing::walk` stats nothing, so every entry is stat'ed again, unreadable folders are skipped silently (diskonaut shows "failed to read n files"), and it is single-threaded | the scan, the totals, delete | plugin `files::disk_usage(root, DuOptions::new().apparent(bool).hidden(true).one_file_system(bool))` → a model of signals (`total`, `count`, `failed`, `last_path`, `done`, `folder(parts)`), with `remove(path)` and a parallel walk; rs-rich-ext: `walk(..).with_metadata()` yielding `Result<(PathBuf, Metadata), (PathBuf, io::Error)>`, and `size_on_disk(&Metadata)`. The generic core shape under it: a **weighted tree** (`rollup::<K>()`: `add(path, weight)`, `remove(path)`, sums and counts up the path, a level's children as a memo) | plugin (files) + core (weighted tree) |
| **A "too small" screen.** diskonaut (and blocks before it) switch to a message under a size and ignore keys but `q`. Each clone writes the `memo` of the viewport, the `switch` and the key guard | every fixed-layout app | `App::min_size(50, 15)` with a default message (`.too_small(\|w, h\| node)` to change it), keys other than quit held while it shows | core |
| **A confirm in the app's own look, and one by typed text.** `cx.confirm` draws `[Y]es (N)o` in the framework's look; its options are the size and the title style. diskonaut's box is red on black with the question, the path and `(y/n)` centred, so the clone builds the modal from labels. A destructive delete of a large folder is where apps ask to type the name (GitHub's repository delete) | delete, quit | `ConfirmOptions::style(..)`, `.answers(Answers::Keys("(y/n)"))`, `.lines([question, detail])`; `cx.confirm_typed(title, expected, on_yes)` with an `input` that enables Yes when it matches | core |

### Friction with the existing APIs

- **`place` works only in `layers`.** The too-small message is a
  `layers([text(..).place(Center, Center)])` of one layer; on a plain
  node `.place` does nothing, silently.
- **A collapsed child's size counts the shared line.** In
  `row([a.fixed(5), b]).collapse_borders()` the next panel starts at
  column 4. Natural once known, but not in the guide; a treemap's
  `(x, w)` from line to line becomes `.fixed(w + 1)`.
- **`each` sizes its children as rows**: a child with no size is one
  row tall, so the rebuilt map needs `.fixed(rows)` from the viewport,
  read untracked when the child is built (a resize changes the key
  through the map's memo, which rebuilds it).
- **`.style()` on a panel fills its border too.** The selected tile's
  fill goes on the panel's content instead, so the joined frame stays
  in the border colour. The frame is `border_style("default")`: the
  dark theme's border is blue, diskonaut's lines are the terminal's.
- **Testing a scan fed from the test's thread.** `wait_for_tasks(true)`
  would wait on a stream that waits on the test, so the live-scan test
  runs without it and polls (`advance`, a 10 ms sleep, look again).
  `Driver::until(|d| .., timeout)` would say that once.
- **Signals and handlers worked well.** The whole state is a `Copy`
  struct of signals and one memo (the map, from the tree, the folder,
  the zoom and the viewport); `after` gives the 250 ms flashes, `spawn`
  the delete with its "Deleting" box (a `switch` on a signal inside the
  modal), `cx.modal_with` every box, and the headless driver with
  `wait_for_tasks(true)` makes the scan and the delete deterministic.

### Components that would help

| Component | What it would replace or enable | API sketch | Where | Value |
|---|---|---|---|---|
| A treemap node | the clone's squarify, the strips-to-nodes recursion, the rebuild, the movement (~400 lines); also any "what takes the room" view: memory by process, bundle size by module, tokens by file, a portfolio. ratatui has no treemap widget; the `treemap` and `streemap` crates lay one out; d3 and Plotly ship one | `treemap(items, \|key, rect\| node).min_tile(8, 3).aspect(2.5).rest(\|n\| node).selected(sig).on_enter(..)` with arrows by geometry built in; nested levels with `.depth(n)` (a folder's tiles inside its tile) | core | high |
| A disk-usage plugin | the tree, the walker and its batching (~220 lines); and the other shape of the same data, ncdu's / dua's / gdu's sorted list with size bars, Enter to descend | `files::disk_usage(root, ..)` as above, and `files::du_list(model)` (rows of name, size, a bar from rs-rich-ext's `SizeBar`, a share), both on the core's weighted tree | plugin (files) + core | high |
| Spatial focus | `step` and the selection signal; also panels on a dashboard moved between by arrows, a grid of cards | `focus_by_arrows()` on a container; `nav::neighbour` for widgets placing their own parts | core | medium |
| Fitting text | the title's telescope and every `fit` (~80 lines); Bubble Tea apps do it by hand, Textual's `Label` truncates only | `text_fit`, `fit_parts`, `truncate_middle` as above | core | medium |
| A rebuild node | the one-key `each` | `rebuild(key, build)` | core | medium |
| A min-size guard | the too-small switch and key guard (~30 lines, in blocks too) | `App::min_size(w, h)` | core | medium |
| Delete through the trash, as a job | diskonaut deletes for good, on the app's thread; the files plugin's `tasks()` runs `Op::Trash`/`Op::Delete` with progress and cancel, which a large folder needs ("Deleting" can take minutes) | `tasks().run(Op::Delete { .. })` already exists; what is missing is the model updating when a job ends (`disk_usage.remove(path)`) | plugin | low |
