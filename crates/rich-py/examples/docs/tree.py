"""The pictures on docs/python/tree.md: run it, or scripts/capture_python_docs.py."""

from _shots import Shots

from rs_rich.tree import Tree


def project(console):
    # --8<-- [start:tree]
    tree = Tree("[b]project[/]")
    source = tree.add("src", guide_style="bold")
    source.add("main.rs")
    source.add("lib.rs")
    tree.add("README.md")
    console.print(tree)
    # --8<-- [end:tree]


shots = Shots("tree")
shots.shot("tree", project, width=30)
shots.finish()
