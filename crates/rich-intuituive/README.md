# rs-rich-intuituive

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="https://raw.githubusercontent.com/buchochelliq-labs/rs-rich-cli/main/docs/assets/intuituive/hero-dark.svg">
  <img src="https://raw.githubusercontent.com/buchochelliq-labs/rs-rich-cli/main/docs/assets/intuituive/hero-light.svg" alt="intuiTUIve: terminal apps from signals, with Memo the elephant and her calf Tui" width="100%">
</picture>

**intuiTUIve**: a reactive, retained terminal UI framework built on
[rs-rich](https://github.com/buchochelliq-labs/rs-rich-cli). It is part of
rs-rich, an addition rather than a port.

You describe the screen once, as a tree of nodes, and keep your state in
signals. When a signal changes, only the nodes that read it draw again, and
only the cells that changed reach the terminal. You never write a draw loop,
call a layout pass, or track what changed.

```rust,no_run
use intuituive::prelude::*;

fn main() -> std::io::Result<()> {
    App::new(|| {
        let count = signal(0);
        column([
            text!("[b]Count:[/] {count}").panel("Counter"),
            label("[dim]+ adds one · q quits"),
        ])
        .on_key("+", move |_| count.update(|c| *c += 1))
        .on_key("q", |cx| cx.quit())
    })
    .run()
}
```

- **Nodes** are built once and kept:
  - `text!` and `label` for console markup;
  - `renderable` for any rich renderable (`Table`, `Markdown`, `Syntax`,
    charts);
  - `column`, `row` and `grid`, with `fixed`, `percent`, `flex` and
    content (`auto`) sizes, `min_size`/`max_size`, gaps, padding and
    spans;
  - `each` for keyed lists, `switch` for one child of several (tabs);
  - `panel` for a titled border;
  - `Log` for streaming output;
  - `component` for `rs-rich-interact`'s inputs, selectors, forms and
    pagers.
- **State** is `signal` and `memo`. A node that reads one while drawing
  subscribes to it; writing it redraws exactly its readers. A `memo`
  notifies only when its value changes.
- **Input:**
  - `on_key` bindings bubble from the focused node up to the root;
  - Tab moves the focus;
  - `on_click` receives clicks, routed by the layout the last frame kept;
  - a focused component takes keys first and shows its caret.
- **Screens:** `cx.push`, `cx.modal` and `cx.pop` keep a stack of screens,
  each with its own state, focus and timers.
- **Background work:** `every(interval, …)` runs a handler on a schedule;
  `spawn`, `spawn_future` and `resource` run slow work off the app's thread
  and write signals with the result; a `Proxy` lets any thread do the same.
- **Themes:** dark, light and mono presets with named styles for markup
  (`[accent]…[/]`), switched at run time with `cx.set_theme`.
- **Inline apps:** `App::inline(rows)` runs in a few rows under the prompt
  and leaves its last frame in the scrollback.
- **Responsive:** `.hide_below(cols)` and `.show_below(cols)` on a node,
  and `@media (max-width: 80)` in a stylesheet, follow the terminal's size;
  `viewport()` reads it.
- **Errors:** `.error_when(…)` puts a node in error, styled by the theme and
  a stylesheet's `:error`, and told to screen readers as invalid.
- **Testing:** `App::render_with(keys, width, height)` and `App::run_on` with
  `rs-rich-interact`'s headless driver run an app without a terminal.
  `App::driver` steps one (`press`, `type_text`, `assert_focused`), and
  `assert_screen!(driver, "name")` compares its screen with a snapshot in
  `tests/snapshots` (`UPDATE_SNAPSHOTS=1` writes it).
- **Inspector:** `INTUITUIVE_INSPECT=1` (or `App::inspector(true)`) docks a
  live view of the node tree, what drew in the last frame, the focus and
  the frame's cost; F12 toggles it.
- **Live styles:** `App::theme_file("theme.ini")` reloads its styles while
  the app runs.

New to it? The
[tutorial](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/docs/guide/intuituive/tutorial.md)
builds a to-do app step by step, and
`cargo generate --git https://github.com/buchochelliq-labs/rs-rich-cli templates/intuituive-app`
starts a project. `intuituive::rich` and `intuituive::interact` re-export
rs-rich and rs-rich-interact, so one dependency is enough.

Coming from ratatui? The
[porting guide](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/docs/guide/intuituive/porting.md)
maps every concept, and `examples/files.rs` rebuilds the core of
[Yazi](https://github.com/sxyazi/yazi), the most-starred ratatui app:
its layout, theme, status line and keymap, previews highlighted in the
background, all from the framework's ready-made components.
ratatui's own example apps are rebuilt too, starting with `demo` and
`demo2`: see
[ratatui's examples, rebuilt](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/docs/guide/intuituive/ratatui-examples.md).

More examples (`cargo run -p rs-rich-intuituive --example <name>`):

| Example | Shows |
|---|---|
| `todo` | the tutorial's app: signals, a list, an input, a modal |
| `screens` | screens, a grid of cards and a modal |
| `inline` | an inline app that keeps the scrollback |
| `meters` | widgets of your own: retained drawing, focus, resize and pointer events, keys seen first |
| `planner` | the components together: a menu bar, a tree, split panes, a calendar, a virtual list, the palette |
| `files` | a Yazi-style file manager: lists in split panes, a preview on a worker, chords with a which-key pop-up, prompts, confirmations, the help, tabs, a right-click menu |
| `scope` | an oscilloscope ported from scope-tui, on the chart widget |
| `dashboard` | an ops dashboard: a service list, a detail pane, a streaming log (the benchmark measures it) |
| `in_ratatui` | an app inside a ratatui program, which owns the loop |
| `demo` | ratatui's classic demo rebuilt: gauges, a sparkline, lists, bars, a sine chart, a server table on a world map, colours; a tick redraws only what moves |
| `demo2` | ratatui's showcase rebuilt: a recipe, an inbox, a traceroute on a map, the weather, and destroy, an effect; its colours in a stylesheet |
| `widgets` | ratatui's widget examples as a gallery: blocks, text, lists, tables, tabs, scrollbars, a calendar, a pop-up, collapsed borders, buttons of your own |
| `charts` | ratatui's chart examples as a gallery: gauges, sparklines, bars, grouped bars, line, area and scatter charts, the canvas and its map |
| `layout` | ratatui's constraint explorer rebuilt: sizes and justify modes with their spacers, each kind of size, flex, alignment and wrapping |
| `colors` | ratatui's colour explorers rebuilt: named, indexed and RGB colours (a swatch at 60 fps), the modifiers, Tailwind's and Material's palettes |
| `canvas` | ratatui's canvas rebuilt: a world map, pong at 60 fps, a pad to draw on with the mouse, boxes; every marker |
| `form` | ratatui's input form and chat rebuilt: fields in error while wrong, a form handed back from `run_for`, an input with modes |
| `calendar` | ratatui's calendar explorer rebuilt: a year of `month` nodes, six looks, today, the seasons and a selected day |
| `downloads` | ratatui's inline demo rebuilt: workers on threads report through a `Proxy`, finished lines printed above the region |
| `pulls` | ratatui's async GitHub table rebuilt: a `resource` loaded in the background, Loading, Loaded or Failed in the title |
| `panic` | ratatui's panic demo rebuilt: the terminal given back before a panic's message, an error returned from `run` |

## Against ratatui

The same ops dashboard is written with ratatui 0.30 and with intuiTUIve in
`tests/versus_ratatui.rs`. It measures the release-mode cost per frame, and
the bytes sent per frame:

| 80x24 | ratatui | intuiTUIve |
|---|---:|---:|
| status tick | 95 µs, 37 B | **15 µs, 18 B** |
| selection move | 97 µs, 184 B | **39 µs, 118 B** |
| log append | 94 µs, 443 B | **47 µs, 428 B** |

At 200x60 the gap widens: a tick costs 18 µs against 339, a selection move
50 against 360, and a log append 248 against 333. intuiTUIve's version is 56
lines of app code against ratatui's 79.

Two more scenes are measured and reported, not gated: a full-screen churn,
where every cell changes each frame (an RGB swatch, as ratatui's
`colors-rgb`), and a braille line chart redrawn each tick. There ratatui is
ahead, since nothing is saved by drawing only what changed when everything
does:

| 80x24, release | ratatui | intuiTUIve |
|---|---:|---:|
| churn, every cell | **326 µs**, 62 kB | 3,042 µs, 66 kB |
| a chart redrawn | **81 µs**, 4.2 kB | 463 µs, **3.1 kB** |

The test asserts this bar, and CI runs it. The design, and the ratatui
problems it is built to avoid, are in
[`docs/design/intuituive.md`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/docs/design/intuituive.md).

## Status

This is an early slice (0.0.x), so the API will change. Python bindings for
the framework come next.

Licensed under MIT.
