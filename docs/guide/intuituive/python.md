# intuiTUIve from Python

`rs_rich.tui` is intuiTUIve from Python, in the `rs-rich` wheel on PyPI
(0.0.6 and later). It has the same API as the Rust crate, name for name, so
the rest of this guide reads the same in either language. Where the Rust
code has a closure, the Python code has a function or a `lambda`.

```bash
pip install rs-rich
```

```python
from rs_rich.tui import App, column, label, signal, text

def build():
    count = signal(0)
    return (
        column([
            text(lambda: f"[b]Count:[/] {count.get()}").panel("Counter"),
            label("[dim]+ adds one · q quits"),
        ])
        .on_key("+", lambda cx: count.update(lambda c: c + 1))
        .on_key("q", lambda cx: cx.quit())
    )

App(build).run()
```

As in Rust, `build` runs once and returns the root node. `text` reads
`count`, so pressing `+` redraws that one line.

## From Rust to Python

| Rust | Python |
|---|---|
| `App::new(\|\| …)` | `App(build)`: `build()` runs once, when the app is made |
| `text!("{count} items")` | `text(lambda: f"{count.get()} items")` |
| `count.update(\|c\| *c += 1)` | `count.update(lambda c: c + 1)` |
| `.on_key("q", \|cx\| cx.quit())` | `.on_key("q", lambda cx: cx.quit())` |
| `.class("warn")` | `.class_("warn")` (`class` is a Python keyword) |
| `Size::Fixed(3)`, `Size::Auto` | `Size.Fixed(3)`, `Size.Auto`, or `3` and `"auto"` (`"40%"` and `"2fr"` too) |
| `Easing::EaseOut`, `Placement::Below`, `Role::MenuBar` | `Easing.EaseOut`, `Placement.Below`, `Role.MenuBar`, or `"ease_out"`, `"below"`, `"menubar"` |
| `Rect::new(x, y, w, h)` | `Rect(x, y, w, h)`, or a tuple `(x, y, w, h)` |
| `Date::new(2026, 10, 7)` | `datetime.date(2026, 10, 7)` |
| `Duration::from_millis(200)` | `0.2` (seconds), or a `timedelta` |
| `spawn_future(future, done)` | `spawn_async(coroutine, done)` |
| `app.render_with(&["+", "q"], 30, 2)` | `app.render_with(["+", "q"], 30, 2)` |
| `App::run_on(&mut Headless::new(script, w, h))` | `run(app, script, width=w, height=h)` |

Import the names you use (`from rs_rich.tui import App, column, signal`), or
the module (`from rs_rich import tui`): a `from rs_rich.tui import *` brings
in `list` and `text`, as Rust's prelude does, over Python's own `list`.

### Signals hold Python objects

A signal holds any Python object. `get()` reads it (a node drawing, or a
memo computing, subscribes to it), and `set(value)` writes it, doing nothing
when the new value is equal (`==`) to the old.

`update(f)` sets the value to `f(value)`. When `f` returns `None`, the
value is kept, and its readers are told all the same, which is how to
change a list or a dict in place:

```python
items = signal([])
items.update(lambda xs: xs + ["new"])    # a new list
items.update(lambda xs: xs.append("x"))  # the same list, changed
```

Setting a mutated object back with `set` changes nothing, since it is equal
to itself: use `update`.

The widgets that need a signal of a particular type take one of yours: a
list's or table's selection (an `int`), a split's ratio (a `float`), a
calendar's day (a `datetime.date`), a tree's path (a list of indices) and
its expanded paths (a set of tuples), a table's sort (`None` or `(column,
Order)`). The first time a signal goes to such a widget it becomes one of
that type and keeps its value, and anything that read it before (a memo
made earlier) follows it. From then on, setting it to a value of another
type raises `TypeError`.

## Custom widgets

Subclass `Widget` and make a node of it with `widget(w)`, as a Rust app
implements the `Widget` trait:

```python
from rs_rich.tui import App, Widget, widget

class Counter(Widget):
    """A counter that a click or `+` counts up."""

    def __init__(self):
        self.n = 0

    def name(self):
        return "counter"

    def draw(self, cx, canvas):
        canvas.print(0, 0, f"count {self.n}", cx.style("accent", "bold"))

    def event(self, cx, event):
        pressed = event.kind == "mouse" and event.mouse.is_press()
        if event.kind == "key" and event.key == "+" or pressed:
            self.n += 1
            cx.redraw()
            return True
        return False

    def focusable(self):
        return True

App(lambda: widget(Counter()).on_key("q", lambda cx: cx.quit())).run()
```

- `draw(cx, canvas)` is the one method to write. The `Canvas` has the
  trait's methods: `print`, `set`, `fill`, `clear`, `markup`, `render` (any
  renderable), `lines` (rows of `Segment`s), `restyle`, `border` and
  `scroll_up`.
- `event(cx, event)` returns whether it used the event. `event.kind` is
  `"key"`, `"key_up"`, `"preview"`, `"mouse"`, `"paste"`, `"focus"`,
  `"hover"` or `"resize"`. `cx.app()` is the `Ctx`, to quit, open a screen
  or move the focus.
- `measure`, `layout`, `scroll`, `caret`, `role`, `cursor`,
  `access_state` and `describe` work as in Rust. A container returns its
  nodes from `children()`. `layout` and `MeasureCx.measure` name a child
  by its index there.
- `name()`, `children()`, `focusable()`, `retained()`, `viewport()` and
  `previews_keys()` are read once, when `widget(w)` makes the node.
- Methods you do not override keep the trait's defaults, and are not
  called in Python.

`cx`, `canvas` and every `Ctx` a handler gets are lent for that one call. A
widget may keep one, but using it after the call raises `RuntimeError`.

## Threads and asyncio

- **The app runs on the Python thread that called `run()`** (or a `Driver`
  method). While it waits for the terminal it releases the GIL, so other
  Python threads run. Each callback takes the GIL back while it runs.
- **Signals, memos, logs and resources belong to the app's thread.** Using
  one from another thread raises `RuntimeError`. Nodes and apps cannot
  cross threads at all.
- **`spawn(work, done)`** runs `work()` on a thread of its own, then
  `done(result, cx)` back on the app's thread, where it may write signals.
  `work` holds the GIL only while it runs Python code, so a `time.sleep` or
  a socket read in it does not stop the app drawing.
- **`spawn_async(coroutine, done)`** awaits a coroutine on an asyncio
  event loop, from a thread of its own: a new loop for the coroutine
  (`asyncio.run`), or with `loop=` a loop of yours that runs on another
  thread (`asyncio.run_coroutine_threadsafe`).
- **`Proxy`**, from `cx.proxy()` or `app.proxy()`, is the way in from a
  thread of your own: `proxy.run(lambda: status.set("done"))`.

```python
async def fetch():
    await asyncio.sleep(1)
    return "fetched"

def build():
    status = signal("waiting")
    spawn_async(fetch(), lambda body, cx: status.set(body))
    return text(lambda: status.get())
```

## Exceptions

A Python exception raised by any callback stops the app. That covers a
handler, a node's function, a memo, a watch, a timer, a widget's method, a
task's work or a proxy's function. When it raises:

- no more of that app's Python code runs, and the app quits at its next
  turn;
- `run()`, `render_with` or `rs_rich.tui.run` raises the exception once the
  terminal is restored;
- with a `Driver`, the call that ran the callback (`key`, `update`,
  `render`, ...) raises it.

Two cases differ. A `resource`'s fetch raising is the resource failing
(`Load.error` holds the message), not the app, as a Rust fetch's `Err`
is. A served app has no Python call to raise from, so its exception goes to
`sys.unraisablehook` and that tab's session ends.

Inside a terminal app, Ctrl+C is a key: it quits the app, unless a binding
takes it. It does not raise `KeyboardInterrupt`.

## Testing

`run(app, script)` runs an app headless on a virtual clock and returns what
it painted. A script is a string of key names, or an `rs_rich.interact`
`Script` for text, pastes, clicks, drags, the wheel, resizes and waits:

```python
from rs_rich.tui import App, Script, run, signal, text

def counter():
    count = signal(0)
    return (
        text(lambda: f"Count: {count.get()}")
        .on_key("+", lambda cx: count.update(lambda c: c + 1))
        .on_key("q", lambda cx: cx.quit())
    )

ran = run(App(counter), Script().keys("+ +").wait(1.0).keys("q"), width=30, height=1)
assert ran.finished              # the app quit before the script ran out
assert ran.screen[0].startswith("Count: 2")
```

`App.wait_for_tasks()` waits for background tasks before each scripted
event, so a test sees a task's result however fast the machine is. For more
control, `App.driver(width, height)` returns the [loop of your
own](index.md#owning-the-loop). It has `update(now)`, `render()`,
`timeout(now)`, `key`, `text`, `paste`, `mouse`, `click`, `resize`,
`screen()`, `lines()`, `accessibility()` (the accessibility tree, as
`AccessNode`s), `take_announcements()` and `finish()`. In linear mode,
`render()` returns the lines a screen reader would read.

## Serving to a browser

```python
from rs_rich.tui import serve

serve("127.0.0.1:8080", lambda: App(build))
```

`serve` prints the URL to open, with its token, and serves one app per
browser tab until Ctrl+C. The function is called on each session's own
thread, so each tab's app (its signals, its nodes) lives there, as in
[rs-rich-web](web.md). `Server(address, app)` takes the options
(`max_sessions`, `token`, `allow_origin`, `title`), and `spawn()` serves in
the background and returns a handle to `stop()` (or a context manager). The
security model is the Rust crate's: a token, an origin check and a session
cap, and nothing beyond this computer unless a proxy with TLS and
authentication is in front.

## Terminal panes and web views

`terminal(command)` runs a program in a pane, and `web_view(url)` shows a
page in a terminal browser, as in [rs-rich-embed](embed.md):

```python
pane = terminal(["htop", "-d", "10"]).on_exit(lambda status, cx: cx.quit())
column([pane.node().panel("htop"), web_view("https://example.com").node()])
```

`ReplayHost(output=…, exit=…)` with `terminal_with(host)`, and
`ProgramEngine.with_hosts(make)`, stand in for a program in tests.

## Examples

`crates/rich-py/examples/tui` has the counter, the [tutorial](tutorial.md)'s
to-do app, the planner (a menu bar, a tree, split panes, a calendar and a
virtual list of a hundred thousand rows), and the counter served to a
browser. Each runs with `python crates/rich-py/examples/tui/<name>.py`.

## What is not bound

Everything the Rust guide shows is bound. What is left out has no Python
counterpart, or a different one:

- **Generic types.** `on_drop` takes every value dragged from Python (Rust
  matches by type), so check the value's type in the handler. `each` and
  `switch` keys are any hashable (or `==`-comparable) objects.
- **The console.** Rust's `DrawCx::console()`, `EventCx::console()` and
  `MeasureCx::console()` hand out the app's `rich::Console`, which Python
  cannot hold. Use `canvas.render`, `canvas.markup` and `cx.style` instead.
  `leaf(draw)` calls `draw(width, height)` without it, and `App::console`
  is not bound.
- **Backends.** `run()` uses crossterm. The termion and termwiz backends are
  off in the wheel, and `App::run_on` with a backend of your own is not
  bound: `run` (headless) and `Driver` cover the same ground.
- **Traits implemented in Python** other than `Widget`: a `PtyHost` or
  `WebEngine` of your own, and an `Announcer` other than a function
  (`App.announcer(f)`). The Chrome and Browsh engines are off in the wheel.
- **Smaller pieces:** `Widget::hidden_children`, the cell-level `Screen`
  (`Driver.screen()` gives the rows, `Driver.lines()` the styled
  segments), `widget::markup_width` and `markup_line`, and `Easing::at`.
