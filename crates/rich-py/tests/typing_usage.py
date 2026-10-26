"""A program written against rs_rich's main APIs, as a user would write it.

scripts/check_python_stubs.py runs `mypy --strict` on this file, so a stub
that is missing, or wrong for callers, fails CI; it also runs the file, so
what type-checks here works too. Not a pytest module.
"""

from __future__ import annotations

import io
from typing import List, Optional

from rs_rich import box
from rs_rich.align import Align
from rs_rich.color import Color, ColorSystem
from rs_rich.columns import Columns
from rs_rich.console import Console, ConsoleOptions
from rs_rich.highlighter import ReprHighlighter
from rs_rich.layout import Layout
from rs_rich.markdown import Markdown
from rs_rich.measure import Measurement
from rs_rich.padding import Padding
from rs_rich.panel import Panel
from rs_rich.pretty import Pretty
from rs_rich.progress import BarColumn, Progress, TaskID, TextColumn
from rs_rich.rule import Rule
from rs_rich.segment import Segment
from rs_rich.style import Style
from rs_rich.syntax import Syntax
from rs_rich.table import Table
from rs_rich.text import Text
from rs_rich.theme import Theme
from rs_rich.tree import Tree


def make_console() -> Console:
    return Console(file=io.StringIO(), width=60, color_system="truecolor", force_terminal=True, record=True)


def styled_text() -> Text:
    text = Text("Hello, ", style="bold")
    text.append("world", style=Style(color="magenta", underline=True))
    text.stylize("italic", 0, 5)
    highlighted: Text = Text.from_markup("[green]ok[/] 42")
    ReprHighlighter().highlight(highlighted)
    return Text.assemble(text, " ", highlighted)


def table() -> Table:
    grid = Table(title="Crates", box=box.ROUNDED, show_lines=True)
    grid.add_column("name", style="cyan", no_wrap=True)
    grid.add_column("version", justify="right")
    for name, version in [("rs-rich", "0.0.13"), ("rs-rich-ext", "0.0.20")]:
        grid.add_row(name, version)
    return grid


def tree() -> Tree:
    root = Tree("workspace")
    crates = root.add("crates", guide_style="dim")
    crates.add(Panel("rich", title="core"))
    return root


def progress(console: Console) -> float:
    with Progress(TextColumn("{task.description}"), BarColumn(), console=console, auto_refresh=False) as bar:
        task: TaskID = bar.add_task("build", total=10)
        for _ in range(10):
            bar.advance(task)
        done = bar.tasks[0].completed
    return done


def main() -> str:
    console = make_console()
    console.push_theme(Theme({"note": "yellow"}))
    console.print(styled_text(), justify="center")
    console.print(table(), tree(), Rule("rule"))
    console.print(Columns([Padding("a", 1), Align.center("b")]))
    console.print(Markdown("# Title\n\nSome *markdown*."))
    console.print(Syntax("print('hi')", "python", line_numbers=True))
    console.print(Pretty({"key": [1, 2, 3]}))
    console.log("logged", style="note")
    layout = Layout()
    layout.split_row(Layout(name="left"), Layout(name="right"))
    layout["left"].update(Panel("left"))
    options: ConsoleOptions = console.options
    measured: Measurement = console.measure("width", options=options)
    lines: List[List[Segment]] = console.render_lines(Text("x"), options)
    color: Color = Color.parse("#ff0000")
    system: Optional[ColorSystem] = color.system if color.system == ColorSystem.TRUECOLOR else None
    with console.capture() as capture:
        console.print("captured")
    captured: str = capture.get()
    done = progress(console)
    exported: str = console.export_text()
    return f"{measured.maximum} {len(lines)} {system} {captured!r} {done} {len(exported) > 0}"


if __name__ == "__main__":
    print(main())
