# Pretty, JSON, inspect and highlighters

```python
from rs_rich.pretty import Pretty, pprint, pretty_repr
```

## Pretty printing

`Console.print` pretty-prints containers, dataclasses, named tuples, attrs
classes and objects with `__rich_repr__`, as Rich does: on one line when it
fits, otherwise expanded one item per line.

```python
from dataclasses import dataclass

from rs_rich.console import Console

@dataclass
class Point:
    x: int
    y: int

console = Console(width=40)
console.print({"name": "rs_rich", "points": [Point(1, 2), Point(3, 4)], "ok": True})
```

```text
{
    'name': 'rs_rich',
    'points': [
        Point(x=1, y=2),
        Point(x=3, y=4)
    ],
    'ok': True
}
```

`Pretty(obj)` is the renderable behind that, with Rich's options:

```text
Pretty(_object, highlighter=None, *, indent_size=4, justify=None,
       overflow=None, no_wrap=False, indent_guides=False, max_length=None,
       max_string=None, max_depth=None, expand_all=False, margin=0,
       insert_line=False)
```

| Argument | Meaning |
|---|---|
| `highlighter` | A highlighter (any callable taking and returning a `Text`); `None` is `ReprHighlighter()`. |
| `indent_size`, `indent_guides` | Indentation of expanded containers, and `│` guides in it. |
| `max_length`, `max_string`, `max_depth` | Abbreviate containers longer than `max_length` (`... +3`), strings longer than `max_string` (`'abc'+7`), and nesting deeper than `max_depth` (`[...]`). |
| `expand_all` | Expand every container. |
| `margin` | Cells to subtract from the width, so containers expand earlier. |
| `insert_line` | Start on a new line when the output is several lines. |

```python
console.print(Pretty(list(range(20)), max_length=5))
console.print(Pretty({"nested": {"deeper": [1, 2]}}, max_depth=1))
console.print(Pretty({"a": [1, 2], "b": "text"}, expand_all=True, indent_guides=True))
```

```text
[0, 1, 2, 3, 4, ... +15]
{'nested': {...}}
{
│   'a': [
│   │   1,
│   │   2
│   ],
│   'b': 'text'
}
```

`pretty_repr(obj, *, max_width=80, indent_size=4, max_length=None,
max_string=None, max_depth=None, expand_all=False)` returns the text without
styles; `pprint(obj, *, console=None, indent_guides=True, ...)` prints it.
`traverse(obj, max_length=None, max_string=None, max_depth=None)` returns the
tree of `Node`s these lay out, and `Node.render(max_width=80, indent_size=4,
expand_all=False)` lays out a tree.

```python
from rs_rich.pretty import traverse

print(pretty_repr({"key": ["a", "b", "c"]}, max_width=12))
node = traverse((1, "two"))
print(str(node), node.children[1].value_repr)
```

```text
{
    'key': [
        'a',
        'b',
        'c'
    ]
}
(1, 'two') 'two'
```

`rs_rich.pretty.install(console=None, overflow="ignore", crop=False,
indent_guides=False, max_length=None, max_string=None, max_depth=None,
expand_all=False)` makes the Python REPL pretty-print its results. (IPython's
formatter is not replaced.)

## JSON

```python
from rs_rich.json import JSON

console.print(JSON('{"name": "rs_rich", "tags": ["fast", "rust"], "stable": false}'))
console.print(JSON.from_data({"compact": [1, 2]}, indent=None))
```

```text
{
  "name": "rs_rich",
  "tags": [
    "fast",
    "rust"
  ],
  "stable": false
}
{"compact": [1, 2]}
```

`JSON(json, indent=2, highlight=True, skip_keys=False, ensure_ascii=False,
check_circular=True, allow_nan=True, default=None, sort_keys=False)` and
`JSON.from_data(data, ...)` encode with Python's `json` and highlight with
`JSONHighlighter`, as Rich does. `JSON.text` is a copy of the highlighted
`Text`.

## inspect

`rs_rich.inspect(obj, *, console=None, title=None, help=False, methods=False,
docs=True, private=False, dunder=False, sort=True, all=False, value=True)`
prints a report on any object; `rs_rich._native.Inspect(obj, ...)` is the
renderable (Rich's `rich._inspect.Inspect`).

```python
import rs_rich

class Account:
    """A bank account."""

    def __init__(self):
        self.owner = "Ada"
        self.balance = 10.5

    def __repr__(self):
        return "Account()"

rs_rich.inspect(Account(), console=Console(width=40))
```

```text
╭─ <class 'Account'> ─╮
│ A bank account.     │
│                     │
│ ╭─────────────────╮ │
│ │ Account()       │ │
│ ╰─────────────────╯ │
│                     │
│ balance = 10.5      │
│   owner = 'Ada'     │
╰─────────────────────╯
```

## Highlighters

```python
from rs_rich.highlighter import RegexHighlighter, ReprHighlighter
```

`ReprHighlighter`, `JSONHighlighter`, `ISO8601Highlighter` and
`NullHighlighter` are Rich's, with Rich's patterns. A highlighter is called
with a `str` or `Text` and returns a highlighted copy. Subclass
`RegexHighlighter` with `highlights` (Python regular expressions with named
groups) and a `base_style`, or `Highlighter` with a `highlight(text)` method:

```python
from rs_rich.theme import Theme

class EmailHighlighter(RegexHighlighter):
    base_style = "example."
    highlights = [r"(?P<email>[\w-]+@([\w-]+\.)+[\w-]+)"]

import io

out = io.StringIO()
themed = Console(file=out, force_terminal=True, color_system="standard",
                 theme=Theme({"example.email": "bold magenta"}))
themed.print(EmailHighlighter()("Send to ada@example.com today"))
print(repr(out.getvalue()))
```

```text
'Send to \x1b[1;35mada@example.com\x1b[0m today\n'
```

`Console(highlighter=...)` makes a highlighter the console's: it then
highlights every printed string (and the strings in tables, panels and other
containers), in place of `ReprHighlighter`.

```python
out = io.StringIO()
console = Console(file=out, force_terminal=True, color_system="standard",
                  theme=Theme({"example.email": "bold magenta"}),
                  highlighter=EmailHighlighter())
console.print("Mail ada@example.com about 42 things")
print(repr(out.getvalue()))
```

```text
'Mail \x1b[1;35mada@example.com\x1b[0m about 42 things\n'
```
