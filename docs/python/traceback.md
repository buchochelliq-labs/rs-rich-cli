# Traceback

```python
from rs_rich.traceback import Traceback
```

`Traceback` renders a Python exception as Rich does: each frame's file, line
and function, the code around the failing line with the line marked and the
failing expression underlined, optionally the frame's local variables, then
the exception, its notes, exception groups and the chain of causes.

```python
from rs_rich.console import Console

def ratio(total, count):
    return total / count

console = Console(width=64)
try:
    ratio(1, 0)
except ZeroDivisionError:
    console.print(Traceback(extra_lines=1))
```

```text
╭───────────── Traceback (most recent call last) ──────────────╮
│ in <module>:8                                                │
│ in ratio:4                                                   │
╰──────────────────────────────────────────────────────────────╯
ZeroDivisionError: division by zero
```

## Constructor

```text
Traceback(trace=None, *, width=100, code_width=88, extra_lines=3, theme=None,
          word_wrap=False, show_locals=False, locals_max_length=10,
          locals_max_string=80, locals_max_depth=None,
          locals_hide_dunder=True, locals_hide_sunder=False,
          locals_overlow=None, indent_guides=True, suppress=(),
          max_frames=100)
```

`trace=None` uses the exception being handled (outside an `except` block it
is a `ValueError`). The arguments are Rich's, including the misspelt
`locals_overlow`:

| Argument | Meaning |
|---|---|
| `width`, `code_width` | The traceback's width and the code's (`None`: all available). |
| `extra_lines` | Lines of code shown around the failing line. |
| `theme` | The code theme (see [Syntax](syntax.md#themes)); `None` is `ansi_dark`. |
| `word_wrap` | Wrap long code lines. |
| `show_locals` and the `locals_` options | Show each frame's local variables, [pretty-printed](pretty.md) with these limits. |
| `indent_guides` | Indent guides in code and locals. |
| `suppress` | Modules or paths whose frames show no code. |
| `max_frames` | Show at most this many frames (at least 4), eliding the middle; `0` shows all. |

`Traceback.from_exception(exc_type, exc_value, traceback, **options)` builds
one for any exception (it spells the option `locals_overflow`), and
`Traceback.extract(exc_type, exc_value, traceback, *, show_locals=False, ...)`
returns the `Trace` it renders: `Trace.stacks` is a list of `Stack`
(`exc_type`, `exc_value`, `frames`, `notes`, `is_cause`, `is_group`,
`exceptions`, `syntax_error`), and each `Frame` has `filename`, `lineno`,
`name`, `locals` (names to [`Node`](pretty.md#pretty-printing)s) and
`last_instruction`.

```python
try:
    ratio(1, 0)
except ZeroDivisionError as error:
    trace = Traceback.extract(type(error), error, error.__traceback__, show_locals=True)
stack = trace.stacks[0]
print(stack.exc_type, stack.exc_value)
print([frame.name for frame in stack.frames], str(stack.frames[-1].locals["count"]))
```

```text
ZeroDivisionError division by zero
['<module>', 'ratio'] 0
```

## Printing exceptions

`Console.print_exception(*, width=100, extra_lines=3, theme=None,
word_wrap=False, show_locals=False, suppress=(), max_frames=100)` prints the
exception being handled. `rs_rich.traceback.install(*, console=None,
width=100, ...)` prints every uncaught exception this way (to a
`Console(stderr=True)` by default) and returns the previous
`sys.excepthook`. IPython's traceback display is not replaced.

## Differences from Rich

Plain output is Rich's byte for byte. The code's colours are the port's
code highlighter's (see [Syntax](syntax.md#colours)); everything else (the
border, title, locals, exception line) is coloured as in Rich. A frame whose
code and locals are too wide to sit side by side shows the locals below the
code, as Rich does; when they fit side by side, the gap between them is one
column, as in Rich.
