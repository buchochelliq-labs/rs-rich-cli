"""The pictures on docs/python/console.md: run it, or scripts/capture_python_docs.py."""

from _shots import Shots

import datetime

from rs_rich.console import Console
from rs_rich.panel import Panel
from rs_rich.text import Text


def printing(console):
    # --8<-- [start:print]
    console.print("[bold]Hello[/], World!", 42, None)
    console.print("a", Text("b"), "c", sep="-", end="!\n")
    console.print("before", Panel.fit("boxed"), "after")
    console.print("one two three four five six seven eight")
    # --8<-- [end:print]


def justify(console):
    # --8<-- [start:justify]
    console.print("centred", justify="center")
    console.print(Panel.fit("right"), justify="right")
    console.print("a long line that will not fit", overflow="ellipsis", no_wrap=True)
    console.print("narrow words here", width=8)
    # --8<-- [end:justify]


def out_rule(console):
    # --8<-- [start:out-rule]
    console.out("[not markup]", 1)
    console.line()
    console.rule("[bold]Section[/]")
    console.rule("left", align="left", characters="=")
    # --8<-- [end:out-rule]


def print_json(console):
    # --8<-- [start:print-json]
    console.print_json('{"name": "rs_rich", "tags": [1, null]}')
    # --8<-- [end:print-json]


def log(console):
    # --8<-- [start:log]
    console.log("starting", 1)
    console.log("same second")
    Console(width=40, log_time=False, log_path=False).log("no columns")
    # --8<-- [end:log]


def render_hook(console):
    # --8<-- [start:render-hook]
    class Boxed:
        def process_renderables(self, renderables):
            return [Panel.fit(renderable) for renderable in renderables]

    console.push_render_hook(Boxed())
    console.print("hooked")
    console.pop_render_hook()
    console.print("plain")
    # --8<-- [end:render-hook]


shots = Shots("console")
shots.shot("print", printing, width=30)
shots.shot("justify", justify, width=20)
shots.shot("out-rule", out_rule, width=24)
shots.shot("print-json", print_json, width=40)
shots.shot("log", log, width=40, log_path=False,
           get_datetime=lambda: datetime.datetime(2026, 9, 25, 12, 0, 0))
shots.shot("render-hook", render_hook, width=30)
shots.finish()
