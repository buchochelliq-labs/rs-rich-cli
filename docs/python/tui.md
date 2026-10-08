# Terminal apps (intuiTUIve)

```python
from rs_rich.tui import App, column, label, signal, text, run
```

`rs_rich.tui` is intuiTUIve, the port's TUI framework (`rs-rich-intuituive`),
from Python. You describe the screen once as a tree of nodes and keep the
state in signals; when a signal changes, the nodes that read it draw again,
and only the cells that changed reach the terminal. Rich has no counterpart,
so the reference is the Rust crate: the API is the same, name for name, and
the [intuiTUIve guide](https://buchochelliq-labs.github.io/rs-rich-cli/guide/intuituive/)
reads the same in either language. Its
[Python page](https://buchochelliq-labs.github.io/rs-rich-cli/guide/intuituive/python/)
maps the Rust names onto Python ones and covers widgets written in Python,
threads, asyncio and browser serving.

## A first app

`App(build)` calls `build()` once, for the root node. `run()` takes the
terminal until a handler quits. Here `rs_rich.tui.run` runs it headless
instead, pressing keys from a script, as a test does:

```python
from rs_rich.tui import App, column, label, run, signal, text

def counter():
    count = signal(0)
    return (
        column([
            text(lambda: f"[b]Count:[/] {count.get()}").auto(),
            label("[dim]+ adds one · q quits"),
        ])
        .on_key("+", lambda cx: count.update(lambda c: c + 1))
        .on_key("q", lambda cx: cx.quit())
    )

ran = run(App(counter), "+ + q", width=30, height=2)
print("\n".join(line.rstrip() for line in ran.screen))
print(ran.finished)
```

```text
Count: 2
+ adds one · q quits
True
```

`App(counter).run()` is the same app in the terminal.

## Widgets and a loop of your own

Every widget of the Rust crate is here: tables, virtual lists, tabs, trees,
the calendar, split panes, menus, the command palette and the help.
`App.driver(width, height)` drives an app from a loop of your own, and
gives the accessibility tree a screen reader would get:

```python
from rs_rich.tui import App, Column, Size, signal, table

def files():
    return table(
        [Column("Name", Size.Auto), Column("Size", Size.Flex(1))],
        [["a.txt", "1K"], ["b.txt", "2K"]],
        signal(0),
    ).label("Files")

driver = App(files).driver(16, 3)
driver.update(0)
driver.render()
driver.key("down")
driver.update(0)
driver.render()
print("\n".join(line.rstrip() for line in driver.screen()))
print(driver.accessibility()[0].describe())
```

```text
Name  Size
a.txt 1K
b.txt 2K
Files, table, 2 of 2: b.txt 2K, selected
```

## Exceptions

A Python exception in any callback (a handler, a node's function, a memo, a
widget's method, a task) stops the app, and `run()` raises it once the
terminal is restored:

```python
def broken():
    return label("x").on_key("b", lambda cx: 1 / 0)

try:
    run(App(broken), "b")
except ZeroDivisionError as error:
    print("raised:", error)
```

```text
raised: division by zero
```

## More

- **The examples** in `crates/rich-py/examples/tui`: the counter, the
  tutorial's to-do app, the planner, and the counter served to a browser.
- **The API reference**: [`rs_rich.tui`](api/rs_rich/tui.md), from the type
  stubs.
