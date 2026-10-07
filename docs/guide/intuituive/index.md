# Terminal apps (intuiTUIve)

`rs-rich-intuituive` (`intuituive`, new and early) is a framework for
full-screen terminal apps. You describe the screen once as a tree of nodes
and keep your state in **signals**. When a signal changes, the nodes that
read it draw again, and only the cells that changed are sent to the
terminal. You never write a draw loop, a layout pass, or "what changed"
bookkeeping.

It builds on `rs-rich-interact`, so its sessions, its headless test driver
and its components (inputs, selectors, forms, pagers) all work inside an
app. Any rich renderable (tables, Markdown, syntax, charts) is a node.

```toml
[dependencies]
rs-rich-intuituive = "*"
```

## A first app

```rust
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

The closure passed to `App::new` runs once. It creates the state (`count`)
and returns the root node. The rest happens on its own:

- `text!` reads `count`, so pressing `+` redraws that one line;
- the label and the panel's border are not touched;
- the terminal receives the one changed digit.

`App::run` takes the screen and restores the terminal on every way out,
including Ctrl+C and a panic.

## Nodes

| Builder | What it shows |
|---|---|
| `text!("…{signal}…")` | Console markup with `format!` arguments; signals format as their value and are read reactively |
| `text(move \|\| …)` | Markup from any closure; every signal it reads is tracked |
| `label("…")` | Markup that never changes |
| `renderable(move \|\| table)` | Any rich renderable, rebuilt when the signals it read change |
| `column([...])`, `row([...])` | Children laid out down or across |
| `each(move \|\| keys, \|key\| node)` | One child per key, kept by key across reorders |
| `log.view()` | A streaming [`Log`](#logs) |
| `component(Input::new("Name"), on_done)` | A `rich-interact` component (see [Components](#components)) |

Sizes along the parent's axis:

- `.fixed(3)`: exactly 3 cells;
- `.percent(40)`: 40% of the parent;
- `.flex(2)`: a weighted share of what is left. A node with no size is
  `flex(1)`.

`.panel("Title")` wraps a node in a rounded border that is highlighted
while the focus is inside it.

## State

```rust
let selected = signal(0usize);
let is_first = memo(move || selected.get() == 0);
```

- `get()` reads a value, and `set(v)` or `update(|v| …)` writes it. A node
  that reads a signal while drawing subscribes to it, and a write redraws
  exactly those nodes. `set` with an equal value does nothing.
- A `memo` derives a value and notifies its readers only when the result
  changes. In a list where each row reads `memo(move || selected.get() == i)`,
  a move redraws two rows, not all of them.
- Signals are `Copy`: move them into as many closures as you need.

## Input and focus

```rust
column([
    label("one").focusable().on_key("x", |_| { /* … */ }).panel("One"),
    label("two").focusable().panel("Two"),
])
.on_key("q", |cx| cx.quit())
```

- **Keys** go to the focused node first, then bubble up through its
  ancestors until a binding uses them.
- **Tab** and **Shift+Tab** move the focus through focusable nodes. A
  handler can move it too, with `cx.focus_next()` or `cx.focus(id)`.
- **Clicks:** `.on_click(|cx| …)` gets the clicks inside the node and
  focuses it. The app remembers where each node was drawn, so you never
  store rectangles yourself.
- **Key names** are the ones `rs-rich-interact` uses: `"q"`, `"ctrl+s"`,
  `"up k"` (either key).

## Components

```rust
use rich_interact::Input;

let name = signal(String::new());
column([
    component(Input::new("Name"), move |value, _| name.set(value)).fixed(1),
    text!("Hello, {name}"),
])
```

- A focused component takes keys first. Keys it does not use bubble on, and
  the terminal caret follows its cursor.
- Its answer (Enter in an `Input`, a pick in a `Select`) goes to the
  closure.
- `.on_cancel(|cx| …)` handles Esc. Without it, Esc bubbles to your own
  bindings.

## Logs

```rust
let log = Log::new(1_000);           // keeps the last 1,000 lines
log.push("[green]started[/]");
log.view().panel("Log")
```

A log renders each line once. On an append it moves the rows already on
screen up and renders only the new lines. That is why a log tail costs less
than redrawing every row, which is ratatui's approach.

## Timers and threads

```rust
App::new(|| {
    let seconds = signal(0);
    every(Duration::from_secs(1), move |_| seconds.update(|s| *s += 1));
    text!("up {seconds}s")
})
```

- **Timers:** `every` runs a handler on a schedule. Call it inside
  `App::new`'s closure, next to the signals it writes.
- **Threads:** signals live on the app's thread. To change state from other
  work, send a closure through a `Proxy`, which you get from `cx.proxy()` or
  `app.proxy()`:

```rust
.on_key("r", move |cx| {
    let proxy = cx.proxy();
    std::thread::spawn(move || {
        let body = fetch();                       // slow, off the UI thread
        proxy.run(move || status.set(body));      // back on the UI thread
    });
})
```

## Testing

`render_with` runs an app headless, pressing keys, and returns the last
screen:

```rust
let screen = app.render_with(&["+", "+", "q"], 30, 2)?;
assert_eq!(screen[0].trim_end(), "Count: 2");
```

For more control, such as clicks, resizes, waits on a virtual clock, or the
exact bytes sent, use `App::run_on` with `rs-rich-interact`'s `Headless`
backend.

## Against ratatui

`tests/versus_ratatui.rs` draws the same ops dashboard with ratatui 0.30 and
with intuiTUIve, and asserts that intuiTUIve is no slower and sends no more
bytes.

| 80x24, release | ratatui | intuiTUIve |
|---|---:|---:|
| status tick | 92 µs, 37 B | **14 µs, 18 B** |
| selection move | 112 µs, 184 B | **43 µs, 118 B** |
| log append | 92 µs, 443 B | **46 µs, 394 B** |

[The design note](../../design/intuituive.md) explains why. It also lists
the architectural problems in ratatui that intuiTUIve is built to avoid:
redraw-everything frames, state kept apart from widgets, hand-written event
routing and hit-testing, and per-widget styling.

Coming from ratatui? [Using rich with ratatui](../ratatui.md) covers
`rs-rich-ratatui`, the interop crate: rich renderables drawn as ratatui
widgets in an existing app, and ratatui widgets run inside rs-rich-interact.

## Status

This is an early slice. Still to come:

- inline (non-full-screen) mode;
- constraint and grid layout;
- screens and navigation;
- app-level themes beyond borders;
- a widget inspector.
