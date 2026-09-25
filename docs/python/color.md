# Color

```python
from rs_rich.color import Color, ColorSystem, ColorTriplet, ColorType
```

`rs_rich.color` corresponds to `rich.color`. As in Rich, `Color` and
`ColorTriplet` are named tuples and `ColorSystem` and `ColorType` are
`IntEnum`s, so they unpack and compare like Rich's; parsing, downgrading and
escape codes are done by the Rust port.

## Color

```text
Color(name, type, number=None, triplet=None)
```

| Member | Meaning |
|---|---|
| `Color.parse(color)` | A name (`"red"`), `"default"`, `"#rrggbb"`, `"color(N)"` or `"rgb(r,g,b)"`, case-insensitive. Anything else raises `ColorParseError` with Rich's message. |
| `Color.from_ansi(number)`, `Color.from_rgb(red, green, blue)`, `Color.from_triplet(triplet)`, `Color.default()` | Other constructors. |
| `system`, `is_system_defined`, `is_default` | The colour system that can show the colour, whether the terminal defines it, and whether it is the default colour. |
| `get_truecolor(theme=None, foreground=True)` | The RGB `ColorTriplet` under a `TerminalTheme` (default: the default theme). |
| `get_ansi_codes(foreground=True)` | The SGR parameters, as a tuple of strings. |
| `downgrade(system)` | The nearest colour in a smaller `ColorSystem`. |

```python
from rs_rich.color import Color, ColorSystem

color = Color.parse("#ff8800")
print(repr(color))
print(color.system, color.get_ansi_codes())
for system in [ColorSystem.EIGHT_BIT, ColorSystem.STANDARD, ColorSystem.WINDOWS]:
    print(repr(color.downgrade(system)))
print(repr(Color.parse("red").get_truecolor()))
```

```text
```

## ColorTriplet, ColorSystem and ColorType

`ColorTriplet(red, green, blue)` has `hex`, `rgb` and `normalized`.
`ColorSystem` is `STANDARD`, `EIGHT_BIT`, `TRUECOLOR` or `WINDOWS`;
`ColorType` adds `DEFAULT`. The module also has Rich's `parse_rgb_hex` and
`blend_rgb`.

```python
from rs_rich.color import ColorTriplet, blend_rgb, parse_rgb_hex

triplet = ColorTriplet(255, 136, 0)
print(triplet.hex, triplet.rgb, triplet.normalized)
print(parse_rgb_hex("102030"), blend_rgb((0, 0, 0), (255, 255, 255)))
```

```text
```

A `Color` prints as a sample, as in Rich:

```python
from rs_rich.color import Color
from rs_rich.console import Console

Console(width=40).print(Color.parse("magenta"))
```

```text
```
