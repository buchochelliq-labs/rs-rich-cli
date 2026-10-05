"""``rs_rich.diagram``: graph diagrams (``rs-rich-diagram``; no Rich counterpart).

``Graph`` is a graph built in code: ``node`` and ``edge`` add to it and return
it, so calls chain. ``Diagram`` draws one with box-drawing characters (or
ASCII), laid out in layers; ``draw_graph`` gives the lines. ``Dot`` draws a
DOT (Graphviz) source natively, and ``parse_dot`` reads one, raising
``DotError`` with the ``line`` (and the unsupported ``construct``).
"""

from ._native import (
    DIAGRAM_MAX_EDGE_LENGTH,
    Diagram,
    DiagramEdge,
    DiagramError,
    DiagramLayoutError,
    DiagramNode,
    Dot,
    DotCluster,
    DotError,
    DotGraph,
    Graph,
    draw_graph,
    parse_dot,
)

# The crate's names for the same things.
Node = DiagramNode
Edge = DiagramEdge
Cluster = DotCluster
MAX_EDGE_LENGTH = DIAGRAM_MAX_EDGE_LENGTH
draw = draw_graph
parse = parse_dot

__all__ = [
    "DIAGRAM_MAX_EDGE_LENGTH",
    "Diagram",
    "DiagramEdge",
    "DiagramError",
    "DiagramLayoutError",
    "DiagramNode",
    "Dot",
    "DotCluster",
    "DotError",
    "DotGraph",
    "Graph",
    "draw_graph",
    "parse_dot",
]
