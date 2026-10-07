# Terminal apps (intuiTUIve)

`rs-rich-intuituive` (`intuituive`, new and early) is a framework for
terminal apps, full-screen or inline. You describe the screen once as a tree of nodes
and keep your state in **signals**. When a signal changes, the nodes that
read it draw again, and only the cells that changed are sent to the
terminal. You never write a draw loop, a layout pass, or "what changed"
bookkeeping.

It builds on `rs-rich-interact`, so its sessions, its headless test driver
and its components (inputs, selectors, forms, pagers) all work inside an
app. Any rich renderable (tables, Markdown, syntax, charts) is a node.

```toml
[dependencies]
rs-rich-intuituive = "0.0.1"
```

One dependency is enough: `intuituive::rich` is rs-rich and
`intuituive::interact` is rs-rich-interact. New here? The
[tutorial](tutorial.md) builds a to-do app step by step, and the template
starts a project with one command:

```bash
cargo generate --git https://github.com/buchochelliq-labs/rs-rich-cli templates/intuituive-app
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
| `grid([sizes], [...])` | Children in rows and columns (see [Grids](#grids)) |
| `each(move \|\| keys, \|key\| node)` | One child per key, kept by key across reorders |
| `switch(move \|\| key, \|key\| node)` | One child at a time, chosen by key; the others are kept (tabs, wizard steps) |
| `log.view()` | A streaming [`Log`](#logs) |
| `component(Input::new("Name"), on_done)` | A `rich-interact` component (see [Components](#components)) |
| `repeating(\|\| Input::new("Add"), on_done)` | The same, built fresh after each answer (an entry box) |

`.panel("Title")` wraps a node in a rounded border that is highlighted
while the focus is inside it, and `.padding(1, 2)` leaves space round it
(rows above and below, columns either side).

## Layout

A child's size along its parent's axis:

- `.fixed(3)`: exactly 3 cells;
- `.percent(40)`: 40% of the parent;
- `.auto()`: as big as its content (the lines a text wraps to, a panel's
  content and border, a list's rows). When a signal the content reads
  changes, the parent lays out again, and the nodes after it move;
- `.flex(2)`: a weighted share of what is left. A node with no size is
  `flex(1)`.

Any of them can be clamped with `.min_size(n)` and `.max_size(n)`. A
flexible child that reaches a clamp keeps it, and the others share the rest,
as CSS flexbox does. When the children ask for more than there is, they
shrink from the last one, to their minimums first.

```rust
column([
    text!("{status}").auto(),                    // as tall as the status
    row([
        sidebar.percent(25).min_size(20),
        main.flex(1),
    ]),
    label("[muted]q quits").fixed(1),
])
.gap(1)
```

`.gap(1)` leaves a row (or, in a `row`, a column) between neighbours.

### Grids

```rust
grid(
    [Size::Fixed(12), Size::Flex(1), Size::Flex(1)],
    [
        label("logo").span(1, 2),     // one column, two rows
        label("title").span(2, 1),    // two columns
        cpu.panel("CPU"),
        memory.panel("Memory"),
    ],
)
.rows([Size::Auto])
.gap(1)
```

- Children fill the grid row by row; each takes the first place it fits.
- Columns are the sizes given. Rows share the height evenly unless
  `.rows([...])` sizes them, and rows past the last size given take that
  size, so `.rows([Size::Auto])` makes every row as tall as its content.

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
- **Showing the focus:** panels highlight their border while the focus is
  inside them. A leaf can show it too: `.focus_style("reverse")` (any style
  or theme name) restyles it, filled to its full width, while it has the
  focus, which is how a list shows its selected row.

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
- A component answers once and then shows its answer. For an entry box
  that takes one entry after another, use `repeating(make, on_done)`: it
  builds a fresh component after each answer.

## Logs

```rust
let log = Log::new(1_000);           // keeps the last 1,000 lines
log.push("[green]started[/]");
log.view().panel("Log")
```

A log renders each line once. On an append it moves the rows already on
screen up and renders only the new lines. That is why a log tail costs less
than redrawing every row, which is ratatui's approach.

## Screens and modals

An app is a stack of screens. A handler opens one with `cx.push`, and
`cx.pop` goes back:

```rust
fn detail(id: u32) -> Node {
    let count = signal(0);
    text!("item {id}: {count}")
        .on_key("+", move |_| count.update(|c| *c += 1))
        .on_key("esc", |cx| cx.pop())
}

label("home").on_key("enter", |cx| cx.push(|| detail(7)))
```

- The screen below is kept, with its state and its focus, and comes back as
  it was.
- A screen's closure can create signals and call `every`; its timers stop
  when it closes.
- `cx.replace(build)` swaps the top screen, as a wizard's steps do.

A modal is a screen in a box over the one below, which keeps drawing:

```rust
.on_key("q", |cx| {
    cx.modal(Size::Auto, Size::Auto, || {
        label("Quit? [b]y[/] / [b]n[/]")
            .padding(0, 1)
            .panel("Quit")
            .on_key("y", |cx| cx.quit())
            .on_key("n esc", |cx| cx.pop())
    })
})
```

Its width and height are cells (`Size::Fixed`), a share of the screen
(`Size::Percent`), the content's size (`Size::Auto`), or the whole screen
(`Size::Flex`). Keys and clicks reach only the top screen.

## Timers and background work

```rust
App::new(|| {
    let seconds = signal(0);
    every(Duration::from_secs(1), move |_| seconds.update(|s| *s += 1));
    text!("up {seconds}s")
})
```

- **Timers:** `every` runs a handler on a schedule. Call it inside
  `App::new`'s closure (or a screen's), next to the signals it writes.
- **Tasks:** `spawn(work, done)` runs `work` on its own thread and `done`
  with the result back on the app's thread, where it can write signals,
  open a screen or quit. `spawn_future` does the same for a future that does
  not need a particular async runtime. Both return a `Task`, which you can
  `cancel()` (its result is dropped).

```rust
.on_key("r", move |_| {
    spawn(fetch_report, move |report, cx| {
        cx.push(move || report_screen(report));
    });
})
```

- **Resources:** `resource(fetch)` loads a value in the background and
  holds `Load::Loading`, `Load::Ready(value)` or `Load::Failed(error)`. A
  node that reads it redraws when it arrives; `reload()` fetches again and
  drops any older result still on its way.

```rust
let user = resource(|| api::user(42));
text(move || match user.get() {
    Load::Loading => "[muted]loading…".into(),
    Load::Ready(user) => format!("Hello, {}", user.name),
    Load::Failed(error) => format!("[bad]{error}"),
})
.on_key("r", move |_| user.reload())
```

- **Other threads:** signals live on the app's thread. Work you start
  yourself (a tokio runtime, a file watcher) sends closures through a
  `Proxy` from `cx.proxy()` or `app.proxy()`: `proxy.run(move ||
  status.set(body))`, or `proxy.run_with(|cx| …)` for a `Ctx` too.

## Themes

A `Theme` styles what the framework draws (borders, the focused border,
panel titles) and names styles for markup. The presets, `Theme::dark()` (the
default), `Theme::light()` and `Theme::mono()`, each define `accent`,
`muted`, `good`, `warn` and `bad`:

```rust
label("[accent]ops[/] · [good]12 up[/] · [bad]1 down[/]")
```

Add your own with `Theme::dark().style("brand", Style::parse("bold magenta")?)`,
set it with `App::theme`, and switch at run time with `cx.set_theme(theme)`;
everything is drawn again in the new theme.

### Theme files, reloaded live

```rust
app.theme_file("theme.ini").run()
```

```ini
[styles]
accent = bold magenta
border.focused = bright_green
```

The file is rich's theme format: a `[styles]` section of `name = style`
lines. `border`, `border.focused` and `title` restyle the framework's parts,
and every name works in markup. The app reads the file again whenever it
changes, so you can tune colours while it runs. A file that does not parse
leaves the last good styles in place, and the [inspector](#inspector) shows
the error. `Theme::load(path)` and `theme.with_config(text)` read the same
format without watching.

## Inline apps

```rust
App::new(|| progress_view()).inline(3).run()
```

`App::inline(rows)` runs in a few rows under the prompt instead of the
alternate screen, like a command's progress output. Only those rows are
drawn, with cursor moves relative to them, so the scrollback above is left
alone. When the app ends, its last frame stays, and the prompt continues
below it. The mouse is left to the terminal.

## Testing

`render_with` runs an app headless, pressing keys, and returns the last
screen:

```rust
let screen = app.render_with(&["+", "+", "q"], 30, 2)?;
assert_eq!(screen[0].trim_end(), "Count: 2");
```

For more control, such as clicks, resizes, waits on a virtual clock, or the
exact bytes sent, use `App::run_on` with `rs-rich-interact`'s `Headless`
backend. `App::wait_for_tasks(true)` waits for background tasks before each
scripted event, so a test sees a task's result however fast the machine is.

## Inspector

```bash
INTUITUIVE_INSPECT=1 cargo run      # any app, no code change
```

or `App::inspector(true)` in code. The inspector docks on the right, and
the app is laid out in the space to its left. F12 shows and hides it. It
shows:

- the node tree as it is, each node's builder (or its `.name("…")`), what
  it holds (a panel's title, a list's length, a grid's shape) and its size,
  or `hidden`;
- in yellow, the nodes that drew in the last frame; reversed, the focused
  node;
- the frame's cost: nodes drawn of those made dirty, damaged rectangles and
  cells, and the bytes sent;
- the theme file's state, and its error if the last edit did not parse.

It is the quickest way to see that a change redraws what it should and
nothing more.

## Against ratatui

`tests/versus_ratatui.rs` draws the same ops dashboard with ratatui 0.30 and
with intuiTUIve, and asserts that intuiTUIve is no slower and sends no more
bytes.

| 80x24, release | ratatui | intuiTUIve |
|---|---:|---:|
| status tick | 95 µs, 37 B | **15 µs, 18 B** |
| selection move | 97 µs, 184 B | **39 µs, 118 B** |
| log append | 94 µs, 443 B | **47 µs, 428 B** |

[The design note](../../design/intuituive.md) explains why. It also lists
the architectural problems in ratatui that intuiTUIve is built to avoid:
redraw-everything frames, state kept apart from widgets, hand-written event
routing and hit-testing, and per-widget styling.

Coming from ratatui? [Using rich with ratatui](../ratatui.md) covers
`rs-rich-ratatui`, the interop crate: rich renderables drawn as ratatui
widgets in an existing app, and ratatui widgets run inside rs-rich-interact.

## Status

This is an early slice (0.0.x), so the API will change. Still to come:
Python bindings for the framework, as rs-rich-interact's components already
have.
