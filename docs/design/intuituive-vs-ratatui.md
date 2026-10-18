# intuiTUIve against ratatui: the gaps, and the demos to port

**Status:** research note for 0.0.19 (workstream 21), for the implementation
batches to work from. It compares:

- **ratatui 0.30.2**: the `main` branch at `0297560` (2026-10-17), that is
  ratatui 0.30.2, ratatui-core 0.1.2 and ratatui-widgets 0.3.2, with their
  32 example apps, the `concepts` examples, the 17 widget examples and the
  VHS tapes. Features new since 0.30.0 are marked "(0.30.1)" or "(0.30.2)".
- **intuiTUIve** on the `additions` branch: rs-rich-intuituive 0.0.4 with
  this release's flex alignment and wrapping, text area, `Surface`,
  responsive layout, `:error`, snapshot tests, wide tables, reorderable lists
  and date and time pickers, and the inspector upgrade (selection, timings,
  an event trace) that lands alongside. All of these count as present.

It lists every gap that matters to an app author, sorts them by value,
splits them into three batches that can be built in parallel, and plans the
ratatui examples to rebuild as intuiTUIve demos, in three more batches.

Earlier notes: [the design spike](intuituive.md) (the architecture, and the
benchmark against ratatui) and [the widgets note](intuituive-widgets.md) (the
widget trait and the first widget gaps, all built). How to move an app over is
in [Porting a ratatui app](../guide/intuituive/porting.md).

## Summary

- **Ahead of ratatui:** everything above the buffer, which ratatui leaves to
  each app:
    - state in signals, and redrawing only what changed;
    - focus, key bubbling, and mouse routing with hover, drag and drop;
    - screens, modals, pop-ups, menus, toasts, the command palette and help;
    - text editing, inputs, forms and pickers;
    - stylesheets, themes and responsive layout;
    - accessibility, testing with scripts and a virtual clock, background
      tasks, serving to a browser;
    - the cost of a frame, which CI gates against ratatui.
- **Level with ratatui:**
    - layout along an axis: every constraint kind but `Ratio`, every flex
      mode but `Legacy`;
    - lists, tables, tabs, charts, and the canvas's shapes;
    - inline apps, panics, the mouse, the kitty keyboard protocol, paste;
    - backends, except termina.
- **Behind ratatui:** the knobs on each widget, and a few framework seams.
  The ten that matter most:
    1. **Block options.** Titles cannot be aligned, put at the bottom,
       repeated or styled; border kinds and sides cannot be set in code;
       there are no shadows.
    2. **No style on a node.** `Block::style` and `.bg()`, the most common
       ratatui calls, have no equivalent in code.
    3. **No text alignment** (`Line::centered`) and no "don't wrap" on text
       nodes.
    4. **List and table options:** highlight symbols and spacing, direction,
       scroll padding, multi-line rows, footers, column alignment and
       spacing, row styles, a column highlight, and an empty selection.
    5. **Scrollbars only inside `scroll()`:** none on lists, tables, trees,
       text areas or logs, none on the left or top, none with other symbols,
       none on its own.
    6. **Chart look:** ratatui's area-filling `Gauge` and its `LineGauge`;
       sparklines taller than a row, with a direction and absent values;
       grouped bar charts with text values and value styles. Charts also do
       not fill the node they are in.
    7. **The canvas:** no world map; no quadrant, sextant, octant or bar
       markers; no background colour; no custom shapes.
    8. **No single-line input bound to a signal.**
    9. **No drawing over children, and no reading cells.** ratatui does both
       by post-processing the buffer (demo2's "destroy").
    10. **Inline apps cannot print above their region** (ratatui's
        `insert_before`).
- **Gap batches**, which share few files and run in parallel:
    - **A**, blocks, text and layout: the framework's layout and drawing;
    - **B**, widgets, inputs and the app shell;
    - **C**, charts, the canvas, colour and the calendar, mostly in
      rs-rich-ext.
- **Demos:** twelve examples in three batches, which start once the gap
  batches have landed:
    - **D1**, the two showcases: `demo` and `demo2`;
    - **D2**, three galleries: `widgets`, `charts` and `layout`;
    - **D3**, seven apps: `colors`, `canvas`, `form`, `calendar`,
      `downloads`, `pulls` and `panic`.

## How to read the matrix

| Mark | Meaning |
|---|---|
| ✓ | Equal: the same capability, by the route named |
| ◐ | Partial: what is missing is said |
| ✗ | Missing |
| ≠ | Different by design: why is said |

The last column is the size of the fix and the gap's id in
[the gap list](#2-prioritized-gaps):

- **S** is a few dozen lines and their tests.
- **M** is a widget's worth: a few hundred lines with tests.
- **L** is new data or a new subsystem, or crosses crates.
- **—** means there is nothing to do.

"Beyond" marks what intuiTUIve has and ratatui leaves to apps. Source paths
are in `crates/rich-intuituive/src/` unless they name a crate. Where each
API the matrix names lives:

| API | File |
|---|---|
| `column`, `row`, `grid`, `each`, `switch`, `list`, `scroll` and its kin, `text`, `label`, `leaf`, `renderable`, `component`; the `Node` builders (`.fixed`, `.gap`, `.panel`, ..) | `node.rs`, with the widgets behind them in `builtin.rs` |
| `Size`, `Align`, `Justify`, `AlignContent`, `StackLayout`, the solver | `layout.rs` |
| `Widget`, `Canvas`, `DrawCx`, `MeasureCx`, `EventCx`, `WidgetEvent` | `widget.rs` |
| `table`, `table_with`, `TableOptions`, `Column`, `virtual_table`, `virtual_list`, `tabs` | `widgets.rs`; scrolling across in `table_scroll.rs` |
| `calendar` and its kin, `CalendarOptions`; the pickers | `calendar.rs`; `datetime.rs` |
| trees, split panes, menus, `text_area`, reordering | `tree.rs`, `split.rs`, `menu.rs`, `text_area.rs`, `reorder.rs` |
| `App`, `Ctx`, `Theme`, `Driver`, `every`, `FrameStats`, modals and pop-ups | `app.rs` |
| stylesheets | `sheet.rs` |
| `Log`; tasks and resources; signals and memos | `log.rs`; `task.rs`; `reactive.rs` |
| `assert_screen!`, `testing::plain` | `testing.rs` |
| `Screen`, `Rect`, `Painter` | `crates/rich-ext/src/surface/screen.rs` (`rich_ext::surface`) |
| `Chart`, `Canvas`, `Gauge`, `Sparkline`, `BarChart` | `crates/rich-ext/src/chart/`: `xy.rs`, `plot_canvas.rs`, `gauge.rs`, `sparkline.rs`, `bar.rs` |
| sessions, backends, `Headless`, events | `crates/rich-interact/src/`: `session.rs`, `headless.rs`, `event.rs` |
| `RichWidget`, `RatatuiComponent`, `lines_to_buffer` | `crates/rich-ratatui/src/` |

## 1. The matrix

### 1.1 Layout

| ratatui | intuiTUIve | Status | Fix |
|---|---|---|---|
| `Constraint::Length(n)` | `.fixed(n)`, `Size::Fixed` (`layout.rs`) | ✓ | — |
| `Constraint::Percentage(p)` | `.percent(p)`, `Size::Percent` | ✓ (of the room after gaps; ratatui's is of the whole area) | — |
| `Constraint::Ratio(a, b)` | none; `.percent(..)` rounds differently | ✗ | S · A4 |
| `Constraint::Min(n)` | `.min_size(n)` on a flexible child | ✓ (CSS semantics: a clamped flex child) | — |
| `Constraint::Max(n)` | `.max_size(n)` | ✓ | — |
| `Constraint::Fill(w)` | `.flex(w)`; `flex(1)` is the default | ✓ | — |
| sizing to content (measured by hand) | `.auto()`, `Size::Auto`: measured again when the content changes | ✓ beyond | — |
| `Flex::Start`, `Center`, `End` | `.justify(Justify::Start/Center/End)` | ✓ | — |
| `Flex::SpaceBetween`, `SpaceAround`, `SpaceEvenly` | `.justify(Justify::SpaceBetween/..)` (0.0.19) | ✓ | — |
| `Flex::Legacy` (the last item takes the excess) | none | ≠ a flexible child takes the excess: make the last one `.flex(1)` | — |
| cross-axis alignment (none: layouts are one-dimensional) | `.align_items(..)`, `.align_self(..)` | ✓ beyond | — |
| wrapping (none) | `.wrap()`, `.align_content(..)` | ✓ beyond | — |
| `Spacing::Space(n)`, `.spacing(n)` | `.gap(n)` | ✓ | — |
| `Spacing::Overlap(n)` (negative spacing) | none: `gap` is a `u16` | ✗ for overlapping siblings; collapsed borders and layers cover its uses | M · A6, A8 |
| `Block::merge_borders` with `Spacing::Overlap(1)`: collapsed borders (0.30) | none | ✗ | M · A6 |
| `Layout::margin`, `horizontal_margin`, `vertical_margin` | `.padding(v, h)`; a sheet's `padding` (one to four values) | ✓ | — |
| nested layouts | nested `column`, `row` and `grid` | ✓ | — |
| grids (by nesting layouts) | `grid(columns, children)`, `.span(..)`, `.rows(..)` | ✓ beyond | — |
| `Layout::split`, `areas::<N>`, `try_areas`, `Rect::layout` | none: the tree is laid out for you | ≠ declarative | — |
| `Layout::spacers`, `split_with_spacers` | `MeasureCx::stack_flow` for a widget's own children; nothing standalone | ✗ | S · A5 |
| the `layout-cache` feature | layout is retained, and runs again only when a sizing signal changes | ≠ | — |
| `Rect`: `inner(Margin)`, `outer`, `offset`, `union`, `intersection`, `intersects`, `contains`, `clamp`, `rows()`, `columns()`, `positions()`, `centered(..)` | `rich_ext::surface::Rect`: `right`, `bottom`, `is_empty`, `contains`, `intersection`, `inner(n)` | ◐ no margins per side, `centered`, `offset`, `union` or iterators | S · A5 |
| `Position`, `Size`, `Offset` | tuples and `Rect` | ≠ | — |
| `Direction::Vertical`, `Horizontal` | `column`, `row` | ✓ | — |
| docking (by hand) | a sheet's `dock: top/bottom/left/right` | ✓ beyond | — |
| responsive layout (by hand) | `.hide_below(..)`, `@media`, `viewport()` (0.0.19) | ✓ beyond | — |

### 1.2 Text and style

| ratatui | intuiTUIve | Status | Fix |
|---|---|---|---|
| `Span` (a styled run) | console markup: `[b red]run[/]` in `text!`, `text(..)`, `label(..)` | ✓ ≠ markup, not types, so no lifetimes in app structs | — |
| `Line`, `Text` | markup with `\n`; any rich `Text` through `renderable(..)` | ✓ | — |
| `Paragraph::alignment`, `Text::centered()` (the whole text) | none on `text`/`label`; a rich `Text` with `.justify(..)` in `renderable(..)` | ◐ | S · A3 |
| `Line::centered()` on some lines of a paragraph | none in markup; a column of aligned nodes | ◐ | Later (L10) |
| `Style` with `fg`, `bg` and modifiers | rich `Style`, `Style::parse("bold red on blue")`, markup, theme names | ✓ | — |
| `Modifier::BOLD`, `DIM`, `ITALIC`, `UNDERLINED`, `REVERSED` | `bold`, `dim`, `italic`, `underline`, `reverse` | ✓ | — |
| `Modifier::SLOW_BLINK`, `RAPID_BLINK`, `HIDDEN`, `CROSSED_OUT` | `blink`, `blink2`, `conceal`, `strike` | ✓ | — |
| (none) | `underline2`, `overline`, `frame`, `encircle` | ✓ beyond | — |
| underline colour (the `underline-color` feature) | none | ≠ the core mirrors Python rich, which has none; adding it is a divergence behind a flag (AGENTS.md), for little gain | — |
| `Stylize` shorthands: `"x".red().bold()` | markup and theme names | ≠ styles stay in markup, themes and stylesheets, where a theme file can change them | — |
| `Color::Reset`, the 16 named, `Indexed(n)`, `Rgb(..)` | `default`, the names, `color(n)`, `#rrggbb`, `rgb(..)` | ✓ beyond: downgraded to what the terminal shows | — |
| `style::palette::tailwind`, `material` | `rich_ext::palette::{tailwind, material}` (0.0.19) | ✓ | — |
| `Color::from_hsl`, `from_hsluv` (the `palette` feature) | `rich_ext::palette::{hsl, hsv, okhsv, oklab, blend, gradient}` (0.0.19) | ✓ ≠ Okhsv in place of HSLuv | — |
| ANSI text (the third-party `ansi-to-tui`) | `rich::Text::from_ansi` in `renderable(..)` | ✓ beyond | — |
| hyperlinks (OSC 8 by hand: the `hyperlink` example) | `[link=https://..]text[/link]`; cells keep the link | ✓ beyond | — |
| `Masked` | `Input::masked` for inputs; nothing for display text | ◐ rarely needed | — |
| wide characters, grapheme clusters | rich's cell widths; continuation cells on the screen | ✓ | — |
| themes (none) | `Theme`, named styles, theme files reloaded live, stylesheets | ✓ beyond | — |

### 1.3 Blocks

| ratatui | intuiTUIve | Status | Fix |
|---|---|---|---|
| `Block::bordered()` | `.panel(title)` (`Panel`, `builtin.rs`) | ✓ rounded, lit while the focus is inside | — |
| `Borders::TOP` and the other sides | none: all four sides or none | ✗ | M · A1 |
| `BorderType::Plain`, `Rounded`, `Double`, `Thick` | a sheet's `border: square/round/double/heavy` (and `ascii`) | ◐ in a stylesheet only, not in code | A1 |
| `BorderType::LightDoubleDashed` and the other dashed kinds, `QuadrantInside`, `QuadrantOutside`, a custom `border::Set` | none | ✗ | A1 |
| `border_style` | the theme's `border` and `border.focused`; a sheet's `border: round red` | ◐ not per panel in code | A1 |
| `title(..)` | `.panel("Title")`; a sheet's `border-title` | ✓ one title | — |
| styled titles (a `Line` of spans) | plain text in the theme's `title` style; markup shows as typed | ✗ | A1 |
| `title_alignment`, centred and right-aligned titles | top left only | ✗ | A1 |
| `title_bottom`, `TitlePosition::Bottom` | none | ✗ | A1 |
| several titles on one edge | one | ✗ | A1 |
| a title without a border: `Block::new().title(..)` | `border: none` drops the title too | ✗ | A1 |
| `title_style` | the theme's `title`, for the whole app | ◐ | A1 |
| `Padding::*` inside the border | `.padding(..)` before `.panel(..)`; a sheet's `padding` on the panel | ✓ | — |
| `Block::style` (a background) | a stylesheet's `background` only | ◐ | S · A2 |
| `Block::shadow(Shadow::..)` (0.30.1) | none, for panels, modals or pop-ups | ✗ | S · A1, B6 |
| `merge_borders(MergeStrategy::Exact)` | none | ✗ | A6 |
| focus and error looks (by hand) | `border.focused`; `border.error` with `.error_when(..)` | ✓ beyond | — |

### 1.4 Widgets

#### Paragraph

| ratatui | intuiTUIve | Status | Fix |
|---|---|---|---|
| `Paragraph::new(text)` | `text!`, `text(..)`, `label(..)` | ✓ | — |
| `.block(..)` | `.panel(..)` | ✓ | — |
| `.style(..)` | markup inside; nothing on the node | ◐ | A2 |
| `.wrap(Wrap { trim })` | always wraps, as rich does | ◐ no "don't wrap", no overflow choice | A3 |
| `.scroll((y, x))` | `scroll_with(child, offset)`, `scroll_both_with(..)`: keys, the wheel, a bar | ✓ beyond | — |
| `.alignment(..)`, `.centered()` | none | ✗ | A3 |
| `line_count`, `line_width` (unstable) | `MeasureCx::measure`, for widgets | ◐ | — |

#### List

| ratatui | intuiTUIve | Status | Fix |
|---|---|---|---|
| `List` with `ListState` | `list(items, selected)` (`node.rs`): keys, scrolling, the theme's `selected` style | ✓ beyond: the keys are built in | — |
| `ListItem` of several lines | the first line only | ✗ | M · B1 |
| `ListItem::style` (a row's background) | markup in the item, as wide as its text | ◐ | B1 |
| `highlight_style` | the theme's `selected`, for the whole app | ◐ | B1, A9 |
| `highlight_symbol`, `repeat_highlight_symbol` | none | ✗ | B1 |
| `HighlightSpacing::Always`, `WhenSelected`, `Never` | none | ✗ | B1 |
| `ListDirection::BottomToTop` | none | ✗ | B1 |
| `scroll_padding(n)` | none: it scrolls only as far as it must | ✗ | B1 |
| `ListState::select(None)` | a `Signal<usize>` always selects a row | ✗ | B1 |
| `ListState::offset` | kept inside | ◐ | B1 |
| a row of any widgets | `each(keys, build)` with `.focus_style(..)` | ✓ beyond | — |
| a million rows | `virtual_list(len, row, selected)` | ✓ beyond | — |
| reordering (none) | `.reorderable(..)` (0.0.19) | ✓ beyond | — |

#### Table

| ratatui | intuiTUIve | Status | Fix |
|---|---|---|---|
| `Table::new(rows, widths)` with `TableState` | `table(columns, rows, selected)`, `table_with(.., TableOptions)` (`widgets.rs`) | ✓ | — |
| `header(Row)` | a sticky header of `Column` titles | ✓ | — |
| the header row's style | the theme's `table.header` | ◐ | B2, A9 |
| `Row::top_margin`, `bottom_margin` | none | ✗ | B2 |
| `footer(Row)` | none | ✗ | M · B2 |
| widths: `Length`, `Percentage`, `Fill` | `Size::Fixed`, `Percent`, `Flex`; and `Size::Auto`, which fits the title and the rows in view | ✓ beyond | — |
| widths: `Min`, `Max`, `Ratio` | none per column | ✗ | B2, A4 |
| `column_spacing(n)` | always 1 | ✗ | B2 |
| `Table::flex` (where spare room goes) | none | ✗ | Later (L8) |
| `Row::height(n)`, cells of several lines | one line a row, cut with `…` | ✗ | B2 |
| `Row::style` (zebra stripes, a row's colour) | markup in each cell, which leaves the gaps unstyled | ◐ | B2 |
| `Cell::style` | markup in the cell | ✓ | — |
| a cell's alignment (its `Text`'s) | none | ✗ | B2 |
| `Cell::column_span` (0.30.1) | none (cells are strings) | ✗ | Later (L8) |
| `row_highlight_style` | the theme's `selected` and `table.selected` | ◐ | A9 |
| `cell_highlight_style` | `TableOptions::cells(signal)` and the theme's `selected.cell` | ✓ | — |
| `column_highlight_style` | none | ✗ | B2 |
| `highlight_symbol`, `highlight_spacing` | none | ✗ | B2 |
| `TableState::select(None)` | always a row | ✗ | B2 |
| `scroll_padding(n)` | none | ✗ | B2 |
| sorting, resizing, scrolling across, frozen columns (none) | `TableOptions` (0.0.18, 0.0.19) | ✓ beyond | — |
| a million rows | `virtual_table(..)` | ✓ beyond | — |

#### Tabs

| ratatui | intuiTUIve | Status | Fix |
|---|---|---|---|
| `Tabs::new(titles).select(i)` | `tabs(titles, selected)`: arrows, 1 to 9, clicks | ✓ beyond | — |
| `highlight_style`, `style` | the theme's `tabs.selected`, `tabs.selected.blur` | ◐ | A9 |
| `divider(..)` | always `│` (the theme's `tabs.divider`) | ✗ | S · B4 |
| `padding(left, right)` | always one space | ✗ | B4 |
| `select(None)` | always one | ✗ | Later |

#### Gauge and LineGauge

| ratatui | intuiTUIve | Status | Fix |
|---|---|---|---|
| `Gauge`: fills its area; the label centred, its colours swapped over the filled part; `ratio`, `percent`, `use_unicode`, `gauge_style` | `rich_ext::chart::FillGauge` (`chart/progress.rs`): `new(ratio)`, `percent`, `label`, `label_style`, `style`, `unicode`, `height` (0.0.19); the one-line `Gauge` keeps its bands and target | ✓ | — |
| `LineGauge`: the label, then `━━━───` in filled and unfilled styles and symbols | `LineGauge`: `label`, `filled_style`, `unfilled_style`, `line(LineSet::Thin/Thick/Double)`, `symbols(filled, unfilled)` (0.0.19) | ✓ | — |

#### Sparkline

| ratatui | intuiTUIve | Status | Fix |
|---|---|---|---|
| `data`, `max`, `style` | `rich_ext::chart::Sparkline`: values, `range`, `min`, `max`, `style`, and minimum, maximum and threshold marks | ✓ beyond | — |
| as tall as its area | `.rows(n)`, or the height it is given (0.0.19) | ✓ | — |
| `RenderDirection::RightToLeft` | `.direction(Direction::RightToLeft)` (0.0.19) | ✓ | — |
| absent values (`None`), `absent_value_symbol`, `absent_value_style` | `Sparkline::optional(..)`, `.absent(symbol, style)` (0.0.19) | ✓ | — |
| a style per bar (`SparklineBar::style`) | `.styles(vec![..])` (0.0.19) | ✓ | — |
| `bar_set` (`THREE_LEVELS`, `NINE_LEVELS`), `Marker::Braille` | `.levels(Levels::Three/Nine)`; `Charset::Blocks`, `Braille`, `Ascii` (0.0.19) | ✓ | — |

#### BarChart

| ratatui | intuiTUIve | Status | Fix |
|---|---|---|---|
| `Bar` with a label and a value; `bar_style`, `max`, `direction` | `BarChart`, `Bar::style`, `max`, `Orientation::Horizontal`, `Vertical` | ✓ | — |
| `BarGroup` with a label; `group_gap` | `BarGroup`, `BarChart::group(..)`, `.group_gap(n)` (0.0.19) | ✓ | — |
| `bar_width` (thickness), `bar_gap` | `.thickness(n)` (`bar_width` stays a bar's length or a column's height), `.bar_gap(n)` (0.0.19) | ✓ | — |
| `Bar::text_value` | `Bar::text(..)` (0.0.19) | ✓ | — |
| `value_style`, `label_style` (the chart's and a bar's) | `.value_style(..)`, `.label_style(..)` on the chart and on a `Bar`; `.values_inside(true)` for values on the bars (0.0.19) | ✓ | — |
| vertical bars as tall as the area | the height it is given, or `bar_width` rows (0.0.19) | ✓ | — |

#### Chart

| ratatui | intuiTUIve | Status | Fix |
|---|---|---|---|
| datasets with a name, a style and a marker | `rich_ext::chart::{Chart, Dataset}`, laid out as ratatui's | ✓ | — |
| `GraphType::Scatter`, `Line`, `Bar` | the same | ✓ | — |
| `GraphType::Area` with `fill_to_y` (0.30.1) | `GraphType::Area` with `Dataset::fill_to(y)` (0.0.19) | ✓ | — |
| `Axis` bounds, title, labels, style | `Axis::bounds`, `title`, `labels`, `style`; `Labels::Auto`, `AxisScale::Log` | ✓ beyond | — |
| labels as styled spans | `Labels::Markup(..)`: console markup (0.0.19) | ✓ | — |
| `labels_alignment` | `Axis::labels_align(HorizontalAlign)` (0.0.19) | ✓ | — |
| `legend_position`, `hidden_legend_constraints` | `LegendPosition` (eight places and `None`), `hidden_legend_fraction` | ✓ | — |
| as tall as its area | the height it is given (`renderable(..)` passes a node's), or `height(n)` (0.0.19) | ✓ | — |

#### Canvas

| ratatui | intuiTUIve | Status | Fix |
|---|---|---|---|
| bounds, `paint(..)` with a closure, `ctx.layer()` | `rich_ext::chart::Canvas::x_bounds`, `y_bounds`, `paint`; `CanvasPainter::layer` | ✓ | — |
| `Points`, `Line`, `Rectangle`, `Circle`, `ctx.print` | `points`, `line`, `rect`, `circle`, `print`, and `polyline` | ✓ beyond | — |
| `Map` with `MapResolution::Low`, `High` | `CanvasPainter::map(MapResolution::Low/High, style)`: our own data from Natural Earth's public-domain coastlines, drawn as lines (0.0.19) | ✓ | — |
| `Marker::Dot`, `Block`, `Braille`, `HalfBlock`, `Custom(c)` | `Marker::Dot`, `Block`, `Braille`, `HalfBlock`, `Ascii(c)` | ✓ | — |
| `Marker::Bar`, `Quadrant`, `Sextant`, `Octant` (0.30) | `Marker::Bar`, `Quadrant`, `Sextant`, `Octant` (0.0.19) | ✓ | — |
| `background_color` | `Canvas::background(style)` (0.0.19) | ✓ | — |
| custom shapes (`Shape`, `Painter::get_point`, `paint`) | `Shape` and `CanvasPainter::shape`, `point` and `area` (0.0.19) | ✓ ≠ shapes draw in world coordinates, not grid pixels | — |
| as tall as its area | the height it is given, or `height(n)` (0.0.19) | ✓ | — |

#### Calendar

| ratatui | intuiTUIve | Status | Fix |
|---|---|---|---|
| `calendar::Monthly` (display only) | `calendar(selected)` (`calendar.rs`): keys, clicks, the wheel, a range, today | ✓ beyond: interactive | — |
| `CalendarEventStore` (styled dates) | `CalendarOptions::event(date, style)`, `events(fn)` (0.0.19) | ✓ | — |
| `show_surrounding(style)` | `.surrounding(style)`; a click selects a surrounding day (0.0.19) | ✓ beyond | — |
| `show_weekdays_header(style)`, `show_month_header(style)` | `.weekday_style(..)`, `.month_style(..)`, `.first_weekday(Weekday::Sunday)` (0.0.19) | ◐ the headers cannot be hidden | — |
| `default_style` | `.day_style(..)` (0.0.19) | ✓ | — |
| a month that only shows (twelve make a year) | `month(shown, options)`: no focus, no keys (0.0.19) | ✓ | — |

#### Scrollbar

| ratatui | intuiTUIve | Status | Fix |
|---|---|---|---|
| `Scrollbar` with `ScrollbarState`, on its own | none | ✗ | M · B3 |
| `ScrollbarOrientation::VerticalRight`, `HorizontalBottom` | drawn by `scroll`, `scroll_x` and `scroll_both` when the content overflows | ✓ | — |
| `VerticalLeft`, `HorizontalTop` | none | ✗ | B3 |
| `thumb_symbol`, `track_symbol`, `begin_symbol`, `end_symbol`, `symbols(..)` | always `┃` on `│`, and `━` on `─` | ✗ | B3 |
| `thumb_style`, `track_style` | the theme's `scrollbar.thumb` and `scrollbar` | ✓ | — |
| on a list, a table, a tree, a text area, a log | none | ✗ | B3 |
| dragging the thumb, clicking the track (none in ratatui) | none | ✗ beyond ratatui once built | B3 |

#### The rest

| ratatui | intuiTUIve | Status | Fix |
|---|---|---|---|
| `Clear` | nodes own their cells; modals and pop-ups clear what they cover | ≠ | — |
| `Fill` (0.30.1) | a `leaf`, or `Canvas::fill` in a widget | ◐ | S · A7 |
| `RatatuiLogo`, `RatatuiMascot` | none | ≠ ratatui's branding: the demos draw intuiTUIve's own mark | — |

### 1.5 Terminal

| ratatui | intuiTUIve | Status | Fix |
|---|---|---|---|
| `ratatui::init()`, `restore()`, `run(..)`, `DefaultTerminal` | `App::run()` takes the terminal and gives it back | ≠ | — |
| `Viewport::Fullscreen` (the alternate screen) | the default | ✓ | — |
| `Viewport::Inline(n)` | `App::inline(n)` | ✓ | — |
| `Terminal::insert_before(n, ..)`: lines into the scrollback above an inline viewport | none | ✗ | M · B6 |
| `Viewport::Fixed(rect)` | `App::driver(w, h)` in a loop of your own (`examples/in_ratatui.rs`) | ◐ | Later (L5) |
| `Frame::set_cursor_position` | `Widget::caret`; a component's cursor; the accessible cursor | ✓ | — |
| the cursor's shape, the window title (crossterm commands an app writes) | none: the app owns the output | ✗ | S · Later (L3) |
| a panic hook that restores the terminal | the session's hook, installed by `App::run` | ✓ | — |
| a frame rate of the app's choosing (tick loops) | redraws on change; `every(interval)`; animations at about 60 frames a second; `Driver::timeout` | ≠ | — |
| `Frame::count()` | none; `FrameStats` has the nodes drawn and the bytes | ◐ | S · B6 |
| crossterm, termion and termwiz backends | `BackendKind::Crossterm`, `Termion`, `Termwiz` | ✓ | — |
| the termina backend (0.30.2) | none | ✗ | L · Later (L6) |
| `TestBackend` | `rich_interact::headless::Headless`, `App::driver` | ✓ | — |
| mouse capture, at the app's choice | always on full-screen, always off inline | ◐ no switch | S · B6 |
| the horizontal wheel (`ScrollLeft`, `ScrollRight`) | none; Shift with the wheel instead | ✗ | S · Later (L4) |
| the kitty keyboard protocol (by hand) | on where the terminal has it; key releases as `WidgetEvent::KeyUp` | ✓ beyond | — |
| bracketed paste | on; `WidgetEvent::Paste` | ✓ | — |
| focus gained and lost (`EnableFocusChange`) | none | ✗ | M · Later (L1) |
| resizes | laid out again; `WidgetEvent::Resize`; `viewport()` | ✓ | — |
| suspending on Ctrl+Z (by hand) | built in | ✓ beyond | — |
| synchronized output (by hand) | automatic where the terminal has it | ✓ beyond | — |

### 1.6 The widget API and drawing

| ratatui | intuiTUIve | Status | Fix |
|---|---|---|---|
| `Widget::render(self, area, buf)` | `Widget::draw(&mut self, cx, canvas)`, kept between frames (`widget.rs`) | ✓ ≠ retained | — |
| `StatefulWidget` | state in the widget, and in signals | ≠ | — |
| `WidgetRef`, boxed widgets in a container | `Box<dyn Widget>`, `Vec<Node>` | ✓ | — |
| custom widgets | `impl Widget` and `widget(..)`: measuring, children, layout, events, focus, the caret, hover, roles | ✓ beyond | — |
| writing cells: `buf[(x, y)]`, `set_string`, `set_line`, `set_span`, `set_style` | `Canvas::print`, `markup`, `lines`, `lines_at`, `set`, `fill`, `restyle`, `render`, `border` | ✓ | — |
| reading cells: `buf[(x, y)].symbol()`, `.style()` | none | ✗ | S · A7 |
| post-processing a frame (`frame.buffer_mut()` after rendering: demo2's "destroy") | none | ✗ | M · A7 |
| drawing over what was drawn (draw order is up to the app) | parents draw before their children, and siblings never overlap | ◐ | A7, A8 |

### 1.7 Testing

| ratatui | intuiTUIve | Status | Fix |
|---|---|---|---|
| `TestBackend::new(w, h)` with `Terminal::draw` | `App::driver(w, h)`, `App::render_with(keys, w, h)` | ✓ | — |
| `assert_buffer_lines`, `Buffer::with_lines` | `assert_screen!`, `testing::plain` | ✓ | — |
| comparing styled buffers | `assert_screen_styled!` | ✓ | — |
| `assert_cursor_position` | `Driver::cursor()` | ✓ | — |
| `assert_scrollback_lines` (after `insert_before`) | the headless recording's bytes | ◐ | B6 |
| `insta` snapshots | snapshot files, diffs, `UPDATE_SNAPSHOTS=1`; `testing::plain` feeds `insta` too | ✓ ≠ | — |
| scripted keys, clicks and time (none) | `press`, `type_text`, `event`, `update(now)`, `assert_focused` | ✓ beyond | — |

### 1.8 Async, logging and tracing

| ratatui | intuiTUIve | Status | Fix |
|---|---|---|---|
| async: the app's own `tokio::select!` over events and channels (`async-github`) | `spawn_future`, `resource`, `Proxy`, with any runtime | ✓ beyond | — |
| loading states, by hand | `Load::Loading`, `Ready`, `Failed`, `reload()` | ✓ beyond | — |
| `tracing` to a file (the `tracing` example) | what any program does; `rich_ext::adapters::EventLayer` (the `tracing` feature) | ✓ | — |
| a log pane (the third-party `tui-logger`) | `Log` (`log.rs`), drawn line by line | ◐ no scrolling back, no follow mode, no tracing sink | M · Later (L2) |

### 1.9 Performance

| ratatui | intuiTUIve | Status | Fix |
|---|---|---|---|
| a diff of the whole buffer | a diff of the damaged rectangles only | ✓ beyond | — |
| the cost of a frame | `tests/versus_ratatui.rs`: a tick, a selection move and a log line, gated in CI | ✓ beyond | — |
| a frame where every cell changes (`colors-rgb`) | not measured | ◐ | S · B7 |

### 1.10 Interop

| Direction | How | Status |
|---|---|---|
| rich renderables in a ratatui app | `rs-rich-ratatui`'s `RichWidget` | ✓ |
| ratatui widgets in intuiTUIve | `RatatuiComponent` in `component(..)` (`tests/ratatui_widgets.rs`) | ✓ |
| intuiTUIve in a ratatui app | `App::driver` and `lines_to_buffer` (`examples/in_ratatui.rs`) | ✓ |
| styles each way | `to_rich_style`, `to_ratatui_style`; the losses are in [the spike](intuituive.md#ratatui-interop) | ✓ |

### 1.11 What ratatui leaves to apps

These are not gaps, but they explain why a port is usually shorter:

- focus and Tab order, key bubbling, described bindings, the help and the
  command palette;
- mouse routing, hover, drag and drop, tooltips, text selection and copying;
- screens, modals, pop-ups anchored to nodes, menus, toasts, animations;
- timers, tasks and resources;
- text areas, inputs, forms, file, date and time pickers;
- trees, split panes, virtual lists and tables;
- stylesheets with states and `@media`, themes reloaded live;
- accessibility, with a linear mode for screen readers;
- serving an app to a browser, the inspector, and Python bindings.

## 2. Prioritized gaps

Ordered by value to an app author, weighed by how many ports need them. The
**batch** column says who builds it.

| # | Gap | Why it matters | Size | Batch |
|---|---|---|---|---|
| 1 | A1: panel titles, borders, sides and shadows | Every ratatui app titles its blocks; centred titles, top-only and thick borders are in most of the examples | M | A |
| 2 | A2: a style on any node | `Block::style` and `.bg()` are the most common ratatui calls; a coloured background has no code route today | S | A |
| 3 | A3: text alignment, wrapping and overflow | `Line::centered()` is in every widget example's title | S | A |
| 4 | B1: list options | Highlight symbols, multi-line items and the rest are in `demo`, `demo2`, `todo-list` and the `list` example | M | B |
| 5 | B2: table options | Footers, row styles, multi-line rows and aligned columns are in `demo`, `demo2`, `table` and the `table` example | M | B |
| 6 | B3: scrollbars everywhere | `demo2`, `table`, `scrollbar`: lists and tables show where they are | M | B |
| 7 | C0: charts fill their node | Every chart in a layout; today it takes a `leaf` and `.height(h)` | S | C |
| 8 | C1: an area gauge and a line gauge | `demo`, `demo2`, `gauge`, `inline`, the gauge examples | M | C |
| 9 | C2: sparklines taller than a row | `demo`, `demo2`, the `sparkline` example | M | C |
| 10 | C3: grouped bars with text values and styles | `demo`, `demo2`, `weather`, the bar chart examples | M | C |
| 11 | B5: a single-line input bound to a signal | `input-form`, `user-input`, and any form with live validation | M | B |
| 12 | A9: theme styles for a subtree | A list's or table's highlight, a tab strip's look, per instance, without a style option on every widget | M | A |
| 13 | C5: the canvas: a map, markers, a background, shapes | The world map is the centrepiece of `demo`'s second tab and of `demo2`'s traceroute | M | C |
| 14 | B4: tab strip options | `demo2` and the `tabs` example draw tabs without dividers or padding | S | B |
| 15 | A7: drawing over children, reading cells, `fill` | Effects (`demo2`'s destroy), overlays and badges | M | A |
| 16 | B6: the app shell: printing above an inline app, modal shadows, an exit value, a mouse switch, a frame count | `inline`, `popup`, `shadow`, `input-form`, `panic` | M | B |
| 17 | C7: calendar events, surrounding days, display-only months | `calendar-explorer`, `demo2`'s weather, the `calendar` example | M | C |
| 18 | A8: layers (overlapping children) | Content over a background (`demo2`'s swatch), a badge over a chart | S | A |
| 19 | C6: palettes and colour helpers | `table`, `gauge`, `todo-list` (tailwind); `demo2`, `colors-rgb` (Okhsv) | S | C |
| 20 | A4: ratio sizes | `demo`'s colour table, `traceroute`, `calendar-explorer` | S | A |
| 21 | A5: a layout function with spacers; `Rect` helpers | `constraint-explorer`, `flex`; widgets that place a pop-up | S | A |
| 22 | C4: area charts, styled and aligned axis labels | `chart`, `demo` | S | C |
| 23 | A6: collapsed borders | `collapsed-borders`; dashboards of panes | M | A |
| 24 | B7: the churn benchmark | Checks the claim to beat ratatui where every cell changes | S | B |

Not planned, by design (≠ in the matrix): underline colour, `Stylize`,
`Flex::Legacy`, ratatui's logo and mascot, `StatefulWidget`, ratatui's
`init`/`restore`.

### Batch A: blocks, text and layout

The framework's layout and drawing. Size: about 13 points, counting S as 1
and M as 2; A6 and A9 are the hardest.

**Files:**

- `src/builtin.rs`: `Panel` and `PanelLook`; `Stack` and `Grid`, for
  collapsing borders; a new `Layers` widget.
- `src/node.rs`:
    - the builders;
    - `text()` becomes a widget with options;
    - `border_box` and `edges` take titles and sides;
    - `Node::draw` gains the node's own style, theme styles for a subtree,
      `draw_over`, and overlap between siblings.
- `src/widget.rs`: reading cells on a `Canvas`; `Widget::draw_over`; the
  console a subtree draws with; `wrap_lines` for ratios.
- `src/layout.rs`: `Size::Ratio`; a public `Track`, `split` and
  `split_with_spacers`.
- `src/sheet.rs`: the new properties.
- New `src/border.rs`: `Border`, `Sides`, `Shadow`, `Edge` and the glyph
  merge table.
- New `src/effect.rs`: `effect` and `fill`.
- `src/lib.rs`: exports and the prelude.
- `crates/rich-ext/src/surface/screen.rs`: `Rect` helpers.
- Tests: `tests/panels.rs`, `tests/node_style.rs`, `tests/text_options.rs`,
  `tests/collapsed.rs`, `tests/layers.rs`, `tests/effects.rs`,
  `tests/theme_styles.rs`, with snapshots in `tests/snapshots/`.
- Docs: the guide's Layout, Themes and "Your own widgets" sections, a new
  Panels section (`docs/guide/intuituive/index.md`), and `CHANGELOG.md`.

#### A1. Panel titles, borders, sides and shadows

- **API.** Builders on a node wrapped by `.panel(..)`. On any other node they
  do nothing, as `.gap()` does on a non-stack.

    ```rust
    label("…")
        .panel("[b]Pull requests[/]")    // markup now, top left as before
        .title_align(Align::Center)      // where .panel's title sits
        .title_right("[dim]loading…")    // another title on the top edge
        .title_bottom("[dim]q quits")    // a title on the bottom edge
        .add_title(Edge::Bottom, Align::End, "1/20")  // the general form
        .border(Border::Thick)
        .borders(Sides::TOP)             // Sides::ALL is the default; TOP | LEFT
        .border_style("bright_black")    // while the focus is elsewhere
        .title_style("bold white")
        .shadow(Shadow::MediumShade)     // or Light/Dark shade, Block, Dim, Symbol('$')
    ```

- **`Border`:** `Rounded` (the default), `Plain`, `Thick`, `Double`, `Ascii`,
  the six dashed kinds (`LightDoubleDashed` to `HeavyQuadrupleDashed`),
  `QuadrantInside`, `QuadrantOutside`, `Custom(BorderSet)` and `None`.
  `BoxKind` in `sheet.rs` maps onto it.
- **`Sides`:** constants `ALL`, `NONE`, `TOP`, `RIGHT`, `BOTTOM` and `LEFT`,
  combined with `|`. The panel's inside shrinks only on the sides it
  draws, as ratatui's does.
- **Titles:**
    - Titles keep today's look: ` Title `, with one edge glyph between a
      left title and the corner.
    - Several titles with the same alignment sit side by side, one edge glyph
      apart.
    - When they do not fit, the right-hand ones give way first, then the
      centred ones, and the left ones are cut with `…`.
    - A title reserves its row even with `Border::None` or that side off,
      as ratatui's does.
- **Shadows:** a shadow takes a strip on the right and the bottom of the
  panel's own rectangle, and the box shrinks by the offset. A panel cannot
  draw outside its rectangle, so modals get their shadow from the layer
  instead (B6).
- **Accessibility:** the name is still the first top title, as plain text.
- **Stylesheet:**
    - `border: thick` (and `plain`) as aliases, and the dashed and quadrant
      kinds;
    - `border-title-align`;
    - `border-subtitle` and `border-subtitle-align`, for the bottom title;
    - `border-sides: top bottom`.
- **Where:** `builtin.rs` (`Panel`, `PanelLook`), `node.rs` (`border_box`,
  `edges`, the builders), `border.rs`, `sheet.rs`.
- **Tests,** snapshots unless said:
    - `panel_titles`: left, centre and right on the top, and two bottom
      titles;
    - `panel_sides`: the top only, and left with right;
    - `panel_kinds`: each `Border` side by side;
    - `panel_title_markup.styled`;
    - `panel_title_no_border`;
    - `panel_shadow`;
    - the focus still recolours the border;
    - a stylesheet's `border-title-align: center` draws what the code does.

#### A2. A style on any node

- **API:** `.style("white on #101830")`, which takes theme names too
  (`.style("accent")`), and the shorthands `.bg("#101830")` and
  `.fg("grey70")`.
- **Semantics:** those of a stylesheet's `color` and `background` for this
  node. The style goes under what the node and its children draw, and fills
  the rest of its rectangle, gaps included. Code wins over the sheet, and
  markup inside wins over both.
- **Where:** `node.rs`. Add an `own_style: Option<String>` field. In
  `Node::draw`, combine it over `SheetNode::style` in the existing underlay
  pass, which already works on damaged parts only. Resolve theme names when
  the theme changes.
- **Tests:**
    - `node_style_fill.styled`: a `column` with `.bg("blue")` is blue in
      every cell, gaps included;
    - markup inside keeps its colour;
    - `.style()` beats a stylesheet's `background`;
    - a theme switch recolours `.style("accent")`;
    - a tick that redraws one child does not resend the whole fill.

#### A3. Text alignment, wrapping and overflow

- **API:** on `text`, `label` and `text!`:
    - `.text_align(TextAlign::Center)`: `Left`, `Center`, `Right` or
      `Justify`;
    - `.no_wrap()`: lines are cut at the edge, not wrapped;
    - `.overflow(Overflow::Ellipsis)`: rich's `Overflow` (`Fold`, `Crop`,
      `Ellipsis`), re-exported.
- **Stylesheet:** `text-align`, `text-wrap: wrap/nowrap` and
  `text-overflow: ellipsis/clip/fold`.
- **Where:** `node.rs`. `text()` becomes a `TextNode` widget, so the builders
  reach it with `widget_mut`. It renders `Text::from_markup(..)` with
  `.justify(..)`, and sets `no_wrap` and `overflow` on the options. `label()`
  and the macro build on it. Measuring is unchanged: alignment does not
  change a text's natural width. Also `sheet.rs`.
- **Tests:**
    - `text_centered`: a three-line label in a panel;
    - `text_right`;
    - `text_no_wrap_ellipsis`;
    - `text_justify`;
    - a stylesheet's `text-align: center` matches the code;
    - an `.auto()` row still measures the natural width.

#### A4. Ratio sizes

- **API:** `Size::Ratio(u32, u32)`, `.ratio(1, 3)`, and `1/3` in a
  stylesheet's `size` and grid tracks.
- **Semantics:** `total × a / b` of the room after gaps, as a percentage is.
  A zero `b` gives zero. A remainder from rounding goes to the last ratio
  track, as flex shares do, so three thirds fill the room exactly.
- **Where:**
    - `layout.rs`: `solve`, `grow_for_span`;
    - `widget.rs`: the bases in `wrap_lines`;
    - `node.rs`: the builder;
    - `sheet.rs`: parsing.
- **Breaking:** `Size` gains a variant. Mark it `#[non_exhaustive]` at the
  same time, so the next one is not breaking.
- **Tests:** units in `layout.rs`:
    - three `Ratio(1, 3)` in 100 give 33, 33 and 34;
    - mixed with `Fixed` and `Flex`;
    - a grid of `1/3` tracks.

#### A5. A layout function with spacers; `Rect` helpers

- **API:**
    - `intuituive::layout::split(rect, axis, &[Track], StackLayout) ->
      Vec<Rect>`;
    - `split_with_spacers(..) -> (Vec<Rect>, Vec<Rect>)`, the spacers being
      the rooms between and around the items;
    - `Track`, made public: `Track::new(Size).min(n).max(n)` and
      `.content(n)` for `Size::Auto`.
- **`Rect` helpers** on `rich_ext::surface::Rect`: `centered(width, height)`,
  `inset(top, right, bottom, left)`, `offset(dx, dy)`, `union`, `intersects`,
  `rows()` and `columns()`.
- **Where:** `layout.rs` (pure functions over `solve` and `justify_offsets`),
  `lib.rs`, and `crates/rich-ext/src/surface/screen.rs`.
- **Tests:**
    - units: with `Justify::SpaceBetween` the spacers are the gaps, and with
      `Center` they are the two ends;
    - doc tests for the `Rect` helpers.

#### A6. Collapsed borders

- **API:** `.collapse_borders()` on a `row`, `column` or `grid` whose children
  are panels, and `border-collapse: collapse` in a stylesheet.
- **Behaviour:**
    - The container draws one shared frame, with junctions (`┬ ┴ ├ ┤ ┼`).
    - Each child's title sits on its top edge.
    - Each child's content goes between the lines.
    - The edges round the focused child are drawn in `border.focused`.
      `.focus_border(Border::Thick)` draws them thick, as the
      `collapsed-borders` example does for the selected pane.
- **Why not `Spacing::Overlap`:** with overlapping siblings, one that
  redraws wipes its neighbour's corner. The container knows every line, so it
  draws the junctions right whichever child redraws.
- **Where:** `builtin.rs` (a `collapse` flag on `Stack` and `Grid`; the child
  panels give up their own boxes and keep their titles), `border.rs` (the
  junction table), `node.rs` (the builder) and `sheet.rs`.
- **Tests:**
    - `collapsed_row`;
    - `collapsed_grid`, two by two;
    - `collapsed_focus`: the lit edges follow Tab;
    - a hidden child (`display: none`) reflows the lines.

#### A7. Drawing over children, reading cells, and `fill`

- **API:**
    - `Widget::draw_over(&mut self, cx: &mut DrawCx, canvas: &mut Canvas)`.
      The default does nothing. It runs after the widget's children have
      drawn, in a frame where the widget or any child drew. Its canvas is not
      cleared, and what it writes is damage.
    - `Canvas::cell(x, y) -> Option<CellView>`, with `.symbol()` and
      `.style()`: what is on the screen there now, which earlier siblings, the
      parent or the widget itself drew.
    - `Canvas::set_cell(x, y, symbol, style)`.
    - `effect(child, |canvas: &mut Canvas, cx: &DrawCx| ..)`: a node
      that runs the closure as its `draw_over`. The closure reads signals, so
      `cx.animate(phase, ..)` or `every(..)` drive it: demo2's destroy is
      `effect(app, ..)` reading a `phase` signal. No changes to the app loop
      are needed.
    - `fill(symbol, style)`: a node that fills its area with a symbol, like
      ratatui's `Fill`.
- **Where:** `widget.rs` (the trait method, `Canvas` reads), `node.rs`
  (`Node::draw` calls `draw_over` after the children), `effect.rs` and
  `lib.rs`.
- **Tests:**
    - a `draw_over` that reverses its children's cells;
    - a child's change redraws the overlay;
    - `cell` returns what a child wrote;
    - `effect_drip`: an effect moves cells down a row per frame, on the
      virtual clock, snapshot after three frames;
    - `fill_dots`.

#### A8. Layers

- **API:** `layers([base, top, ..])` stacks its children on one rectangle,
  later ones over earlier ones. A child is placed with
  `.inset(top, right, bottom, left)` (as CSS's `inset`), or with
  `.place(Align, Align)` at its content's size.
- **The framework fix:**
    - A `Widget::overlapping()` flag, default false and true for `Layers`.
    - Under it, a child redraws where an earlier sibling's damage in this
      frame touches it, as children already do where their parent drew over
      them.
    - `hit_path` prefers the later of two siblings at the same depth.
- **Where:** `builtin.rs` (`Layers`), `node.rs` (the children loop,
  `hit_path`, the builders) and `widget.rs` (the flag).
- **Tests:**
    - `layers_badge`: a ticking base under a top panel does not draw over it;
    - a click goes to the top layer;
    - the Tab order and the accessibility tree follow the order of the
      children.

#### A9. Theme styles for a subtree

- **API:** `.theme_styles([("selected", "bold yellow on #303030"),
  ("table.header", "bold underline")])` on any node. The named styles hold
  for the markup and the widgets inside it, over the app's theme, as CSS
  custom properties do. It is the per-instance answer to ratatui's
  `highlight_style`, `header` style and `Tabs::style`, with no style option on
  each widget.
- **Where:** `node.rs` and `widget.rs`. A node with theme styles draws and
  measures its subtree with a console that has those styles pushed: a clone
  made once per theme change and kept on the node. `FrameState` and
  `MeasureCx` hand that console down. Building `FrameState` touches one line
  of `app.rs`, which batch B owns: keep the change to that line.
- **Tests:**
    - `theme_styles_list.styled`: two lists, one with
      `.theme_styles([("selected", "underline")])`;
    - `[selected]x[/]` markup inside picks the override;
    - a theme switch keeps it;
    - a node without theme styles draws with no clone.

### Batch B: widgets, inputs and the app shell

Size: about 13 points; B2 and B6 are the largest.

**Files:**

- New `src/list.rs`: the list leaf moves out of `node.rs` and becomes a
  `List` widget with options. `node.rs` keeps `pub use crate::list::list`.
- `src/reorder.rs`: `ListModel` learns multi-line rows and direction.
- `src/widgets.rs` (`Table`, `Tabs`, re-exports) and `src/table_scroll.rs`
  (column spacing when scrolling across).
- New `src/scrollbar.rs`: `scrollbar` and `scrollbar_across` move out of
  `node.rs`.
- `src/builtin.rs`: `ScrollView` only.
- `src/tree.rs`, `src/text_area.rs` (scrollbars and the single-line mode),
  `src/log.rs` (scrollbars).
- New `src/input.rs`.
- `src/app.rs`: modal options, printing above, exit values, the mouse switch,
  the frame count.
- `src/lib.rs`.
- Tests: `tests/list_options.rs`, `tests/table_options.rs`,
  `tests/scrollbar.rs`, `tests/tabs_options.rs`, `tests/input.rs`,
  `tests/app_shell.rs`, `tests/versus_ratatui.rs`.
- Docs: the guide's "Tables, tabs and scrolling", Components, "Screens and
  modals" and "Inline apps" sections; the porting guide's table.

**Overlap with A:** `node.rs` (B only deletes code), `builtin.rs` (B only
touches `ScrollView`) and `lib.rs`. **Overlap with the inspector work:**
`app.rs`.

#### B1. List options

- **API:** builders on a `list` node, as `text_area`'s are:

    ```rust
    list(move || items.get(), selected)
        .symbol("[red]>>[/] ")             // the highlight symbol, markup
        .repeat_symbol()                   // on each line of a multi-line item
        .highlight_spacing(HighlightSpacing::Always)  // WhenSelected is the default; Never
        .direction(ListDirection::BottomToTop)
        .scroll_padding(1)                 // rows kept in view around the selection
        .multiline()                       // items keep their lines and wrap
        .row_style(|i| (i % 2 == 1).then(|| "on grey15".into()))  // whole rows
        .offset(first)                     // Signal<usize>: the first item in view
    ```

- **An empty selection:** `list_optional(items, selected:
  Signal<Option<usize>>)`. `None` shows no highlight, and ↓ selects the first
  item. Inside, the widget holds a `Selection` enum. `list` keeps its
  signature: taking `impl Into<Selection>` would break type inference for
  `signal(0)`.
- **Where:** `list.rs`, `reorder.rs` (rows of different heights, direction),
  `node.rs` (deletion only), `widgets.rs` (re-exports `HighlightSpacing` and
  `ListDirection`).
- **Tests:**
    - `list_symbol`, with each spacing;
    - `list_bottom_to_top`;
    - `list_multiline`, with the repeated symbol;
    - `list_scroll_padding`;
    - `list_zebra.styled`;
    - `list_none`: no highlight, ↓ selects the first item;
    - `.reorderable` still works with options;
    - accessibility positions are unchanged.

#### B2. Table options

- **API:** builders on `Column` and `TableOptions`, keeping its convention:

    ```rust
    Column::new("Qty", Size::Fixed(7)).align(TextAlign::Right).min(4).max(12)
    TableOptions::default()
        .footer(|| vec!["Total".into(), "".into(), "135 kcal".into()])
        .header_gap(1)                     // ratatui's header bottom_margin
        .column_spacing(2)
        .symbol("» ")                      // the highlight symbol
        .highlight_spacing(HighlightSpacing::Always)
        .row_style(|i| (i % 2 == 1).then(|| "on grey11".into()))
        .multiline()                       // rows as tall as their tallest cell
        .column_highlight()                // the cursor's column, in `selected.column`
        .scroll_padding(1)
    ```

- **An empty selection:** `table_optional(columns, rows,
  Signal<Option<usize>>, options)`, as for lists.
- **Virtual tables:** `virtual_table` takes `.row_height(n)` rather than
  `.multiline()`, since its rows are never all in hand.
- **Where:** `widgets.rs`: the `first` and `body` arithmetic moves from rows
  to lines; keys still move by row. Also `table_scroll.rs`.
- **Tests:**
    - `table_footer`;
    - `table_aligned`: numbers on the right;
    - `table_spacing`;
    - `table_symbol`;
    - `table_zebra.styled`;
    - `table_multiline`: ratatui's four-line rows;
    - `table_column_highlight.styled`;
    - `table_none`;
    - with multi-line rows, ↓ moves a whole row, and PgDn a screenful of
      rows.

#### B3. Scrollbars everywhere

- **API:**

    ```rust
    Scrollbar::new()                       // the right side; Show::Auto
        .side(Side::Left)                  // Right, Left, Bottom, Top
        .thumb("▐")
        .track(None)
        .ends(Some("↑"), Some("↓"))
        .show(Show::Always)                // Auto, Always, Never
    scroll(child).scrollbar(Scrollbar::new().thumb("▐"))
    list(..).scrollbar(Scrollbar::new())   // also tables, virtual lists and tables, trees, text areas, logs
    scrollbar(position, len, view, Scrollbar::new())  // a node of its own, for a view of your own
    ```

- **Symbol sets:** `ScrollbarSymbols::{THIN, VERTICAL, DOUBLE_VERTICAL,
  HORIZONTAL, DOUBLE_HORIZONTAL}`. `THIN` is today's look and stays the
  default.
- **Styles:** the theme's `scrollbar` and `scrollbar.thumb`, and a new
  `scrollbar.arrow`.
- **The mouse:** a press on the track pages, and dragging the thumb sets the
  position (with `capture_mouse`).
- **Where:** `scrollbar.rs`. Every widget draws the bar as its own chrome, in
  a column or row it keeps for it. Also `builtin.rs` (`ScrollView`),
  `widgets.rs`, `list.rs`, `tree.rs`, `text_area.rs` and `log.rs`.
- **Tests:**
    - `scrollbar_arrows`;
    - `scrollbar_left_no_track`;
    - `scrollbar_bottom_custom`;
    - `list_scrollbar`;
    - `table_scrollbar`;
    - dragging the thumb scrolls, and a click on the track pages;
    - `Show::Never` keeps no column;
    - text mode leaves the bar blank.

#### B4. Tab strip options

- **API:** builders on a `tabs` node: `.divider(" • ")` (or `""`),
  `.tab_padding("", "")` (`.padding` is taken) and `.titles_align(Align)`.
  Styles come from A9 (`.theme_styles([("tabs.selected", ..)])`).
- **Where:** `widgets.rs`: `Tabs`, its `measure`, and the title spans the
  mouse hits.
- **Tests:**
    - `tabs_dot_divider`;
    - `tabs_no_padding`: demo2's strip;
    - a click hits the right title with any divider and padding;
    - `measure` counts the divider.

#### B5. A single-line input bound to a signal

- **API:**

    ```rust
    input(name)                            // Signal<String>, both ways
        .placeholder("First name")
        .mask('•')
        .max_length(40)
        .on_submit(|value: &str, cx| ..)   // Enter; used, so it does not bubble
    number_input(age, 0..=150)             // Signal<i64>: ↑/↓, PgUp/PgDn, digits, the wheel
        .step(1)
    ```

- **How:** `input` is `text_area`'s editor in a single-line mode:
    - Enter submits rather than inserting a line;
    - pasted newlines become spaces;
    - the line scrolls sideways to the caret;
    - there is no gutter.

  Validation is `.error_when(..)`, as anywhere.
- **Where:** `text_area.rs` (the mode), `input.rs` (`input`, `number_input`),
  `lib.rs` and the prelude.
- **Tests:**
    - typing sets the signal, and setting the signal replaces the text;
    - Enter submits and inserts nothing;
    - a mask shows `•` while the signal holds the text;
    - `input_scrolled`: a long line follows the caret;
    - `number_input` clamps;
    - Tab goes through three inputs, as `input-form` does;
    - the role is a text box, with the text as its value.

#### B6. The app shell

- **Modal options:** `cx.modal_with(width, height, ModalOptions::new()
  .shadow("on grey11").backdrop("dim"), build)`, and `cx.popup_with(..)`.
    - The shadow is a strip offset one cell right and down, restyling the
      screen below, as ratatui's `Shadow::overlay().style(..)` does.
    - The backdrop restyles the whole screen below while the modal is open.
    - Styles are given as strings, so this does not wait on A1's `Shadow`.
- **Printing above an inline app:** `cx.print_above(markup)` and
  `Proxy::print_above(markup)`.
    - Inline, the lines go into the scrollback above the region, which moves
      down, as ratatui's `insert_before` does.
    - Full-screen, they print after the app ends.
    - A loop of your own reads them with `Driver::take_printed()`.
- **An exit value:**
    - `cx.quit_with(value)`;
    - `cx.fail(error)`, after which `App::run` returns the error;
    - `App::run_for::<T>() -> io::Result<Option<T>>`;
    - `Driver::take_exit::<T>()`.
- **A mouse switch:** `App::mouse(Option<bool>)`. `None`, the default, is
  today's rule; `Some(true)` captures the mouse inline, and `Some(false)`
  leaves it to the terminal full-screen.
- **A frame count:** `FrameStats::frames`.
- **Where:** `app.rs` (`Ctx`, `Nav::Modal` and `Nav::Popup`, the layer's
  drawing, `Driver`, `run_on`), with the inline printing beside the region
  code `Driver::set_origin` already has. Perhaps
  `crates/rich-interact/src/headless.rs`, for recording printed lines.
- **Tests:**
    - `modal_shadow`;
    - `modal_backdrop.styled`;
    - in an inline region, the printed lines come before the region's redraw,
      and `take_printed` returns them;
    - `quit_with` reaches `run_for`;
    - `fail` makes `run_on` return the error;
    - `mouse(Some(false))` reaches `SessionOptions`.

#### B7. The churn benchmark

- **What:** a `Scenario::Churn` in `tests/versus_ratatui.rs`: every cell
  changes each frame (an RGB swatch, as `colors-rgb`), drawn by a custom
  widget on one side and a ratatui widget on the other. Also a
  `Scenario::Chart`: a chart redrawn each tick.
- **The bar:** report the numbers first (`eprintln!`, as the table does), and
  gate them only once they are known. Put them in the README's table.

### Batch C: charts, the canvas, colour and the calendar

Mostly rs-rich-ext. Size: about 13 points; making C5's map data is the
longest part.

**Status: built** (0.0.19; the matrix rows above say how). Where the build
differs from the sketch below:

- **C1:** `FillGauge` also takes `percent`, `label_style`, `width` and
  `height`; `LineGauge` takes `symbols(filled, unfilled)` as well as a
  `LineSet`, for the `line-gauge` example's `▰▱` look.
- **C2:** a value's own style wins over the `min_max` and `threshold`
  marks; right to left, the sparkline sits against the right edge of its
  width, its summary before the bars.
- **C3:** `bar_gap` defaults to a cell vertical and no rows horizontal,
  `group_gap` to a cell or two rows; a horizontal group's label takes the
  first row between groups, as ratatui's does. Values on their bars are
  the bar's colour reversed unless a value style is set.
- **C5:** the map is drawn as lines, not points, so its simplified coasts
  stay whole at any size; the data is about 5 KB (1:110m) and 22 KB
  (1:50m). Custom shapes draw in world coordinates; `CanvasPainter::area`
  is public too.
- **C7:** `today`, `min` and `max` stay public fields; the new settings are
  builders. `Weekday` is intuiTUIve's, and a click on a surrounding day
  selects it.

**Files:**

- `crates/rich-ext/src/chart/xy.rs`, `chart/plot_canvas.rs`,
  `chart/canvas.rs` (the new markers' rasters), `chart/sparkline.rs`,
  `chart/bar.rs` and `chart/mod.rs`.
- New `chart/progress.rs`, for `FillGauge` and `LineGauge`.
- New `chart/world.rs`, with `world_110m.bin` and `world_50m.bin`, and
  `scripts/world_map.py` to make them.
- New `crates/rich-ext/src/palette.rs`, with `palette/tailwind.rs` and
  `palette/material.rs`; and `crates/rich-ext/src/lib.rs`.
- `crates/rich-intuituive/src/calendar.rs`.
- Tests: units and doc tests in rs-rich-ext;
  `crates/rich-intuituive/tests/calendar_options.rs` and `charts_fill.rs`.
- Docs: `docs/guide/ext/charts.md`, and the calendar paragraph of the
  intuiTUIve guide.

There is no overlap with A or B beyond `crates/rich-intuituive/src/lib.rs`.

#### C0. Charts fill their node

- **What:** `Chart`, `Canvas`, a vertical `BarChart`, a `Sparkline` with
  rows and `FillGauge` take `options.height` when they were given no height.
  `renderable(..)` already sets it, so `renderable(move || Chart::new(..))`
  fills its node. An explicit `.height(n)` still wins. rich's own `Layout`
  passes a height too, which is what one wants there as well.
- **Tests:** in rs-rich-ext, rendering with `update_height(7)` gives seven
  lines; `charts_fill` in intuiTUIve.

#### C1. An area gauge and a line gauge

- **API:**

    ```rust
    use rich_ext::chart::{FillGauge, LineGauge, LineSet};
    FillGauge::new(0.42).label("42.00%").style("magenta on black italic bold").unicode(true)
    LineGauge::new(0.42).label("Downloading: 42%")
        .filled_style("light_blue").unfilled_style("grey23").line(LineSet::Thick)
    ```

- **`FillGauge`:** ratatui's `Gauge`. It fills its area, with eighths at the
  edge when `unicode` is on. The label sits centred on the middle row, its
  colours swapped over the filled part. Without a label it shows the
  percentage. ASCII: `#`, and `=` for a half.
- **`LineGauge`:** the label, a space, then the line in its two styles,
  `━`/`─` or the thin and double sets.
- **Theme keys:** `chart.gauge.filled`, `chart.gauge.unfilled`.
- **Where:** `progress.rs`, `mod.rs` (exports, `STYLES`).
- **Tests:** exact strings, including widths 0 to 3 and the inverted label;
  styled exports for the swap; ASCII.

#### C2. Sparklines taller than a row

- **API:**

    ```rust
    Sparkline::new(values).rows(3)                 // or fill the height given (C0)
        .direction(Direction::RightToLeft)
    Sparkline::optional([Some(1.0), None, Some(3.0)]).absent("░", "red")
        .styles(vec![None, Some("red".into()), None])  // a style per bar
        .levels(Levels::Three)                     // ratatui's three-level set; nine is the default
    ```

- **Where:** `sparkline.rs`. `Sparkline` stays `Clone` and `PartialEq`, so
  per-bar styles are a `Vec`, not a closure.
- **Tests:** three-row bars, exactly; right to left; the absent symbol; a
  styled export with per-bar styles; Braille over several rows.

#### C3. Grouped bars, text values and styles

- **API:**

    ```rust
    BarChart::new()
        .group(BarGroup::new("Q1")
            .bar(Bar::new("Mon", 76.0).text("76°").style("red").value_style("grey70 on red bold")))
        .bar_gap(1).group_gap(3).thickness(3)   // ratatui's bar_width is our thickness
        .label_style("yellow").value_style("black on green italic")
        .values_inside(true)                    // values on the bar, as ratatui draws them
    ```

- **`bar_width`** keeps its meaning (a bar's length, or a column's height),
  which the docs say plainly next to `thickness`.
- **Where:** `bar.rs`.
- **Tests:**
    - grouped and vertical, with group labels;
    - horizontal, values inside the bars: demo2's seasons;
    - text values wider than a bar are centred and cut (ratatui's issue 513);
    - thickness 5;
    - the gaps.

#### C4. Area charts, styled and aligned axis labels

- **API:** `GraphType::Area` with `Dataset::fill_to(y)`;
  `Labels::Markup(Vec<String>)`, so `Labels::Text` keeps its plain meaning;
  `Axis::labels_align(Align)`.
- **Where:** `xy.rs`.
- **Tests:** an area fill; a styled export with bold end labels; y labels
  aligned right and centred.

#### C5. The canvas: a map, markers, a background, shapes

- **Markers:** `Marker::Bar` (`▄`), `Quadrant` (2×2), `Sextant` (2×3,
  Symbols for Legacy Computing) and `Octant` (2×4, Unicode 16). A font
  without octants shows boxes, which the docs say. Each falls back to `#` or
  `*` on an ASCII console, as the others do.
- **The map:** `CanvasPainter::map(MapResolution::Low/High, style)`, its
  points in longitude and latitude. The data is our own, made from Natural
  Earth's public-domain coastlines (1:110m and 1:50m) by
  `scripts/world_map.py`. It is kept as `include_bytes!` of `i16` pairs at a
  hundredth of a degree (about 20 KB for the high resolution), not copied
  from ratatui's `world.rs`. The script is not run in CI.
- **A background:** `Canvas::background(style)`.
- **Custom shapes:** `pub trait Shape { fn draw(&self, painter: &mut
  CanvasPainter); }` and `CanvasPainter::shape(&dyn Shape)`, with
  `CanvasPainter::point(x, y, style)` for single points.
- **Where:** `plot_canvas.rs`, `canvas.rs` (the rasters), `world.rs`,
  `mod.rs`.
- **Tests:**
    - each marker on a diagonal line, exactly;
    - the map at 80×20 lights the cells of a few known coasts, Australia's
      among them;
    - a styled export with the background;
    - a custom shape.

#### C6. Palettes and colour helpers

- **API:**
    - `rich_ext::palette::tailwind::{SLATE, .., ROSE}`: the 22 palettes,
      each with `c50` to `c950` as `&'static str` hex. They work in markup
      (`format!("[on {}]", SLATE.c900)`) and in `Style::parse`.
    - `rich_ext::palette::material::*`, with the accent shades.
    - Colour helpers in the same module, each returning a `rich::Color`:
      `hsl(h, s, l)`, `hsv(h, s, v)`, `okhsv(h, s, v)`, `oklab(l, a, b)`,
      `blend(a, b, t)`, `gradient(&stops, t)`.
- **Credit** in the module docs: Tailwind CSS's palette (MIT, Tailwind
  Labs); Material Design's colours (Apache-2.0, Google); Oklab and Okhsv by
  Björn Ottosson.
- **Tests:** `SLATE.c950` is `#020617`; `okhsv` of known points, to a
  tolerance; `blend` at its ends; `gradient` between stops.

#### C7. Calendar events, surrounding days, display-only months

- **API:**

    ```rust
    calendar_options(day, CalendarOptions::new()
        .event(Date::new(2026, 12, 25), "red underline")
        .events(|date| holidays(date))          // or a function
        .surrounding("dim")                     // the days of the months either side
        .first_weekday(Weekday::Sunday)
        .weekday_style("bold green")
        .month_style("bold")
        .day_style("bold on grey19"))           // ratatui's default_style
    month(move || shown.get(), options)         // only shows: no focus, no keys
    ```

- **Breaking:** `CalendarOptions` stops being `Copy`, because of the strings
  and the function. `datetime_picker` keeps working: it shares the drawing.
- **Where:** `calendar.rs`.
- **Tests:**
    - `calendar_events.styled`;
    - `calendar_surrounding`;
    - `calendar_sunday_first`;
    - `calendar_year`: twelve `month` nodes in a grid;
    - the keys and the range are unchanged.

### Later

These are worth building, but after the batches:

- **L1. Terminal focus events.** `Event::FocusGained` and `FocusLost` in
  rs-rich-interact (crossterm, termwiz; termion reads the raw sequence), and
  an app-level `focused()` signal. Size M.
- **L2. Logs.**
    - Scrolling back, with a follow mode: the tail stops while you read and
      resumes at the end.
    - A `tracing` sink into a `Log`: `intuituive::log::tracing_layer(log,
      proxy)`, behind a `tracing` feature that turns on rs-rich-ext's.
    - Size M.
- **L3. The cursor's shape and the window title.** `App::title(..)`,
  `cx.set_title(..)` and a text input's `.caret_shape(..)`. Size S.
- **L4. The horizontal wheel.** `MouseKind::ScrollLeft` and `ScrollRight`.
  Size S.
- **L5. A fixed viewport in `App::run`**, a rectangle of the terminal. Size M.
- **L6. The termina backend** in rs-rich-interact. Size L.
- **L7. Nodes in a panel's border**: the `tabs` example puts the tab strip on
  the content's top edge. Size M.
- **L8. Typed table cells,** for `column_span` and per-cell alignment; and
  `Table::flex`. Size M.
- **L9. Python bindings** for everything above. Size M.
- **L10. Alignment per line** inside one text node. Size S.
- **Tabs with nothing selected** (`select(None)`). Size S.

## 3. The demos

### 3.1 ratatui's examples, and what intuiTUIve has today

The 32 apps, and the `concepts` examples:

| ratatui | Shows | intuiTUIve today | Plan |
|---|---|---|---|
| `demo` | tabs; a gauge, a sparkline and a line gauge; lists and a log; a bar chart; a sine chart; a server table; a world map; colours | `dashboard`, a smaller ops board | port as `demo` (D1) |
| `demo2` | the README showcase: About, Recipe, Email, Traceroute, Weather; a theme; "destroy" | none | port as `demo2` (D1) |
| `calendar-explorer` | a year of months with events, cycling styles | `planner`'s calendar | port as `calendar` (D3) |
| `canvas` | a world map, pong at 60 fps, drawing with the mouse, boxes, every marker | none (`scope` uses `Chart`) | port as `canvas` (D3) |
| `chart` | an animated sine chart; bar, line and scatter charts | `scope` | `charts`, the Chart tab (D2) |
| `color-explorer` | the named colours on backgrounds; 256 indexed; greys | none | `colors` (D3) |
| `colors-rgb` | an RGB swatch animated at 60 fps, with an fps read-out | none | `colors`, the RGB tab (D3) |
| `constraint-explorer` | edit constraints and see every flex mode, with the spacers | `align` | port as `layout` (D2) |
| `constraints` | each constraint kind, one tab each | `align` | `layout`, the Sizes tab (D2) |
| `custom-widget` | buttons drawn by hand, with mouse states | `meters` | `widgets`, the Custom tab (D2) |
| `flex` | each flex mode, with spacing | `align` | `layout`, the Flex tab (D2) |
| `gauge` | four gauges: percentage and ratio, with and without unicode | `meters` | `charts`, the Gauge tab (D2) |
| `hello-world`, `minimal` | the smallest apps | the README's first app | covered |
| `hyperlink` | a link through OSC 8, by hand | a test (`tests/app.rs`) | `widgets`, the Text tab (D2) |
| `inline` | download workers, gauges, finished lines printed above | `inline`, one progress line | port as `downloads` (D3) |
| `input-form` | three fields with a focus, an age spinner, the cursor | `component(Form)`, without an example | port as `form` (D3) |
| `modifiers` | every modifier on colours | none | `colors`, the Modifiers tab (D3) |
| `mouse-drawing` | drawing by dragging | `board` (drag and drop) | `canvas`, the Draw panel (D3) |
| `panic` | the terminal given back on a panic and on an error | the behaviour, without an example | port as `panic` (D3) |
| `popup` | a centred pop-up over content | `screens` (a modal), `files` (a pop-up) | `widgets`, the Popup tab (D2) |
| `release-header` | ratatui's release banner | — | not ported: ratatui's branding |
| `scrollbar` | four scrollbar looks | `editor`, `board` | `widgets`, the Scrollbar tab (D2) |
| `table` | multi-line rows, stripes, row, column and cell highlights, a column span, palettes, a scrollbar | `tables` (sorting, resizing, a cell cursor) | `widgets`, the Table tab (D2) |
| `todo-list` | stripes, a symbol, deselecting, a detail pane | `todo`, the tutorial's | covered; refine `todo` once B1 lands |
| `tracing` | `tracing` to a file | none | later, with L2 |
| `user-input` | an input with editing and normal modes, a message list | `todo`'s `repeating` input | `form`, the Chat tab (D3) |
| `volatility-surface` | a 3D wireframe on a Braille canvas | none | `canvas`, an optional Surface tab (D3) |
| `weather` | 24 hourly bars with text values | none | `charts`, the Bars tab (D2) |
| `widget-ref-container` | boxed widgets in a container | `Vec<Node>`, `Box<dyn Widget>` | covered (≠) |
| `advanced-widget-impl` | `Widget` on a type, a reference, a mutable reference | the guide's "Your own widgets", `meters` | covered (≠) |
| `async-github` | a table of pull requests loaded in the background, the loading state in the title | `board` (a lazy tree), `files` (a preview worker) | port as `pulls` (D3) |
| `concepts/state` | eleven ways to hold state | signals: the porting guide's first step | covered (≠) |

The 17 widget examples:

| ratatui-widgets | Shows | Plan |
|---|---|---|
| `barchart` | vertical and horizontal bars | `charts`, Bars |
| `barchart-grouped` | revenue by quarter, in groups | `charts`, Groups |
| `block` | bordered, styled, custom borders | `widgets`, Block |
| `calendar` | a month with events | `widgets`, Calendar |
| `canvas` | the map and shapes | `charts`, Canvas |
| `chart` | a line chart and an area chart | `charts`, Chart |
| `collapsed-borders` | four panes sharing borders, the selected one thick | `widgets`, Borders |
| `gauge` | gauges | `charts`, Gauge |
| `line-gauge` | line gauges | `charts`, Gauge |
| `list` | a list, and a bottom-to-top list | `widgets`, List |
| `logo` | ratatui's logo | not ported: branding |
| `paragraph` | centred text, wrapped text, a masked secret | `widgets`, Text |
| `scrollbar` | vertical and horizontal bars | `widgets`, Scrollbar |
| `shadow` | four shadows behind pop-ups | `widgets`, Popup |
| `sparkline` | three sparklines | `charts`, Sparkline |
| `table` | a header, a footer, highlights | `widgets`, Table |
| `tabs` | a dot divider, padding | `widgets`, Tabs |

### 3.2 The demos to port

Every demo follows the conventions of the existing ports (`files`, `scope`,
`meters`):

- **Testable:** it exposes `pub fn <name>_app(..) -> App` for its tests,
  which include the example with `#[path = "../examples/<name>.rs"]`, as
  `tests/scope.rs` does.
- **Deterministic:** randomness comes from a seeded xorshift written in the
  example (the crate has no `rand`), time from timers (so a test moves the
  virtual clock), and `--frozen` stops the clock for screenshots, as
  `meters` does.
- **Refined:** each port shows what intuiTUIve adds, beyond reproducing the
  original:
    - tabs, lists and tables take the mouse;
    - bindings have descriptions, so `?` shows the help and Ctrl+P the
      palette (`.help_key("?")`, `.palette_key("ctrl+p")`);
    - nodes have labels, so the linear mode reads well;
    - only what moves redraws, and a test checks it with `FrameStats`.

  A port reproduces the original's look and keys, and adds these without
  getting in their way.
- **Snapshots** go in `tests/snapshots/`, at 80×24 unless the demo says
  otherwise.

#### D1.1 `demo`: the classic demo

- **Mirrors** ratatui's `demo`.
- **The tab strip,** in a panel titled with the app's name: Graphs, World and
  Colours.
- **Graphs:**
    - a "Graphs" panel holding a `FillGauge` (the progress, with an
      `xx.xx%` label, magenta on black, bold italic), a three-row green
      sparkline of random values, and a thick `LineGauge`;
    - below it, on the left, the tasks list (`> `, bold) and the log list
      (each level in its colour, turning every tick), over a bar chart of 24
      bars (turning every tick, three cells thick, two apart, values black on
      green, labels yellow);
    - on the right, a chart of two sine waves (dots, and Braille) whose
      window moves each tick, with axis titles and bold end labels;
    - a footer paragraph: under the rainbow, every modifier, `10€`, wrapped.
- **World:**
    - a server table: a yellow header with a gap under it, and the failed
      servers red, blinking and struck;
    - a map with a rectangle, a circle round one server, lines between the
      servers, and a green or red X on each.
- **Colours:** the 16 named colours and `default`, as foreground and
  background, in three `Ratio(1, 3)` columns.
- **Keys:** ←/→ (h/l) tabs, ↑/↓ (j/k) tasks, `t` toggles the chart, `q`
  quits. `--tick-rate MS` (250) and `--unicode false`, which uses the
  three-level sets and dot markers.
- **Refinements:**
    - a tick redraws the gauges, the sparkline, the log, the bars and the
      chart, but not the tasks, the footer or the tab strip, and a test
      asserts it;
    - the inspector shows the same.
- **Needs:** A1 (markup titles), A4, A9, B1, B2, B4, C0, C1, C2, C3, C4, C5.
- **Tests,** in `tests/demo.rs`, at 100×32:
    - `demo_graphs`: ten ticks, seeded;
    - `demo_graphs_no_chart`: after `t`;
    - `demo_world`;
    - `demo_colours` and `demo_colours.styled`;
    - `demo_plain`: with `--unicode false`;
    - a tick's `FrameStats::drawn` stays under a bound, and the footer is not
      among the nodes drawn;
    - ↓ moves the task selection.

#### D1.2 `demo2`: the showcase

- **Mirrors** ratatui's `demo2`. It comes with `examples/demo2.tcss`.
- **The frame:**
    - a dark-blue background over the whole app;
    - a title bar: the name, then a tab strip with no dividers or padding:
      Recipe, Email, Traceroute, Weather, and About as the untitled first tab;
    - a centred bar of key chips at the bottom.
- **About:**
    - an RGB swatch behind: Okhsv, the hue across and the value down, in `▀`
      cells;
    - intuiTUIve's own mark in half blocks on the left, which changes colour
      with j/k;
    - a description under a top-only border with a centred title.
- **Recipe:**
    - inside the swatch's margins, a panel with a centred bold title;
    - the steps of a recipe of our own, beside an ingredients table with
      multi-line rows, a light yellow selection and a `▐` scrollbar.
- **Email:**
    - a strip of folders, and the inbox: `>>` on the selection and a `▐`
      scrollbar;
    - the open email under a thick top border: From and Subject in bold, a
      dim rule, the body.
- **Traceroute:**
    - a table of hops with names of our own, a left `▌` scrollbar and a
      centred title;
    - a three-row ping sparkline under it, in a thick panel with a centred
      title;
    - a half-block map of a region, with the selected hop's path drawn as a
      line between two coloured points.
- **Weather:**
    - a month, today in bold red, with month and weekday headers;
    - the week's temperatures as bars with values like `76°`, red over 70
      and yellow under;
    - the seasons as horizontal bars with their values inside;
    - a line gauge, "Downloading: n%", whose colour follows its progress.
- **Destroy:** `d` makes the cells drip down the screen, then our wordmark
  fades in over them. It is an `effect` over the whole app, driven by
  `cx.animate`.
- **Keys:** h/l/←/→/Tab tabs; j/k/↑/↓ inside a tab; d/Delete destroys; q/Esc
  quits.
- **Refinements:**
    - the theme is `demo2.tcss`, a stylesheet reloaded live when its path is
      given, rather than a struct of styles;
    - the tabs, the inbox and the tables take the mouse;
    - it fits any size: under 80 columns the swatch's margins go, and under
      60 the map hides. ratatui's is fixed at 81×18 for its recording.
- **Needs:** A1, A2, A3, A7, A9, B1, B2, B3, B4, C1, C2, C3, C5, C6, C7.
  A swatch with content over it is a widget with children today (it draws
  the swatch as chrome); A8's layers are an option, not a need.
- **Tests,** in `tests/demo2.rs`, at 81×18 as ratatui's recording:
    - `demo2_about`, `demo2_recipe`, `demo2_email`, `demo2_traceroute`,
      `demo2_weather`;
    - `demo2_recipe.styled`;
    - `demo2_destroy`: two seconds of the virtual clock, seeded;
    - `demo2_narrow`: at 58×18, without the swatch's margins or the map.

#### D2.1 `widgets`: a widget gallery

- **Mirrors** ratatui-widgets' examples, one tab each; 1 to 9, ←/→ and
  clicks move between them.
- **Block:** bordered; styled bold italic blue on black; a rounded red border
  with a title.
- **Text:** a centred paragraph; a wrapped recipe paragraph with bold and
  italic runs, a masked secret, and a link (ratatui's `hyperlink`).
- **List:**
    - a list with `> ` and a reversed highlight;
    - a bottom-to-top chat with multi-line items, a red symbol, scroll
      padding and the symbol repeated.
- **Table:**
    - a bold header with a gap, an italic footer, column spacing, row, column
      and cell highlights, a `🍴 ` symbol;
    - ratatui's `table` app: stripes, four-line rows, the column highlight,
      cycling tailwind palettes, a scrollbar.
- **Tabs:** a dot divider, padding, the content in a panel with centred text.
- **Scrollbar:** ratatui's `scrollbar` app: right with arrows; left with no
  track; bottom with a begin arrow and a custom thumb; bottom with a custom
  track.
- **Calendar:** a month with events, surrounding days and a weekday header.
- **Popup:** ratatui's `popup` and `shadow`: `p` toggles a 60%×20% modal over
  a blue panel, and four shadow looks stand side by side.
- **Borders:** ratatui's `collapsed-borders`: four panes sharing borders; the
  arrows pick one, which is drawn thick.
- **Custom:** ratatui's `custom-widget`: three buttons, each a `Widget` with
  normal, selected and pressed looks, hover and clicks.
- **Needs:** A1, A2, A3, A6, A9, B1, B2, B3, B4, B6, C6, C7.
- **Tests,** in `tests/widgets.rs`:
    - `widgets_block`, `widgets_text`, `widgets_list`, `widgets_table`,
      `widgets_table_app`, `widgets_tabs`, `widgets_scrollbar`,
      `widgets_calendar`, `widgets_popup`, `widgets_borders`,
      `widgets_custom`;
    - a click presses a button;
    - the link's cells carry its URL.

#### D2.2 `charts`: a chart gallery

- **Gauge:** ratatui's `gauge` app: four `FillGauge`s (a percentage, a ratio
  with a custom label, ASCII, unicode), animated; and the `line-gauge`
  example's three looks.
- **Sparkline:** left to right; right to left with absent values; Braille.
- **Bars:** ratatui's `weather`, 24 hourly bars with `°` values coloured by
  temperature; and the `barchart` example's vertical and horizontal charts.
- **Groups:** `barchart-grouped`: revenue by quarter for each company,
  vertical and horizontal.
- **Chart:**
    - ratatui's `chart` app: a moving sine window of two datasets; a bar
      chart; a line chart with its legend top left; a scatter chart with
      custom markers;
    - the `chart` example's area chart.
- **Canvas:** the `canvas` example's map and shapes.
- **Needs:** A1, C0 to C5.
- **Tests,** in `tests/charts.rs`, at 100×30:
    - `charts_gauge`, stopped at 42%;
    - `charts_sparkline`, `charts_bars`, `charts_groups`;
    - `charts_chart`, the window stopped;
    - `charts_canvas`.

#### D2.3 `layout`: a layout explorer

- **Mirrors** ratatui's `constraint-explorer`, `constraints` and `flex`.
- **Explorer:**
    - a row of blocks you edit: ←/→ picks one; 1 to 6 sets its kind (fixed,
      percent, ratio, flex, min, max); ↑/↓ its value; a and x add and remove
      blocks; + and - the gap;
    - the row is drawn once for each `Justify` mode, with the spacers between
      (from `split_with_spacers`), and each block shows its size and its
      width.
- **Sizes:** one tab for each kind, as `constraints` has.
- **Flex:** the `flex` example's cases, in a scroll.
- **Refinements:** a last tab shows what ratatui has no counterpart for,
  `align_items` and `wrap`, and points at the `align` example.
- **Needs:** A4, A5.
- **Tests,** in `tests/layout.rs`, at 100×30: `layout_explorer`,
  `layout_explorer_gap`, `layout_sizes_ratio`, `layout_flex`.

#### D3.1 `colors`: colours

- **Mirrors** ratatui's `color-explorer`, `colors-rgb` and `modifiers`.
- **Named:** the 16 colours as foreground on five backgrounds, and as
  background under five foregrounds.
- **Indexed:** 0 to 255, as `color-explorer` lays them out, and the greys.
- **RGB:** a full-screen Okhsv swatch at 60 fps, with an fps read-out;
  `--frozen` stops it.
- **Modifiers:** each modifier, on five colours.
- **Palettes:** tailwind's 22 palettes as swatches, and material's.
- **Needs:** A2, C6; B7 measures the RGB tab.
- **Tests,** in `tests/colors.rs`, at 100×32: `colors_named` and
  `colors_named.styled`, `colors_indexed`, `colors_modifiers`,
  `colors_palettes`, and `colors_rgb` stopped.

#### D3.2 `canvas`: the canvas

- **Mirrors** ratatui's `canvas` and `mouse-drawing`.
- **Panels:**
    - the world map, with "You are here" moved by hjkl;
    - a pong ball at 60 fps;
    - a pad to draw on: press and drag, and Space changes the colour;
    - boxes with numbers along the axes.
- **Keys:** Enter cycles the marker through every kind.
- **Optional:** a Surface tab, a 3D wireframe after `volatility-surface`,
  built as a `Shape`: the arrows rotate it, z and x zoom.
- **Needs:** C0, C5.
- **Tests,** in `tests/canvas.rs`: `canvas_map`, one `canvas_marker_<kind>`
  for each marker, `canvas_drawn` after a scripted drag, and `canvas_pong`
  after a number of frames.

#### D3.3 `form`: forms

- **Mirrors** ratatui's `input-form` and `user-input`.
- **Form:**
    - first name, last name and age (↑/↓ or digits), with Tab between them;
    - each field in error while it is wrong (`error_when`);
    - Enter on the last field submits, the app quits with the form, and
      `main` prints it.
- **Chat:** an input with normal and editing modes (`e` edits, Esc stops),
  and the messages listed above it.
- **Needs:** B5, B6 (the exit value).
- **Tests,** in `tests/form.rs`: `form_empty`, `form_filled`, `form_error`
  and `form_error.styled`, `form_messages`; submitting hands the form to
  `Driver::take_exit`.

#### D3.4 `calendar`: a year

- **Mirrors** ratatui's `calendar-explorer`.
- **Shows:** twelve `month` nodes in a four-by-three grid; holidays and
  seasons styled; today; the selected day.
- **Keys:** `s` cycles the six looks (surrounding days, the weekday header,
  the month header); n/p a month; hjkl a day or a week.
- **Needs:** A3, C7.
- **Tests,** in `tests/calendar.rs`, on 2026-10-17: `calendar_year`,
  `calendar_year_surrounding`, `calendar_year_headers`.

#### D3.5 `downloads`: an inline app

- **Mirrors** ratatui's `inline`.
- **Shows:** an inline region of eight rows: a total `LineGauge`, and a
  `FillGauge` for each of four workers, which run on threads and report
  through a `Proxy`.
- **Behaviour:**
    - each finished download is printed above the region with
      `cx.print_above`, as in "Finished 7 (34 KB) in 1.2s";
    - when all are done the app quits, and the region stays in the
      scrollback.
- **Needs:** B6, C1.
- **Tests,** in `tests/downloads.rs`, headless on a virtual clock:
  `downloads_running`, the printed lines from `Driver::take_printed`, and
  `downloads_done`.

#### D3.6 `pulls`: loading in the background

- **Mirrors** ratatui's `async-github`.
- **Shows:** a table of pull requests loaded in the background (`resource`),
  the panel's right-hand title saying Loading, Loaded or Failed. j/k move,
  and `r` reloads.
- **Data:** a built-in fixture behind a delay. The crate has no HTTP client,
  so the example makes no requests. An option could read JSON from a
  command's output (`--from "gh api .."`).
- **Needs:** A1 (a right-hand title), B2.
- **Tests,** in `tests/pulls.rs`, with `wait_for_tasks(true)`:
  `pulls_loading`, `pulls_loaded`, `pulls_failed`.

#### D3.7 `panic`: panics and errors

- **Mirrors** ratatui's `panic`.
- **Keys:** `p` panics in a handler, and the terminal is given back before
  the message prints; `e` ends the app with an error (`cx.fail`); `q` quits.
- **Needs:** B6 (the exit value).
- **Tests,** in `tests/panic.rs`:
    - in a PTY, with rs-rich-interact's harness running the test binary as
      its own child as `tests/backends.rs` does, after `p` the alternate
      screen is left and the message is on the screen;
    - `e` makes `run_on` return the error.

### 3.3 Demo batches

They start once the gap batches have landed: every demo needs some of A, B
and C. Where a gap slipped, the demo leaves that part out with a comment
naming the gap, rather than working round it.

| Batch | Examples | Other files |
|---|---|---|
| **D1, the showcases** | `examples/demo.rs`, `examples/demo2.rs`, `examples/demo2.tcss` | `tests/demo.rs`, `tests/demo2.rs`, their snapshots; a new `docs/guide/intuituive/ratatui-examples.md` with a section for each batch; its nav entry in `mkdocs.yml`; the README's examples table; ratatui in the porting guide's "Credit for ports" |
| **D2, the galleries** | `examples/widgets.rs`, `examples/charts.rs`, `examples/layout.rs` | `tests/widgets.rs`, `tests/charts.rs`, `tests/layout.rs`, their snapshots; the galleries' section of `ratatui-examples.md`; README rows |
| **D3, the apps** | `examples/colors.rs`, `canvas.rs`, `form.rs`, `calendar.rs`, `downloads.rs`, `pulls.rs`, `panic.rs` | a test file for each, their snapshots; the apps' section of `ratatui-examples.md`; README rows |

Whichever batch lands first creates `ratatui-examples.md` with the three
section headings, so the others edit lines apart from each other. Tapes for
the recordings gallery (`docs/tapes/<name>.tape`, with credit in
`docs/recordings.md`) can follow in the docs workstream.

### 3.4 Licensing and credit

ratatui is MIT-licensed: Copyright (c) 2016-2022 Florian Dehau, and
2023-2025 the Ratatui Developers.

- **Rebuild, don't copy.** Each port rebuilds the original's behaviour and
  look with intuiTUIve's API and copies none of its code, as the `files` and
  `scope` ports do. A port that did copy code would have to keep ratatui's
  notice with it.
- **Credit in the header,** as `examples/scope.rs` credits scope-tui:

    ```rust
    //! A port of ratatui's `demo` example
    //! (<https://github.com/ratatui/ratatui/tree/main/examples/apps/demo>),
    //! rebuilt on intuiTUIve. It reuses none of ratatui's code: it rebuilds
    //! the behaviour and the look with intuiTUIve's API.
    //!
    //! Ported from ratatui 0.30 (<https://github.com/ratatui/ratatui>), by
    //! Florian Dehau and the Ratatui Developers, MIT licence. The design, the
    //! layout and the keys are theirs; the credit for them is theirs.
    ```

- **Credit in the porting guide:** add ratatui to its "Credit for ports"
  section, and to the recordings gallery when the tapes land.
- **Rebuild the data too.** Some of ratatui's sample data is third-party:
    - `demo2`'s recipe comes from a recipe site;
    - its traceroute hosts are a song's lyrics;
    - the world map is a third party's point set.

  Write our own sample text, and make the map from Natural Earth's
  public-domain data (C5).
- **Don't reproduce ratatui's branding:** the logo, the mascot, the release
  header. The demos draw intuiTUIve's own mark.
- **Credit the palettes and formulas** in their module (C6): Tailwind CSS,
  MIT; Material Design, Apache-2.0; Oklab and Okhsv, Björn Ottosson.

## 4. Method

- **ratatui:** every public item of ratatui-core (layout, style, text,
  buffer, symbols, widgets, terminal, backends) and ratatui-widgets; the
  `CHANGELOG` from 0.30.0 to 0.30.2, to date each feature; and every example
  app, widget example and tape.
- **intuiTUIve:** read in its sources (`src/`), its guide, the two design
  notes, its tests, and the rs-rich-ext and rs-rich-interact modules it
  draws with.
- **Claims:** each "none" in the matrix was checked against the sources: for
  example, the panel's title is written as a plain segment, `gap` is a
  `u16`, the list renders only the first line of an item, and only
  `ScrollView` draws a scrollbar.
- **Sizes** are estimates, so that the batches come out about equal.
