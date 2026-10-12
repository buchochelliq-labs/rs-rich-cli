"""The pictures on docs/python/index.md: run it, or scripts/capture_python_docs.py."""

from _shots import Shots

from rs_rich.table import Table


def star_wars(console):
    # --8<-- [start:star-wars]
    table = Table(title="Star Wars Movies")
    table.add_column("Released", justify="right", style="cyan", no_wrap=True)
    table.add_column("Title", style="magenta")
    table.add_row("Dec 20, 2019", "Star Wars: The Rise of Skywalker")

    console.print(table)
    # --8<-- [end:star-wars]


shots = Shots("index")
shots.shot("star-wars", star_wars, width=50)
shots.finish()
