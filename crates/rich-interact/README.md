# rs-rich-interact

Interactive terminal components for
[rs-rich](https://github.com/buchochelliq-labs/rs-rich-cli), the Rust port of
Python's `rich`: the layer between printing and a full TUI framework. This
crate is an rs-rich addition, not a port: `rich` has no interactive
components.

A **component** is a state machine: it handles one event and renders one
view. Two drivers run it, and a component behaves the same under both:

- `run(component, &options)` takes the terminal, drives the component to an
  outcome and gives the terminal back, like `Prompt.ask`;
- an `EventLoop` runs several components at once, with ticks and timers, and
  repaints only when something changed.

```rust
use rich_interact::{headless, Component, Context, Event, Flow, KeyCode, Outcome, View};

/// Counts Up presses; Enter returns the count.
struct Counter(u32);

impl Component for Counter {
    type Output = u32;
    fn handle(&mut self, event: &Event, _: &Context<'_>) -> Flow<u32> {
        match event.key().map(|key| key.code) {
            Some(KeyCode::Up) => self.0 += 1,
            Some(KeyCode::Enter) => return Flow::Done(self.0),
            _ => {}
        }
        Flow::Continue
    }
    fn render(&self, context: &Context<'_>) -> View {
        View::new(context.markup(&format!("count: [bold]{}[/]", self.0)))
    }
}

// Tests drive it headless: scripted keys in, frames out.
let (outcome, record) = headless::run(Counter(0), headless::Script::new().keys("up up enter"), 40, 5);
assert_eq!(outcome.unwrap(), Outcome::Done(2));
assert_eq!(record.last_frame(), "count: 2");
```

Ready-made components:

- `Select` and `MultiSelect`: fuzzy pickers with highlighted matches and a
  preview pane whose border drags with the mouse;
- `TableSelect` and `TreeSelect`: pick a table's row (and copy it, or a
  cell, as text, CSV or JSON) or a tree's node, with folding, breadcrumbs
  and a filter that keeps each match's ancestors;
- `DataExplorer` (the `data` feature): explore a JSON, YAML, TOML, XML, INI
  or dotenv document, copying a node's path or value;
- `ThemePicker`: themes, including plugins' theme packs, with a live
  preview;
- `Input`: one line with validation, history, and suggestions, from a list or
  a background provider;
- `Confirm`: a confirmation sheet with a body, warnings and several choices;
- `Form`: text, masked (password), choice and toggle fields, with each error under its
  field;
- `Pager`: page any renderable, with search;
- `TextArea`: multi-line text, wrapped and scrolled, with a character limit;
- `FilePicker`: browse from a root, with a fuzzy filter, previews, hidden
  files, a file/directory mode, extensions and an optional root jail;
- `ColorPicker`: rich's named colours, the 256 palette, or hex and RGB, with
  a live swatch;
- `AssetPicker`: emoji, box styles and spinners, with a preview.

Every list reloads its items while keeping the query, the focus and the
marks (`Select::reload`, on a key or from a channel), and components copy
to the terminal's clipboard with `clipboard::copy`, through OSC 52 where the
terminal takes it.

Build your own from the same pieces:

- **Containers are components**: `Column`, `Row` and `Stack` lay children
  out; `Split` puts two side by side or stacked with a draggable border;
  `Tabs` keeps each tab's state; `Layers` opens modal and popover layers
  with a backdrop, a focus trap and Escape to dismiss.
- **Focus, routing and bubbling**: keys go to the focused child and bubble
  up (`Flow::Ignored`) when it does not use them; mouse events arrive in each
  child's coordinates; Tab and Shift+Tab move focus through nested
  containers.
- **A keymap registry**: every component declares its keys (action,
  description, context), rebindable per component (with its
  `rebind`) or process-wide from `context.action = keys` overrides, and
  listed for help and hints.
- **A public kit**: the line helpers and `ListState`, `ScrollState`,
  `FilterState`, `TextBuffer`, `Divider` and `ActionMenu`, which the
  built-ins are made of. See `examples/custom_component.rs`.

Overlays and chrome, read from the keymap:

- **`Overlays`** wraps any component with a command palette (Ctrl+O), a
  searchable help overlay (F1) and a shortcut sheet (`?`, F2), and runs
  what the palette picks. Each overlay (`Palette`, `Help`, `Shortcuts`,
  `Menu`) is also a component of its own.
- **Actions in a modal**: Ctrl+K opens an item's actions in a modal, and a
  region's actions through `Overlays::actions`.
- **A status bar and breadcrumbs**: `StatusBar` shows text, key hints,
  spinners and badges; `Breadcrumbs` shows a path. See
  `examples/overlays.rs`.

What else is in the box:

- **Painting cell by cell.** Views are painted through
  `rich_ext::frame::Frame::diff`: only changed cells are written, so an idle
  loop writes nothing.
- **Images in the cells.** Given a graphics source (`EventLoop::graphics`,
  or `run_with_graphics` for one component), the painter draws micro assets
  as Kitty, iTerm2 or Sixel images over their cells; with the `micro`
  feature, `AssetPicker` lists micro assets and `StatusItem::micro` puts one
  in the status bar.
- **A session that always restores the terminal.** Raw mode, the alternate
  screen, mouse and bracketed paste are undone on every way out: finishing,
  `?`, Ctrl+C and a panic. `Flow::Handoff(command)` gives the terminal to
  `$EDITOR` or a pager and takes it back.
- **A viewport**: a scrollable window over rendered lines, and a minimal pager
  on its own.
- **One item model** (`Item<T>`: label, description, metadata, preview,
  actions, keywords) behind every picker.
- **Actions** on items, table rows, tree nodes and files: on their keys and
  in a menu (Ctrl+K), from the caller or from plugins
  (`rs-rich-plugin-api`'s `CustomAction`).
- **The mouse, when a component asks for it**: clicks and drags in the
  component's own coordinates, hyperlinks as `Event::Link`, buttons in
  `Confirm` and `Form`, and scripted mouse events in the headless driver.
- **A degradation policy.** With no terminal on stdin or stdout, under `CI`,
  or with `TERM=dumb`, `run` starts no session. Instead it asks line by line,
  returns the component's default, or fails, as the caller chooses. Nothing
  blocks on a pipe.

Linux, macOS and Windows are supported through `crossterm`. The PTY tests run
on Unix.

Licensed under MIT.
