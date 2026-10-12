"""The pictures on docs/python/text.md: run it, or scripts/capture_python_docs.py."""

from _shots import Shots

from rs_rich.panel import Panel
from rs_rich.text import Text


def overflow(console):
    # --8<-- [start:overflow]
    text = Text("abcdefghijklmnop", overflow="ellipsis", no_wrap=True)
    console.print(text)
    console.print(Panel(text))
    console.print(Panel(Text("right", justify="right")))
    # --8<-- [end:overflow]


def stylize(console):
    # --8<-- [start:stylize]
    text = Text("Hello")
    text.append(", World", style="bold").append("!")
    text.stylize("italic", 0, 5)
    text.stylize("underline", -1)
    # --8<-- [end:stylize]
    # --8<-- [start:stylize-print]
    console.print(text)
    # --8<-- [end:stylize-print]


def highlight(console):
    # --8<-- [start:highlight]
    text = Text("foo 123 bar 45")
    count = text.highlight_regex(r"(?P<word>[a-z]+) (?P<number>\d+)", style_prefix="repr.")
    text.highlight_words(["bar"], "bold")
    # --8<-- [end:highlight]
    # --8<-- [start:highlight-print]
    console.print(text)
    # --8<-- [end:highlight-print]


shots = Shots("text")
shots.shot("overflow", overflow, width=12)
shots.shot("stylize", stylize, width=20)
shots.shot("highlight", highlight, width=20)
shots.finish()
