# Porting a ratatui app

This page is for people with a [ratatui](https://ratatui.rs) app who want to
move it to intuiTUIve. You do not have to move all of it at once:

| Path | What changes | When to choose it |
|---|---|---|
| **1. Use rich inside ratatui** | Nothing; you add widgets | You want rich's tables, Markdown, syntax or charts, and nothing else |
| **2. Host ratatui widgets in intuiTUIve** | The app shell and state | You want the framework now, and to port widgets as you touch them |
| **3. Port fully** | Everything | A new major version, or a small app |

Paths 2 and 3 are the same work done at different speeds, so the recipe below
covers both. Three complete ports show where it leads:

- **An ops dashboard**, written both ways in
  [`tests/versus_ratatui.rs`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/rich-intuituive/tests/versus_ratatui.rs):
  the ratatui version and the intuiTUIve one draw the same screen, and CI
  measures them against each other.
- **A Yazi-style file manager.** [Yazi](https://github.com/sxyazi/yazi) is
  the most-starred app built on ratatui. It is rebuilt on intuiTUIve in
  [`examples/files.rs`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/rich-intuituive/examples/files.rs)
  (`cargo run -p rs-rich-intuituive --example files`), with three columns
  (parent, current, preview), vim keys, previews loaded and highlighted in
  the background, hidden files, sorting and filtering. See
  [the walkthrough](#worked-example-a-yazi-style-file-manager) below.
- **An oscilloscope.** [scope-tui](https://github.com/alemidev/scope-tui)
  draws audio as an oscilloscope, a vectorscope and a spectroscope, twenty
  or more times a second. It is rebuilt in
  [`examples/scope.rs`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/rich-intuituive/examples/scope.rs)
  (`cargo run -p rs-rich-intuituive --example scope`), with its keys,
  options, trigger, averaging and Braille plots. See
  [the walkthrough](#worked-example-scope-tui-an-oscilloscope) below.

## What changes, in one table

| ratatui | intuiTUIve |
|---|---|
| An `App` struct holding state | Signals: `let selected = signal(0)` |
| `terminal.draw(\|frame\| …)` every frame | A tree built once in `App::new(\|\| …)`; only what changed draws |
| `Layout::vertical([..]).areas(r)` | `column([..])` / `row([..])` / `grid(..)`, sizes on the children |
| `Constraint::Length(n)` | `.fixed(n)` |
| `Constraint::Percentage(p)` | `.percent(p)` |
| `Constraint::Fill(w)` / `Min(0)` | `.flex(w)` (the default) |
| `Constraint::Min(n)` / `Max(n)` | `.min_size(n)` / `.max_size(n)` on any child |
| Content-sized rows (measured by hand) | `.auto()` |
| `Block::bordered().title(t)` | `.panel(t)`; `.padding(v, h)` for space |
| `Paragraph`, `Line`, `Span` | `text!("…{signal}…")`, `label("…")`, console markup |
| `List` + `ListState` | `list(items, selected)` (scrolls, highlights, binds ↑↓ jk) |
| `Table` + `TableState` | `renderable(\|\| table)` with a rich `Table`, or `each` rows |
| `Tabs` | `switch(\|\| tab.get(), build)` plus a label |
| `Gauge`, `Sparkline`, `BarChart`, `Chart` | `rich_ext::chart` (`Gauge`, `Sparkline`, `BarChart`, `LineChart`) via `renderable` |
| `Clear` + a centred popup | `cx.modal(width, height, build)` |
| `Canvas` / a custom `Widget` | `leaf(\|console, w, h\| lines)`, or keep the widget (path 2) |
| `crossterm::event::read()` + `match` | `.on_key("q", …)` on the node that owns the key; keys bubble |
| A focus enum and Tab handling | `.focusable()` / `.focus_style(..)`; Tab and Shift+Tab are built in |
| Mouse hit-testing against stored `Rect`s | `.on_click(…)` |
| A tick rate in the loop | `every(interval, …)` |
| Threads and channels into the loop | `spawn(work, done)`, `resource(fetch)`, `Proxy::run` |
| "When X changes, do Y" in the loop | `watch(\|\| x.get(), \|x, cx\| …)` |
| `ratatui::init()` / `restore()` | `App::run()` (restores on every exit, panics included) |
| `Viewport::Inline(n)` | `App::inline(n)` |
| `TestBackend` and buffer asserts | Headless scripts: keys, clicks, resizes, a virtual clock |

## Path 1: rich inside ratatui

Nothing about your app changes. `rs-rich-ratatui` makes any rich renderable a
ratatui `Widget`:

```rust
frame.render_widget(&RichWidget::new(&table), area);
frame.render_widget(RichWidget::markup("[b]q[/] quit"), footer);
```

See [Using rich with ratatui](../ratatui.md).

## Path 2: ratatui widgets inside intuiTUIve

`rs-rich-ratatui`'s `RatatuiComponent` (its `interact` feature) runs a
ratatui draw function as a component, and `component(..)` puts it in the
tree. The widget can read signals: when they change, it draws again.

```toml
[dependencies]
rs-rich-intuituive = "0.0.1"
rs-rich-ratatui = { version = "0.0.1", features = ["interact"] }
ratatui = "0.30"
```

```rust
use intuituive::prelude::*;
use ratatui::widgets::{Block, Gauge, Widget};
use rich_ratatui::RatatuiComponent;

App::new(|| {
    let done = signal(1u16);
    // Your existing widget code, unchanged, reading a signal.
    let gauge = RatatuiComponent::<(), ()>::new(move |area, buf| {
        Gauge::default()
            .block(Block::bordered().title("Progress"))
            .percent(done.get() * 10)
            .render(area, buf);
    });
    column([
        component(gauge, |_, _| {}).no_focus().fixed(3),
        text!("done {done}/10").auto(),
    ])
    .on_key("+", move |_| done.update(|d| *d += 1))
    .on_key("q", |cx| cx.quit())
})
.run()
```

This is tested in `tests/ratatui_widgets.rs`. From here, port one widget at
a time, the busiest first: each port also moves that part of the screen from
"redraw when anything changes" to "redraw when its own state changes".

## Path 3: the recipe

### 1. State becomes signals

```rust
// ratatui
struct App {
    selected: usize,
    tick: u64,
    log: Vec<String>,
}
```

```rust
// intuiTUIve, inside App::new(|| { … })
let selected = signal(0usize);
let tick = signal(0u64);
let log = Log::new(500);
```

Signals are `Copy`: move them into every closure that needs them, with no
`Rc<RefCell<..>>`. Derived values become memos:
`let left = memo(move || todos.with(|t| t.iter().filter(|t| !t.done).count()))`.
In handlers, read with `get()`; to read without subscribing anything (rarely
needed), use `get_untracked()`.

### 2. The draw function becomes a tree

A ratatui `draw` runs every frame and lays out from scratch. In intuiTUIve
you build the same shape once:

```rust
// ratatui
let [header, body, footer] = Layout::vertical([
    Constraint::Length(1),
    Constraint::Min(0),
    Constraint::Length(1),
])
.areas(frame.area());
let [left, right] =
    Layout::horizontal([Constraint::Percentage(40), Constraint::Percentage(60)]).areas(body);
```

```rust
// intuiTUIve
column([
    header.fixed(1),
    row([services.percent(40), detail_and_log.percent(60)]),
    footer.fixed(1),
])
```

Each closure inside the tree (`text!`, `text(..)`, `renderable(..)`, `list`'s
items) re-runs only when the signals it read change. You no longer pass
`Rect`s around; when you need one (for a click), the framework keeps it.

### 3. Widgets become nodes

Most widgets have a direct counterpart (see the table above). Three are
worth a closer look:

- **Lists.** `list(items, selected)` is `List` and `ListState` together. It
  keeps the selection in view, highlights it in the theme's `selected` style
  and binds ↑/↓, j/k, Home/End and PageUp/PageDown while it has the focus.
  Bind Enter yourself.
- **Tables.** Build a rich `Table` in `renderable(move || …)`. Columns size
  themselves, and cells take markup. For a table whose rows change one at a
  time, `each(keys, row)` keeps each row's node, so only changed rows draw.
- **Popups.** Replace `Clear` plus a centred `Rect` with
  `cx.modal(Size::Auto, Size::Auto, build)`. Keys go to the modal until
  `cx.pop()`.

Styles move from `Style::new().fg(..)` to markup (`"[bold red]…[/]"`) or,
better, theme names (`"[bad]…[/]"`) that a [theme file](index.md#theme-files-reloaded-live)
can change while the app runs. `rs-rich-ratatui`'s `to_rich_style`
converts an existing ratatui `Style`.

### 4. The event loop becomes bindings

```rust
// ratatui
match event::read()? {
    Event::Key(key) => match key.code {
        KeyCode::Up | KeyCode::Char('k') => app.selected = app.selected.saturating_sub(1),
        KeyCode::Char('q') => return Ok(()),
        _ => {}
    },
    _ => {}
}
```

```rust
// intuiTUIve
list(items, selected)                 // ↑/k and ↓/j are built in
    .on_key("enter", move |cx| open(selected.get(), cx));
root.on_key("q", |cx| cx.quit());
```

Keys go to the focused node first and bubble up to its ancestors, so a key
bound on the root works anywhere unless something deeper uses it. Replace a
hand-written `Focus` enum with `.focusable()` nodes: Tab and Shift+Tab move
between them, panels highlight while the focus is inside, and
`.focus_style("reverse")` marks a focused row.

### 5. Ticks, threads and side effects

| In your loop | Becomes |
|---|---|
| `if last_tick.elapsed() >= tick_rate { app.on_tick() }` | `every(tick_rate, move \|_\| …)` |
| A thread sending results down a channel | `spawn(work, move \|result, cx\| …)` |
| A value that loads in the background | `resource(fetch)`: `Loading`, `Ready`, `Failed`, `reload()` |
| A tokio task updating state | `proxy.run(move \|\| signal.set(v))` from the task |
| "If the selection changed, reload the preview" | `watch(\|\| selected.get(), \|i, cx\| …)` |

`watch` is the one most ports need: it replaces the "compare with last
frame's value" checks a ratatui loop accumulates.

### 6. Tests

`TestBackend` asserts become scripts, which press keys, type text, click,
resize and wait on a virtual clock:

```rust
let rows = app.render_with(&["j", "j", "enter", "q"], 80, 24)?;
assert!(rows[3].contains("selected"));
```

For more control, `App::run_on` with `intuituive::interact::headless::Headless`
records every frame and every byte sent. `App::wait_for_tasks(true)` makes
background work finish before the next scripted key, so tests of loading
states are deterministic.

### 7. Check the result

Run the port with `INTUITUIVE_INSPECT=1`. The [inspector](index.md#inspector)
shows which nodes drew in each frame. A key press should redraw what it
changed and nothing else. If a whole panel redraws on every tick, a closure
inside it reads a signal it does not need.

## Worked example: a Yazi-style file manager

[`examples/files.rs`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/rich-intuituive/examples/files.rs)
reproduces Yazi's core behaviour in about 420 lines, including its helpers.
Its tests are in `tests/files.rs`. It does not reuse any of Yazi's code: it
is a rebuild of the behaviour, showing how a large ratatui app's ideas map.

!!! note "Credit"
    Ported from [Yazi](https://github.com/sxyazi/yazi), by sxyazi and its
    contributors (MIT licence). Its three-column design, keys and behaviour
    are theirs.

![The file manager: parent, current directory and a highlighted preview](../../media/tapes/files/files.png)

| Yazi does | The rebuild uses |
|---|---|
| Three columns: parent, current, preview | `row([parent, current, preview]).gap(1)` with `.flex(1)`, `.flex(4)`, `.flex(3)` |
| A selectable, scrolling file list | `list(move \|\| rows, selected)` |
| The parent column marks the current directory | a second `list(..)` with `.no_focus()`, its selection kept in step by a `watch` |
| Previews loaded and highlighted off the UI thread | a `watch` on the selection starts a `spawn`; a generation counter drops stale results |
| Remembers where you were when you go up | `h` selects the directory you came from |
| Hidden files, sort order, filter | signals read by one `memo` of the directory listing |
| Filter prompt | `cx.modal(..)` holding an `Input` component |
| Help overlay | `cx.modal(Size::Auto, Size::Auto, help)` |
| Watches the disk | `every(2s)` re-reads; the listing is a memo, so an unchanged directory draws nothing |

![The help overlay, a modal sized to its content](../../media/tapes/files/files-help.png)

Two things the port shows:

1. **Slow work belongs on a worker, including rendering.** Highlighting a
   language for the first time compiles its grammar, which takes over a
   second in a debug build. The preview worker therefore highlights the text
   too, at the pane's size, and the app's thread only copies the finished
   lines onto the screen. Yazi does this for the same reason.
2. **The listing is data, the list is a view.** One memo holds the filtered,
   sorted entries. The list, the status bar and the preview `watch` all read
   it, and each updates only when the part it read changes.

## Worked example: scope-tui, an oscilloscope

[`examples/scope.rs`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/rich-intuituive/examples/scope.rs)
rebuilds [scope-tui](https://github.com/alemidev/scope-tui) in about 1,200
lines. Its tests are in `tests/scope.rs`. Like the file manager, it reuses
none of the original's code. Where the file manager redraws when you press
a key, this app redraws whenever a new buffer of audio arrives.

!!! note "Credit"
    Ported from [scope-tui](https://github.com/alemidev/scope-tui) 0.3.5, by
    alemi (MIT licence). Its design, keys, options and display layout are
    alemi's.

![The oscilloscope: two channels and the zero line, paused](../../media/tapes/scope/oscilloscope.png)

| scope-tui does | The rebuild uses |
|---|---|
| A loop that blocks on the audio source, then draws | `every(buffer period)` pulls the source's newest buffer into a `frame` signal |
| Audio from PulseAudio or cpal | raw PCM from a file or stdin (audio piped from `parec`), read on its own thread; a built-in test signal |
| Three `DisplayMode` trait objects, mutated by key handlers | one signal per mode's settings, and one per shared setting (`Graph`) |
| The header, a `Table` with percentage columns | a `row` of `text` cells with `.percent(…)` and `.gap(1)` |
| `Chart` with Braille `Dataset`s, axis titles and a legend | a `leaf` that draws on rs-rich-ext's `DotCanvas`, laid out as `Chart` is |
| `rustfft` for the spectrum | a 40-line radix-2 FFT in the example |
| Shift ×10, Ctrl ×5, Alt ×⅕ on every step | the same bindings, made in a loop over the modifiers |
| `h` hides the interface | `switch` on the `show_ui` setting |

![The vectorscope: left against right, a Lissajous figure](../../media/tapes/scope/vectorscope.png)

What the port shows:

1. **Redraw follows what changed.** The plot reads the frame, so it draws
   once per buffer. Each header cell reads only its own value. The fps cell
   draws once a second, and the other cells draw only when a key changes
   them. While the scope is paused nothing draws at all, and identical
   buffers (the `--still` test signal) draw nothing either.
2. **Take the newest frame from a stream rather than queueing them.** A
   reader thread keeps only the latest buffer. When the terminal falls
   behind, the app skips old buffers rather than drawing a backlog.

It differs from scope-tui in four small ways:

- The trigger threshold is in the same -1 to 1 units as the samples.
- The spectrum is padded to a power of two.
- The header says when the input ends.
- It has no audio-device backends: pipe audio in instead.

![The spectroscope: each channel's spectrum on a log frequency axis](../../media/tapes/scope/spectroscope.png)

## Credit for ports

Every app ported into this repository names where it came from, who made
it and its licence: in the example's header comment, in its section of
this guide, and in the recordings gallery. A port that copies code from the
original must also keep the original's copyright and licence notice with
that code, as MIT and Apache-2.0 require. The ports here reuse none, so
the credit is all they carry. Port your own app the same way.

## Common questions

**Where did my `Frame` go?** There is none. Nodes draw themselves into the
rectangle the layout gives them. A `leaf(|console, width, height| …)`
returning rich segments is the closest thing to a custom `Widget::render`.

**Can I keep my ratatui backend?** intuiTUIve runs on rs-rich-interact's
terminal session (crossterm) or its headless driver, not on ratatui
backends.

**Do I lose performance?** No: [the benchmark](index.md#against-ratatui) has
intuiTUIve ahead on every scenario it measures, because it redraws and sends
only what changed.

**What about async runtimes?** Keep yours. Send results to the app with a
`Proxy`, or use `spawn_future` for futures that do not need a particular
runtime.
