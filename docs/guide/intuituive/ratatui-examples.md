# ratatui's examples, rebuilt

ratatui's example apps, rebuilt on intuiTUIve one by one. Each page section
says what the example shows, how to run it, its keys, and what intuiTUIve
adds to the original. They are ports in the sense of the
[porting guide](porting.md): none of ratatui's code is reused, the
behaviour and the look are rebuilt with intuiTUIve's API, and the sample
data is our own. Each example credits ratatui in its header comment, and
the porting guide's [Credit for ports](porting.md#credit-for-ports) does too.
The plan, with the gaps each port needed closed first, is in
[intuiTUIve against ratatui](../../design/intuituive-vs-ratatui.md#3-the-demos).

Every port follows the same conventions, so it can be tested and recorded:

- **Testable.** `examples/<name>.rs` exposes `<name>_app(..)`, and
  `tests/<name>.rs` includes the example and drives it headless, with
  snapshots of its screens in `tests/snapshots/`.
- **Deterministic.** Random data comes from a seeded generator in the
  example, and time from timers, so a test moves a virtual clock.
  `--frozen` stops the clock where an example has one.
- **Refined.** Tabs, lists and tables take the mouse; `?` lists the keys
  and Ctrl+P finds a command, from the bindings' descriptions; the nodes
  have labels, so [linear mode](index.md#linear-mode) reads well; and only
  what moves draws again, which a test checks.

## The showcases

### `demo`: the classic demo

```bash
cargo run -p rs-rich-intuituive --example demo
cargo run -p rs-rich-intuituive --example demo -- --tick-rate 100 --unicode false
cargo run -p rs-rich-intuituive --example demo -- --frozen
```

ratatui's first demo: three tabs of widgets, fed by a timer.

- **Graphs.** A gauge, a sparkline and a line gauge; a list of tasks and a
  log that turns a line each tick; a bar chart; a chart of two sine waves
  whose window moves on; and a paragraph of styled, wrapped text.
- **World.** A table of servers, the failed one red, blinking and struck
  through, beside a world map with the servers, the lines between them, a
  circle and a rectangle.
- **Colours.** Each named colour, and the terminal's own, as a foreground
  and as a background.

```text
--8<-- "crates/rich-intuituive/tests/snapshots/demo_graphs.txt"
```

| Keys | What they do |
|---|---|
| ←/→, h/l | the tab before or after, going round |
| ↑/↓, k/j | the task before or after, going round |
| t | show or hide the chart |
| ? | the keys |
| Ctrl+P | the command palette |
| q | quit |

A click picks a tab, a task or a server, and the wheel moves through the
tasks. `--tick-rate MS` sets how often the data moves (250 ms by default),
`--unicode false` draws with the three-level block sets and dots, and
`--linear` runs it in linear mode.

What intuiTUIve adds:

- **Only what moves draws again.** The gauges, the sparkline, the log, the
  bars and the chart each read a signal of their own, so a tick draws them
  and nothing else: not the tasks, the footer or the tab strip.
  `tests/demo.rs` checks it with the frame's numbers and the
  [inspector](index.md#inspector)'s timings, and the inspector shows it
  as it runs (run it with `INTUITUIVE_INSPECT=1`).
- **The widgets are rs-rich-ext's charts** as nodes: `FillGauge`,
  `Sparkline`, `LineGauge`, `BarChart`, `Chart` and the `Canvas` with its
  world map, each in a `renderable(..)` or a `leaf(..)` that fills its
  node. The panels' titles take markup, and `.border(Border::None)` gives
  the gauges their headings, as ratatui's blocks with a title and no
  border do.
- **The charts have labels**, so linear mode names them in words rather
  than reading out Braille.

### `demo2`: the showcase

```bash
cargo run -p rs-rich-intuituive --example demo2
cargo run -p rs-rich-intuituive --example demo2 -- crates/rich-intuituive/examples/demo2.tcss
```

ratatui's second demo, the one in its README, over a colour swatch:

- **About.** intuiTUIve's mark, a terminal window with a prompt, in half
  blocks over the swatch, and a description under a rule.
- **Recipe.** A recipe's steps beside its ingredients, a table with rows of
  several lines and a scrollbar.
- **Email.** An inbox with a scrollbar, and the open email under a thick
  rule.
- **Traceroute.** A table of hops, a sparkline of ping times, and a map of
  Europe with the selected hop's path.
- **Weather.** A month with today marked, the week's highs as bars, the
  seasons as bars with their values inside, and a download's line gauge
  that turns from yellow to red as it fills.
- **Destroy.** d makes the cells drip down the screen, then our wordmark
  fades in over them; d again brings the app back.

```text
--8<-- "crates/rich-intuituive/tests/snapshots/demo2_email.txt"
```

| Keys | What they do |
|---|---|
| ←/→, h/l, Tab | the tab before or after |
| ↑/↓, k/j | up and down inside the tab: the mark's colour, the ingredient, the email, the hop, the download |
| d, Delete | destroy, or bring back |
| ? | the keys |
| Ctrl+P | the command palette |
| q, Esc | quit |

A click picks a tab, an ingredient, an email or a hop, and the wheel moves
through them. `--today 2026-10-18` sets the day the calendar marks.

What intuiTUIve adds:

- **A stylesheet.** The frame's colours, the panels' and the key chips' are
  in `examples/demo2.tcss`, which the app reads again whenever it changes
  when its path is given, rather than in a struct of styles. The
  highlights inside the widgets (the selected tab, row or email) are theme
  styles given to each widget with `.theme_styles(..)`.
- **Any size.** ratatui's is fixed at 81×18, the size of its recording.
  This one fits the terminal: under 80 columns the swatch's margins go,
  and under 60 the map and the mark hide.
- **Destroy is an effect.** `effect(app, ..)` runs over the whole app,
  reading a signal that `cx.animate` moves, so a test runs it on the
  virtual clock and gets the same screen each time. While it is idle, the
  effect costs nothing: a key draws only what it changes, not the swatch,
  the title bar or the key chips.
- **The swatch is chrome.** It is a widget of its own that draws the
  swatch round its content and leaves the content's area blank, as
  ratatui's demo clears it, so the content's padding takes its background.

## The galleries

### `widgets`: a widget gallery

```bash
cargo run -p rs-rich-intuituive --example widgets
cargo run -p rs-rich-intuituive --example widgets -- --tab 4
```

ratatui's widget examples, one tab for each family, with its `table`,
`scrollbar`, `popup`, `custom-widget` and `hyperlink` apps folded in:

- **Block.** Plain, styled, rounded red, thick, double and dashed boxes;
  titles left, right, centred and on the bottom edge; a box with only its
  top and bottom; a shadow.
- **Text.** Lines centred, at the right and justified; a recipe wrapped
  with bold and italic runs; a line cut with an ellipsis; a masked secret
  (m shows it); a link, which the terminal opens through OSC 8.
- **List.** A list with `> ` and a reversed highlight, and a chat that
  runs bottom to top, its messages on several lines, a red `>>` repeated
  on each, with scroll padding.
- **Table.** A table of dishes with a bold header and a gap under it, an
  italic footer of totals, wider column spacing, the row, column and cell
  highlights and a `🍴` symbol; v shows ratatui's table app: striped rows
  of four lines, a column highlight, a double scrollbar, and tailwind
  palettes that c cycles.
- **Tabs.** One strip in four looks (the default line, a dot between,
  brackets round each title, centred), all on one selection, over the
  selected tab's content.
- **Scrollbar.** ratatui's four looks: down the right with arrows, down
  the left with no track, along the bottom with a begin arrow and a thumb
  of its own, and along the bottom with a track of its own.
- **Calendar.** A month with its events and the days of the months
  around it, the next month with the weeks starting on Sunday, and the
  events listed.
- **Popup.** p opens a pop-up 60% wide and 20% tall over a blue panel,
  with a shadow; under it stand four shadow looks.
- **Borders.** Four panes sharing their borders; the one with the focus
  is drawn thick.
- **Custom.** Three buttons drawn by hand: a widget each, with a face, a
  lit top edge and a shadowed bottom one, picked under the pointer or the
  focus, pressed while the mouse holds them.

```text
--8<-- "crates/rich-intuituive/tests/snapshots/widgets_table_app.txt"
```

| Keys | What they do |
|---|---|
| ←/→, 1-9, 0 | the tab before or after, or tab 1 to 10 |
| ↑/↓, Tab | inside the tab: the list item, the row, the pane, the button |
| m | Text: show or mask the secret |
| v, c | Table: the other table; the table app's next colours |
| ←/→ | Table: the cell (the numbers then change tabs) |
| n, p | Tabs: the inner tab; Calendar: the month |
| j/k, h/l | Scrollbar: scroll down and up, across |
| p, Esc | Popup: open and close the pop-up |
| Enter, Space | Custom: press the button |
| ? | the keys |
| Ctrl+P | the command palette |
| q | quit |

A click picks a tab, an item, a row, an inner tab, a pane or a button, and
the wheel scrolls. `--tab N` opens on tab N, and `--linear` runs it in
linear mode. The gauges, sparklines, bar charts and the canvas are in the
chart gallery.

What intuiTUIve adds:

- **The widgets are options, not types.** A list's symbol, direction and
  padding, a table's footer, gaps and highlights, a panel's titles,
  sides and shadow are builders on the node, and the highlights are
  theme styles given to each widget with `.theme_styles(..)`.
- **The custom buttons are ordinary widgets.** Each implements `Widget`:
  it draws on its canvas, asks `cx.hovered()` and `cx.focused()`, and
  captures the mouse while it is held.
- **Only what changes draws again.** A key in the List tab draws the list
  and nothing else; `tests/widgets_gallery.rs` checks it.

### `charts`: a chart gallery

```bash
cargo run -p rs-rich-intuituive --example charts
cargo run -p rs-rich-intuituive --example charts -- --tick-rate 100
cargo run -p rs-rich-intuituive --example charts -- --frozen
```

ratatui's chart examples, on rs-rich-ext's charts:

- **Gauge.** ratatui's `gauge` app: four gauges filling at their own
  speeds, a percentage, a ratio with a label of its own, whole cells and
  eighths of a cell; and the `line-gauge` example's looks, thin, thick,
  double and shapes of their own.
- **Sparkline.** Random values left to right; right to left with missing
  values marked `×`; Braille, two values a cell; and the first one's
  values as bars beside them. They move a value each tick.
- **Bars.** ratatui's `weather` app, the day's temperatures by the hour
  with `°` values, coloured by how warm it is; and the `barchart`
  example's vertical and horizontal charts.
- **Groups.** `barchart-grouped`: revenue by quarter for three companies
  of our own, vertical and horizontal.
- **Chart.** ratatui's `chart` app: a sine and a cosine in a window that
  moves with the clock, with bold end labels and a legend; an area chart;
  a chart of bars; a line chart with its legend top left; and a scatter
  of four markers with its legend bottom right. The axes show each kind
  of label and alignment.
- **Canvas.** The `canvas` example's world map, with "You are here", and
  shapes: a rectangle, a circle, lines, points and text. m goes through
  every marker, Braille to octants.

```text
--8<-- "crates/rich-intuituive/tests/snapshots/charts_chart.txt"
```

| Keys | What they do |
|---|---|
| ←/→, h/l, 1-6 | the tab before or after, or tab 1 to 6 |
| m | the canvas's next marker |
| Space | stop or start the clock |
| ? | the keys |
| Ctrl+P | the command palette |
| q | quit |

A click picks a tab. `--tick-rate MS` sets how often the data moves (200
ms by default), `--frozen` moves it on 42 ticks (the first gauge at 42%)
and stops the clock, and `--linear` runs it in linear mode.

What intuiTUIve adds:

- **Only what moves draws again.** The gauges, the sparklines and the
  moving chart read the clock; a tick draws them and nothing else, and on
  a tab where nothing moves it draws nothing. `tests/charts.rs` checks it.
- **The charts fill their nodes.** Each is a `renderable(..)` or a
  `leaf(..)` laid out like any other node.
- **The charts have labels**, so linear mode names them in words.

### `layout`: a layout explorer

```bash
cargo run -p rs-rich-intuituive --example layout
```

ratatui's `constraint-explorer`, `constraints` and `flex`:

- **Explorer.** A row of blocks you edit, drawn once for each `Justify`
  mode with the spacers between them (`·`, from `split_with_spacers`),
  each block with its size on its top edge and the width it got on its
  bottom one.
- **Sizes.** Each kind of size (fixed, percent, ratio, flex, min, max) in
  rows of real nodes, each showing the width it got.
- **Flex.** Each `Justify` mode, with no gap and a gap of two, in a
  scroll.
- **Beyond.** What ratatui's layout has no counterpart for:
  `align_items` across a row, and a row that wraps. The `align` example
  changes each with a key.

```text
--8<-- "crates/rich-intuituive/tests/snapshots/layout_explorer.txt"
```

| Keys | What they do |
|---|---|
| Tab, Shift+Tab | the next or the tab before |
| ←/→ | Explorer: the block; Sizes: the kind |
| 1 to 6 | Explorer: make the block fixed, percent, ratio, flex, min or max |
| ↑/↓ | Explorer: the block's size; Flex: scroll |
| a, x | Explorer: add a block, remove the picked one |
| +, - | Explorer: a wider or narrower gap |
| ? | the keys |
| Ctrl+P | the command palette |
| q | quit |

ratatui's `Length`, `Percentage`, `Ratio` and `Fill` are `Size::Fixed`,
`Size::Percent`, `Size::Ratio` and `Size::Flex`; its `Min(n)` and `Max(n)`
are a flexible node with `.min_size(n)` or `.max_size(n)`. A flexible
block takes the room left over, so with one in the row every `Justify`
mode looks the same, as in CSS.

## The apps

The smaller apps come with the batch after.
