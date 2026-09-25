# Table

```python
from rs_rich.table import Table
```

A `Table` lays out rows and columns of text. It corresponds to
`rich.table.Table`, with Rich's column-width algorithm.

## Constructor

```text
Table(*headers, title=None, caption=None, box=HEAVY_HEAD, show_header=True,
      show_lines=False, show_edge=True, expand=False, border_style=None)
```

| Argument | Meaning |
|---|---|
| `*headers` | Column headers, the same as calling `add_column(header)` for each. |
| `title`, `caption` | Markup drawn centred above and below the table. |
| `box` | A [box constant](box-markup-errors.md#boxes), or `None` for no borders. The default is `box.HEAVY_HEAD`. Without a header, head-styled boxes draw plain (`HEAVY_HEAD` as `SQUARE`), as in Rich. |
| `show_header` | Draw the header row. |
| `show_lines` | Draw a line between rows. |
| `show_edge` | Draw the outer border. |
| `expand` | Fill the available width. |
| `border_style` | A style for the borders. |

```python
from rs_rich.console import Console
from rs_rich.table import Table

table = Table("Planet", "Moons", title="Solar system")
table.add_row("Earth", "1")
table.add_row("Mars", "2")
Console(width=40).print(table)
```

```text
   Solar system   
┏━━━━━━━━┳━━━━━━━┓
┃ Planet ┃ Moons ┃
┡━━━━━━━━╇━━━━━━━┩
│ Earth  │ 1     │
│ Mars   │ 2     │
└────────┴───────┘
```

## add_column

```text
add_column(header="", *, style=None, header_style=None, justify="left",
           overflow="ellipsis", width=None, min_width=None, max_width=None,
           ratio=None, no_wrap=False)
```

| Argument | Meaning |
|---|---|
| `header` | The header, as markup. |
| `style` | A style for the column's cells. |
| `header_style` | A style for the whole header cell. |
| `justify` | `"left"`, `"center"`, `"right"`, `"full"` or `"default"`. |
| `overflow` | `"fold"`, `"crop"`, `"ellipsis"` or `"ignore"`. |
| `width`, `min_width`, `max_width` | A fixed width, or bounds, in cells. |
| `ratio` | This column's share of the free width when the table expands. |
| `no_wrap` | Keep cells on one line. |

An invalid `justify` or `overflow` raises `ValueError`. So does a `width`,
`min_width` or `max_width` above 65536 (the widest console), or a `ratio`
above 4294967295 (2³² − 1). Rich accepts larger values, but no terminal is
that wide.

```python
from rs_rich import box
from rs_rich.console import Console
from rs_rich.table import Table

table = Table(box=box.SIMPLE, expand=True)
table.add_column("Name", ratio=1)
table.add_column("Score", justify="right", width=6)
table.add_row("alpha", "97")
table.add_row("a much longer name that wraps", "3")
Console(width=32).print(table)
```

```text
                                
  Name                   Score  
 ────────────────────────────── 
  alpha                     97  
  a much longer name         3  
  that wraps                    
                                
```

## add_row

```text
add_row(*cells)
```

This adds a row. Each cell is a `str` (markup), a [`Text`](text.md) (used as
is, never parsed), `None` (empty), or any other renderable: a
[`Panel`](panel.md), another table, or [your own class](protocol.md), sized
by its `__rich_measure__`. Fewer cells than columns leaves the rest empty,
and more raises `ValueError`; a cell that is not renderable raises
`NotRenderableError`.

`row_count` is the number of rows added.

```python
from rs_rich.console import Console
from rs_rich.table import Table
from rs_rich.text import Text

table = Table("markup", "text", "none", box=None)
table.add_row("[bold]b[/]", Text("[not markup]"), None)
print(table.row_count)
Console(width=40).print(table)
```

```text
1
 markup  text          none 
 b       [not markup]       
```

A table is built when it is printed, so rows added after it was put in a
panel still appear.
