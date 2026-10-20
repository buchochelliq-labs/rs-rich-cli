# Panels, themes and stylesheets

How nodes look: panels (boxes, titles, rules and shared borders), themes
(the named styles the framework and your markup use), a style on a node,
and stylesheets, which style and lay out an app from a file that reloads
while it runs.

## Panels

`.panel(title)` wraps a node in a box: rounded, its title at the top left,
lit in the theme's `border.focused` while the focus is inside. Titles are
console markup. Builders after `.panel` change the rest, as ratatui's
`Block` options do (on any other node they do nothing):

```rust
label("3 open, 1 merged")
    .panel("[b]Pull requests[/]")
    .title_align(Align::Center)                    // where .panel's title sits
    .title_right("[dim]loading…")                  // another title on the top edge
    .title_bottom("[dim]q quits")                  // a title on the bottom edge
    .add_title(Edge::Bottom, Align::End, "1/20")   // the general form
    .border(Border::Thick)                         // or Plain, Double, Ascii, the dashed kinds, None…
    .borders(Sides::TOP | Sides::BOTTOM)           // Sides::ALL is the default
    .border_style("bright_black")                  // while the focus is elsewhere
    .title_style("bold white")
    .shadow(Shadow::MediumShade)                   // or LightShade, DarkShade, Block, Dim, Symbol('$')
```

- **Titles** keep the look of `.panel`'s: ` Title `, one glyph of line in
  from the corner. Several with the same alignment sit side by side, one
  cell apart. When they do not fit, the right-hand ones give way first,
  then the centred ones, and the left ones are cut with `…`. An edge with a
  title keeps its row even when its line is not drawn, so
  `.border(Border::None)` gives a heading above the content, as ratatui's
  `Block::new().title(..)` does. The first top title, as plain text, is the
  panel's accessible name.
- **Sides.** The inside grows into the sides that are not drawn.
- **Shadows** take a strip on the right and the bottom of the panel's own
  rectangle, in the theme's `shadow` style; the box is a cell narrower and
  shorter for it.

Titles at the left, the centre and the right, and two at the bottom:

```text
--8<-- "crates/rich-intuituive/tests/snapshots/panel_titles.txt"
```

Each kind of border, `Border::Custom(BorderSet { .. })` (glyphs of your
own) last, then three shadows:

```text
--8<-- "crates/rich-intuituive/tests/snapshots/panel_kinds.txt"
```

```text
--8<-- "crates/rich-intuituive/tests/snapshots/panel_shadow.txt"
```

A stylesheet sets the same: `border: thick` (and every kind by name),
`border-title`, `border-title-align`, `border-subtitle` (the bottom
title), `border-subtitle-align` and `border-sides: top bottom`. Code wins
over the sheet.

### Titles and borders that follow state

`.panel_with(|| markup)` makes a panel whose title is read again as the
signals it reads change (`Diff: src/lib.rs`, `Processes (3 of 210)`);
`.title_with(|| ..)` does the same on a panel already made, and
`.add_title_with(edge, align, || ..)` adds one beside `add_title`'s. The
kind of box can follow state too: `.focus_border(Border::Double)` while
the focus is inside (Slumber's selected pane), `.border_with(|| ..)` from
anything else.

```rust
list(rows, cursor)
    .panel_with(move || format!("Commit {}/{}", cursor.get() + 1, count.get()))
    .border(Border::Plain)
    .focus_border(Border::Double)
```

### Rules, and a border on any node

`rule()` is a line across, one row tall, in the theme's border style.
Directly inside a panel, from its left side to its right, it joins the
border, in the panel's kind and focus colour: the `├──┤` between the parts
of a help box. Padding between them does not keep it apart: a rule at the
inside edge of a padded column (`.padding(0, 1)`), or of a panel a
stylesheet pads, runs across the padding to the border, so the column is
padded once rather than each part. `.border(Border::Thick)`
picks the kind of a rule on its own, and it is left out of what assistive
technology reads.

```rust
column([label("Keys"), rule(), label("q quit")]).panel("Help")
```

`.borders(Sides::BOTTOM)` (or any sides) on a node that is not a panel
puts it in a panel without a title first: an underlined header, a status
line with a rule above it.

### Collapsed borders

`.collapse_borders()` on a `row`, `column` or `grid` of panels draws one
frame for them all: neighbours share their edges, joined where lines meet.
Each panel's titles sit on its own edges and its content between the
lines. The lines round the panel with the focus are lit in
`border.focused`, and `.focus_border(Border::Thick)` draws them thick, as
ratatui's `collapsed-borders` example does for the selected pane:

```rust
column([
    pane("Top"),
    row([pane("Left"), pane("Right")]).collapse_borders(),
    pane("Bottom"),
])
.collapse_borders()
.border(Border::Plain)
.focus_border(Border::Thick)
```

```text
--8<-- "crates/rich-intuituive/tests/snapshots/collapsed_focus_left.txt"
```

A collapsed container inside another shares its lines and takes its look;
a `grid` whose top and bottom panes `.span(2, 1)` draws the same.
`.border(..)` and `.border_style(..)` on the container set its lines, gaps
are ignored, and a hidden child gives its room to the others. In a
stylesheet: `border-collapse: collapse`, with the container's `border` for
its lines.

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

The framework's own parts look for names of their own and fall back to
these: a modal's or pop-up's panel title (the palette's, the help's, a
confirm's) is drawn in `popup.title`, else `title`; the which-key keys in
`which_key.key`, else `accent`, its separators in `which_key.sep` and its
descriptions in `which_key.desc`; notifications in `notify.info`,
`notify.warn` and `notify.error`, else `accent`, `warn` and `bad`. The
presets also name the [stock buttons](controls.md#buttons)' faces,
`button`, `button.primary` and `button.danger`; the other
[form controls](controls.md#the-theme-styles) fall back to `accent`,
`good` and `muted`.

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
leaves the last good styles in place, and the [inspector](testing.md#inspector) shows
the error. `Theme::load(path)` and `theme.with_config(text)` read the same
format without watching.

### A style on a node

`.style("white on #101830")` draws any node in a style, ratatui's
`Block::style`; `.bg(colour)` and `.fg(colour)` set one colour. Each takes
a theme style's name too (`.style("accent")`, `.bg("bad")`), looked up
again when the theme changes. The style goes under what the node and its
children draw, as a stylesheet's `color` and `background` do, and fills the
rest of the node's rectangle, gaps and borders included. Code wins over a
stylesheet, and markup inside wins over both:

```rust
column([label("one"), label("[yellow]two[/] and more"), row([..]).gap(2)])
    .gap(1)
    .bg("blue")
```

```text
--8<-- "crates/rich-intuituive/tests/snapshots/node_style_fill.styled.txt"
```

Only what drew is restyled: a tick that redraws one child sends its cells,
not the whole fill.

### Theme styles for a subtree

`.theme_styles([(name, style), ..])` gives a node and everything inside it
theme styles of its own, over the app's, as CSS custom properties do. They
hold for markup (`[selected]…[/]`) and for the widgets inside, which look
their styles up by name: a list's `selected`, a table's `table.header`, a
tab strip's `tabs.selected`, a panel's `border` and `title`. It is the
per-instance answer to ratatui's `highlight_style` and the like, with no
style option on each widget:

```rust
row([
    list(items, a),
    list(items, b).theme_styles([("selected", "underline")]),
])
```

```text
--8<-- "crates/rich-intuituive/tests/snapshots/theme_styles_list.styled.txt"
```

A style may name another theme style (`("selected", "accent")`), which
follows a theme switch. The node keeps one copy of the console and theme
with its styles, made again only when the theme changes; a node without
theme styles makes none.

## Stylesheets

A stylesheet styles and lays out nodes from outside the code, in a subset
of CSS:

```rust
App::new(build).stylesheet_file("app.tcss").run()
```

```css
/* app.tcss */
#status { dock: bottom; size: 1; background: $accent; color: black; }
#preview { size: 3fr; padding: 0 1; border: round $accent; border-title: Preview; }
.danger:focus { background: red; text-style: bold; }
panel table:hover { text-style: underline; }
```

**Selectors** match a node's kind (`label`, `text`, `table`, `panel`,
`column`, `row`, `grid`: what the [inspector](testing.md#inspector) shows), its
`.name("…")` as `#name`, its classes as `.class`, and its states:

- `:focus` and `:focus-within`;
- `:hover`, for the node under the pointer and its ancestors;
- `:selected` and `:disabled`, while `.selected_when(…)` or
  `.disabled_when(…)` holds;
- `:error` (or CSS's `:invalid`), while `.error_when(…)` holds (see
  [Errors](#errors)).

Spaces separate a node from its ancestors (`panel label`). Sibling
selectors and `!important` are not supported. The more specific rule wins,
then the later one.

`.class("warn big")` gives a node fixed classes, and
`.class_when("danger", move || load.get() > 90)` gives it a class while a
condition holds, following the signals the condition reads.

**`@media`** rules hold at some sizes of the
[viewport](layout.md#responsive-layout) only:

```css
@media (max-width: 80) {
    #sidebar { display: none; }
    #main { grid-columns: 1fr; }
}
@media (min-width: 120) and (max-height: 30) { #log { size: 50%; } }
```

The conditions are `min-width`, `max-width`, `min-height` and
`max-height`, in cells, inclusive as in CSS, joined with `and`; the space
after `@media` may be left out, as in CSS (`@media(max-width: 80)`). Inside,
rules are written as anywhere else, and the more specific rule still wins,
then the later one, whether it is in a block or not. Blocks do not nest,
and other `@` rules are an error.

**Properties:**

| | |
|---|---|
| `color`, `background` | a colour (`red`, `#ff8800`, `grey23`) or a theme style's colour (`$accent`) |
| `text-style` | `bold`, `dim`, `italic`, `underline`, `reverse`, `strike`, `blink`, `none` (every one off); a later rule's replaces an earlier one's |
| `border`, `border-title` | `round`, `square` (or `plain`), `heavy` (or `thick`), `double`, `ascii`, `dashed` and the other dashed kinds (`light-triple-dashed`, `heavy-quadruple-dashed`, …), `quadrant-inside`, `quadrant-outside`, or `none` (which takes a panel's own border, its titles and their cells away), then a colour; a title (markup) |
| `border-title-align`, `border-subtitle`, `border-subtitle-align` | `left`, `center` or `right`; a title on the bottom edge, and where it sits |
| `border-sides` | the sides drawn: `top`, `right`, `bottom`, `left` (several), `all` or `none` |
| `border-collapse` | `collapse` draws one frame for a row's, column's or grid's panels (see [Collapsed borders](#collapsed-borders)), or `separate` |
| `text-align`, `text-wrap`, `text-overflow` | a text's `left`, `center`, `right` or `justify`; `wrap` or `nowrap`; `fold`, `clip` or `ellipsis` |
| `size`, `min-size`, `max-size` | `3`, `50%`, `1/3`, `2fr`, `auto`, along the parent's axis |
| `padding` | one to four numbers, as in CSS |
| `gap`, `grid-columns`, `grid-rows` | a stack's or grid's spacing; a grid's tracks |
| `display` | `none` hides the node and what is inside, in any container (a split's pane, a scroll's or a switch's child), takes it out of the Tab order, and out of a grid's cells |
| `dock` | `top` or `bottom` in a column, `left` or `right` in a row: kept at that edge |
| `align-items`, `align-self` | `flex-start`, `center`, `flex-end` or `stretch` (`align-self` also `auto`): where a stack's children sit across its axis |
| `justify-content` | `flex-start`, `center`, `flex-end`, `space-between`, `space-around` or `space-evenly`: what a stack does with the room its children leave |
| `flex-wrap`, `align-content` | `nowrap` or `wrap`; where a wrapping stack's lines sit (the `justify-content` values, or `stretch`) |

**How it applies:**

- **Colours inherit, as in CSS.** They go under what a node draws, so a
  widget's own colours and markup's `[green]` stay, and they fill in the
  rest of the node's area, gaps included.
- **Code wins.** `.fixed(3)` beats `size: 5`, `.style()` and `.bg()` beat
  `background`, and `.focus_style()` and `.hover_style()` go over the
  sheet's colours. A node whose code sets no size takes the sheet's.
- **What follows states.** States and `class_when` change colours and text
  styles. Layout is decided by a node's kind, name and fixed classes, and
  decided again when a resize crosses an `@media` rule's bounds (then
  everything draws once). A resize that crosses none matches nothing again.
- **Borders.** A `border` on a panel restyles the panel's own box. On
  another node it draws a box round it, inside its area, with its
  `border-title`, `border-subtitle` and `border-sides`. On a container
  with `border-collapse: collapse` it is the kind and colour of the shared
  lines.
- **Live.** A sheet file is read again whenever it changes. One that does
  not parse is reported in a toast with its line and column, and the last
  good sheet stays. `App::stylesheet(css)` takes a string.
  `Stylesheet::parse` checks one in a test.
- **Cost.** Rules are matched once per node and sheet, and a node's style
  is worked out only when it draws. An app without a sheet does none of
  it.

The `styled` example is a status page whose look is all in its sheet.
Given the sheet's path, it reads the file again whenever it changes:

```bash
cargo run -p rs-rich-intuituive --example styled -- crates/rich-intuituive/examples/styled.tcss
```

```css
--8<-- "crates/rich-intuituive/examples/styled.tcss"
```

The code names the nodes and gives them classes:

```rust
--8<-- "crates/rich-intuituive/examples/styled.rs:card"
```

```rust
--8<-- "crates/rich-intuituive/examples/styled.rs:sheet"
```

The recording edits the sheet while the app runs: the borders and the
title change, then a misspelt property is reported and the last good
sheet stays.

<div class="tape-player" data-cast="../../media/tapes/stylesheet/stylesheet.cast" data-poster="npt:0:3">
  <img src="../../media/tapes/stylesheet/stylesheet.gif" alt="A status page styled by a stylesheet, restyled when the sheet changes">
</div>

| Styled | `:focus` and `:hover` |
|---|---|
| ![The status page as the sheet styles it](../../media/tapes/stylesheet/styled.png) | ![The down service focused, another hovered](../../media/tapes/stylesheet/states.png) |
| **Reloaded** | **A mistake** |
| ![Heavy borders and a magenta title after the edit](../../media/tapes/stylesheet/reloaded.png) | ![A toast naming the line and column](../../media/tapes/stylesheet/error.png) |

### Errors

`.error_when(condition)` puts a node in error while the condition holds: a
field whose value is wrong, a form that cannot be sent. Three things
follow, alongside the focus and `:disabled`:

- **The theme** gives it a look of its own, for every app: a panel draws
  its border in the theme's `border.error` style, and another node's text
  takes the theme's `error` style, under the text's own colours as a
  stylesheet's go. Both are the theme's `bad` style unless the theme (or a
  [theme file](#theme-files-reloaded-live)) names them.
- **The stylesheet** matches it with `:error`, or CSS's `:invalid`.
- **Assistive technology** is told it is invalid: `AccessState::invalid`,
  `invalid` in [linear mode](accessibility.md#linear-mode)'s line, `aria-invalid` in a
  browser.

```rust
let age = signal(String::new());
column([
    component(Input::new("Age"), move |answer, _| age.set(answer)),
    text!("Age: {age}")
        .label("Age")
        .error_when(move || age.with(|a| a.parse::<u8>().is_err())),
])
```

```css
:error { background: $bad; color: white; }
```
