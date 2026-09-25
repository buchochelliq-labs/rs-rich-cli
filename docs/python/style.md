# Style

```python
from rs_rich.style import Style
```

A `Style` is a set of attributes, colours and a link. It corresponds to
`rich.style.Style`. Anywhere a `style=` argument is accepted, a style string
such as `"bold red on white"` works too.

## Constructor

```text
Style(*, color=None, bgcolor=None, bold=None, dim=None, italic=None,
      underline=None, blink=None, reverse=None, conceal=None, strike=None,
      link=None)
```

- **Colours.** `color` and `bgcolor` take any colour Rich understands: a
  name (`"red"`, `"bright_black"`), `"#rrggbb"`, `"rgb(r,g,b)"` or
  `"color(208)"`.
- **Attributes.** An attribute set to `True` turns on, `False` turns off (it
  overrides a style underneath), and `None` leaves it alone.
- **Links.** `link` is a URL, drawn as a terminal hyperlink.
- **Errors.** An unknown colour raises `rs_rich.errors.StyleSyntaxError`,
  a `ConsoleError`, as in Rich.

A style is immutable, and `str(style)` gives its definition:

```python
from rs_rich.style import Style

print(Style(bold=True, color="red"))
print(Style(italic=False, bgcolor="#102030"))
print(Style())
```

```text
bold red
not italic on #102030
none
```

## Parsing

```text
Style.parse(definition) -> Style
```

This parses a style definition, the same grammar markup tags use. A
definition that does not parse raises `StyleSyntaxError`.

```python
from rs_rich.style import Style

print(Style.parse("bold not dim cyan on bright_black"))
print(Style.parse("bold red") == Style(bold=True, color="red"))
```

```text
bold not dim cyan on bright_black
True
```

## Combining

`a + b` combines two styles. `b` wins where both set the same attribute or
colour:

```python
from rs_rich.style import Style

combined = Style(bold=True, color="red") + Style(color="blue")
print(combined == Style.parse("bold blue"))
```

```text
True
```

## Hashing

Styles are hashable, as in Rich. Equal styles hash alike, however they were
written, so a style can key a `dict` or go in a `set`:

```python
from rs_rich.style import Style

names = {Style.parse("bold red"): "alert"}
print(names[Style(color="red", bold=True)])
print(len({Style.parse("bold"), Style(bold=True)}))
```

```text
alert
1
```

## Using styles

```python
import io
from rs_rich.console import Console
from rs_rich.style import Style
from rs_rich.text import Text

out = io.StringIO()
console = Console(file=out, force_terminal=True, color_system="truecolor")
console.print(Text("styled", style=Style(bold=True, color="#ff8800")))
print(repr(out.getvalue()))
```

```text
'\x1b[1;38;2;255;136;0mstyled\x1b[0m\n'
```
