# Table

```python
from rs_rich.table import Table
```

A `Table` lays out rows and columns of text. It corresponds to
`rich.table.Table`, with Rich's column-width algorithm.

## Constructor

```text
Table(*headers, title=None, caption=None, width=None, min_width=None,
      box=HEAVY_HEAD, safe_box=None, padding=(0, 1), collapse_padding=False,
      pad_edge=True, expand=False, show_header=True, show_footer=False,
      show_edge=True, show_lines=False, leading=0, style="none",
      row_styles=None, header_style="table.header", footer_style="table.footer",
      border_style=None, title_style=None, caption_style=None,
      title_justify="center", caption_justify="center", highlight=False)
Table.grid(*headers, padding=0, collapse_padding=True, pad_edge=False, expand=False)
```

| Argument | Meaning |
|---|---|
| `*headers` | Column headers, the same as calling `add_column(header)` for each. |
| `title`, `caption` | Drawn above and below the table: markup, or a [`Text`](text.md) (drawn as it is, in its own style and justification). |
| `title_style`, `caption_style` | The style of a markup title or caption (default `table.title`, `table.caption`). |
| `title_justify`, `caption_justify` | `"left"`, `"center"` (the default), `"right"`, `"full"` or `"default"`. |
| `width` | The table's width, borders included; setting it expands the table to that width. |
| `min_width` | The table's minimum width, borders included. |
| `show_footer` | Draw a footer row from each column's `footer`. |
| `leading` | Blank lines between rows (drawn with the box's side glyphs); it takes over from `show_lines`. |
| `row_styles` | Styles that the rows cycle through, such as `["", "dim"]` for zebra stripes. |
| `header_style`, `footer_style` | The header and footer rows' style (default `table.header`, `table.footer`); `None` is no style. |
| `box` | A [box constant](box-markup-errors.md#boxes), or `None` for no borders. The default is `box.HEAVY_HEAD`. Without a header, head-styled boxes draw plain (`HEAVY_HEAD` as `SQUARE`), as in Rich. |
| `show_header` | Draw the header row. |
| `show_lines` | Draw a line between rows. |
| `show_edge` | Draw the outer border. |
| `expand` | Fill the available width. |
| `border_style` | A style for the borders. |
| `padding` | Space around each cell: `n`, `(vertical, horizontal)` or `(top, right, bottom, left)`. |
| `collapse_padding`, `pad_edge` | Merge neighbouring cells' padding; pad the outer edge. |
| `style` | A style under the whole table (the borders take it too). |
| `highlight` | Highlight the cells' strings, as the console highlights printed ones. |
| `safe_box` | Replace boxes a legacy Windows console cannot draw (`None`: the console's setting). |

`Table.grid()` is a table with no borders and no header, for laying things
out in columns.

Every style argument takes a `Style`, a style definition or a theme name.

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
add_column(header="", footer="", *, header_style=None, highlight=None,
           footer_style=None, style=None, justify="left", vertical="top",
           overflow="ellipsis", width=None, min_width=None, max_width=None,
           ratio=None, no_wrap=False)
```

| Argument | Meaning |
|---|---|
| `header`, `footer` | The header and footer: markup, a [`Text`](text.md) or any renderable. The footer shows with `show_footer=True`. |
| `style` | A style for the column's cells. |
| `header_style`, `footer_style` | A style for the whole header or footer cell, over the table's. |
| `justify` | `"left"`, `"center"`, `"right"`, `"full"` or `"default"`. |
| `vertical` | `"top"`, `"middle"` or `"bottom"`: where a short cell sits in a tall row. |
| `highlight` | Highlight this column's strings (`None`: the table's `highlight`). |
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
add_row(*cells, style=None, end_section=False)
```

This adds a row. Each cell is a `str` (markup), a [`Text`](text.md) (used as
is, never parsed), `None` (empty), or any other renderable: a
[`Panel`](panel.md), another table, or [your own class](protocol.md), sized
by its `__rich_measure__`. Fewer cells than columns leaves the rest empty,
and more adds columns (with empty headers), as in Rich; a cell that is not
renderable raises `NotRenderableError`.

`style` styles the whole row, over the column's style. `end_section=True`
draws a line beneath the row, as does `add_section()` for the last row
added. `row_count` is the number of rows added.

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

Every constructor argument is also an attribute you can read and set, as in
Rich (`table.show_header = False`, `table.title = "..."`, `table.box = None`,
`table.padding = 0`, ...): they take effect when the table next prints.
`padding` reads back as `(top, right, bottom, left)`. Rich's `columns` and
`rows` lists are not attributes; use `add_column` and `add_row`.

Footers, sections, row styles and a fixed width:

```python
from rs_rich.console import Console
from rs_rich.table import Table

table = Table(title="Fruit", caption="per crate", caption_justify="right",
              show_footer=True, row_styles=["", "dim"], width=30)
table.add_column("Name", "Total")
table.add_column("Qty", "12", justify="right")
table.add_row("apple", "3")
table.add_row("banana", "4", end_section=True)
table.add_row("cherry", "5", style="bold")
Console(width=40).print(table)
```

```text
            Fruit             
┏━━━━━━━━━━━━━━━━━┳━━━━━━━━━━┓
┃ Name            ┃      Qty ┃
┡━━━━━━━━━━━━━━━━━╇━━━━━━━━━━┩
│ apple           │        3 │
│ banana          │        4 │
├─────────────────┼──────────┤
│ cherry          │        5 │
├─────────────────┼──────────┤
│ Total           │       12 │
└─────────────────┴──────────┘
                     per crate
```

`leading`, `min_width`, and a row with more cells than columns:

```python
from rs_rich import box
from rs_rich.console import Console
from rs_rich.table import Table

table = Table("a", "b", box=box.SIMPLE, leading=1, min_width=20)
table.add_row("1", "2")
table.add_row("3", "4", "extra")
Console(width=40).print(table)
```

```text
                    
  a    b            
 ────────────────── 
  1    2            
                    
  3    4    extra   
                    
```
