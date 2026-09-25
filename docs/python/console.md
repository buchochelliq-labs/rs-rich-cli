# Console

```python
from rs_rich.console import Console
```

A `Console` renders objects and writes them to a file: `sys.stdout` unless
you give it another. It corresponds to `rich.console.Console`, and its
methods take Rich's arguments. Everything it renders is rendered by the Rust
port; the console only converts arguments and writes the result.

## Constructor

```text
Console(*, color_system="auto", force_terminal=None, force_jupyter=None,
        force_interactive=None, soft_wrap=False, theme=None, stderr=False,
        file=None, quiet=False, width=None, height=None, style=None,
        no_color=None, tab_size=8, record=False, markup=True, emoji=True,
        emoji_variant=None, highlight=True, log_time=True, log_path=True,
        log_time_format="[%X]", highlighter=None, legacy_windows=None,
        safe_box=True, get_datetime=None, get_time=None)
```

Every argument is keyword-only.

| Argument | Default | Meaning |
|---|---|---|
| `file` | `None` | A text file to write to. `None` means `sys.stdout` (or `sys.stderr` with `stderr=True`), looked up at each print, so redirecting `sys.stdout` later still works. |
| `width` | `None` | Columns to render in, at most 65536; a larger width is a `ValueError`. `None`: for a file that is not a terminal, `$COLUMNS` (when it is at most 65536) or 80, as in Rich; on a terminal, the standard streams' size, then `$COLUMNS`, then 80. |
| `height` | `None` | Rows. `None`: `$LINES` or 25 off a terminal; on one, the terminal's height. |
| `color_system` | `"auto"` | `"auto"` detects from the terminal. Otherwise `"standard"`, `"256"`, `"truecolor"`, `"windows"`, or `None` for no colour. Anything else is a `ValueError`. |
| `force_terminal` | `None` | Treat `file` as a terminal (`True`) or not (`False`). `None` asks `file.isatty()`. A file that is not a terminal gets no colour unless `color_system` names one. |
| `force_interactive` | `None` | Override `is_interactive` (a terminal that is not dumb). |
| `no_color` | `None` | Drop colours but keep bold, italic and the other attributes. `None` follows the `NO_COLOR` environment variable. |
| `record` | `False` | Keep everything printed, for [`export_text`, `export_html` and `export_svg`](#recording-and-export). |
| `markup`, `emoji`, `highlight` | `True` | Read console markup, replace `:emoji_codes:`, and highlight numbers, strings, `True`/`False`/`None`, URLs and other patterns in printed strings. |
| `highlighter` | `None` | What highlights printed strings: any [highlighter](pretty.md#highlighters) (a `RegexHighlighter` subclass, `NullHighlighter`, or a callable taking and returning a `Text`). `None` is Rich's `ReprHighlighter`. Also the `highlighter` property (settable). |
| `tab_size` | `8` | Spaces per tab for printed text. |
| `emoji_variant` | `None` | `"emoji"` or `"text"`: the variant `:emoji:` codes get by default (on markup without tags, as in Rich). |
| `soft_wrap` | `False` | The default for `print(soft_wrap=...)`. |
| `style` | `None` | A style applied under everything printed. |
| `theme` | `None` | A [`Theme`](#themes) of named styles. `None`: Rich's default theme. |
| `stderr` | `False` | Write to `sys.stderr` when no `file` is given. |
| `quiet` | `False` | Print nothing. |
| `log_time`, `log_path`, `log_time_format` | `True`, `True`, `"[%X]"` | The columns [`log`](#log) shows, and how it formats the time (a `strftime` format, or a callable taking a `datetime` and returning a `Text`). |
| `get_datetime`, `get_time` | `None` | The clocks `log` and animations read (`datetime.now`, `time.monotonic`). |
| `safe_box` | `True` | Avoid box characters that legacy Windows consoles cannot draw. |
| `legacy_windows` | `None` | Legacy Windows mode; `None` is `False`. |
| `force_jupyter` | `None` | `True` raises `NotImplementedError`: there is no Jupyter output. |

```python
import io
from rs_rich.console import Console

console = Console(file=io.StringIO())
print(console.width, console.height, console.is_terminal, console.color_system)

console = Console(file=io.StringIO(), width=40, force_terminal=True, color_system="256")
print(console.width, console.is_terminal, console.color_system)
```

```text
80 25 False None
40 True 256
```

## Properties

| Property | Type | Meaning |
|---|---|---|
| `file` | file | Where output goes. Settable. |
| `width`, `height` | `int` | Columns and rows. Settable. |
| `size` | `ConsoleDimensions` | `(width, height)` as a named tuple. Settable. |
| `options` | [`ConsoleOptions`](protocol.md#consoleoptions) | The default render options. |
| `is_terminal` | `bool` | Whether output goes to a terminal (after `force_terminal`). |
| `is_dumb_terminal` | `bool` | A terminal whose `TERM` is `dumb` or `unknown`. |
| `is_interactive` | `bool` | A terminal that is not dumb, unless `force_interactive` says otherwise. Settable. |
| `color_system` | `str` or `None` | `"standard"`, `"256"`, `"truecolor"`, `"windows"`, or `None`. |
| `encoding` | `str` | The file's encoding, lower case; `"utf-8"` when it has none. |
| `no_color`, `legacy_windows`, `safe_box`, `tab_size`, `stderr`, `is_alt_screen` | | As given (or detected). |
| `quiet`, `soft_wrap`, `record` | `bool` | As given. Settable. |
| `get_time`, `get_datetime` | callable | The clocks. |

## print

```text
print(*objects, sep=" ", end="\n", style=None, justify=None, overflow=None,
      no_wrap=None, emoji=None, markup=None, highlight=None, width=None,
      height=None, crop=True, soft_wrap=None, new_line_start=False)
```

This prints objects as Rich does:

- A `str` is console markup: `[bold red]error[/]`. Markup, emoji codes and
  highlighting apply to it, unless `markup`, `emoji` or `highlight` say
  otherwise for this call.
- Strings, [`Text`](text.md) objects, and anything else printed as its `str`
  (numbers, `None`) are joined with `sep` into one text, which ends with
  `end`.
- Any other renderable ([`Table`](table.md), [`Panel`](panel.md), or your own
  class, see [The render protocol](protocol.md)) prints on its own lines.
- Containers, dataclasses, `__rich_repr__` objects and the others Rich
  pretty-prints print with [`Pretty`](pretty.md), as in Rich.
- An [`Emoji`](text.md) prints as one segment with no newline after it, so
  consecutive emoji share a line, as in Rich.

```python
from rs_rich.console import Console
from rs_rich.panel import Panel
from rs_rich.text import Text

console = Console(width=30)
console.print("[bold]Hello[/], World!", 42, None)
console.print("a", Text("b"), "c", sep="-", end="!\n")
console.print("before", Panel.fit("boxed"), "after")
console.print("one two three four five six seven eight")
```

```text
Hello, World! 42 None
a-b-c!
before
╭───────╮
│ boxed │
╰───────╯
after
one two three four five six 
seven eight
```

`justify` (`"left"`, `"center"`, `"right"`, `"full"` or `"default"`) aligns
the text within the width; left, center and right also align other
renderables, as Rich wraps them in `Align`. `overflow` (`"fold"`, `"crop"`,
`"ellipsis"`, `"ignore"`) and `no_wrap` control long lines; `width` narrows
the output; `crop=False` lets lines run past the width; `soft_wrap=True`
does all of that at once (no wrapping, no cropping), as Rich does.

```python
from rs_rich.console import Console
from rs_rich.panel import Panel

console = Console(width=20)
console.print("centred", justify="center")
console.print(Panel.fit("right"), justify="right")
console.print("a long line that will not fit", overflow="ellipsis", no_wrap=True)
console.print("narrow words here", width=8)
```

```text
      centred       
           ╭───────╮
           │ right │
           ╰───────╯
a long line that wi…
narrow 
words 
here
```

`style` applies a style under everything printed, and `new_line_start`
starts on a fresh line when the output has more than one line. In colour,
markup and styles become ANSI codes:

```python
import io
from rs_rich.console import Console

out = io.StringIO()
console = Console(file=out, force_terminal=True, color_system="truecolor")
console.print("[bold]b[/] [red]r[/]")
console.print("styled", style="italic")
print(repr(out.getvalue()))
```

```text
'\x1b[1mb\x1b[0m \x1b[31mr\x1b[0m\n\x1b[3mstyled\x1b[0m\n'
```

After writing, `print` calls `file.flush()`, as Rich does after every print,
so output appears at once even on a buffered file. A `file` therefore needs
a `flush` method as well as `write`: without one, `print` writes and then
raises `AttributeError`, as in Rich.

**Errors:**
- Markup that does not parse raises `rs_rich.errors.MarkupError`.
- An object that is not renderable raises `rs_rich.errors.NotRenderableError`.
- Errors raised by `file.write` or `file.flush`, or by your own renderable's
  `__rich__`, `__rich_console__` or `__rich_measure__`, propagate, and
  nothing is printed.

## out, line and rule

```text
out(*objects, sep=" ", end="\n", style=None, highlight=None)
line(count=1)
rule(title="", *, characters="─", style="rule.line", align="center")
```

`out` writes the objects' `str` with no markup, emoji, wrapping or cropping
(highlighting and `style` still apply). `line` writes blank lines. `rule`
draws a horizontal rule with an optional markup title; `style` may name a
theme style.

```python
from rs_rich.console import Console

console = Console(width=24)
console.out("[not markup]", 1)
console.line()
console.rule("[bold]Section[/]")
console.rule("left", align="left", characters="=")
```

```text
[not markup] 1

─────── Section ────────
left ===================
```

## log

```text
log(*objects, sep=" ", end="\n", style=None, justify=None, emoji=None,
    markup=None, highlight=None, log_locals=False, _stack_offset=1)
```

This prints the objects like `print`, with the time on the left and the
calling file and line on the right, as Rich's `Console.log` does. A time
equal to the previous record's is left blank. The path links to the file
(OSC 8), without Rich's random link id. The objects may be any renderables
(a table, a panel, a dict pretty-printed); `log_locals=True` adds a panel of
the caller's local variables, as Rich's `render_scope` draws it.

```python
import datetime
from rs_rich.console import Console

console = Console(width=40, log_path=False,
                  get_datetime=lambda: datetime.datetime(2026, 9, 25, 12, 0, 0))
console.log("starting", 1)
console.log("same second")
Console(width=40, log_time=False, log_path=False).log("no columns")
```

```text
[12:00:00] starting 1                   
           same second                  
no columns                              
```

## input

```text
input(prompt="", *, markup=True, emoji=True, password=False, stream=None)
```

This prints the prompt (markup) with no newline, then reads a line: with
Python's `input()`, with `getpass` when `password=True`, or with
`stream.readline()` when a stream is given.

```python
import io
from rs_rich.console import Console

console = Console()
name = console.input("[bold]Name:[/] ", stream=io.StringIO("Ada\n"))
print(repr(name))
```

```text
Name: 'Ada\n'
```

## print_json

```text
print_json(json=None, *, data=None, indent=2, highlight=True, skip_keys=False,
           ensure_ascii=False, check_circular=True, allow_nan=True,
           default=None, sort_keys=False)
```

This pretty-prints a JSON string, or `data` encoded as JSON, in Rich's
colours, without wrapping. The options are `json.dumps`'s (`indent=None` is
one line) and `highlight=False` prints it plain, as in Rich.

```python
from rs_rich.console import Console

Console(width=40).print_json('{"name": "rs_rich", "tags": [1, null]}')
```

```text
{
  "name": "rs_rich",
  "tags": [
    1,
    null
  ]
}
```

## Capturing

`capture()` is a context manager that keeps what is printed inside it
instead of writing it; `get()` returns it (with ANSI codes, if any) once the
block has ended. `begin_capture()` and `end_capture()` do the same without a
`with`. `with console:` holds output back and writes it all when the block
ends. Only the calling thread's output is held, as in Rich.

```python
from rs_rich.console import Console

console = Console(width=20)
with console.capture() as capture:
    console.print("[bold]captured[/]")
console.print("got", repr(capture.get()))
```

```text
got 'captured\n'
```

## Themes

`Theme(styles, inherit=True)` (in `rs_rich.theme`) names styles; with
`inherit`, Rich's default styles come too. `push_theme(theme)` and
`pop_theme()` change the console's styles, and `use_theme(theme)` does both
around a `with` block. `get_style(name, default=None)` looks a name up, or
parses it as a style; failing both raises `MissingStyle`. Popping the
console's own theme raises `ThemeStackError`.

```python
from rs_rich.console import Console
from rs_rich.theme import Theme

console = Console(width=30, theme=Theme({"warning": "bold red"}))
with console.use_theme(Theme({"warning": "underline"})):
    print(console.get_style("warning"))
print(console.get_style("warning"))
```

```text
underline
bold red
```

## Recording and export

With `record=True` the console keeps everything it writes:

| Method | Returns |
|---|---|
| `export_text(*, clear=True, styles=False)` | Plain text; `styles=True` keeps ANSI codes. |
| `export_html(*, theme=None, clear=True, code_format=None, inline_styles=False)` | An HTML page. `theme` is a [`TerminalTheme`](#terminal-themes). |
| `export_svg(*, title="Rich", theme=None, clear=True, code_format=None, font_aspect_ratio=0.61, unique_id=None)` | An SVG image of a terminal window. |
| `save_text(path, ...)`, `save_html(path, ...)`, `save_svg(path, ...)` | Write the same to a file (UTF-8). |

Each export clears the record unless `clear=False`, and raises
`RuntimeError` on a console without `record=True`. `code_format` is a
template with Rich's fields (`{code}`, `{stylesheet}`, `{foreground}`,
`{background}` for HTML; `{chrome}`, `{lines}`, `{styles}` and the others for
SVG); a field it does not know is a `KeyError`, as `str.format` raises.
Everything printed is recorded, including a `Live` display's redrawn frames.

```python
import io
from rs_rich.console import Console

console = Console(file=io.StringIO(), width=20, record=True)
console.print("[bold]recorded[/] output")
console.rule("end")
print(console.export_text(clear=False), end="")
print(console.export_html(inline_styles=True).count("font-weight: bold"))
print(repr(console.export_text()))
```

```text
recorded output
─────── end ────────
1
''
```

### Terminal themes

`rs_rich.terminal_theme` has Rich's palettes, `DEFAULT_TERMINAL_THEME`,
`SVG_EXPORT_THEME`, `MONOKAI`, `DIMMED_MONOKAI` and `NIGHT_OWLISH`, and
`TerminalTheme(background, foreground, normal, bright=None)` for your own
(colours are `(r, g, b)` tuples; `normal` and `bright` hold 8 each).

## Rendering without printing

| Method | Returns |
|---|---|
| `measure(renderable, *, options=None)` | A [`Measurement`](protocol.md#measurement): the fewest and most cells it needs. |
| `render(renderable, options=None)` | A list of [`Segment`](protocol.md#segment)s; every line ends with a newline segment. |
| `render_lines(renderable, options=None, *, style=None, pad=True, new_lines=False)` | A list of lines, each a list of segments, padded to the width unless `pad=False`. |
| `render_str(text, *, style=None, justify=None, overflow=None, emoji=None, markup=None, highlight=None)` | A `Text`, as a printed string would become. |

These are what a [`__rich_console__`](protocol.md) method uses to render its
children.

```python
from rs_rich.console import Console
from rs_rich.panel import Panel

console = Console(width=40)
print(console.measure("hello world"))
lines = console.render_lines(Panel("x"), console.options.update_width(7))
print(["".join(segment.text for segment in line) for line in lines])
```

```text
Measurement(minimum=5, maximum=11)
['╭─────╮', '│ x   │', '╰─────╯']
```

## Terminal control

`clear(home=True)`, `bell()`, `show_cursor(show=True)` and
`set_alt_screen(enable=True)` write control codes, on a terminal only;
`show_cursor` and `set_alt_screen` return whether they did.

`set_window_title(title)` sets the terminal's title (on a terminal only, and
returns whether it did).

`status`, `pager` and `screen` are the [live display](live.md) context
managers, and `print_exception` prints the exception being handled as a
[`Traceback`](traceback.md) (a `ValueError` outside an `except` block).

## Render hooks and live displays

`push_render_hook(hook)` makes every later print pass its renderables through
`hook.process_renderables(renderables)` and print the list that returns;
`pop_render_hook()` removes the hook pushed last. This is how a
[`Live`](live.md) display redraws itself below whatever else is printed.
`set_live(live)` (which returns whether it is the only display),
`clear_live()` and `_live_stack` keep track of the running displays, as in
Rich.

```python
from rs_rich.console import Console
from rs_rich.panel import Panel

class Boxed:
    def process_renderables(self, renderables):
        return [Panel.fit(renderable) for renderable in renderables]

console = Console(width=30)
console.push_render_hook(Boxed())
console.print("hooked")
console.pop_render_hook()
console.print("plain")
```

```text
╭────────╮
│ hooked │
╰────────╯
plain
```

## The global console

`rs_rich.print` prints to a console shared by the whole program, which
`rs_rich.get_console()` returns. It is created on first use and writes to
`sys.stdout`. `rs_rich.print(..., file=f)` prints to a new console on `f`
instead; `rs_rich.print_json` prints JSON on the global console, and
`rs_rich.reconfigure(**kwargs)` replaces it with `Console(**kwargs)`.

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
including `rs_rich.print` from a worker thread. Each print writes all its
output before another thread's print on the same console writes, so lines
from different threads never interleave. A thread waiting for its turn
releases the GIL.

Rendering holds no lock, so a renderable's `__rich_console__` may use the
console (its width, `measure`, `render_lines`, even `print`) while it
renders. A `print` to a console from inside that console's own `file.write`
(or `flush`) raises `RuntimeError` ("Console is already printing") instead of
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
