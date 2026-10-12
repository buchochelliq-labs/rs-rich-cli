"""The pictures on docs/python/getting-started.md: run it, or scripts/capture_python_docs.py."""

from _shots import Shots

from rs_rich import art
from rs_rich.markdown import Markdown
from rs_rich.mermaid import Mermaid
from rs_rich.panel import Panel
from rs_rich.table import Table
from rs_rich.tree import Tree


def first_output(console):
    # --8<-- [start:first-output]
    console.print("Hello, [bold magenta]World[/]!", ":sparkles:")
    console.print({"name": "rs-rich", "version": [0, 0, 1], "rust": True})
    # --8<-- [end:first-output]


def tour(console):
    # --8<-- [start:tour]
    table = Table("Crate", "Version", title="Cohort")
    table.add_row("rs-rich", "0.0.8")
    table.add_row("rs-rich-ext", "0.0.10")

    tree = Tree("rs_rich")
    tree.add("console").add("Console")
    tree.add("table").add("Table")

    console.print(Panel.fit(table, title="Table in a panel"))
    console.print(tree)
    # --8<-- [end:tour]


def markdown(console):
    # --8<-- [start:markdown]
    console.print(Markdown("# Notes\n\n- fast\n- **byte-identical** to Rich"))
    # --8<-- [end:markdown]


def image(console):
    # --8<-- [start:image]
    def gradient(w, h):
        pixels = bytearray()
        for y in range(h):
            for x in range(w):
                pixels += bytes((x * 255 // (w - 1), y * 255 // (h - 1), 128, 255))
        return art.ArtImage.frombytes("RGBA", (w, h), pixels)

    image = gradient(32, 16)
    console.print(art.ImageArt(image, mode="blocks", width=32))
    # --8<-- [end:image]


def mermaid(console):
    # --8<-- [start:mermaid]
    console.print(Mermaid("graph LR\n  A[Start] --> B{Ready?}\n  B -->|yes| C((Go))"))
    console.print(Mermaid("graph TD\n  A --> B", ascii=True))
    # --8<-- [end:mermaid]


shots = Shots("getting-started")
shots.shot("first-output", first_output, width=40)
shots.shot("tour", tour, width=40)
shots.shot("markdown", markdown, width=40)
shots.shot("image", image, width=40)
shots.shot("mermaid", mermaid, width=50)
shots.finish()
