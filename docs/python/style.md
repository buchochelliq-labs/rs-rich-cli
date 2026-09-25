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
      underline=None, blink=None, blink2=None, reverse=None, conceal=None,
      strike=None, underline2=None, frame=None, encircle=None, overline=None,
      link=None, meta=None)
```

- **Colours.** `color` and `bgcolor` take a [`Color`](color.md) or any
  colour string Rich understands: a name (`"red"`, `"bright_black"`),
  `"#rrggbb"`, `"rgb(r,g,b)"` or `"color(208)"`.
- **Attributes.** An attribute set to `True` turns on, `False` turns off (it
  overrides a style underneath), and `None` leaves it alone.
- **Links.** `link` is a URL, drawn as a terminal hyperlink.
- **Meta.** `meta` is a dict of data for applications (Textual's event
  handlers). It takes part in `==` and `repr`; it has no effect on output.
- **Errors.** An unknown colour raises `rs_rich.color.ColorParseError`, as
  in Rich; in `Style.parse` it raises `rs_rich.errors.StyleSyntaxError`, a
  `ConsoleError`.

A style is immutable. `str(style)` gives its definition and `repr(style)`
Rich's repr:

```python
from rs_rich.style import Style

print(Style(bold=True, color="red"))
print(Style(italic=False, bgcolor="#102030"))
print(Style())
print(repr(Style(bold=True, color="red")))
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

## Properties and methods

| Member | Meaning |
|---|---|
| `bold`, `dim`, `italic`, `underline`, `blink`, `blink2`, `reverse`, `conceal`, `strike`, `underline2`, `frame`, `encircle`, `overline` | `True`, `False` or `None` (unset). |
| `color`, `bgcolor` | The [`Color`](color.md)s, or `None`. |
| `link`, `link_id`, `meta` | The link URL, its id, and a copy of the meta data. |
| `transparent_background`, `background_style`, `without_color` | Whether the background is unset or default; a style with only the background; a copy without colours. |
| `bool(style)` | `False` for a style that sets nothing. |
| `Style.null()`, `Style.from_color(color=None, bgcolor=None)`, `Style.from_meta(meta)`, `Style.on(meta=None, **handlers)` | Other constructors. |
| `Style.normalize(definition)`, `Style.pick_first(*values)`, `Style.combine(styles)`, `Style.chain(*styles)` | Class helpers, as in Rich. |
| `copy()`, `clear_meta_and_links()`, `update_link(link=None)` | Modified copies. |
| `render(text="", *, color_system=ColorSystem.TRUECOLOR, legacy_windows=False)` | `text` wrapped in the style's escape codes. |
| `get_html_style(theme=None)` | CSS for the style under a `TerminalTheme`. |
| `test(text=None)` | Write the styled text (default: the definition) to stdout. |

```python
from rs_rich.color import ColorSystem
from rs_rich.style import Style

style = Style.parse("bold #ff8800 on blue")
print(style.bold, style.italic, repr(style.color))
print(repr(style.render("hot", color_system=ColorSystem.STANDARD)))
print(style.get_html_style())
print(repr(Style.combine([Style(bold=True), Style(color="red"), Style(bold=False)])))
```

```text
```

## StyleStack

`StyleStack(default_style)` keeps a stack where each pushed style is combined
with the one below: `push(style)`, `pop()` (returns the new current style) and
`current`.

## Themes

```text
Theme(styles=None, inherit=True)
Theme.from_file(config_file, source=None, inherit=True)
Theme.read(path, inherit=True, encoding=None)
```

A `Theme` maps style names to styles for `Console(theme=...)`,
`push_theme` and `use_theme`. `styles` is a new dict each time, in Rich's
order (the default styles first when inherited); `config` is the theme as a
config file. `from_file` and `read` read a `[styles]` section with Python's
`configparser`, as Rich does, so a bad file raises the same errors.

`ThemeStack(theme)` has Rich's `push_theme(theme, inherit=True)`,
`pop_theme()` (a `ThemeStackError` for the base theme) and `get(name,
default=None)`.

```python
import io
from rs_rich.theme import Theme, ThemeStack

theme = Theme.from_file(io.StringIO("[styles]\nwarning = bold red\n"), inherit=False)
print(theme.config)
stack = ThemeStack(theme)
stack.push_theme(Theme({"info": "dim cyan"}, inherit=False))
print(stack.get("warning"), "/", stack.get("info"))
```

```text
```
