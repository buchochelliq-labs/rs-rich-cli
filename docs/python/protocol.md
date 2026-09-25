# The render protocol

Your own classes render with `rs_rich` the way they do with Rich: through
`__rich__`, `__rich_console__` and `__rich_measure__`. They print on their
own, and they work anywhere a renderable goes: table cells, panels, and the
children of other renderables. The output is byte-for-byte Rich 15.0.0's.

## `__rich__`

A class whose `__rich__()` returns something renderable (a markup string, a
`Text`, a `Panel`, another object with `__rich__`) prints as that:

```python
from rs_rich.console import Console
from rs_rich.panel import Panel

class User:
    def __init__(self, name):
        self.name = name

    def __rich__(self):
        return f"[bold]{self.name}[/] (user)"

console = Console(width=30)
console.print(User("ada"))
console.print(Panel(User("grace"), title="who"))
```

```text
ada (user)
╭─────────── who ────────────╮
│ grace (user)               │
╰────────────────────────────╯
```

`__rich__` is called when the object is printed, not when it is put in a
panel or table, so it shows the object's state at that moment.

## `__rich_console__`

`__rich_console__(self, console, options)` yields what to print: strings
(console markup), `Text`s, [`Segment`](#segment)s, or any other renderable,
including more of your own. `console` is the printing `Console`, and
`options` a [`ConsoleOptions`](#consoleoptions) saying how much room there
is.

```python
from rs_rich.console import Console
from rs_rich.segment import Segment
from rs_rich.style import Style
from rs_rich.table import Table
from rs_rich.text import Text

class Report:
    def __rich_console__(self, console, options):
        yield "[bold]Report[/]"
        yield Text(f"{options.max_width} columns available")
        yield Segment("raw segment", Style(italic=True))
        yield Segment.line()
        table = Table("key", "value")
        table.add_row("status", "ok")
        yield table

Console(width=24).print(Report())
```

```text
Report
24 columns available
raw segment
┏━━━━━━━━┳━━━━━━━┓
┃ key    ┃ value ┃
┡━━━━━━━━╇━━━━━━━┩
│ status │ ok    │
└────────┴───────┘
```

A yielded string or `Text` ends its line; a `Segment` does not, so end a
line of segments with `Segment.line()`. Inside a container, `options` is the
container's: in a panel, `max_width` is the width inside the border.

```python
from rs_rich.console import Console
from rs_rich.panel import Panel
from rs_rich.table import Table

class Width:
    def __rich_console__(self, console, options):
        yield f"width {options.max_width}"

console = Console(width=30)
console.print(Width())
console.print(Panel(Width()))
table = Table("a", "b", box=None)
table.add_row(Width(), "x")
console.print(table)
```

```text
width 30
╭────────────────────────────╮
│ width 26                   │
╰────────────────────────────╯
 a                          b 
 width 25                   x 
```

A `__rich_console__` may use the console while it renders: its width,
`console.measure`, `console.render_lines` to render a child to lines, or
`console.render` for its segments. No lock is held, and the console's other
threads are not blocked.

```python
from rs_rich.console import Console
from rs_rich.segment import Segment

class Framed:
    def __init__(self, child):
        self.child = child

    def __rich_console__(self, console, options):
        width = options.max_width - 2
        for line in console.render_lines(self.child, options.update_width(width)):
            yield Segment("|")
            yield from line
            yield Segment("|")
            yield Segment.line()

Console(width=12).print(Framed("some words to wrap"))
```

```text
|some words|
|to wrap   |
```

## `__rich_measure__`

Containers that size themselves to their content (a table column, a
`Panel.fit`) ask each child for its minimum and maximum width.
`__rich_measure__(self, console, options)` answers with a
[`Measurement`](#measurement) (a `(minimum, maximum)` pair also works). A
class without it takes all the width offered, as in Rich.

```python
from rs_rich.console import Console
from rs_rich.measure import Measurement
from rs_rich.panel import Panel

class Badge:
    def __rich_console__(self, console, options):
        yield "[reverse] OK [/]"

    def __rich_measure__(self, console, options):
        return Measurement(4, 4)

class Greedy:
    def __rich_console__(self, console, options):
        yield "greedy"

console = Console(width=20)
console.print(Panel.fit(Badge()))
console.print(Panel.fit(Greedy()))
print(console.measure(Badge()), console.measure(Greedy()))
```

```text
╭──────╮
│  OK  │
╰──────╯
╭──────────────────╮
│ greedy           │
╰──────────────────╯
Measurement(minimum=4, maximum=4) Measurement(minimum=0, maximum=20)
```

## Errors

An exception raised by `__rich__`, `__rich_console__` or `__rich_measure__`
propagates out of `print`, wherever the object is (in a panel, a table
cell), and nothing is printed. An object that is none of a string, a
`Segment` or a renderable raises `NotRenderableError`; so does a
`__rich_console__` that returns something that is not iterable.

```python
from rs_rich.console import Console
from rs_rich.errors import NotRenderableError
from rs_rich.panel import Panel

class Broken:
    def __rich_console__(self, console, options):
        yield "half"
        raise ValueError("no data")

console = Console(width=20)
try:
    console.print(Panel(Broken()))
except ValueError as error:
    print("ValueError:", error)
try:
    console.print(Panel(object()))
except NotRenderableError as error:
    print("NotRenderableError")
```

```text
ValueError: no data
NotRenderableError
```

Renderables nest at most 100 deep (panels in panels, or your own objects
yielding each other); deeper raises `RecursionError`, as Rich does at a
similar depth.

## The protocol's types

### ConsoleOptions

`rs_rich.console.ConsoleOptions`, what `__rich_console__` and
`__rich_measure__` receive, and `Console.options`:

| Attribute | Meaning |
|---|---|
| `min_width`, `max_width` | The width range to render in. |
| `height`, `max_height` | A fixed height (or `None`), and the most rows there are. |
| `justify`, `overflow`, `no_wrap` | Overrides for text (`None`: the text's own). |
| `highlight`, `markup` | Overrides for strings rendered inside (`None`: the console's). |
| `size` | The console's `(width, height)`. |
| `is_terminal`, `encoding`, `legacy_windows`, `ascii_only` | About the output. |

`update(**changes)` returns a copy with some of these changed (`width=`
sets both widths); `update_width(w)`, `update_height(h)`,
`update_dimensions(w, h)`, `reset_height()` and `copy()` do as in Rich.

### Measurement

`rs_rich.measure.Measurement(minimum, maximum)` behaves like Rich's named
tuple: it unpacks, compares with tuples, and has `span`, `normalize()`,
`with_maximum()`, `with_minimum()`, `clamp()` and
`Measurement.get(console, options, renderable)`.

### Segment

`rs_rich.segment.Segment(text, style=None, control=None)` is a piece of
text in one style. `Segment.line()` is a newline. A segment unpacks to
`(text, style, control)` and has `cell_length` and `is_control`.
`Console.render` returns segments, and `__rich_console__` may yield them.

```python
from rs_rich.console import Console
from rs_rich.text import Text

console = Console(width=20)
for segment in console.render(Text("hi", style="bold")):
    print(repr(segment.text), segment.style)
```

```text
'hi' bold
'\n' None
```
