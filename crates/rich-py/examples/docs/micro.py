"""The pictures on docs/python/micro.md: run it, or scripts/capture_python_docs.py."""

from _shots import Shots

from rs_rich.micro import MicroMarkup


def markup(console):
    # --8<-- [start:markup]
    console.print(MicroMarkup("Deploying :micro:status/loading: then :micro:status/success:"))
    # --8<-- [end:markup]


shots = Shots("micro")
shots.shot("markup", markup, width=40)
shots.finish()
