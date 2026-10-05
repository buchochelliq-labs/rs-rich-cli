"""``rs_rich.diagram``: ``rs-rich-diagram`` from Python (0.0.15 workstream 5).

Rich has no diagrams, so each expected drawing is what the Rust crate draws
for the same graph: the cases are ``rich_diagram``'s own doc examples, which
its doc tests assert byte for byte.
"""

from __future__ import annotations

import io

import pytest

from rs_rich import _native, diagram
from rs_rich.console import Console
from rs_rich.markdown import Markdown

API_DB = "┌─────┐         ┌────┐\n│ API ├──reads─►│ DB │\n└─────┘         └────┘\n"


def drawn(renderable, width: int = 40) -> str:
    """What `print` writes for `renderable` at `width`: the drawing and the
    one newline `print` ends it with (no blank line after it)."""
    out = io.StringIO()
    Console(file=out, width=width, color_system=None).print(renderable)
    text = out.getvalue()
    assert not text.endswith("\n\n"), repr(text)
    return text


def api_db() -> diagram.Graph:
    return diagram.Graph("LR").node("api", "API").node("db", "DB").edge("api", "db", label="reads")


def test_the_module_exports_native_objects_and_the_rust_names():
    for name in diagram.__all__:
        assert getattr(diagram, name) is getattr(_native, name)
    assert diagram.Node is diagram.DiagramNode
    assert diagram.parse is diagram.parse_dot
    assert issubclass(diagram.DotError, diagram.DiagramError)


def test_a_graph_built_in_code_draws():
    assert drawn(diagram.Diagram(api_db())) == API_DB
    assert diagram.draw_graph(api_db()) == API_DB.splitlines()


def test_the_builder_chains_and_reads_back():
    graph = diagram.Graph().node("a", shape="round").edge("a", "b", stroke="dotted", end=None, start="circle")
    assert repr(graph) == "<Graph TD nodes=2 edges=1>"
    assert [(n.id, n.label, n.shape) for n in graph.nodes] == [("a", "a", "round"), ("b", "b", "rect")]
    edge = graph.edges[0]
    assert (edge.source, edge.target, edge.stroke, edge.start, edge.end) == (0, 1, "dotted", "circle", None)
    assert graph.node_index("b") == 1
    assert len(graph) == 2
    graph.direction = "lr"
    assert graph.direction == "LR"
    graph.link("b", "c", label="peer")
    assert graph.edges[1].end is None and graph.edges[1].label == "peer"


def test_a_diagram_copies_its_graph():
    graph = api_db()
    picture = diagram.Diagram(graph)
    graph.node("cache")
    assert len(picture.graph) == 2
    assert drawn(picture) == API_DB


def test_ascii():
    lines = diagram.Diagram(api_db()).drawing(ascii=True)
    assert all(line.isascii() for line in lines)
    assert drawn(diagram.Diagram(api_db(), ascii=True)) == "\n".join(lines) + "\n"


def test_bad_names_are_value_errors():
    with pytest.raises(ValueError, match="direction"):
        diagram.Graph("up")
    with pytest.raises(ValueError, match="shape"):
        diagram.Graph().node("a", shape="blob")
    with pytest.raises(ValueError, match="stroke"):
        diagram.Graph().edge("a", "b", stroke="wavy")
    with pytest.raises(ValueError, match="min_length"):
        diagram.Graph().edge("a", "b", min_length=0)


def test_dot_draws_natively():
    out = drawn(diagram.Dot("digraph { rankdir=LR; a -> b }"))
    assert "│ a ├─►│ b │" in out


def test_parse_dot_reads_the_graph():
    parsed = diagram.parse_dot('digraph G { label="Services"; subgraph cluster_b { label="Backend"; api; db } api -> db }')
    assert parsed.directed and not parsed.strict
    assert (parsed.name, parsed.label) == ("G", "Services")
    assert [c.id for c in parsed.clusters] == ["cluster_b"]
    assert parsed.clusters[0].label == "Backend"
    assert [n.id for n in parsed.graph.nodes] == ["api", "db"]
    assert diagram.Dot("graph { a -- b }").parsed().directed is False


def test_dot_errors_name_the_line_and_the_construct():
    with pytest.raises(diagram.DotError) as raised:
        diagram.parse_dot("digraph {\n  a -> b\n  a:p -> c\n}")
    assert raised.value.line == 3
    assert raised.value.construct == "a node port (`a:…`)"
    assert str(raised.value).startswith("line 3: a node port")
    with pytest.raises(diagram.DotError) as syntax:
        diagram.parse_dot("digraph { a -> }")
    assert syntax.value.construct is None
    # The renderable shows what it cannot draw as source under a note.
    assert "a:p" in drawn(diagram.Dot("digraph { a:p -> b }"), width=60)


def test_markdown_dot_fences_are_left_to_the_plugin_host():
    # Without a fence renderer a ```dot block stays code, as Rich draws it.
    out = io.StringIO()
    Console(file=out, width=40, color_system=None).print(Markdown("```dot\ndigraph { a -> b }\n```"))
    assert "digraph" in out.getvalue()
