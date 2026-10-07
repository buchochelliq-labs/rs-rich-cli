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
  - `column` and `row`, with `fixed`, `percent` and `flex` sizes;
  - `each` for keyed lists;
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
- **Timers and threads:** `every(interval, …)` runs a handler on a schedule,
  and a `Proxy` lets another thread write signals safely.
- **Testing:** `App::render_with(keys, width, height)` and `App::run_on` with
  `rs-rich-interact`'s headless driver run an app without a terminal.

## Against ratatui

The same ops dashboard is written with ratatui 0.30 and with intuiTUIve in
`tests/versus_ratatui.rs`. It measures the release-mode cost per frame, and
the bytes sent per frame:

| 80x24 | ratatui | intuiTUIve |
|---|---:|---:|
| status tick | 86 µs, 37 B | **12 µs, 18 B** |
| selection move | 96 µs, 184 B | **39 µs, 118 B** |
| log append | 95 µs, 443 B | **46 µs, 394 B** |

At 200x60 the gap widens: a tick costs 17 µs against 354, a selection move
46 against 309, and a log append 224 against 362. intuiTUIve's version is 56
lines of app code against ratatui's 79.

The test asserts this bar, and CI runs it. The design, and the ratatui
problems it is built to avoid, are in
[`docs/design/intuituive.md`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/docs/design/intuituive.md).

## Status

This is an early slice (0.0.x), so the API will change. Inline
(non-full-screen) mode, constraint and grid layout, and screens with
navigation come next.

Licensed under MIT.
