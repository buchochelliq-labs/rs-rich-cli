"""The pictures on docs/python/pretty.md: run it, or scripts/capture_python_docs.py."""

from _shots import Shots

from dataclasses import dataclass

import rs_rich
from rs_rich.json import JSON
from rs_rich.pretty import Pretty


def points(console):
    # --8<-- [start:pretty]
    @dataclass
    class Point:
        x: int
        y: int

    console.print({"name": "rs_rich", "points": [Point(1, 2), Point(3, 4)], "ok": True})
    # --8<-- [end:pretty]


def options(console):
    # --8<-- [start:options]
    console.print(Pretty(list(range(20)), max_length=5))
    console.print(Pretty({"nested": {"deeper": [1, 2]}}, max_depth=1))
    console.print(Pretty({"a": [1, 2], "b": "text"}, expand_all=True, indent_guides=True))
    # --8<-- [end:options]


def json_data(console):
    # --8<-- [start:json]
    console.print(JSON('{"name": "rs_rich", "tags": ["fast", "rust"], "stable": false}'))
    console.print(JSON.from_data({"compact": [1, 2]}, indent=None))
    # --8<-- [end:json]


def account(console):
    # --8<-- [start:inspect]
    class Account:
        """A bank account."""

        def __init__(self):
            self.owner = "Ada"
            self.balance = 10.5

        def __repr__(self):
            return "Account()"

    rs_rich.inspect(Account(), console=console)
    # --8<-- [end:inspect]


shots = Shots("pretty")
shots.shot("pretty", points, width=40)
shots.shot("options", options, width=40)
shots.shot("json", json_data, width=40)
shots.shot("inspect", account, width=40)
shots.finish()
