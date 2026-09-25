# Panel

```python
from rs_rich.panel import Panel
```

A `Panel` draws a border around one renderable, with optional titles. It
corresponds to `rich.panel.Panel`.

## Constructor

```text
Panel(renderable, box=ROUNDED, *, title=None, title_align="center",
      subtitle=None, subtitle_align="center", expand=True, border_style=None,
      width=None, padding=None)
```

| Argument | Meaning |
|---|---|
| `renderable` | Any renderable: a `str` (markup), [`Text`](text.md), [`Table`](table.md), another `Panel`, or [your own class](protocol.md). Anything else raises `NotRenderableError` when the panel is printed. |
| `box` | A [box constant](box-markup-errors.md#boxes). The default is `box.ROUNDED`, and `None` is a `ValueError`. |
| `title`, `subtitle` | Markup drawn in the top and bottom borders. |
| `title_align`, `subtitle_align` | `"left"`, `"center"` or `"right"`; anything else is a `ValueError`. |
| `expand` | Fill the available width. `False` fits the content; see [`Panel.fit`](#fit). |
| `border_style` | A style for the border. |
| `width` | A fixed width in cells. |
| `padding` | Space inside the border: `n`, `(vertical, horizontal)` or `(top, right, bottom, left)`. The default is `(0, 1)`. Other shapes, and any side above 65536, raise `ValueError`. |

A `str` inside a panel is markup but is not highlighted, as in Rich, where a
panel renders its content with `highlight=False`.

Panels (and other renderables) nest up to 100 deep. Printing a deeper chain
raises `RecursionError`, as Rich does a little past that depth:

```python
from rs_rich.console import Console
from rs_rich.panel import Panel

nested = "x"
for _ in range(101):
    nested = Panel(nested)
try:
    Console(width=40).print(nested)
except RecursionError as error:
    print(error)
```

```text
maximum recursion depth exceeded: rs_rich renders at most 100 nested renderables
```

```python
from rs_rich.console import Console
from rs_rich.panel import Panel

console = Console(width=30)
console.print(Panel("Hello, [bold]World[/]!", title="Greeting", subtitle="rs_rich"))
```

```text
╭───────── Greeting ─────────╮
│ Hello, World!              │
╰───────── rs_rich ──────────╯
```

## fit

```text
Panel.fit(renderable, box=ROUNDED, *, title=None, title_align="center",
          subtitle=None, subtitle_align="center", border_style=None,
          width=None, padding=None)
```

This is a panel that fits its content instead of filling the width: the
same as `expand=False`.

```python
from rs_rich import box
from rs_rich.console import Console
from rs_rich.panel import Panel
from rs_rich.table import Table

table = Table("key", "value", box=box.SIMPLE)
table.add_row("answer", "42")
console = Console(width=40)
console.print(Panel.fit(table, title="inner", box=box.DOUBLE, padding=(0, 2)))
```

```text
╔═══════ inner ════════╗
║                      ║
║    key      value    ║
║   ────────────────   ║
║    answer   42       ║
║                      ║
╚══════════════════════╝
```
