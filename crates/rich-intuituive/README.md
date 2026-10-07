# rs-rich-intuituive

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
- **Testing:** `App::render_with(keys, width, height)` and `App::run_on` with
  `rs-rich-interact`'s headless driver run an app without a terminal.
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
three columns, vim keys, previews highlighted in the background.
`examples/scope.rs` rebuilds [scope-tui](https://github.com/alemidev/scope-tui),
an oscilloscope, vectorscope and spectroscope that redraws with every
buffer of audio.

## Credits

The example ports rebuild the behaviour of other people's ratatui apps
and reuse none of their code; the designs are theirs:

- `examples/files.rs`: [Yazi](https://github.com/sxyazi/yazi), by sxyazi
  and contributors (MIT).
- `examples/scope.rs`: [scope-tui](https://github.com/alemidev/scope-tui),
  by alemi (MIT).

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

The test asserts this bar, and CI runs it. The design, and the ratatui
problems it is built to avoid, are in
[`docs/design/intuituive.md`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/docs/design/intuituive.md).

## Status

This is an early slice (0.0.x), so the API will change. Python bindings for
the framework come next.

Licensed under MIT.
