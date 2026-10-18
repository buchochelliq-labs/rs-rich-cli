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

The widget, chart and layout galleries come with the next batch of ports.

## The apps

The smaller apps come with the batch after.
