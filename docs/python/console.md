# Console

```python
from rs_rich.console import Console
```

A `Console` renders objects and writes them to a file: `sys.stdout` unless
you give it another. It corresponds to `rich.console.Console`.

## Constructor

```text
Console(*, file=None, width=None, height=None, color_system="auto",
        force_terminal=None, no_color=None, record=False, highlight=True,
        emoji=True, safe_box=True)
```

Every argument is keyword-only.

| Argument | Default | Meaning |
|---|---|---|
| `file` | `None` | A text file to write to. `None` means `sys.stdout`, looked up at each print, so redirecting `sys.stdout` later still works. |
| `width` | `None` | Columns to render in, at most 65536; a larger width is a `ValueError`. `None`: for a file that is not a terminal, `$COLUMNS` (when it is at most 65536) or 80, as in Rich; on a terminal, its width. |
| `height` | `None` | Rows. `None`: the terminal's height. |
| `color_system` | `"auto"` | `"auto"` detects from the terminal. Otherwise `"standard"`, `"256"`, `"truecolor"`, `"windows"`, or `None` for no colour. Anything else is a `ValueError`. |
| `force_terminal` | `None` | Treat `file` as a terminal (`True`) or not (`False`). `None` asks `file.isatty()`. A file that is not a terminal gets no colour unless `color_system` names one. |
| `no_color` | `None` | Drop colours but keep bold, italic and the other attributes. `None` follows the `NO_COLOR` environment variable. |
| `record` | `False` | Keep everything printed, for [`export_text`](#export_text). |
| `highlight` | `True` | Highlight numbers, strings, `True`/`False`/`None`, URLs and other patterns in printed strings. |
| `emoji` | `True` | Replace `:emoji_codes:` in printed strings. |
| `safe_box` | `True` | Avoid box characters that legacy Windows consoles cannot draw. |

```python
import io
from rs_rich.console import Console

console = Console(file=io.StringIO())
print(console.width, console.is_terminal, console.color_system)

console = Console(file=io.StringIO(), width=40, force_terminal=True, color_system="256")
print(console.width, console.is_terminal, console.color_system)
```

```text
80 False None
40 True 256
```

## Properties

| Property | Type | Meaning |
|---|---|---|
| `file` | file | Where output goes: the file given, else the current `sys.stdout`. |
| `width` | `int` | Columns. |
| `height` | `int` | Rows. |
| `is_terminal` | `bool` | Whether output goes to a terminal (after `force_terminal`). |
| `color_system` | `str` or `None` | `"standard"`, `"256"`, `"truecolor"`, `"windows"`, or `None`. |

## print

```text
print(*objects, sep=" ", end="\n", justify=None)
```

This prints objects, each followed by a newline.

- A `str` is console markup: `[bold red]error[/]`. Markup, emoji codes and
  highlighting apply to it.
- An `int`, `float`, `bool` or `None` prints as its `str`.
- A `Text`, `Table` or `Panel` renders on its own lines.
- Consecutive strings, and the numbers among them, are joined with `sep` into
  one line.
- Long lines wrap at the console width.

```python
from rs_rich.console import Console
from rs_rich.text import Text

console = Console(width=30)
console.print("[bold]Hello[/], World!", 42, None)
console.print("a", "b", "c", sep="-")
console.print(Text("a Text object"))
console.print("one two three four five six seven eight")
```

```text
Hello, World! 42 None
a-b-c
a Text object
one two three four five six 
seven eight
```

`justify` (`"left"`, `"center"`, `"right"`, `"full"` or `"default"`) aligns
strings within the width:

```python
from rs_rich.console import Console

console = Console(width=20)
console.print("centred", justify="center")
console.print("right", justify="right")
```

```text
      centred       
               right
```

In colour, markup becomes ANSI styles:

```python
import io
from rs_rich.console import Console

out = io.StringIO()
Console(file=out, force_terminal=True, color_system="truecolor").print("[bold]b[/] [red]r[/]")
print(repr(out.getvalue()))
```

```text
'\x1b[1mb\x1b[0m \x1b[31mr\x1b[0m\n'
```

After writing, `print` calls `file.flush()`, as Rich does after every print,
so output appears at once even on a buffered file. A `file` therefore needs
a `flush` method as well as `write`: without one, `print` writes and then
raises `AttributeError`, as in Rich. `rule` flushes too.

**Errors:**
- Markup that does not parse raises `rs_rich.errors.MarkupError`.
- Errors raised by `file.write` or `file.flush` propagate.
- Other objects (a `dict`, a `list`, your own class) raise
  `NotImplementedError`.
- `end` other than `"\n"`, and `justify` with a non-string object, raise
  `NotImplementedError`.

## rule

```text
rule(title="", *, characters="─", style=None)
```

This draws a horizontal rule across the width, with an optional markup title
in the middle. `style` is a style string or [`Style`](style.md) for the line.

```python
from rs_rich.console import Console

console = Console(width=24)
console.rule()
console.rule("[bold]Section[/]")
console.rule(characters="=")
```

```text
────────────────────────
─────── Section ────────
========================
```

## export_text

```text
export_text(*, clear=True, styles=False) -> str
```

This returns everything printed since the console was created, or since the
last clearing export. It needs `record=True`, and raises `RuntimeError`
without it.

- `styles=False` returns plain text.
- `styles=True` keeps the ANSI codes.
- `clear=False` keeps the record for later exports.

```python
import io
from rs_rich.console import Console

console = Console(file=io.StringIO(), width=20, record=True)
console.print("[bold]recorded[/] output")
console.rule("end")
print(console.export_text(), end="")
print(repr(console.export_text()))
```

```text
recorded output
─────── end ────────
''
```

## The global console

`rs_rich.print` prints to a console shared by the whole program, which
`rs_rich.get_console()` returns. It is created on first use and writes to
`sys.stdout`.

```python
import rs_rich

rs_rich.print("[italic]from[/] the global console", 1)
print(rs_rich.get_console() is rs_rich.get_console())
```

```text
from the global console 1
True
```

## Threads

A `Console`, and every other `rs_rich` object, can be used from any thread,
including `rs_rich.print` from a worker thread. Each `print` or `rule` writes
all its output before another thread's print on the same console starts, so
lines from different threads never interleave. A thread waiting for its turn
releases the GIL.

A `print` to a console from inside that console's own `file.write` (or
`flush`) raises `RuntimeError` ("Console is already printing") instead of
waiting for itself.

```python
import io
import threading
from rs_rich.console import Console

console = Console(file=io.StringIO(), width=20)
workers = [threading.Thread(target=console.print, args=(f"worker {n}",)) for n in range(3)]
for worker in workers:
    worker.start()
for worker in workers:
    worker.join()
print(sorted(console.file.getvalue().splitlines()))
```

```text
['worker 0', 'worker 1', 'worker 2']
```
