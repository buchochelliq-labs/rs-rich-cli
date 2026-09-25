# Rules, padding, alignment and bars

```python
from rs_rich.align import Align, VerticalCenter
from rs_rich.bar import Bar
from rs_rich.constrain import Constrain
from rs_rich.padding import Padding
from rs_rich.rule import Rule
from rs_rich.spinner import SPINNERS, Spinner
from rs_rich.styled import Styled
```

The small renderables that shape other content. Each corresponds to the Rich
class of the same name and module (`rich.rule.Rule` and so on), renders byte
for byte as Rich 15.0.0 does, and works anywhere a renderable does:
`Console.print`, table cells, panels, [columns](layout.md#columns),
[groups](layout.md#group) and [trees](tree.md).

As in Rich, the objects keep what they were given and render it when printed,
so you can change an attribute (`rule.title = ...`, `padding.left = 2`) up to
then.

## Rule

```text
Rule(title="", *, characters="─", style="rule.line", end="\n", align="center")
```

A horizontal line across the width, with an optional title (a `str` of
markup, or a [`Text`](text.md)). `characters` must be at least one cell wide
and `align` one of `"left"`, `"center"` and `"right"`, or `ValueError` is
raised. On a console whose encoding is not UTF, a title's rule falls back to
`-`, as Rich's does. `Console.rule(...)` prints one.

```python
from rs_rich.console import Console

console = Console(width=30)
console.print(Rule())
console.print(Rule("[b]Section[/]"))
console.print(Rule("left", align="left", characters="="))
```

```text
──────────────────────────────
────────── Section ───────────
left =========================
```

A rule's `end` is written after it inside containers. At the top level
`Console.print` always ends the line, so `end=""` does not join the next
output onto it as it does in Rich.

## Padding

```text
Padding(renderable, pad=(0, 0, 0, 0), *, style="none", expand=True)
Padding.indent(renderable, level)
Padding.unpack(pad)
```

Space around a renderable. `pad` is CSS style: `n`, `(n,)`,
`(vertical, horizontal)` or `(top, right, bottom, left)`; other shapes raise
`ValueError` (`"1, 2 or 4 integers required for padding; 3 given"`), and so
does any side above 65536. `style` colours the padding and the content's
background. With `expand=False` the padding fits the content instead of
filling the width; `Padding.indent` is that with `level` cells on the left.

```python
console = Console(width=20)
console.print(Padding("padded", (1, 4), style="on blue"))
console.print(Padding.indent("indented", 4))
```

```text
                    
    padded          
                    
    indented
```

## Align

```text
Align(renderable, align="left", style=None, *, vertical=None, pad=True,
      width=None, height=None)
Align.left(...), Align.center(...), Align.right(...)
VerticalCenter(renderable, style=None)
```

`Align` places a renderable left, centred or right in the width, and with
`vertical` (`"top"`, `"middle"` or `"bottom"`) within `height` (or the height
it is given, as in a [layout](layout.md)). `pad=False` leaves the right side
unpadded, `width` limits the content and `style` colours the space around it.
`VerticalCenter` is Rich's older vertical centring, kept for compatibility.

```python
from rs_rich.panel import Panel

console = Console(width=24)
console.print(Align.center(Panel.fit("centred")))
console.print(Align.right("[i]right[/]", vertical="bottom", height=2))
```

```text
       ╭─────────╮      
       │ centred │      
       ╰─────────╯      
                        
                   right
```

## Constrain and Styled

```text
Constrain(renderable, width=80)
Styled(renderable, style)
```

`Constrain` renders within at most `width` cells (`None` for no limit).
`Styled` lays a style under everything a renderable draws; the renderable's
own styles win where they are set.

```python
console = Console(width=40)
console.print(Constrain(Panel("at most 16 cells wide"), 16))
```

```text
╭──────────────╮
│ at most 16   │
│ cells wide   │
╰──────────────╯
```

## Bar

```text
Bar(size, begin, end, *, width=None, color="default", bgcolor="default")
```

A solid block bar from `begin` to `end` out of `size`, with eighth-cell
precision at each edge; `width=None` fills the width.

```python
console = Console(width=20)
console.print(Bar(100, 25, 70))
```

```text
     █████████      
```

## Spinner

```text
Spinner(name, text="", *, style=None, speed=1.0)
spinner.render(time)
spinner.update(*, text="", style=None, speed=None)
SPINNERS
```

An animation: `render(time)` returns the frame for `time` seconds after the
first render (a `Text`, or, when `text` is another renderable, the frame
beside it). Printed, a spinner shows the frame for the console's
`get_time()`, so each refresh of a live display moves it on. `SPINNERS` is
Rich's table of `{name: {"interval": ms, "frames": [...]}}`; an unknown name
raises `KeyError`.

```python
spinner = Spinner("line", "loading", style="green")
print([spinner.render(t).plain for t in (0.0, 0.13, 0.26)])
print(len(SPINNERS), SPINNERS["dots"]["interval"])
```

```text
['- loading', '\\ loading', '| loading']
89 80
```

Unlike Rich, `render` returns a private grid renderable rather than a `Table`
when the text is not a `str` or `Text`; it prints the same.
