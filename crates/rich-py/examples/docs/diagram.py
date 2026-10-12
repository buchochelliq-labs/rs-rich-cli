"""The pictures on docs/python/diagram.md: run it, or scripts/capture_python_docs.py."""

from _shots import Shots

from rs_rich import diagram


def graph(console):
    # --8<-- [start:graph]
    graph = (
        diagram.Graph("LR")
        .node("web", "Browser", shape="round")
        .node("api", "API")
        .node("db", "Postgres", shape="cylinder")
        .edge("web", "api", label="HTTPS")
        .edge("api", "db", label="reads")
    )
    console.print(diagram.Diagram(graph))
    print("\n".join(diagram.draw_graph(diagram.Graph().edge("a", "b"), ascii=True)))
    # --8<-- [end:graph]


def dot(console):
    # --8<-- [start:dot]
    console.print(diagram.Dot("digraph { rankdir=LR; build -> test -> ship }"))
    try:
        diagram.parse_dot("digraph {\n  a -> b\n  a:out -> c\n}")
    except diagram.DotError as error:
        print(error.line, error.construct)
    # --8<-- [end:dot]


shots = Shots("diagram")
shots.shot("graph", graph, width=60)
shots.shot("dot", dot, width=80)
shots.finish()
