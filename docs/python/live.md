# Live displays, status, screen and pager

```python
from rs_rich.live import Live
from rs_rich.live_render import LiveRender
from rs_rich.status import Status
from rs_rich.screen import Screen
from rs_rich.pager import Pager, SystemPager
```

These correspond to `rich.live`, `rich.live_render`, `rich.status`,
`rich.screen` and `rich.pager`, and to the console methods
`Console.status()`, `Console.screen()` and `Console.pager()`. Every frame is
rendered in Rust; the output is byte-for-byte Rich 15.0.0's (see
`tests/test_live.py`).

## Live

```text
Live(renderable=None, *, console=None, screen=False, auto_refresh=True,
     refresh_per_second=4, transient=False, redirect_stdout=True,
     redirect_stderr=True, vertical_overflow="ellipsis", get_renderable=None)
```

| Argument | Meaning |
|---|---|
| `renderable` | What to show. Change it with `update()`. |
| `console` | The console to draw on; the global console by default. |
| `screen` | Draw on the terminal's alternate screen (implies `transient`). |
| `auto_refresh` | Redraw from a background thread, `refresh_per_second` times a second. With `False`, call `refresh()` (or `update(..., refresh=True)`). |
| `transient` | Erase the display when it stops. |
| `redirect_stdout`, `redirect_stderr` | On a terminal, send `print()` output (and `sys.stderr`) through the console, above the display. |
| `vertical_overflow` | A display taller than the terminal is `"crop"`ped, ends in an `"ellipsis"` line, or stays `"visible"`. |
| `get_renderable` | A callable returning the renderable to show at each refresh. |

`start(refresh=False)`, `stop()`, `update(renderable, *, refresh=False)` and
`refresh()` work as in Rich, and a `Live` is a context manager. Properties:
`is_started`, `renderable`, `console`, `auto_refresh`, `transient`,
`refresh_per_second`, `vertical_overflow`.

On a terminal the display is redrawn in place, and whatever else the console
prints while it runs appears above it. Written to a file (or anything that is
not a terminal), a display writes only its final frame when it stops:

```python
from rs_rich.console import Console
from rs_rich.live import Live
from rs_rich.panel import Panel

console = Console(width=30)
with Live(console=console, auto_refresh=False) as live:
    for step in range(3):
        live.update(Panel(f"step {step}"), refresh=True)
console.print()
```

```text
╭────────────────────────────╮
│ step 2                     │
╰────────────────────────────╯
```

### Threads

With `auto_refresh`, a daemon `threading.Thread` redraws the display. It
holds the GIL while it draws, like any Python thread, and waits on a
`threading.Event` in between, so the rest of the program keeps running.
`stop()` (or leaving the `with` block, also on an exception) ends it, shows
the cursor again and restores `sys.stdout` and `sys.stderr`. A display still
running when the interpreter exits has its thread stopped by an `atexit`
hook.

Displays nest: a `Live` started while another runs on the same console is
drawn as part of the outer one.

## LiveRender

`LiveRender(renderable, style="", vertical_overflow="ellipsis")` is the
renderable a `Live` draws. It remembers the shape of its last render:
`last_render_height`, `position_cursor()` and `restore_cursor()` return the
control codes that move the cursor back over it.

```python
from rs_rich.console import Console
from rs_rich.live_render import LiveRender

console = Console(width=20)
render = LiveRender("one\ntwo")
console.print(render)
console.print()
print(render.last_render_height, repr(str(render.position_cursor())))
```

```text
one
two
2 '\r\x1b[2K\x1b[1A\x1b[2K'
```

## Status

```text
Status(status, *, console=None, spinner="dots", spinner_style="status.spinner",
       speed=1.0, refresh_per_second=12.5)
Console.status(status, *, spinner="dots", spinner_style="status.spinner",
               speed=1.0, refresh_per_second=12.5)
```

A spinner and a message in a transient `Live`. `update(status=None, *,
spinner=None, spinner_style=None, speed=None)` changes any of them.
`renderable` is the spinner. It is the live area's own spinner renderable
(frames from core's `Spinner`), not `rs_rich.spinner.Spinner`, and an unknown
spinner name falls back to `dots` where Rich raises `KeyError`.

```python
from rs_rich.console import Console

console = Console(width=40)
with console.status("Working...") as status:
    status.update("Almost there", spinner="line")
console.print("done")
```

```text
done
```

## Screen

`Screen(*renderables, style=None, application_mode=False)` fills the
console's size, cropping or padding its content. `Console.screen(hide_cursor=True,
style=None)` returns a context that switches to the alternate screen (on a
terminal) and whose `update(*renderables, style=None)` draws a `Screen`.

```python
from rs_rich.console import Console
from rs_rich.screen import Screen

console = Console(width=12, height=3)
console.print(Screen("top", "next"))
console.print()
```

```text
top         
next        
            
```

## Pager

`Console.pager(pager=None, styles=False, links=False)` collects what is
printed inside it and shows it in a pager when the block ends: the system
pager (`SystemPager`, as `pydoc` uses) unless you pass a `Pager` subclass
with a `show(content)` method. `styles=False` removes colours and styles;
`links=False` removes hyperlinks.

```python
from rs_rich.console import Console
from rs_rich.pager import Pager


class Collect(Pager):
    def show(self, content):
        print(repr(content))


console = Console(width=40, force_terminal=True)
with console.pager(Collect()):
    console.print("[bold]paged[/] text")
```

```text
'paged text\n'
```

## Differences from Rich

- The render hook: Rich's `Live` hooks into the console's print pipeline;
  the bindings' console has no hooks, so a running display wraps the
  console's `file` instead (`console.file` returns a wrapper that passes
  every attribute through, and is restored when the display stops). Output
  is the same, with two exceptions: while a display runs on a terminal, the
  redrawn display after another print is not recorded (`record=True`), and
  prints held back by `with console:` are redrawn around once, together.
- `Status.renderable` is not an `rs_rich.spinner.Spinner` (see above).
- Jupyter is not supported.
