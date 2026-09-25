# Layout, columns and groups

```python
from rs_rich.columns import Columns
from rs_rich.console import Console
from rs_rich.containers import Renderables
from rs_rich.layout import Layout
from rs_rich.panel import Panel
```

The renderables that arrange others. Each corresponds to the Rich class of
the same name and renders byte for byte as Rich 15.0.0 does.

## Columns

```text
Columns(renderables=None, padding=(0, 1), *, width=None, expand=False,
        equal=False, column_first=False, right_to_left=False, align=None,
        title=None)
columns.add_renderable(renderable)
```

Renderables in as many columns as fit the width. `equal` gives every column
the widest item's width, `expand` fills the width, `column_first` fills top to
bottom, `right_to_left` starts on the right, `align` aligns each item in its
column, `width` fixes the column width and `title` is drawn above.

```python
console = Console(width=30)
words = "alpha beta gamma delta epsilon zeta eta theta".split()
console.print(Columns(words))
console.print(Columns(words, equal=True, column_first=True))
```

```text
alpha beta gamma delta epsilon
zeta  eta  theta              
alpha delta   eta  
beta  epsilon theta
gamma zeta         
```

## Group

```text
Group(*renderables, fit=True)
@group(fit=True)
```

`rich.console.Group`: several renderables one after another, as one
renderable (in a panel, say). With `fit` the group measures as its widest
child; without it, as the whole width. The `group` decorator turns a function
that yields renderables into one that returns a `Group`.

```python
from rs_rich.console import Group, group

console = Console(width=30)
console.print(Panel.fit(Group("first", Panel("second"))))


@group()
def lines():
    yield "one"
    yield "two"


console.print(Panel.fit(lines()))
```

```text
╭────────────╮
│ first      │
│ ╭────────╮ │
│ │ second │ │
│ ╰────────╯ │
╰────────────╯
╭─────╮
│ one │
│ two │
╰─────╯
```

`rich.containers.Renderables` (a list that
renders its items in turn) is `rs_rich.containers.Renderables`, and
`rich.containers.Lines` is `rs_rich.containers.Lines` (see [Text](text.md)).
`rich.measure.measure_renderables(console, options, renderables)` is
`rs_rich.measure.measure_renderables`.

## Layout

```text
Layout(renderable=None, *, name=None, size=None, minimum_size=1, ratio=1,
       visible=True)
```

A region of fixed height divided into rows and columns. `split_column(...)`
stacks sub-layouts, `split_row(...)` puts them side by side, `split(...,
splitter="row")` does either (an unknown splitter raises `NoSplitter`), and
`add_split(...)` and `unsplit()` change a split. A sub-layout is sized by
`size`, or shares the space by `ratio` (never below `minimum_size`); an
invisible one is left out. `layout["name"]` (or `layout.get("name")`) finds a
sub-layout and `update(renderable)` sets its content; a layout with no content
shows a placeholder with its name and size, as in Rich.

```python
console = Console(width=40, height=10)
layout = Layout()
layout.split_column(Layout(name="header", size=3), Layout(name="body"))
layout["body"].split_row(Layout(name="left"), Layout(name="right", ratio=2))
layout["header"].update(Panel("header"))
layout["left"].update("left side")
console.print(layout)
```

```text
╭──────────────────────────────────────╮
│ header                               │
╰──────────────────────────────────────╯
left side    ╭─── 'right' (27 x 7) ────╮
             │    Layout(              │
             │        name='right',    │
             │        ratio=2          │
             │    )                    │
             │                         │
             ╰─────────────────────────╯
```

A layout's height is the console's (or the `height=` given to `print`).
`layout.tree` is a [`Tree`](tree.md) of the structure, `layout.render(console,
options)` returns `{layout: LayoutRender(region, lines)}` for each leaf, and
`layout.map` holds the last render's. `Region`, `LayoutRender`, `Splitter`,
`RowSplitter`, `ColumnSplitter` and `LayoutError` are in `rs_rich.layout` too.

```python
console.print(layout.tree)
```

```text
⬍ Layout()
├── ⬍ Layout(name='header', size=3)
└── ⬌ Layout(name='body')
    ├── ⬍ Layout(name='left')
    └── ⬍ Layout(name='right', ratio=2)
```

`Layout.refresh_screen` raises `NotImplementedError`: the bindings' console
cannot redraw part of the screen. Print the layout again, or show it in a
`Live` display, instead. A custom `Splitter` subclass also raises
`NotImplementedError`; the row and column splitters are supported.
