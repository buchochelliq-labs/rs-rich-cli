# intuiTUIve gaps: bottom

## Round 1: bottom

[`examples/top.rs`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/rich-intuituive/examples/top.rs)
rebuilds [bottom](https://github.com/ClementTsang/bottom) (`btm`), the
system monitor, on intuiTUIve with no framework change. It is 2,706 lines
(after rustfmt): about 325 are the Linux data source (`/proc/stat`,
`/proc/meminfo`, `/proc/net/dev`, `/proc/diskstats`, `/proc/[pid]/{stat,status,io,cmdline}`,
`/sys/class/hwmon`, `df` for the disks' sizes and `kill` for the signals,
all through std), 700 the domain (bottom's units, the sliding history,
the search query language, sorted, grouped and tree rows), and the rest
the widgets, layout, keys and dialogs. The source sits behind a `Source`
trait (`sample()`, `kill(pid, signal)`), sampled on a `latest_every`
thread. Its tests ([`tests/top.rs`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/rich-intuituive/tests/top.rs),
769 lines, 25 tests) feed scripted samples through that trait, record the
signals sent, run the app on the app's clock (`wait_for_tasks(true)`,
`Driver::advance`) and assert on the rendered text.

What is there, with bottom's layout, strings, colours and default keys:

- **The default layout**: CPU across the top (30%), memory beside
  temperatures over disks (40%, 4:3), network beside processes (30%,
  1:1), plain borders, the focused widget's border in light blue.
- **CPU**: a Braille line per core and the average over a 60 s window
  (`60s` to `0s`, `0%` to `100%`), the legend at the right (15%): `All`,
  `AVG`, `CPU0`… with their use, each in its colour; ↑/↓ on the legend
  picks what the graph shows.
- **Memory**: RAM and swap lines, the legend box at the top right
  (`RAM: 25%   4.0GiB/16.0GiB`), hidden when too big, as bottom's.
- **Network**: RX and TX in bits a second, bottom's linear y scale (the
  top 1.5 times the largest rate in view, four labels in its unit), the
  legend `RX: 1.4Kb/s    All: 3.4GB`.
- **Temperatures** and **Disks** tables, sorted by `s`/`t` and
  `d m u n t p r w` (again reverses), or a click on a header.
- **Processes**: PID, Name, CPU%, Mem%, R/s, W/s, T.Read, T.Write, User,
  State, Time, bottom's cell formats; sorted by CPU; `c m p n` sort (again
  reverses), `I` inverts, `s`/F6 opens the "Sort By" menu at the left,
  Tab groups by name (PID becomes Count, usage summed), `P` the full
  command, `%` memory as sizes, `t`/F5 tree mode (`├─`, `└─`, `│`, `+` for
  a collapsed branch; Space, `+`, `-`, ←, → collapse and expand), `z`
  hides kernel threads, Ctrl+U/Ctrl+D half a page.
- **Search** (`/` or Ctrl+F): bottom's box under the table (`> query`,
  an error line, `Case(Alt+C)  Whole(Alt+W)  Regex(Alt+R)` lit when on,
  `Esc to close`); the query language's keywords (`pid`, `cpu`, `mem`,
  `memb` with units, `read`, `write`, `user`, `state`), comparisons, `and`
  (or a space), `or`, `!`; filtering as you type; Esc closes and keeps the
  filter; a tree keeps the ancestors of what it finds.
- **Kill** (`dd`, F9 or Delete): bottom's Linux dialog, "Kill process
  'cargo' with PID 420? Press ENTER to confirm." over the list of signals
  with `15: TERM` picked (`0: Cancel` sends nothing); a group's every
  process; a failure is an error notification. `--disable_advanced_kill`
  gives the yes/no confirm (`cx.confirm`).
- **Widget focus**: Ctrl or Shift+arrows, `H J K L` and `A S W D` move to
  the widget on that side, from where each was drawn; a click selects one.
- **Expand** (`e`): the selected widget fills the screen with `Esc to go
  back` at the right; `e` or Esc returns, the focus kept.
- **Zoom**: `+`/`-` (and the wheel over a graph) change that graph's
  window by 15 s, from 30 s to the 10 minutes kept; `=` resets it.
- **Freeze** (`f`, `Frozen, press 'f' to unfreeze` on the last row),
  **reset** (Ctrl+R), **help** (`?`, bottom's sections in a scrolling
  box), **quit** (`q`, `Q`, Ctrl+C).
- **Basic mode** (`-b`): per-core pipe gauges (`AVG [|||||    39%]`),
  RAM/SWP gauges, RX/TX and totals, and one table at a time with bottom's
  `◄ Processes`/`Temperatures ►` carousel, moved with Ctrl+←/→ or `H`/`L`.

Left out: the battery, GPU, disk I/O and temperature graphs, cache and
ARC lines, custom layouts from a config file (and the config file
itself), regex search (no regex engine among the crate's dependencies:
the toggle shows, a pattern matches as text), the time axis auto-hiding,
the legend position options, the packet counts, the widgets' scroll
position in their titles, and per-process GPU columns. Linux only.

What is bespoke, and why:

- **The rectangle registry and neighbour search** (`Places`,
  `neighbour`, about 120 lines): bottom moves the selection by its layout
  tree; intuiTUIve has no directional focus, so each widget's focusable
  node is kept by name with a `RectHandle`, and the nearest one on a side
  is picked. It is also how a node a `switch` builds later gets the focus
  (a pending name and a `watch` on its handle). Both are gaps below.
- **The pipe gauges** in basic mode are formatted text (`pipe_gauge`), as
  wide as `viewport()` allows: there is no gauge node in that style, and a
  text closure does not know its width.
- **The history** (`History`, `series`): a sliding window by time over
  the samples, what a time-series chart would hold.

### New gaps and bugs

| Gap or bug | Needed for | API sketch or repro | Where |
|---|---|---|---|
| Panel titles are fixed when built: `.panel(title)`, `add_title`, `title_right` take a string | bottom's `CPU ─ 0.52 0.48 0.40` (the load average, every sample), `Processes (3 of 210)` when expanded; any title with a count or a state | `.panel_with(\|\| markup)`, `.add_title_with(Edge, Align, \|\| markup)` that read signals | core |
| No directional focus: Tab order only, and a handler cannot ask where widgets are | Ctrl/Shift+arrows, HJKL, WASD between widgets (bottom, every tiling dashboard, tmux-like panes) | `cx.focus_toward(Direction::Left)`: the nearest focusable node (or `.focus_group()` root) on that side of the focused one, by the last frame's rectangles | core |
| `cx.focus(id)` of a node a `switch` has not built yet is lost, and `cx.after(Duration::ZERO, ..)` runs before the frame that builds it | the search box and sort menu taking the focus when they open; the expanded widget, and back; the process table rebuilt with new titles | `cx.focus_when_shown(id)` kept pending until the node is laid out; or `Node::focus_on_show()` | core |
| A handler cannot ask which node has the focus | one place for "the selected widget's" keys (`e`, `+`, `-`): today each widget binds its own copy so the binding knows whose it is | `Ctx::focused() -> Option<NodeId>`, `Ctx::focus_within(id) -> bool` | core |
| A table's columns are fixed: titles cannot follow state | PID → Count when grouped, Name → Command (`P`), Mem% → Mem (`%`): the clone has a `switch` with a table per set of titles, each with its own sort signal mirrored to the view, and moves the focus across | `table_columns(\|\| Vec<Column>, rows, selected, options)`, or `Column::title_with(\|\| ..)` | core |
| A table cannot drop columns that do not fit: a `Flex` column gets what the `Auto` ones leave (Name vanished at 58 columns until given `.min(16)`), and the last `Auto` columns are cut with `…` | bottom hides columns from the right and keeps Name; disks at 50 columns lose R/s and W/s whole | `Column::priority(n)` (dropped lowest first, whole), `TableOptions::drop_columns()` | core |
| **Bug:** a table (and a list) takes Shift+↑/↓ as ↑/↓: its key match checks only Ctrl and Alt. An ancestor's `shift+up` binding never fires while a table has the focus. Repro: `column([table(..)]).on_key("shift+up", f)`, press Shift+↑: `f` is not called, the selection moves | bottom's Shift+arrows between widgets; the clone makes every widget's panel `previews_keys` | treat a key with Shift as not the plain arrow (unless the table scrolls across with it) | core |
| A table's keys are fixed: `g` jumps to the top at once, `s` sorts when `.sort` is set | bottom's `gg`, `s` (the sort menu; the temperature table's sort by sensor) need `previews_keys` round the table | `TableOptions::keys(KeyMap)` or `.no_key("g")`; the table's `g` as a `g g` sequence option | core |
| An `.auto()` `switch` measures its page's content, ignoring the page's `.fixed(n)` and `.min_size(n)` | the 11-wide sort menu came out 9 wide (its title cut to `So…`); the clone pads the menu's items | measure a child's own fixed size, as a `row` lays it out | core |
| No reactive hiding: `class_when` restyles but does not lay out (`display: none` follows fixed classes only), `hide_below` follows the viewport | expand: the clone builds a second view per widget under a `switch` on the expanded widget, so every bit of state must live in signals, and focus is moved by hand | `.show_when(\|\| bool)` / `.hide_when(..)`, a reactive `display: none` | core |
| A text node does not know its width | basic mode's gauges sized from `viewport()` rather than their cell | `text_sized(\|width\| markup)`, or the gauge node below | core |
| No regex engine may be added | bottom's Regex(Alt+R) search | a pattern matcher in rs-rich-ext (`text::Pattern::regex(..)`), used by the core finder's matcher too | plugin (rs-rich-ext) |
| `rich_ext::format` has one style (`1.5 kB`, `2.0 GiB`, a space, `bytes`) | bottom's `4.0GiB`, `900MiB`, `2KB/s`, `1.4Kb/s` (bits), `1m 15.00s`; the clone has its own helpers | `format::Units::binary().precision(1).compact().bits()` with `.bytes(n)`, `.rate(n)`; a duration style | plugin (rs-rich-ext) |
| `Driver::event` does not settle: a test of a mouse event must call `settle()`, where `press` does | the wheel test | `event` settles as `press` does, or `event_settled` | core |

### Friction with the existing APIs

- **Panels and ratatui's `Block` titles differ by a glyph.** ratatui puts
  ` Sort By ` against the corner (`┌ Sort By ┐`, 11 wide); a panel puts a
  line glyph first (`┌─ Sort By ─┐`, 13), so bottom's narrow boxes cut
  their titles. A `.title_inset(0)` would port them as they were.
- **Charts are rebuilt whole every sample.** `renderable(|| Chart)` makes
  every dataset again from the history (seven series, 60 points each);
  fine at a second, but a time-series node that appends a point would be
  cheaper and would not need the `History` in the example.
- **`switch` keeps every page**, which suits tabs, but the expanded views,
  the process tables per title set and the search box each keep their
  nodes (and their `watch`es) forever once shown.
- **The process table's sort is two signals.** The table needs its own
  `Signal<Option<(usize, Order)>>` for the header's arrow and clicks; the
  view keeps the sort with the other options, so two `watch`es mirror
  them.
- **A modal's list needs a name to be found**: `focused_name()` is `None`
  for an unnamed list, which read at first as the focus being lost.
- **`previews_keys` everywhere.** Four containers preview keys (each
  widget's panel, the process table's, the temperature and disk tables'),
  each to get round a table's own bindings.

### Components that would help

| Component | What it would replace or enable in top.rs | API sketch | Where | Value |
|---|---|---|---|---|
| A time-series chart with a sliding window | `History`, `series`, `time_axis`, the three chart builders and the zoom keys (bottom vendors ratatui's `Chart` as `TimeChart` for this; Textual has `PlotextPlot`, ratatui-toolkit and tui-realm have live charts) | core: `window_by_time(signal, Duration)` (a `recent` by time, generic); plugin (`viz`): `time_chart(series, window).zoom(step, min..max).y(Scale::Percent \| Auto)` with the `60s`/`0s` axis and wheel zoom | core + plugin | high |
| Directional focus | `Places`, `neighbour`, `move_focus`, the four bindings on each widget | `App::directional_focus("ctrl+left shift+left H A", ..)` or `cx.focus_toward(dir)` | core | high |
| Dense sortable tables with column priorities | the per-title `switch`, the mirrored sort, the padded menu, `.min(16)` on Name | `table_columns(\|\| cols, ..)`, `Column::priority`, `TableOptions::keys` | core | high |
| A tree table | `tree_rows` and its prefixes, `collapsed`, the Space/←/→/+/- bindings (bottom's tree mode; htop's, btop's) | `tree_table(columns, roots, children, selected)` with the core `tree`'s keys and per-row cells | core | high |
| Layout from a config | bottom's `[[row]]`/`[[row.child]]` layouts with `ratio` and `default=true`; the hand-built `dashboard` | `layout_from(toml_or_value, \|type\| node)`: rows and columns of named widgets with weights; a stylesheet already sizes nodes, this would build them | core (generic: names to nodes) | medium |
| Expand (zoom) a pane | the `expanded` signal, the second view per widget, `Esc to go back` | `node.zoomable()` and `cx.zoom(id)`: the node drawn over the whole screen, its state kept (tmux's `z`, Zellij's fullscreen) | core | medium |
| A gauge node | `pipe_gauge` and the `viewport()` sums | `gauge(\|\| ratio).label(..).inner(..).style(Gauge::Pipe \| Blocks \| Line)` (ratatui's `Gauge`/`LineGauge`, btop's and htop's meters) | core | medium |
| Unit formatting | `binary_string`, `decimal_string`, `per_second`, `bits_label`, `format_time`, `memory_label` | `rich_ext::format::Units` (binary/decimal, bits, precision, compact) | plugin (rs-rich-ext) | medium |
| A system-metrics source | the 325-line `Linux` source and `Sample` | none in core (a domain); plugin: `system::sample()` (CPU, memory, network, disks, temperatures, processes) and `system::kill(pid, signal)`, on `sysinfo` or `/proc`, cross-platform; fed through the core's `latest_every` | plugin | high |
| A query language for tables | `Query`, `Cond`, `keyword`, `unit_bytes` | core: `table_filter(\|row\| ..)` with a parsed `field op value and/or !` expression over named columns; plugin: process keywords | core + plugin | medium |
| Mouse selection in tables, wheel per widget | the wheel works; a click selects a row and focuses a widget already | `.on_wheel(\|cx, delta\| ..)` sugar over `on_mouse` | core | low |
