# Building your own components

`rs-rich-interact` 0.0.2 makes composition the way to build. The built-in
components are made of public pieces, containers are components too, and a
keymap records every key. You can write a component from the pieces, put it
beside the built-ins, and run the result like any other component.

This page covers:

- the [kit](#the-kit) of line helpers and state types;
- the [containers](#containers) (`Column`, `Row`, `Stack`, `Split`, `Tabs`
  and `Layers`);
- how [focus and events](#focus-routing-and-bubbling) travel between them;
- the [keymap](#the-keymap) registry;
- components [written in Python](#components-from-python) and
  [shipped by plugins](#components-from-plugins), which compose with the
  built-ins the same way.

A complete example is in the repository:
[`crates/rich-interact/examples/custom_component.rs`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/rich-interact/examples/custom_component.rs).
It builds a checklist from kit pieces and composes it with `Select`,
`Input` and `Confirm` in a split, tabs and two modals. To run it:

```bash
cargo run -p rs-rich-interact --example custom_component
```

## The kit

`rich_interact::kit` holds what the built-ins are made of.

**Line helpers** build the lines of a `View`:

| Helper | What it does |
|---|---|
| `text(s, &style)`, `plain(s)` | make one segment |
| `width(&line)` | measure a line in cells |
| `fit(line, n)` | crop a line to `n` cells |
| `pad(line, n)` | crop or pad a line to exactly `n` cells |
| `slice(&line, from, to)` | the cells `from..to` of a line |
| `overlay(&base, x, &top)` | draw `top` over `base` from cell `x` |
| `restyle(&line, &style)` | put a style over a line (a backdrop) |
| `frame(lines, inner, title, &style)` | box lines with a rounded border and a title |
| `place(&mut lines, x, y, &boxed, backdrop)` | draw a box over a view, dimming it first if asked |
| `highlight(label, positions, base, matched)` | style the characters a filter matched |
| `question(&theme, prompt)` | the `? prompt › ` line every built-in starts with |
| `pasted(s, newline)`, `shown(s)` | make untrusted text safe to show |

**State types** remember things between events. They share the built-ins'
movement and editing rules, and none of them renders anything:

| Type | Holds | Used by |
|---|---|---|
| `ListState` | the cursor over rows, the first row shown, and a selection by index | `Select`, `MultiSelect` |
| `ScrollState` | an offset into lines, with the pager keys and the wheel | `Viewport`, `Pager`, `Confirm` |
| `FilterState` | a query, and the candidates that match it, ranked by the fuzzy matcher, with the positions to highlight | `Select` and the pickers |
| `TextBuffer` | one line and a caret that moves by grapheme cluster: insert, Backspace, Delete, Ctrl+U, Ctrl+W, and an undo point | `Input`, `Form` |
| `Divider` | a border between two panes that the mouse drags and the keyboard nudges, with minimum sizes | `Split`, `Select`'s preview |
| `ActionMenu` | the Ctrl+K menu of actions for one target | `Select`, `overlay::Menu` |

A checklist that filters as you type needs only three of these. The list
below is abridged from the example:

```rust
use rich_interact::keymap::{keys, Keymap};
use rich_interact::kit::{self, FilterState, ListState, Theme};
use rich_interact::{Component, Context, Event, Flow, KeyCode, View};

pub struct Checklist {
    items: Vec<String>,
    filter: FilterState,
    list: ListState,
    keymap: Keymap,
}

impl Checklist {
    pub fn new(items: Vec<String>) -> Checklist {
        let mut list = ListState::new();
        list.set_len(items.len());
        Checklist {
            filter: FilterState::new(items.iter().cloned()),
            items,
            list,
            keymap: Keymap::new("checklist")
                .bind("up", keys("up"), "move up")
                .bind("down", keys("down"), "move down")
                .bind("tick", keys("space"), "tick or untick")
                .bind("done", keys("enter"), "finish"),
        }
    }
}

impl Component for Checklist {
    type Output = Vec<String>;

    fn handle(&mut self, event: &Event, _: &Context<'_>) -> Flow<Vec<String>> {
        let Some(key) = event.key() else { return Flow::Ignored };
        match self.keymap.action(key) {
            Some("up") => self.list.step(-1, 8),
            Some("down") => self.list.step(1, 8),
            Some("tick") => {
                if let Some(index) = self.filter.index(self.list.cursor()) {
                    self.list.toggle(index);
                }
            }
            Some("done") => {
                let ticked = self.list.selected();
                return Flow::Done(ticked.into_iter().map(|i| self.items[i].clone()).collect());
            }
            _ => match key.code {
                KeyCode::Char(c) if !key.modifiers.ctrl => {
                    self.filter.push(c);
                    self.list.set_len(self.filter.len());
                    self.list.reset(0, 8);
                }
                // Not ours: the container may want it.
                _ => return Flow::Ignored,
            },
        }
        Flow::Continue
    }

    fn render(&self, context: &Context<'_>) -> View {
        let theme = Theme::default();
        let mut lines = vec![kit::question(&theme, "Todo")];
        for position in self.list.visible(8) {
            let (index, matched) = &self.filter.matches()[position];
            let mut line = vec![kit::plain(if self.list.is_selected(*index) { "◉ " } else { "○ " })];
            line.extend(kit::highlight(&self.items[*index], matched, None, &theme.matched));
            lines.push(kit::fit(line, context.width));
        }
        View::new(lines)
    }

    fn keymap(&self) -> Keymap {
        self.keymap.clone()
    }
}
```

The built-ins also expose their parts. `Select` has `filter()`, `list()`,
`divider()` and `menu()` for its state, `event()` to handle an event as it
does, and `list_lines()` and `footer()` to draw part of it. `replace_items`,
`focus_item`, `set_values`, `set_heading`, `set_prefixes`, `set_hidden`,
`set_hints` and `set_steady` are the hooks `TableSelect`, `TreeSelect`,
`FilePicker` and `AssetPicker` use to build on it. `Input` exposes its
`buffer()`, `field()`, `resolve()` and `suggestion_rows()`, as `Form` uses
them.

## Containers

Every container is a `Component`, so containers nest, and you can pass a
whole composition to `run`, an `EventLoop` or `headless::run`. Children of
one container share its output type. A child whose output differs is adapted
with `ComponentExt::map`, which says what its answer means for the whole:

```rust
use rich_interact::compose::ComponentExt;
use rich_interact::{Flow, Input, Select};

// Picking finishes the whole composition with the file.
let picker = Select::new("File", ["a.rs", "b.rs"]).map(|file| Flow::Done(file.to_string()));
// Answering records the name and carries on; the input stays, showing
// its answer, and focus moves past it.
let name = Input::new("Name").map(|_name| Flow::Continue);
```

Cancelling a child cancels the composition, unless `.on_cancel(...)` says
otherwise. `Label` (markup) and `Painted` (a closure) show something without
taking focus.

| Container | Lays out |
|---|---|
| `Column`, `Row`, `Stack` | children along an axis, each `Size::Fixed(n)`, `Size::Flex(weight)` or `Size::Auto` (in a column, the rows it renders), with a `gap` |
| `Split::horizontal(a, b)`, `Split::vertical(a, b)` | two panes with a border between them. The mouse drags the border (`with_mouse(true)`); Alt+H and Alt+L move it side by side, Alt+K and Alt+J stacked. `ratio`, `at`, `min` and `mins` set its position and minimums |
| `Tabs` | one child at a time under a bar of titles. Every tab keeps its state. Alt+Right and Alt+Left (or Ctrl+PageDown and Ctrl+PageUp) switch tabs, Alt+1 to Alt+9 pick one, and a click on a title shows it |
| `Layers` | a base with `Layer::modal` and `Layer::popover` over it |

A modal dims the base, traps focus (Tab cycles inside it) and ignores
clicks outside it. A popover has no backdrop, and a click outside closes
it. Either layer takes the keys while it is open. Escape dismisses the top
layer when the layer does not use Escape, and a layer that cancels is
dismissed rather than cancelling the host. A layer whose child answers
closes, and the answer goes up, so a mapped `Flow::Done` finishes the host.
Layers open with `Layers::open_on(action, keys, description, factory)`, or
from anywhere through a `LayerHandle`:

```rust
use rich_interact::compose::{ComponentExt, Layer, Layers, Split, Tabs};
use rich_interact::keymap::keys;
use rich_interact::{Confirm, Flow, Select};

#[derive(Debug)]
enum App { Picked(String), Quit }

let files = Select::new("File", ["a.rs", "b.rs"]).map(|f| Flow::Done(App::Picked(f.into())));
let more = Select::new("More", ["c.rs"]).map(|f| Flow::Done(App::Picked(f.into())));
let app = Layers::new(Tabs::new().tab("Files", files).tab("More", more))
    .open_on("quit", keys("ctrl+q"), "quit", || {
        let confirm = Confirm::new("Quit?")
            .map(|answer| if answer == "yes" { Flow::Done(App::Quit) } else { Flow::Continue });
        Layer::modal(confirm).title("Quit").size(30, 6)
    });
```

## Focus, routing and bubbling

Each child gets a `Rect` of its container's space and a `Context` of that
size. It renders into that size and handles events at it.

- **Keys and pastes** go to the focused child. A child that does not use an
  event returns `Flow::Ignored`, and the event bubbles. The container tries
  its own bindings, then passes the event to its own container. At the top,
  the event loop treats `Ignored` as `Continue`. Every built-in returns
  `Ignored` for the keys it does not use.
- **Mouse events** go to the child under the pointer, in the child's own
  coordinates, if the child asked for the mouse. A press holds that child
  until the release, so a drag stays with it. A click focuses the child it
  lands on.
- **Resizes and ticks** reach every child. A container ticks as often as
  its most frequent child asks.
- **A hand-off's `Event::Returned`** goes back to the child that handed off.
- **Tab and Shift+Tab** (the `focus-next` and `focus-previous` bindings)
  move focus through every focusable child in order, into and out of nested
  containers. At the top, focus wraps round.

Focus keys bubble like any other key, so a child that uses Tab keeps it. A
multi-select marks with Tab, and an input with a suggestion showing
completes with it. A container can change the order by implementing
`Component::focus_step` and `Component::focus_enter`. A container can also
take a key before its children do, with `shortcut(...)`, or after they
leave it, with `on(...)`:

```rust
use rich_interact::compose::{Column, FOCUS_NEXT};
use rich_interact::keymap::keys;
use rich_interact::{Flow, Input};

let form = Column::new()
    .child(Input::new("Name").map(|_| Flow::Continue))
    .child(Input::new("Email").map(|_| Flow::Continue))
    .rebind(FOCUS_NEXT, keys("f6 tab"))
    .on("save", keys("ctrl+s"), "save", || Flow::Done(()));
```

A component that is not a container needs nothing extra. It takes focus by
default, and `focusable()` returns `false` for one that shows only.

## The keymap

A component declares its keys in a `Keymap`. Each `Binding` has an action,
the keys that trigger it, a description and a context (`select`, `input`,
`split`, `tabs`, ...). The component then asks what a key means with
`keymap.action(key)`, instead of matching the key itself.

`Component::keymap()` returns the bindings that apply now. A container's
keymap lists its focused child's bindings first, then its own. The help
overlay, the shortcut sheet, the command palette and the status bar's
hints read that list (see [Overlays and chrome](overlays.md)), and the
example's F1 dialog lists keymaps the same way:

```rust
for binding in app.keymap().bindings() {
    println!("{:<20} {}", binding.keys_label(), binding.description);
}
```

You can rebind keys at two levels:

- **one component:** `Select::rebind("down", keys("ctrl+j"))`,
  `Input::rebind(...)`, and `rebind` on every container, which covers
  `focus-next` too;
- **the whole process:** `keymap::install(overrides)`. `Overrides` holds
  `context.action = keys` pairs, set in code with `Overrides::set`, or read
  from text (`Overrides::parse`) or from a configuration table
  (`Overrides::from_pairs`). This is how a configuration file, such as the
  CLI's, can rebind any component:

```text
# one per line; `none` unbinds
select.down = ctrl+n, down
tabs.next   = alt+l
split.grow  = alt+.
select.pick = enter, "#"   # quote a key that is a comma or a hash
overlays.shortcuts = none
```

A `#` at the start of a line or after a space starts a comment, so the hash
and comma keys are written in quotes (`"#"`, `","`). A line with no keys is
an error rather than an unbind, so a stray `#` or `,` cannot unbind an
action by accident.

`Select` (and the views built on it), `Input` and every container dispatch
through their keymaps, so rebinding changes what they do. The other
built-ins declare their keys for help and hints. Rebinding them comes in a
later release.

## Testing a composition

Drive a composition headless, the same way as a single component.
`Script::drag` and `Script::click` reach the mouse. The example's tests are
in `crates/rich-interact/tests/compose.rs`, and a PTY test runs the example
in a real terminal:

```rust
use rich_interact::headless::{self, Script};

let script = Script::new().keys("down space alt+right alt+left enter");
let (outcome, record) = headless::run(app(), script, 72, 16);
assert!(record.frames.iter().any(|frame| frame.contains("Where to?")));
```

## Components from Python

The Python package (`rs_rich.interact`, from PyPI `rs-rich` 0.0.3) has the
same model. Subclass `Component`, and put it in the same containers as the
built-ins:

| Method | Returns | Default |
|---|---|---|
| `render(context)` | a renderable (a `str` is console markup) for `context.width` × `context.height` | none: you write it |
| `handle(event)` | `None` to carry on, `Done(value)`, `Cancel()`, or `Ignored()` to let the event bubble | `Ignored()` |
| `keymap()` | a `Keymap` of the keys it uses, for help and hints | `None` |
| `focusable()`, `mouse()`, `tick()` | whether Tab stops on it, whether it wants the mouse, how often it wants a `"tick"` (seconds) | `True`, `False`, `None` |
| `start(context)`, `default_value()` | a flow before the first paint; the answer without a terminal | `None` |

The containers are `Column`, `Row`, `Stack`, `Split`, `Tabs` and `Layers`
(with `Layer.modal` and `Layer.popover`), plus `Label` and `Map`. A
built-in's answer finishes the whole composition, as its Python value.
`Map(component, done)` says otherwise: `done(value)` returns `None` to carry
on, or `Done(...)` to finish.

```python
from rs_rich.interact import Component, Done, Ignored, Keymap, Select, Split


class Counter(Component):
    def __init__(self):
        self.count = 0
        self.keys = Keymap("counter").bind("up", "up k", "count up").bind("done", "enter", "answer")

    def handle(self, event):
        action = self.keys.action(event)
        if action == "up":
            self.count += 1
        elif action == "done":
            return Done(self.count)
        else:
            return Ignored()  # Tab bubbles up and moves focus

    def render(self, context):
        return f"count [bold]{self.count}[/]"

    def keymap(self):
        return self.keys


app = Split(Counter(), Select("File", ["a.rs", "b.rs"]), ratio=40)
print(app.headless("up tab down enter", width=60, height=6).value)  # b.rs
```

It runs through the same drivers as a single component: `ask`, `run`,
`headless` and `degrade`. The GIL is released between events and taken
back for each call into Python. An exception from `handle`, `render`, a
`Map` or binding callback, or a layer factory ends the run, and the call
that started it raises the exception. See
[Interactive components](https://buchochelliq-labs.github.io/rs-rich-cli/python/interact/#your-own-components) in
the Python docs.

## Components from plugins

A plugin registers a component by name, and an app mounts it beside the
built-ins. The plugin depends on `rs-rich-plugin-api` and core only, so it
implements that crate's small contract, `rich_plugin_api::component`:

- `PluginComponent::handle(event, context)` gets a `ComponentEvent`: a key
  by name (`"up"`, `"ctrl+k"`), a paste, a resize, a tick, the mouse or a
  link. It returns a `ComponentFlow`: `Continue`, `Done(text)`, `Cancel` or
  `Ignored`;
- `render(context)` returns a `ComponentView` of core segments;
- `bindings()`, `focusable()`, `tick()` and `mouse()` have defaults.

The plugin registers a factory, which makes a fresh component for each
mount:

```rust
registrar.component("counter", Arc::new(|| Box::new(Counter::default())));
```

`rich_interact::plugin::PluginView` mounts it from a registry. Its answer
is the plugin's text; map it into your container's output like any other
child:

```rust
use rich_interact::compose::{ComponentExt, Split};
use rich_interact::plugin::PluginView;
use rich_interact::{Flow, Select};

let registry = rich_ext::ExtensionRegistry::with_linked_plugins()?;
let counter = PluginView::mount(&registry, "counter")?; // UnknownComponent lists the names
let app = Split::horizontal(
    counter.map(Flow::Done),
    Select::new("File", ["a.rs", "b.rs"]).map(|file| Flow::Done(file.to_string())),
);
```

The plugin's bindings are listed under its registered name, so the help
overlay shows them and configuration rebinds them (`counter.up = k`); a
rebound key reaches the plugin as the key it declared. A plugin component
that panics does not take the app down. Its pane shows the panic, and it
stops taking focus and events.

Components come from plugins compiled into the program: added with
`add_plugin`, or linked with `export_plugin!`. A runtime plugin (a native
library or a WASM module) exchanges text through a stateless ABI, which has
no component kind in this release. See [Plugins](../../PLUGINS.md).
