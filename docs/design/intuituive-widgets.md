# intuiTUIve widgets: the missing components

**Status:** built (rs-rich-intuituive 0.0.2, rs-rich-ext 0.0.15), except
moving the built-in nodes onto the trait, which stays to do. Where the
build differs from this note:

- **Scrolling draws offscreen** (option (b) below, not (a)). The scroll's
  child draws into a persistent offscreen screen with its own damage, and
  the rows in view are copied out. Only what changed inside draws, so it
  keeps damage exact without teaching every node about clipping. Clicks
  and the caret are translated through it.
- **`scroll` and pop-ups are framework nodes, not widgets**, because they
  change how a subtree is drawn or layered. `table` and `tabs` are built on
  the public trait.
- **The scope re-port removed about 110 lines**, not 200: its own plot
  (axes, legend box, clipping, cell grid) is now rs-rich-ext's `Chart`.
- **Chart axes follow ratatui**: `Labels::None` draws no axis line, and an
  empty `Labels::Values` draws the line with no labels.

## Phases D and E: built in 0.0.2

The two phases after the plan below, from the list of what other
frameworks (ratatui above all) leave out:

| Phase | Built | Where |
|---|---|---|
| D | `tree`, `hsplit`/`vsplit` with a draggable divider, `calendar` with `Date`, `virtual_list` | `src/tree.rs`, `src/split.rs`, `src/calendar.rs`, `src/widgets.rs` |
| E | the command palette and help from described bindings; menu bars, drop-downs and context menus; pop-ups anchored to a `Rect`; toasts; `animate` with easing; text selection and copying with the mouse captured | `src/app.rs`, `src/menu.rs` |

All Phase D widgets are built on the public `Widget` trait, which is the
test that it is enough. Building them found these gaps in it, still open:

1. **Hover is per widget, not per cell.** `DrawCx::hovered()` covers the
   whole widget, and asking subscribes it to every change of the hover
   path. The split keeps the last pointer position itself. A "pointer in
   me" signal, or a leave event, would do better.
2. **Keys reach only the focus path.** With nothing focused, keys go to
   the root, so a container that is not focusable and has no focusable
   child never sees them.
3. **`layout()` runs when the widget redraws.** A container must read its
   sizing signals in `layout` or `draw` to be laid out again. This works,
   but needs documenting.
4. **No focus, blur or resize events.** Widgets compare state at draw time.
5. **The drawing helpers are private.** Markup on one line, its width, and
   combining a style over segments are crate-private; a
   `Canvas::markup(x, y, width, markup, style)` would cover them.

Phase E chose to:

- **Select text only where nothing uses the press.** A press any node or
  widget uses never starts a selection, so drags, splits and buttons keep
  working. Copying goes through OSC 52.
- **Build the palette and help from bindings.** They reuse
  rs-rich-interact's `Palette` and `Help`, so no second command registry is
  needed; a binding without a description stays out.
- **Drive animations from the app clock.** The loop wakes every 16 ms only
  while one runs, and the headless backend's virtual clock tests them.

### Later: accessibility

Designed, not built. The retained tree already holds what a screen reader
needs: each node's kind, label, focus state and the focus order. The plan:

- **A semantic role per node** (`button`, `list`, `listitem`, `tab`,
  `dialog`, `menu`), set by the built-in nodes and by a `Widget::role()`
  method with a default.
- **An accessible text per node**, from its label or a `.describe(text)`
  builder, and the selected row or day for widgets that have one.
- **Announcements**: the focus moving, a toast, a modal opening, sent to
  a sink. The first sink writes them to a side channel (a file or a
  socket) for a bridge; platform bridges (AT-SPI, UI Automation, the
  macOS accessibility API) come after, outside this crate.
- **A text mode** using rs-rich-ext's accessibility profile: no box
  drawing, no colour-only meaning, a linear reading of the screen.

### Later: serving to a browser

Designed, not built. An app already draws through a backend that only
needs cells and events, which is what the headless backend implements, so
a web backend fits the same seam:

- **A WebSocket backend** sends the damaged cells of each frame (or the
  ANSI rs-rich already produces) to an xterm.js page and turns the page's
  key and mouse events back into rs-rich-interact events.
- **A session per connection**, each with its own `App`, since the
  reactive runtime is per thread; a thread per session to start.
- **Off by default**, behind a feature in its own crate
  (`rs-rich-intuituive-web`), so the framework gains no network code. It
  binds to localhost unless told otherwise and has no authentication of
  its own, so exposing it is the user's choice, behind a proxy that has.

The original note follows. Its status was: design note, for discussion. It
follows [the intuiTUIve design spike](intuituive.md), whose plan is done
except for Python, and the two ports that tested the framework on real
apps: the Yazi-style file manager (`examples/files.rs`) and the scope-tui
rebuild (`examples/scope.rs`).

## Summary

- **One foundation is missing: a public widget trait.** Built-in nodes
  (stacks, grids, panels, lists, logs) are cases of a private enum. A
  user's control can only be one of these:
  - a `leaf`, which draws but cannot measure itself apart from drawing,
    hold children, or see the mouse;
  - a `component`, a black box that cannot contain intuiTUIve nodes.

  Nothing a user writes has the powers the built-ins have. A `Widget`
  trait would give it them (measure, lay out children, draw, handle
  events, take the focus, place the caret). Every other gap below can
  then be built as an ordinary widget, by us or by users.
- **Charts: yes, we need a ratatui-style chart, plus a canvas.**
  rs-rich-ext's `LineChart` decides its own axes, labels and legend. The
  scope port therefore spent about 200 lines drawing its own plot (axes,
  legend box, clipping, colour layers). We need:
  - a `Chart` with configurable axes (bounds, titles, labels or none,
    linear or log scale), data sets with marker types, and legend
    placement;
  - a `Canvas` with shapes and layers.

  Both belong in rs-rich-ext, so the CLI and Python can use them too.
- **Then five widgets on the trait:** a scroll container, an interactive
  table, a tab strip, anchored pop-ups, and mouse events beyond clicks
  (hover, drag, wheel).
- **The order matters.** The trait comes first, because the others are
  built on it. Charts come second, and the scope example's plot re-ported
  onto them is the test. The five widgets can then be built in parallel.

## What exists today

What a ratatui app reaches for, and where it is in intuiTUIve:

| ratatui | intuiTUIve today | Gap |
|---|---|---|
| `Block` | `.panel(title)`, `.padding(…)` | none |
| `Paragraph` | `text`, `label` (rich markup) | it can't scroll; see [Scrolling](#3-scrolling) |
| `List` + `ListState` | `list(items, selected)`, `each(keys, build)` | none |
| `Table` + `TableState` | `renderable(…)` of rich's `Table`, or rs-rich-ext's `VirtualTable` | rows can't be selected, and it has no sized columns or sticky header; see [Table](#4-table) |
| `Tabs` | `switch(key, build)` swaps the content | no tab strip; see [Tabs](#5-tabs) |
| `Chart`, `Canvas` | `renderable(…)` of `LineChart`; `DotCanvas` by hand | see [Charts and canvas](#2-charts-and-a-canvas) |
| `Gauge`, `LineGauge`, `Sparkline`, `BarChart` | rs-rich-ext's gauge, sparkline, bars, histogram, heatmap, KPI and timeline renderables, through `renderable(…)` | none; they are documented as rich renderables, not as widgets |
| `Scrollbar` | none | see [Scrolling](#3-scrolling) |
| `Clear` + a popup rect | `cx.modal(width, height, build)`, centred | a pop-up can't be anchored to a node; see [Pop-ups](#6-anchored-pop-ups) |
| Text input (tui-input, tui-textarea) | `component(Input)`, `component(TextArea)` | none |
| `impl Widget for MyType` | `leaf`, `component`, functions returning `Node` | see [The widget trait](#1-the-widget-trait) |
| Mouse events | `on_click` (left button), and full events inside a component | no hover, drag or wheel on nodes; see [Mouse](#7-mouse) |
| `Buffer` cell writes | a leaf returns styled lines | widgets can't write cells; see [The widget trait](#1-the-widget-trait) |

## 1. The widget trait

### Goal

A control written outside the crate can do everything a built-in node
does:

- size itself;
- lay out children that are ordinary `Node`s;
- draw cells directly;
- handle keys and mouse events in its own coordinates;
- take the focus and place the caret;
- redraw only when what it read changes.

This is the sanctioned extension seam, and it is how users "create their
own controls and types".

### Sketch

```rust
/// A node's behaviour. Built-in nodes implement it too, over time.
pub trait Widget: 'static {
    /// What the inspector calls it (`"table"`, `"chart"`).
    fn name(&self) -> &'static str;

    /// Cells along `axis` the widget needs, given `avail` (a 0 on the other
    /// axis: as much as it likes). Signals read here re-measure it when they
    /// change. Default: draw at `avail` and count, as a leaf does today.
    fn measure(&mut self, cx: &mut MeasureCx, axis: Axis, avail: (u16, u16)) -> u16;

    /// Its children, for widgets that hold nodes.
    fn children(&self) -> &[Node] { &[] }

    /// Where each child goes inside `rect`, in the order of `children`.
    fn layout(&mut self, cx: &mut LayoutCx, rect: Rect) -> Vec<Rect> { Vec::new() }

    /// Draw itself (not its children) into `canvas`, which is clipped to the
    /// widget's rectangle and uses its coordinates. Signals read here
    /// subscribe the widget, as a leaf's do.
    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas<'_>);

    /// A key, mouse or focus event, in the widget's own coordinates.
    /// Unused events bubble to the parent, as keys do today.
    fn event(&mut self, cx: &mut EventCx, event: &WidgetEvent) -> Used { Used::No }

    /// Whether Tab can stop here.
    fn focusable(&self) -> bool { false }

    /// Where the text caret goes while it has the focus.
    fn caret(&self) -> Option<(u16, u16)> { None }
}

/// A node from any widget; it takes every builder (`.flex`, `.panel`,
/// `.on_key`, `.name`).
pub fn widget(w: impl Widget) -> Node;
```

### What each part gives

- **`Canvas`** is a clipped, translated view of the screen. It has:
  - `set(x, y, char, style)`;
  - `print(x, y, text, style)`;
  - `lines(&[Vec<Segment>])`, which is what a leaf does now;
  - `fill(rect, style)`.

  Direct cell writes are what the scope plot needed. They skip building
  segments and are the fast path for high-rate drawing. Damage stays the
  widget's rectangle, so the painter needs no change.
- **`DrawCx`** gives the widget:
  - the console and the theme;
  - `focused()` and `hovered()`, which subscribe the widget like any
    other read;
  - `redraw()`, for internal state that is not a signal, such as an
    animation frame.
- **`EventCx`** wraps today's `Ctx` (quit, push, modal, focus) and adds:
  - `redraw()`;
  - `capture_mouse()`, which keeps drag events coming while the pointer
    leaves the widget.
- **Containers.** A widget that returns children is a container. The
  framework draws the children with today's damage logic (`draw_children`)
  after `draw`, so a container draws its chrome first: a table header, a
  scrollbar.

### How it fits what exists

- **The enum stays.** `Kind` gains a `Custom(Box<dyn Widget>)` case first,
  and every built-in keeps working.
- **Built-ins move onto the trait one by one:** panel, pad and log first.
  This proves the trait expresses them, with the benchmark gate as the
  referee. Stack and grid stay native until the gate shows the trait
  costs nothing.
- **`leaf` and `component` stay.** A leaf becomes a one-method widget
  internally.
- **`component` stays for rs-rich-interact components,** which also run
  outside intuiTUIve. A widget is the richer choice inside an app,
  because it can hold nodes.

### Decisions to take

- **Object-safe trait (`Box<dyn Widget>`) or generic nodes?**
  Recommendation: object-safe, as `Kind` already boxes its cases. A
  virtual call per drawn widget does not show up next to rendering.
- **Should `measure` have a default?** Recommendation: yes, the leaf
  behaviour (draw and count). Most widgets then write only `draw`.

## 2. Charts and a canvas

They live in `rich_ext::chart`, as renderables. They are library features
that the CLI (`rich chart`) and the Python bindings want too, so they don't
belong in the TUI crate (see AGENTS.md: library features go in rs-rich-ext).

### `Canvas` (renderable)

- **World coordinates:** `x_bounds` and `y_bounds`, as ratatui's
  `Canvas`.
- **A marker** per canvas or per layer:
  - Braille (2×4 dots a cell, built on today's `DotCanvas`);
  - half blocks (1×2);
  - full blocks;
  - a dot;
  - ASCII.
- **Shapes:** points, line, polyline, rectangle, circle and a text
  label. Each is clipped to the bounds; the Liang–Barsky clipper from the
  scope example moves here.
- **Layers in order,** each with a style. A later layer's cell wins, as
  in the scope plot.
- **Drawing with a closure:** `Canvas::new().paint(|p| { p.line(…);
  p.layer(); … })`.

### `Chart` (renderable)

- **`Axis`:**
  - bounds;
  - a title;
  - labels: none, automatic (n ticks), or a list given by the caller;
  - scale: linear or log;
  - a style.

  The scope spectroscope's log frequency axis becomes `Scale::Log`,
  instead of the app taking `ln` of every point.
- **`Dataset`:** a name, points, line or scatter, a marker and a style.
- **Legend:** top right (ratatui's default), top left, bottom, or none.
  It hides itself when it would cover more than a set share of the plot.
- **Layout:** the same as ratatui's `Chart`, so ported apps look the same.
- **`LineChart` stays** as the automatic convenience, and becomes a preset
  over `Chart`.

### In intuiTUIve

`renderable(move || Chart::new()…)` already works, and redraws when the
signals it reads change. A small `chart` widget on the trait adds the
mouse: the pointer's position mapped to world coordinates, for a hover
read-out or zoom by dragging.

### Acceptance

1. Re-port the scope example's plot onto `Chart` (and the spectroscope's
   grid lines onto `Axis` labels).
2. The example loses its own plot drawing: `draw`, `clip`, the legend box
   and the cell grid, about 200 lines.
3. `docs/tapes/scope.tape` still passes `--check`, or any change to its
   screenshots is explained.

## 3. Scrolling

A `scroll(child)` widget shows a viewport onto a child laid out at its
full height (or width):

- The offset is a signal the app can read and set.
- Arrows, PgUp/PgDn and Home/End scroll it while the focus is inside, and
  so does the mouse wheel over it.
- An optional scrollbar is drawn as the container's chrome.
- When the focus moves to a node outside the viewport, it scrolls that
  node into view.

**Implementation decision.** There are two options:

- **(a)** Lay the child out in a virtual space and translate and clip
  when drawing. This needs a clip rectangle in the frame state, and
  coordinates that can sit above the viewport.
- **(b)** Draw the child into an offscreen screen and copy the visible
  part.

Recommendation: **(a)**. It keeps damage exact and costs nothing for
rows out of view, while (b) renders everything.

**Virtualisation.** A child that knows its row count can draw only the
visible rows: a list, `each`, a table or a log. The scroll widget passes
the visible window down through `MeasureCx` and `DrawCx`. That is what
makes a 100,000-row list cost one screen.

## 4. Table

A `table(rows, columns)` widget, the equivalent of ratatui's `Table` with
`TableState`:

- **Columns** take `Size` constraints, solved with the layout module's
  `solve`, the same solver as rows and grids.
- **A sticky header,** and a row selection signal (optionally a cell),
  moved by keys and clicks.
- **Rows** come from a closure over signals for small tables, or from
  rs-rich-data's `VirtualRows` for large ones. Large tables draw only the
  visible window, as rs-rich-ext's `VirtualTable` (#260) does.
- **Hooks** for sort indicators and click-to-sort, with styles from the
  theme (`table.header`, `selected`).

## 5. Tabs

A `tabs(titles, selected)` strip, paired with today's `switch` for the
content:

- ←/→ change the tab while the strip has the focus, Alt+number from
  anywhere.
- A click on a title selects it.
- Styles come from the theme.
- Dividers and highlight markers can be set, as in ratatui's `Tabs`.

## 6. Anchored pop-ups

`cx.popup(anchor, placement, size, build)` opens a layer like a modal, but
next to a node rather than in the middle of the screen:

- It is placed below, above, right or left of the anchor.
- It flips when it would leave the screen.
- It closes on Esc, or on a click outside if asked to.

Dropdown selects, autocomplete lists, tooltips and context menus are
built on it. The layer code that modals use already handles focus and
redraw; only the placement is new.

## 7. Mouse

rs-rich-interact already reports every mouse event (`Down`, `Up`, `Drag`,
`Moved`, `ScrollUp`, `ScrollDown`). intuiTUIve only routes left clicks.
The proposal:

- **Every event goes to the deepest node under the pointer,** in that
  node's coordinates, and bubbles up like keys.
- **`on_mouse(|cx, event| …)` on any node,** for apps that don't need a
  full widget.
- **Hover is state** that the framework tracks and a widget reads (and is
  subscribed to) in `draw`, so a hover highlight redraws exactly the two
  nodes the pointer left and entered.
- **Drag capture,** through `EventCx::capture_mouse`.
- **Motion events are enabled only while something reads hover or
  captures the mouse.** This keeps apps that don't use the mouse cheap.

## 8. Documenting what already works

The gauge, sparkline, bars, histogram, heatmap, KPI card, timeline, text
area and input are usable now, through `renderable(…)` and `component(…)`.
The guide should list them as widgets, each with a line of code and a
screenshot.

## Plan

| Phase | Work | Proves it |
|---|---|---|
| A | The `Widget` trait, `Canvas`, `Kind::Custom`, mouse routing and hover; panel, pad and log moved onto the trait | The existing tests and the ratatui benchmark gate pass unchanged |
| B | `Chart` and `Canvas` in rs-rich-ext (with `LineChart` as a preset), and a `chart` widget | The scope example re-ported onto them; its tape still checks |
| C | `scroll`, `table`, `tabs`, `popup` as widgets on the trait | An example and tests each, and one more port of a table-heavy ratatui app (a candidate is [bottom](https://github.com/ClementTsang/bottom) or [gitui](https://github.com/gitui-org/gitui), credited as the earlier ports are) |
| D | Guide pages for every widget and for writing your own; recordings; Python bindings for widgets once the trait is stable | Docs build strict; tapes check |

**Versioning:**

- Phase A changes rs-rich-intuituive's public API (a new trait, and the
  mouse events), so it ships as rs-rich-intuituive 0.0.2 or later.
- Phase B adds to rs-rich-ext's public API, a minor bump under its own
  SemVer.
- Core rs-rich is not touched at any point.

## Open questions

1. Is an object-safe `Widget` trait right (recommended), or should
   widgets be generic?
2. Should built-ins move onto the trait (recommended, gradually), or
   stay as enum cases for good?
3. Do `Chart` and `Canvas` belong in rs-rich-ext (recommended) or in the
   TUI crate?
4. Scrolling by translating and clipping (recommended), or offscreen?
5. Should Python bindings for widgets come in phase D, or wait for the
   Python bindings of the framework itself?
