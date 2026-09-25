# Boxes, markup and errors

## Boxes

```python
from rs_rich import box
```

`rs_rich.box` has Rich's box styles, for the `box=` argument of
[`Table`](table.md) and [`Panel`](panel.md):

| Constant | Top-left corner and edges |
|---|---|
| `ASCII`, `ASCII2`, `ASCII_DOUBLE_HEAD` | `+-` |
| `SQUARE`, `SQUARE_DOUBLE_HEAD` | `┌─` |
| `MINIMAL`, `MINIMAL_HEAVY_HEAD`, `MINIMAL_DOUBLE_HEAD` | inner lines only |
| `SIMPLE`, `SIMPLE_HEAD`, `SIMPLE_HEAVY` | horizontal lines under the header |
| `HORIZONTALS` | horizontal lines only |
| `ROUNDED` | `╭─` |
| `HEAVY`, `HEAVY_EDGE`, `HEAVY_HEAD` | `┏━` |
| `DOUBLE`, `DOUBLE_EDGE` | `╔═` |
| `MARKDOWN` | a Markdown table |

A box is an opaque value. Its `repr` is its name (`box.ROUNDED`), where Rich's
is `Box(...)` with the box's characters. Pass one of these constants; anything
else raises `TypeError`.

```python
from rs_rich import box
from rs_rich.console import Console
from rs_rich.table import Table

console = Console(width=30)
for style in [box.ASCII, box.MARKDOWN]:
    table = Table("a", "b", box=style)
    table.add_row("1", "2")
    console.print(table)
print(repr(box.ROUNDED))
```

```text
+-------+
| a | b |
|---+---|
| 1 | 2 |
+-------+
         
| a | b |
|---|---|
| 1 | 2 |
         
box.ROUNDED
```

## Markup

Printed strings, `Text.from_markup`, table cells and headers, and panel and
table titles are console markup: `[bold]`, `[red on white]`,
`[link=https://example.com]` and `[/]` to close, with the same grammar as
Rich.

```python
from rs_rich.markup import escape
```

`escape(text)` backslash-escapes the `[` of anything that looks like a tag, so
that text from outside prints literally:

```python
from rs_rich.console import Console
from rs_rich.markup import escape

user_input = "[bold]not a tag[/bold]"
print(escape(user_input))
Console(width=40).print(escape(user_input))
```

```text
\[bold]not a tag\[/bold]
[bold]not a tag[/bold]
```

## Errors

```python
from rs_rich.errors import ConsoleError, MarkupError, StyleSyntaxError
```

| Exception | Base | Raised when |
|---|---|---|
| `ConsoleError` | `Exception` | Never raised itself; the base of the two below, as in Rich. |
| `MarkupError` | `ConsoleError` | Markup does not parse, for example a closing tag with no opening one. |
| `StyleSyntaxError` | `ConsoleError` | A style definition or colour does not parse. |

Other errors are Python's own:

| Exception | Raised when |
|---|---|
| `ValueError` | An argument has an invalid value, such as `justify="middle"` or a table row with more cells than columns. |
| `TypeError` | An argument has the wrong type, such as a `style` that is not a string or `Style`, or a `box` that is not a box constant. |
| `NotImplementedError` | Something Rich supports that this version does not yet. It is raised instead of rendering differently from Rich. |
| `RuntimeError` | `export_text` on a console created without `record=True`. |

```python
from rs_rich.console import Console
from rs_rich.errors import ConsoleError, MarkupError

try:
    Console().print("[/bold]")
except MarkupError as error:
    print(type(error).__name__, isinstance(error, ConsoleError), isinstance(error, ValueError))
```

```text
MarkupError True False
```
