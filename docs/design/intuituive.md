# intuiTUIve design spike (milestone 11)

**Status:** design note, with a prototype and measurements. Nothing here is
in a published crate yet. The prototype lives in
[`docs/design/intuituive/prototype`](https://github.com/buchochelliq-labs/rs-rich-cli/tree/main/docs/design/intuituive/prototype),
a standalone crate that is not a workspace member, not published and not
built by CI. Run it with `cargo run --release` from that directory.

## Summary

- **Recommendation: build intuiTUIve as a retained tree with fine-grained
  reactive state, drawn into a retained cell buffer with damage tracking,
  on top of `rs-rich-interact`.** In the prototype, rich renders only the
  nodes whose state changed (1 to 3 of 31 per frame), writes them into
  their rectangles, and only those rectangles are diffed and sent.
- **It beats ratatui on the interactions that dominate real apps.** A
  status tick takes **20 µs against ratatui's 122** at 80x24 and **35 against
  369** at 200x60. A selection move takes 46 against 101, and 91 against 354.
  The one loss is a streaming log: rich re-renders every visible line on each
  append, which costs 1.3 times ratatui at 80x24 and 2.3 times at 200x60. A
  streaming leaf would fix that (see [Results](#results)).
- **The model is what makes it fast, not tuning.** With rich's rendering
  unchanged, re-rendering everything each frame (today's `rich-interact`
  model) is 5 to 10 times slower than ratatui. Retention alone does not close
  the gap, because the painter then rebuilds and diffs the whole screen.
  Retention, damage rectangles and a cell buffer together do.
- **ratatui interop works both ways and costs little.** Any rich renderable
  draws as a ratatui widget, and any ratatui widget runs as a
  `rich-interact` component. Styles round-trip losslessly for the common
  subset, and the losses are listed below.
- **Less code for the author:** the same dashboard is 66 lines of author code
  in the prototype (56 in the framework crate's API) against ratatui's 79,
  counting each side's state as well as its drawing. The app holds no draw loop, no layout pass and no
  "what changed" bookkeeping.
- **Decided:** our own cell screen with a ratatui adapter, interop in
  `rs-rich-ratatui`, and the framework in a new crate (see
  [Decisions](#decisions-2026-10-07)). The work is in [Plan](#plan).

## What "better than ratatui" means

ratatui is the bar: mature, fast, backend-agnostic, with a large widget
ecosystem. It is deliberately a *library of widgets plus a buffer diff*, so
everything above that is left to each app: state, events, focus, keymaps,
text input, scrolling, theming and testing. "Better" therefore has to mean
**as fast, and everything above the buffer done for you**. Each line of the
scorecard is something a reviewer can check:

| Dimension | ratatui | intuiTUIve target | Where this stands |
|---|---|---|---|
| Frame cost on small updates | redraws every widget each frame, diffs the whole buffer | renders only what changed, diffs only damage | **measured here: 2 to 10 times faster** |
| Frame cost on bulk updates | baseline | within 1.2 times | **not yet:** log append 1.3 to 2.3 times slower; needs a streaming leaf |
| Bytes to the terminal | cell diff | cell diff, no more | **measured: no more than ratatui** (31 against 37 per tick, 437 against 443 per log line) |
| State and invalidation | app's job (`App` struct, redraw-all) | signals: write a value, the right nodes update | prototype (signals, keyed identity) |
| Events, focus, keymaps | app's job | built in: bubbling, Tab focus, keymap registry | **shipped in `rich-interact` 0.0.14** |
| Components (input, select, pager, forms, palette) | third-party crates of varying upkeep | built in, themed, one API | **shipped (0.0.13 and 0.0.14)** |
| Rich content in a TUI | spans and paragraphs | Markdown, syntax, tables, trees, tracebacks, charts, diagrams, images, as leaves | available (`rich`, `rich-ext`, `rich-art`) |
| Layout | constraint solver (`Layout`) | fixed, flex, constraints and grid, declared on the tree | **shipped in `rs-rich-intuituive`:** fixed, percent, flex, content-sized, min/max, gaps, padding, grid with spans |
| Testing | `TestBackend` buffer asserts | headless driver with scripted keys, plus tapes and screenshots | **shipped (headless, `rich record`)** |
| Pipes, CI, `NO_COLOR`, screen readers | app's job | degradation policy, line I/O fallback | **shipped (`policy`)** |
| Python | none | the same components from Python | shipped for components; the framework would follow |
| Adoption path | — | use rich inside ratatui today; ratatui widgets inside intuiTUIve | **prototype: both directions** |

## What the spike built

All in the prototype crate:

| Module | What it is |
|---|---|
| `reactive.rs` | Signals in a thread-local store. A node that reads a signal while it renders subscribes to it; a write marks its subscribers dirty. Subscriptions are recorded again on every render, so a branch that stops reading a signal stops depending on it. |
| `tree.rs` | A retained tree: leaves, stacks (fixed and flex sizes), borders and keyed lists that reconcile children by key. It has two outputs: lines, composed through the caches, and a ratatui `Buffer`, where a frame writes only dirty leaves into their rectangles and records damage. `App` implements `rich_interact::Component`, so a tree runs under the existing event loop, the blocking driver and the headless driver. |
| `paint.rs` | A row-aware painter, the intermediate step the measurements went through. |
| `interop.rs` | Both ratatui directions, the style mapping, and buffer and lines converters (see [ratatui interop](#ratatui-interop)). |
| `dashboard.rs` | One ops dashboard written twice, with ratatui and with the prototype: a header, a service list with a selection, a detail pane, a log tail and a status line. |
| `bench.rs` | The benchmark below. |

17 tests cover the store, the tree, keyed identity, the buffer path drawing
exactly what the lines path draws, the headless driver, the dashboard and
interop.

## Results

The benchmark runs 500 frames per scenario and takes the best of three runs.
Timings on this machine vary by about 30% from run to run, so treat
differences under 1.5 times as noise.

- **ratatui** draws through its `Terminal`, with its `CrosstermBackend`
  writing into memory.
- **The rich variants** paint with `rich-interact`'s `Painter`, the row
  painter, or the prototype's cell buffer.
- **The cell-buffer variant** sends through the same `CrosstermBackend` as
  ratatui, so its byte counts compare like for like.

The scenarios:

- **tick:** the footer counter changes;
- **select:** the selection moves one row;
- **log:** a log line arrives and the counter changes.

| Size | Scenario | ratatui | rich, re-render all | rich, signals + retained | + row painter | **+ cell buffer** |
|---|---|---:|---:|---:|---:|---:|
| 80x24 | tick | 122 µs, 37 B | 591 µs | 306 µs | 60 µs | **20 µs, 31 B** |
| 80x24 | select | 101 µs, 184 B | 617 µs | 453 µs | 282 µs | **46 µs, 156 B** |
| 80x24 | log | 112 µs, 443 B | 894 µs | 719 µs | 503 µs | **144 µs, 437 B** |
| 200x60 | tick | 369 µs, 37 B | 2,131 µs | 1,069 µs | 82 µs | **35 µs, 31 B** |
| 200x60 | select | 354 µs, 280 B | 2,037 µs | 1,491 µs | 697 µs | **91 µs, 156 B** |
| 200x60 | log | 357 µs, 1,480 B | 3,611 µs | 3,686 µs | 2,289 µs | **831 µs, 1,474 B** |

Nodes rendered per frame, of 31: 31 for re-render-all. Retained renders 2
for a tick, 9 for a select and 6 for a log append. In the cell buffer,
containers no longer render at all, which leaves 1, 3 and 2.

What the columns show:

1. **Re-render everything** is today's model. Even with rich's own diff
   writing the fewest bytes, it is 5 to 10 times slower than ratatui: rich
   rendering is costlier per cell than ratatui's widgets, and it runs for
   every node every frame.
2. **Signals + retained** cuts the nodes rendered by 3 to 15 times, but
   barely moves the time. A breakdown of a retained tick at 80x24 shows why:
   - tree: 26 µs;
   - copying the lines into a `View`: 13 µs;
   - `Painter`: **219 µs**. It clones, sanitises and segments every row of
     the screen into a cell frame before it diffs.
3. **Row painter.** Skipping unchanged rows makes a tick cheap, but a select
   or log frame still recomposes whole rows of segments through every
   container, and the log changes every row it crosses.
4. **Cell buffer.** Containers stop composing. Dirty leaves write their own
   rectangles, and only damaged rectangles are diffed. Rich's cost now scales
   with what changed, not with the screen.

**Where it still loses: the log.** Each append re-renders the whole log leaf,
up to 56 lines of rich `Text` at 200 columns, about 600 µs. ratatui's
`Paragraph` lays out the same lines more cheaply. Two ways to fix it, which
can be combined:

- a streaming leaf that renders only appended lines and shifts its existing
  cells in the buffer;
- the terminal's scroll region, so the scroll costs one line of bytes.

Both fit the damage model.

**Bytes.** Per update, the cell buffer sends no more than ratatui (it uses
ratatui's encoder). Its first frame is larger: 7,182 bytes against 3,845 at
80x24. Damage rectangles overlap (a border, then its child), so some cells
are sent twice; merging rectangles before the diff fixes that.

### Interop costs

| Operation | Time |
|---|---:|
| A 60-row rich `Table` drawn into a 100x40 ratatui `Buffer` through `RichWidget` | 1.34 ms per frame |
| A whole 200x50 ratatui `Buffer` converted to rich lines (`buffer_to_lines`) | 405 to 565 µs |

- **`RichWidget`:** rich renders all the rows before cropping to 40. Inside
  intuiTUIve that cost is paid only when the table's state changes.
- **`buffer_to_lines`:** about 40 ns a cell, mostly cloning `Style`s, whose
  colour names are heap strings. A ratatui widget inside intuiTUIve converts
  only its own rectangle, and only when it is dirty.

## ratatui interop

Both directions are in `interop.rs`. Ten tests cover them, including runs
under the headless driver and a Panel → `Buffer` → lines round trip that is
exact.

- **rich in ratatui.** `RichWidget` is a ratatui `Widget` over any rich
  `Renderable` (`RichWidget::markup("[bold]hi")`, a `Table`, Markdown,
  `Syntax`). It is the zero-commitment adoption path: a ratatui app can
  draw rich content today.
- **ratatui in intuiTUIve.** `RatatuiComponent` runs any
  `Fn(Rect, &mut Buffer)` drawing ratatui widgets as a `rich-interact`
  component, with an optional event handler and state. In the cell-buffer
  design a ratatui widget is a leaf that draws straight into its rectangle,
  with no conversion at all, if the screen is a ratatui `Buffer` (see the
  decisions).
- **Style mapping.** It is lossless for the 16 named colours, 256 indexed,
  RGB, `default` ↔ `Reset`, and 9 of rich's 13 attributes (`blink2` maps to
  rapid blink, `conceal` to hidden). The losses:
  - **dropped:** rich's `underline2`, `frame`, `encircle` and `overline`;
  - **dropped:** rich's hyperlinks and meta, since ratatui cells have no
    links;
  - **dropped:** ratatui's underline colour;
  - **renamed:** colour names come back by number (`grey0` as `color(16)`);
  - **read as unset:** an explicit `default` colour, because a fresh ratatui
    cell is `Reset`.
- **Widths.** rich and ratatui measure some characters differently. The
  converter places each segment at the column rich measured, so a
  disagreement cannot shift the rest of the line.
- **ratatui 0.30 notes:**
  - ratatui is now split into crates: `Buffer`, `Cell`, `Style` and `Widget`
    live in `ratatui-core`, widgets in `ratatui-widgets`.
  - Wide characters' trailing cells are not marked; you find them by the
    leading symbol's width.
  - `Cell::skip` is deprecated in favour of `CellDiffOption`, whose
    `ForcedWidth` is the one route to links or images in a cell.
  - ratatui and `rich-interact` both use crossterm 0.29.

## Proposed architecture

```text
 signals (per-app runtime)        events ─▶ keymap / focus (rich-interact 0.0.14)
        │ subscribe on read                      │ handlers write signals
        ▼                                        ▼
 retained node tree ── keyed identity, layout (fixed / flex / constraints / grid)
   leaves: rich renderables · rich-interact components · ratatui widgets
        │ only dirty leaves render, into their rectangles
        ▼
 retained cell screen + damage rectangles
        │ diff damaged rectangles against the previous screen
        ▼
 encoder ─▶ terminal (alt screen or inline) · headless · tapes / SVG
```

- **Built on `rich-interact`, not beside it.** The root is a `Component`, so
  sessions, drivers, the headless driver, degradation, keymaps, focus and
  every existing component carry over. The 0.0.14 containers (`Stack`,
  `Split`, `Tabs`, `Layers`) become tree nodes that keep their routing.
- **Per-app runtime.** The prototype's store is thread-local with one dirty
  set, so two apps on one thread take each other's invalidations. The
  framework scopes it per app, and adds memos (derived values, so twenty
  rows don't all depend on `selected`), effects, and disposal when a keyed
  child is removed.
- **Damage is the contract between nodes and the screen.** Overlays, modals
  and popovers are layers: closing one damages what was under it. Graphics
  placements (micro assets) attach to rectangles and are redrawn when their
  cells are damaged, the rule `rich-interact`'s painter already follows.

## What the spike did not test

These are open, not solved:

- **Layout:** constraints and grid (only fixed and flex here).
- **Memos and effects;** async work feeding signals.
- **Focus and keymaps on tree nodes:** the 0.0.14 routing exists, but is not
  yet wired into the prototype.
- **Overlays and z-order damage.**
- **Inline (non-alternate-screen) mode.**
- **Graphics placements in the cell screen.**
- **Wide characters at a damage rectangle's edge:** the damaged diff skips a
  wide character's trailing cell, but does not widen the rectangle to it.
- **Python bindings** for the framework layer.
- **A real terminal:** every number above is in memory.

## Decisions (2026-10-07)

The maintainer decided:

1. **Our own cell screen, with a ratatui adapter.** The screen is a cell
   buffer of our own, which keeps hyperlinks and rich's full attribute set.
   A ratatui widget draws into a scratch ratatui `Buffer` the size of its
   rectangle, and an adapter converts those cells into ours. That costs
   about 40 ns a cell, paid only while the widget is dirty. The prototype
   measured ratatui's `Buffer`, so the slice re-measures with ours before
   it ships.
2. **ratatui interop in its own crate,** `rs-rich-ratatui`. The
   `rich` → ratatui direction needs only `rich` and `ratatui-core`; the
   intuiTUIve adapter sits behind a feature. Nothing else in the workspace
   depends on ratatui.
3. **The framework in a new crate** on top of `rs-rich-interact`, so the
   component API stays stable while the framework's settles.
4. **The scorecard is the public bar,** with the benchmark in CI as a
   regression gate. "Better" also means **easier to learn and use**, and
   **free of the architectural problems ratatui apps hit** (next section).

## The architectural problems we design out

These are the structural pain points ratatui apps commonly report. Each is a
consequence of ratatui's scope (a widget library plus a buffer diff), not a
bug, and each is a design requirement here.

| ratatui problem | Why it happens | intuiTUIve's answer |
|---|---|---|
| **Everything redraws every frame**, so cost grows with the screen, not with what changed. | Immediate mode: the app calls `draw`, and every widget renders into a fresh buffer that is diffed whole. | Retained nodes, signals, damage rectangles: work scales with the change (measured above). |
| **State lives apart from the widget.** `StatefulWidget` takes its state as a separate argument, and widgets are consumed by value each frame. | Widgets are short-lived values; anything that must survive a frame belongs to the app. | A component owns its state for its whole life, keyed identity keeps it across reorders, and signals make it observable. |
| **No event routing, focus or hit-testing.** Each app maps keys to panes itself, and must remember last frame's `Rect`s to route a mouse click. | Layout happens inside `draw`, and its results are thrown away. | Layout is retained on the tree. Events bubble from the focused node, clicks hit-test against the retained rectangles, and keymaps are declared (`rich-interact` 0.0.14). |
| **Every app writes its own event loop,** and async (a background fetch updating the UI) is wired by hand. | The library stops at drawing by design. | A runtime ships the loop, and an async task writes a signal from any thread. The app never calls `draw`. |
| **Styling is per widget,** with no theme to change an app's look in one place. | `Style`s are values set on each widget. | App-level themes and named styles on the tree, building on rich's theme stack. |
| **Cells hold a symbol and a style only.** Hyperlinks and images need workarounds outside the cell model. | The `Cell` type has no place for them. | Our cell screen carries links and graphics placements; micro assets and images already ride on them in `rich-interact`. |
| **Text input, scrolling, forms and lists come from third-party crates** of varying upkeep, each with its own conventions. | Out of a widget library's scope. | Built in, themed, one API, already shipped as `rich-interact` components. |
| **Tests assert on a buffer,** with no scripted keys, clicks or timing. | `TestBackend` is a buffer. | The headless driver plays scripted keys, clicks, resizes and waits, and `rich record` turns the same script into docs media. |
| **Lifetimes in text types** (`Line<'a>`, `Span<'a>`) leak into app structs. | Borrowing text avoids copies in a redraw-everything model. | Retained nodes own their content, so authors write owned strings and markup. |

ratatui's strengths are kept: crossterm and other backends, a fast cell diff,
a stable cell model, and its widget ecosystem, which runs inside intuiTUIve
through the adapter.

**Easier to learn** is measured too: the dashboard is 66 lines against
79 (state included on both sides), and the first tutorial app should need no knowledge of buffers, frames,
layout passes or event loops.

## Plan

1. **This spike.**
2. **`rs-rich-ratatui`.** The interop crate, made ready for production from
   `interop.rs`:
   - `RichWidget`;
   - the style and cell conversions;
   - docs for "use rich in your ratatui app";
   - the intuiTUIve leaf adapter behind a feature.
3. **intuiTUIve runtime and screen**, the new framework crate:
   - the per-app reactive runtime, with signals, memos and disposal;
   - the retained tree, with keyed identity and damage tracking;
   - our cell screen with links, and the damaged diff with merged rectangles
     and wide characters at the edges;
   - the 0.0.14 containers, focus and keymaps on the tree;
   - a streaming leaf for logs;
   - the dashboard benchmark in CI against ratatui.
4. **Layout and app shell** (done, with inline mode added):
   - layout: content sizes, min/max constraints, gaps, padding and grid;
   - screens, modals and navigation;
   - async tasks writing signals;
   - app-level theming;
   - inline (non-full-screen) apps.
5. **Developer experience:**
   - a widget inspector (the tree, dirty nodes and damage, live);
   - hot reload of styles;
   - a project template and a tutorial;
   - Python.
