# Text

```python
from rs_rich.text import Text
```

A `Text` is a string with styled spans. It corresponds to `rich.text.Text`.
Offsets are Python character offsets, as in Rich, so `stylize("bold", 0, 2)`
covers two characters whatever their encoding.

## Constructor

```text
Text(text="", style=None, *, justify=None, overflow=None, no_wrap=None)
```

| Argument | Meaning |
|---|---|
| `text` | The plain string. It is not parsed as markup; see [`from_markup`](#from_markup). |
| `style` | A style for the whole text: a style string (`"bold red"`), a theme name, or a [`Style`](style.md). |
| `justify` | `"left"`, `"center"`, `"right"`, `"full"` or `"default"`. |
| `overflow` | What a line too long for its space does: `"fold"`, `"crop"`, `"ellipsis"` or `"ignore"`. |
| `no_wrap` | `True` to keep each line whole, then apply `overflow`. |

An invalid `justify` or `overflow` raises `ValueError`, and a `style` that is
neither a string nor a `Style` raises `TypeError`.

`justify`, `overflow` and `no_wrap` apply where the text is laid out in a
space of its own, such as a panel or a table cell. A `Text` printed by itself
takes the console's settings, as in Rich:

```python
from rs_rich.console import Console
from rs_rich.panel import Panel
from rs_rich.text import Text

console = Console(width=12)
text = Text("abcdefghijklmnop", overflow="ellipsis", no_wrap=True)
console.print(text)
console.print(Panel(text))
console.print(Panel(Text("right", justify="right")))
```

```text
abcdefghijkl
mnop
╭──────────╮
│ abcdefg… │
╰──────────╯
╭──────────╮
│    right │
╰──────────╯
```

## from_markup

```text
Text.from_markup(text, *, style=None, justify=None) -> Text
```

This parses console markup into a `Text`. `style` is a base style under the
markup's own. Markup that does not parse raises `rs_rich.errors.MarkupError`.

```python
from rs_rich.text import Text

text = Text.from_markup("[bold]bold[/] and [italic]italic[/]")
print(text.plain)
```

```text
bold and italic
```

## Methods and properties

| Member | Meaning |
|---|---|
| `plain` | The string without styles. |
| `len(text)` | The number of characters. |
| `str(text)` | The same as `plain`. |
| `append(text, style=None)` | Append a `str` (with an optional style) or another `Text` (keeping its spans; a `style` is then a `ValueError`). Returns the text itself, so calls chain. Anything else is a `TypeError`. |
| `stylize(style, start=0, end=None)` | Apply a style to characters `start` up to `end`. Negative offsets count from the end, `end=None` means the end, and offsets out of range are clamped. |

```python
import io
from rs_rich.console import Console
from rs_rich.text import Text

text = Text("Hello")
text.append(", World", style="bold").append("!")
text.stylize("italic", 0, 5)
text.stylize("underline", -1)
print(text.plain, len(text))

out = io.StringIO()
Console(file=out, force_terminal=True, color_system="truecolor").print(text)
print(repr(out.getvalue()))
```

```text
Hello, World! 13
'\x1b[3mHello\x1b[0m\x1b[1m, World\x1b[0m\x1b[4m!\x1b[0m\n'
```

Offsets count characters, not bytes:

```python
from rs_rich.text import Text

text = Text("日本語 text")
text.stylize("bold", 0, 2)
print(len(text))
```

```text
8
```
