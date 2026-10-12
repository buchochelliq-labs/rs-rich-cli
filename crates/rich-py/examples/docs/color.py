"""The pictures on docs/python/color.md: run it, or scripts/capture_python_docs.py."""

from _shots import Shots

from rs_rich.color import Color


def sample(console):
    # --8<-- [start:sample]
    console.print(Color.parse("magenta"))
    # --8<-- [end:sample]


shots = Shots("color")
shots.shot("sample", sample, width=40)
shots.finish()
