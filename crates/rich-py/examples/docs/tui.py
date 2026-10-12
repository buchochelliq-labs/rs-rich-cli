"""The pictures on docs/python/tui.md: run it, or scripts/capture_python_docs.py.

Each picture is the screen a headless run left, in colour: replayed from the
bytes `run` wrote, or built from the driver's styled lines. What the example
prints is on the page as text.
"""

from _shots import Shots, screen

from rs_rich.text import Text
from rs_rich.tui import App, Column, Size, column, label, run, signal, table, text


# --8<-- [start:counter]
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
# --8<-- [end:counter]


def first_app(console):
    # --8<-- [start:run]
    ran = run(App(counter), "+ + q", width=30, height=2)
    print("\n".join(line.rstrip() for line in ran.screen))
    print(ran.finished)
    # --8<-- [end:run]
    console.print(screen(ran.output, 30))


# --8<-- [start:files]
def files():
    return table(
        [Column("Name", Size.Auto), Column("Size", Size.Flex(1))],
        [["a.txt", "1K"], ["b.txt", "2K"]],
        signal(0),
    ).label("Files")
# --8<-- [end:files]


def driven(console):
    # --8<-- [start:driver]
    driver = App(files).driver(16, 3)
    driver.update(0)
    driver.render()
    driver.key("down")
    driver.update(0)
    driver.render()
    print("\n".join(line.rstrip() for line in driver.screen()))
    print(driver.accessibility()[0].describe())
    # --8<-- [end:driver]
    lines = Text()
    for number, line in enumerate(driver.lines()):
        if number:
            lines.append("\n")
        for segment in line:
            lines.append(segment.text, segment.style)
    console.print(lines)


shots = Shots("tui")
shots.shot("counter", first_app, width=30, stdout=False)
shots.shot("driver", driven, width=24, stdout=False)
shots.finish()
