# Porting a ratatui app

This page is for people with a [ratatui](https://ratatui.rs) app who want to
move it to intuiTUIve. You do not have to move all of it at once:

| Path | What changes | When to choose it |
|---|---|---|
| **1. Use rich inside ratatui** | Nothing; you add widgets | You want rich's tables, Markdown, syntax or charts, and nothing else |
| **2. Host ratatui widgets in intuiTUIve** | The app shell and state | You want the framework now, and to port widgets as you touch them |
| **3. Port fully** | Everything | A new major version, or a small app |
| **4. Host intuiTUIve in ratatui** | One pane | ratatui keeps the terminal and the loop, and an intuiTUIve app runs in part of the frame |

!!! tip "Coming from ratatui, and looking for a feature?"
    [intuiTUIve against ratatui](../../design/intuituive-vs-ratatui.md)
    compares the two feature by feature: what intuiTUIve does the same way,
    what it does differently and why, and what is still missing and planned.

Paths 2 and 3 are the same work done at different speeds, so the recipe below
covers both. Three complete ports show where it leads:

- **An ops dashboard**, written both ways in
  [`tests/versus_ratatui.rs`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/rich-intuituive/tests/versus_ratatui.rs):
  the ratatui version and the intuiTUIve one draw the same screen, and CI
  measures them against each other.
- **A Yazi-style file manager.** [Yazi](https://github.com/sxyazi/yazi) is
  the most-starred app built on ratatui. It is rebuilt on intuiTUIve in
  [`examples/files.rs`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/rich-intuituive/examples/files.rs)
  (`cargo run -p rs-rich-intuituive --features files --example files`), on
  the `files` plugin's Miller columns, with Yazi's layout, theme, status
  line and default keymap (chords and their which-key pop-up included),
  previews by type loaded in the background, selection and visual mode,
  yank and paste, trash and delete as background tasks, create and rename,
  find, filter, sorting, git signs, archives, a fuzzy finder, tabs and the
  help. See
  [the walkthrough](#worked-example-a-yazi-style-file-manager) below.
- **An oscilloscope.** [scope-tui](https://github.com/alemidev/scope-tui)
  draws audio as an oscilloscope, a vectorscope and a spectroscope, twenty
  or more times a second. It is rebuilt in
  [`examples/scope.rs`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/rich-intuituive/examples/scope.rs)
  (`cargo run -p rs-rich-intuituive --example scope`), with its keys,
  options, trigger, averaging, Braille plots and live capture (with the
  `audio` feature). See
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
| `List` + `ListState` | `list(items, selected)` (scrolls, highlights, binds ↑↓ jk); `list_optional` for `select(None)` |
| `highlight_symbol`, `HighlightSpacing`, `ListDirection`, `scroll_padding`, `ListItem::style` | `.symbol(..)`, `.highlight_spacing(..)`, `.direction(..)`, `.scroll_padding(n)`, `.row_style(\|i\| ..)` on the list; `.multiline()` for items of several lines |
| `Table` + `TableState` | `table(columns, rows, selected)` (sized columns, a sticky header, keys, clicks); `virtual_table` for huge ones; `table_optional` for `select(None)` |
| `footer`, `column_spacing`, `Row::height`, `Row::style`, `column_highlight_style`, a cell's alignment | `TableOptions::footer(..)`, `.column_spacing(n)`, `.multiline()` or `.row_height(n)`, `.row_style(..)`, `.column_highlight()`; `Column::align(Align::End)` |
| `Tabs` | `tabs(titles, selected)` with `switch(\|\| tab.get(), build)` for the content; `.divider(..)`, `.tab_padding(..)` |
| `Scrollbar` and a scroll offset | `scroll(child)`: a viewport with keys, the wheel and a scrollbar; `.scrollbar(Scrollbar::new()..)` on lists, tables, trees, text areas and logs; `scrollbar(position, len, view, bar)` on its own |
| An input field (`tui-input`, by hand) | `input(signal)`, `number_input(signal, range)` |
| `Chart`, `Canvas` | `rich_ext::chart::{Chart, Canvas}` (laid out as ratatui's) via `renderable` |
| `Gauge`, `Sparkline`, `BarChart` | `rich_ext::chart` (`Gauge`, `Sparkline`, `BarChart`) via `renderable` |
| `Clear` + a centred popup | `cx.modal(width, height, build)`; `cx.modal_with(.., ModalOptions::new().shadow(..).backdrop(..), ..)` for a shadow |
| A popup placed next to a widget | `cx.popup(anchor, Placement::Below, width, height, build)` |
| A custom `Widget` | `impl Widget` and `widget(w)` (cells, events, children, focus); `leaf` for drawing only; or keep the widget (path 2) |
| `crossterm::event::read()` + `match` | `.on_key("q", …)` on the node that owns the key; keys bubble. Or keep your loop and feed an `App::driver` (path 4) |
| A focus enum and Tab handling | `.focusable()` / `.focus_style(..)`; Tab and Shift+Tab are built in |
| Mouse hit-testing against stored `Rect`s | `.on_click(…)`, `.on_mouse(…)`; events arrive in the node's own coordinates and bubble |
| A tick rate in the loop | `every(interval, …)` |
| Threads and channels into the loop | `spawn(work, done)`, `resource(fetch)`, `Proxy::run` |
| "When X changes, do Y" in the loop | `watch(\|\| x.get(), \|x, cx\| …)` |
| `ratatui::init()` / `restore()` | `App::run()` (restores on every exit, panics included) |
| Returning a choice or an error from the loop | `cx.quit_with(value)` and `App::run_for::<T>()`; `cx.fail(error)` |
| `Viewport::Inline(n)` | `App::inline(n)` |
| `Terminal::insert_before(n, ..)` | `cx.print_above(markup)`, `Proxy::print_above` |
| `EnableMouseCapture` at the app's choice | `App::mouse(Some(true))` or `Some(false)` |
| `Frame::count()` | `FrameStats::frames` (`Driver::stats()`) |
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
rs-rich-intuituive = "0.0.2"
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
inside it reads a signal it does not need: the inspector's trace (Shift+F12,
then 4) names each signal written and the nodes it made dirty, and its
timings (3) show which nodes take the time.

## Path 4: keep your loop

A ratatui program can keep its terminal, its loop and its frame, and give
part of the screen to an intuiTUIve app. `App::driver(width, height)` runs
the app without a loop of its own. Each turn, call:

- `update(elapsed)`;
- `render()`, then copy `driver.screen().lines()` into your buffer with
  rs-rich-ratatui's `lines_to_buffer`;
- `event(…)` with the app's events, translated by
  `rich_interact::event::from_crossterm`.

`examples/in_ratatui.rs` is the whole program, and the guide's
[Owning the loop](index.md#owning-the-loop) has the details. This is the
way in for an app too big to port at once: move one pane at a time.

## Worked example: a Yazi-style file manager

[`examples/files.rs`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/rich-intuituive/examples/files.rs)
reproduces Yazi as it looks and behaves out of the box: its 1:4:3 columns,
its default dark theme, icons and type colours, header, tab strip and
status line, and its default keymap. It is built only from the
framework's ready-made components: the core's bars, tabs and dialogs, and
the `files` plugin's Miller columns, previews, openers, tasks and finder.
What it writes itself is Yazi's keymap and what each key does (about 1,450
lines, half of round 2's). Its tests are in `tests/files.rs`. It does not
reuse any of Yazi's code: it is a rebuild of the behaviour, showing how a
large ratatui app's ideas map.

    cargo run -p rs-rich-intuituive --features files --example files
    cargo run -p rs-rich-intuituive --features files,archive,watch,art --example files

!!! note "Credit"
    Ported from [Yazi](https://github.com/sxyazi/yazi), by sxyazi and its
    contributors (MIT licence). Its layout, keys, theme and behaviour are
    theirs. The icons and type colours (`--icons`, for a Nerd Font) are
    Yazi's defaults, through rs-rich-ext's `filetype`, whose glyphs come
    from nvim-web-devicons (MIT).

![The file manager: parent, current directory and a highlighted preview](../../media/tapes/files/files.png)

| Yazi does | The rebuild uses |
|---|---|
| Three columns, parent, current and preview, in a 1:4:3 ratio, with rails the mouse drags; each directory remembers its hovered entry; a click in the parent column goes there, one in a directory's preview enters it | `files::miller(start)`: `cwd`, `cursor`, `hovered`, `hidden`, `sort`, `linemode` and `find` are its signals; `arrow`, `page`, `enter`, `leave`, `cd` and `refresh` its moves; `view_with(MillerOptions)` draws it |
| One row per entry: icon, the type's colour (sniffed from the first bytes) across the row, the hovered row reversed, markers, find matches with `[2/5]`, git signs and a line mode at the right | the plugin's `file_list` inside the columns; `MillerOptions::markers(\|index, path\| Marker)` and `.git(git_status(model))`; `Linemode::{Size, Permissions, Mtime, Btime, Owner}` |
| A directory listing sorted, filtered, followed on disk | each column's `DirModel` (`m.current.filter`, `m.sort`, `m.hidden`); with `watch` it follows the disk |
| Previews by type, off the UI thread, stale results dropped: code highlighted, images, archives, directories | the plugin's `preview` inside the columns; `archive` lists tar and zip, `art` draws images; `rich_ext::syntax::preload` warms the grammars at start-up |
| Header: the path cut from the left, `~` for home, `(filter: x)`, and the selected or yanked count | `breadcrumb(path).home_tilde().crumb_suffix(..)` in the theme's `breadcrumb` styles (cyan) beside a `text` |
| A status line: mode, size and name; task progress, permissions, `Top`/`Bot`/percent and position, with rounded caps on a Nerd Font | `status_bar(left, right)` of `seg(..).style_with(..)` segments, `.caps(Caps::Rounded)` with icons; `files::coloured_permissions` |
| Tabs (`tt`, 1–9, `[ ]`, Ctrl+C), the strip shown from two tabs | `tabs(titles, active).auto_hide().on_select(\|to, _\| switch_tab(to))`; the tab keeps its history by watching `m.cwd` |
| One keymap that wins over the focused column, with sequences (`gg`, `,s`, `cc`, `tt`) and a which-key pop-up | the root column `.previews_keys()` holds every `bind` and `bind_sequence` (`"comma s"`); `App::which_key(true)` in Yazi's which-key colours |
| Prompts for find, filter, create and `cd` at the top centre, 50 wide; rename below the hovered row with the caret before the extension | `cx.prompt_with(title, text, PromptOptions::new().title_style("default").on_change(..).on_cancel(..), ..)`; `.at(Origin::At(row), ..)` from `m.row_rects()` and `.caret(Caret::At(n))` |
| Confirmations before trashing and deleting (70 x 20), overwriting (50 x 15) and quitting with tasks running | `cx.confirm_with(title, body, files, ConfirmOptions::new().size(..), on_yes)` |
| Enter and `o` open with the first opener for the type; `O` picks one below the row | `files::opener_rules()` (Yazi's `[open]` rules) and `files::open_with`; `cx.pick_with("Open with:", Origin::At(row), ..)`; `extract` runs as a task |
| Copy, move, trash (the system's trash) and delete as background tasks with progress; `w` shows them | `files::tasks()` with rs-rich-ext's `file_ops::Op`; `tasks_panel(jobs)` in a modal; failures are notifications too |
| fzf and zoxide jumps (`z`, `Z`) | `files::finder(walk(cwd))`, which streams the walk; rs-rich-interact's `Jump` over the directories visited |
| Errors as notifications at the top right | `cx.notify(Level::Error, ..)` and `App::toasts_at(Corner::TopRight)` |
| The help, searchable | `cx.modal` holding an `input`, a `rule()` and a `list` |
| (not in Yazi) a command palette and a right-click menu | `.palette_key("ctrl+p")`; `context_menu(cx, items)` in the columns' `on_mouse` |

![The which-key pop-up after `,`](../../media/tapes/files/files-which.png)

![The help](../../media/tapes/files/files-help.png)

![The confirmation before trashing a file](../../media/tapes/files/files-confirm.png)

![The filter, applied as it is typed](../../media/tapes/files/files-filter.png)

![An image preview, with the `art` feature](../../media/tapes/files/files-image.png)

![An archive's entries, with the `archive` feature](../../media/tapes/files/files-archive.png)

![The rename prompt below the hovered row, the caret before the extension](../../media/tapes/files/files-rename.png)

![The finder (`z`), its walk streaming in](../../media/tapes/files/files-finder.png)

![The task manager (`w`) with a failed move, and its notification](../../media/tapes/files/files-tasks.png)

![A notification at the top right](../../media/tapes/files/files-notify.png)

Where it differs from Yazi, and why:

- **Trash** is the FreeDesktop trash, Yazi's on Linux; on macOS and
  Windows, whose bins rs-rich-ext does not write, trashing fails with a
  notification that says so.
- **Binary files** preview as a hex dump; Yazi's `file` previewer prints
  `file -bL`'s classification.
- **Opening**: `$EDITOR` (split on spaces) and the system opener cover
  Yazi's `edit`, `open`, `play` and `reveal`, and `extract` with the
  `archive` feature; there is no shell (`;`), `fd`/`rg` search or bulk
  rename.
- **Images** are drawn in half blocks: the `image` node places no Kitty
  or Sixel graphics yet.
- **The hovered row** has no rounded ends with a Nerd Font: the core
  list can put its highlight under the markup now, but the `files` rows
  neither draw the ends nor reach that option.
- **Icons** are off unless `--icons` is given or `RICH_NERD_FONT=1` says a
  Nerd Font is in use: an installed Nerd Font may not be the terminal's.

Two things the port shows:

1. **Slow work belongs on a worker, including rendering.** Highlighting a
   language for the first time compiles its grammar, which takes seconds
   in a debug build for Markdown and its embedded languages. The app warms
   the likely grammars at start-up, and the preview worker highlights the
   text at the pane's size, so the app's thread only copies the finished
   lines onto the screen. Yazi does this for the same reason.
2. **The components are signals, so the app adds to them by watching.**
   Miller's columns know nothing of tabs or visual mode; the app watches
   `m.cwd` to keep each tab's history and end visual mode, and reads
   `m.hovered`, `m.cursor` and `m.current` for its status line and
   markers, as Yazi's Lua components read its `cx`.

## Worked example: scope-tui, an oscilloscope

[`examples/scope.rs`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/rich-intuituive/examples/scope.rs)
rebuilds [scope-tui](https://github.com/alemidev/scope-tui) in about 1,300
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
| A loop that blocks on the audio source, then draws | each source becomes a signal of the newest buffer, and a `watch` takes each one in: the test signal is made on the app's clock with `every(buffer period)`, a file or a pipe is read on a thread of its own with `latest(…)`, and a sound card is `audio::input(…)` |
| Audio from cpal (`audio`) and PulseAudio (`pulse`) | intuiTUIve's `audio` feature, on cpal: `audio [DEVICE] [--list]`, and `pulse` as ALSA's `pulse` device; raw PCM from a file or stdin; a built-in test signal |
| Three `DisplayMode` trait objects, mutated by key handlers | one signal per mode's settings, and one per shared setting (`Graph`) |
| The header, a one-row `Table` with percentage columns | a `row` of `widgets::cell`s with `.percent(…)`, `.gap(1)` and `.solver(Solver::Ratatui)`, styled as the table is with `.style(…)` |
| `Chart` with Braille `Dataset`s, axis titles and a legend, and axes without labels (so without lines) | rs-rich-ext's `Chart`, laid out as ratatui's, with the same `Dataset`s and `Labels::None`, and drawn as ratatui 0.29 draws: `out_of_bounds(OutOfBounds::Drop)`, `layering(Layering::PerDataset)`, `legend_style("")` and `legend_clear(false)`; the spectroscope's frequencies on a log `Axis` |
| `rustfft` for the spectrum | `rich_ext::dsp::fft`, `hann` and `magnitude` |
| Colours parsed by ratatui (`--axis-color DarkGray`) | `rich_ratatui::style_from_color_name` |
| Shift ×10, Ctrl ×5, Alt ×⅕ on every step | the same bindings, made in a loop over the modifiers |
| `h` hides the interface | `switch` on the `show_ui` setting |

![The vectorscope: left against right, a Lissajous figure](../../media/tapes/scope/vectorscope.png)

What the port shows:

1. **Redraw follows what changed.** The plot reads the frame, so it draws
   once per buffer. Each header cell reads only its own value. The fps cell
   draws once a second, and the other cells draw only when a key changes
   them. While the scope is paused nothing draws at all, and identical
   buffers (the `--still` test signal) draw nothing either.
2. **Take the newest frame from a stream rather than queueing them.**
   `latest` and `audio::input` both keep only the newest buffer. When the
   terminal falls behind, the app skips old buffers rather than drawing a
   backlog. A thread's buffers are numbered, because a `watch` runs only
   when what it reads changes, and two buffers of silence are equal.
3. **Ported layouts can keep ratatui's arithmetic.** `Solver::Ratatui`
   places the header's cells in the same columns as scope-tui's table at
   every width, and `cell` cuts them as a table cell is cut.

It differs from scope-tui in a few deliberate ways:

- `audio` and `pulse` need the crate's `audio` feature (and ALSA's headers
  on Linux); without it they say so and point at `file -`. `pulse` opens
  ALSA's `pulse` device, so it reads PulseAudio's default source: a named
  PulseAudio source, a sink's monitor among them, cannot be chosen, and
  `--server-buffer` is read but has no effect. `audio --timeout` is read
  but not enforced, and `audio --list` lists names without their
  configurations.
- Its `file` source reads a whole buffer per channel. scope-tui 0.3.5's
  reads half of one, so its waveform fills half the width and its
  spectroscope stops at the first frame; this one draws what scope-tui's
  audio sources do.
- The trigger threshold keeps scope-tui's 16-bit units, and compares them
  with the samples scaled back up, so it triggers on every source.
- `?` lists the keys, and options may follow the source.

![The spectroscope: each channel's spectrum on a log frequency axis](../../media/tapes/scope/spectroscope.png)

## Credit for ports

Every app ported into this repository names where it came from, who made
it and its licence: in the example's header comment, in its section of
this guide, and in the recordings gallery. A port that copies code from the
original must also keep the original's copyright and licence notice with
that code, as MIT and Apache-2.0 require. The ports here reuse none, so
the credit is all they carry. Port your own app the same way.

The ports so far, and whose they are:

- **[Yazi](https://github.com/sxyazi/yazi)**, by sxyazi and its
  contributors, MIT licence: `files`.
- **[scope-tui](https://github.com/alemidev/scope-tui)**, by alemi, MIT
  licence: `scope`.
- **[ratatui](https://github.com/ratatui/ratatui)**'s example apps, by
  Florian Dehau and the Ratatui Developers, MIT licence: rebuilt from
  ratatui 0.30, with sample data of our own, and listed in
  [ratatui's examples, rebuilt](ratatui-examples.md).

## Common questions

**Where did my `Frame` go?** There is none. Nodes draw themselves into the
rectangle the layout gives them. A `leaf(|console, width, height| …)`
returning rich segments is the closest thing to a custom `Widget::render`.

**Can I keep my ratatui backend?** `App::run` uses rs-rich-interact's
terminal session (crossterm). To keep your own terminal and loop, ratatui's
included, drive the app with `App::driver` (path 4). `App::run_with` drives
it with termion or termwiz instead, behind this crate's features of those
names (see [Backends](index.md#backends)).

**Do I lose performance?** No: [the benchmark](index.md#against-ratatui) has
intuiTUIve ahead on every scenario it measures, because it redraws and sends
only what changed.

**What about async runtimes?** Keep yours. Send results to the app with a
`Proxy`, or use `spawn_future` for futures that do not need a particular
runtime.
