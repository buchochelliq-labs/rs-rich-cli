# Diagrams

```python
from rs_rich import diagram
```

`rs_rich.diagram` is the `rs-rich-diagram` crate from Python: graphs drawn
with box-drawing characters (or ASCII), laid out in layers, and DOT
(Graphviz) sources parsed and drawn natively. Rich has no counterpart. The
same layout draws [Mermaid](mermaid.md) flowcharts.

| Name | What it is |
|---|---|
| `Graph` | a graph built in code; `node` and `edge` return it, so calls chain |
| `Diagram` | a `Graph` as a renderable |
| `draw_graph(graph, ascii=False)` | the drawing as a list of lines |
| `Dot` | a DOT source as a renderable |
| `parse_dot(source)` | a DOT source read into a `DotGraph`, or `DotError` |

`DiagramNode`, `DiagramEdge` and `DotCluster` are also exported as `Node`,
`Edge` and `Cluster`, and `parse_dot` and `draw_graph` as `parse` and
`draw`, their Rust names.

## Graphs

```text
Graph(direction="TD")
Graph.node(id, label=None, *, shape="rect") -> Graph
Graph.edge(source, target, *, label=None, stroke="solid", start=None, end="arrow",
           min_length=1) -> Graph
Graph.link(source, target, *, label=None) -> Graph
Diagram(graph, *, ascii=None)
```

`direction` is `"TD"` (or `"TB"`), `"BT"`, `"LR"` or `"RL"`. Shapes are
`rect`, `round`, `stadium`, `subroutine`, `cylinder`, `circle`,
`double_circle`, `asymmetric`, `rhombus`, `hexagon`, `parallelogram`,
`parallelogram_alt`, `trapezoid` and `trapezoid_alt`; strokes `solid`,
`thick`, `dotted` and `invisible`; heads `arrow`, `circle`, `cross` or
`None`. An edge to an id with no node adds one labelled with its id, and
`node` on an existing id relabels it. `link` is an edge with no heads.

A `Diagram` copies the graph when it is made. It measures to the width of
its drawing; given less, it is cropped, never wrapped. `ascii=True` draws
with ASCII only; `None` follows the console.

```python
from rs_rich.console import Console
from rs_rich import diagram

graph = (
    diagram.Graph("LR")
    .node("web", "Browser", shape="round")
    .node("api", "API")
    .node("db", "Postgres", shape="cylinder")
    .edge("web", "api", label="HTTPS")
    .edge("api", "db", label="reads")
)
console = Console(width=60, color_system=None)
console.print(diagram.Diagram(graph))
print(diagram.draw_graph(diagram.Graph().edge("a", "b"), ascii=True))
```

```text
╭─────────╮         ┌─────┐         ╭──────────╮
│ Browser ├──HTTPS─►│ API ├──reads─►│ Postgres │
╰─────────╯         └─────┘         ╰──────────╯
['+---+', '| a |', '+-+-+', '  |', '  v', '+---+', '| b |', '+---+']
```

`Graph.nodes` and `Graph.edges` read the graph back (`DiagramNode`: `id`,
`label`, `shape`; `DiagramEdge`: `source` and `target` as node indexes,
`label`, `stroke`, `start`, `end`, `length`). A graph too large to lay out
(more than 2000 edges or 5000 nodes, checked before any layout work, or a
layout past its point or cell caps) renders as a one-line note;
`draw_graph` and `Diagram.drawing()` raise `DiagramLayoutError` instead.

## DOT

```text
Dot(source, *, ascii=None)
parse_dot(source) -> DotGraph
```

`Dot` draws a DOT source with the same layout: `digraph` and `graph`,
`rankdir`, node shapes and labels, edge styles and labels, chains, and
clusters (drawn as frames around their nodes), and `rank=same`. What the parser does not
support (a node port, an HTML-like label, the `record` shape, ...) is shown
as the source under a note naming it and its line. `parse_dot` raises
`DotError` instead, with `line` and `construct` (`None` for a syntax error).
A source over 64 KB, with more than 500 nodes or 2000 edges (counted as
`{ ... }` groups expand), or nested more than 64 levels deep is refused the
same way, never drawn in part.
A `DotGraph` has `graph`, `directed`, `strict`, `name`, `label`, `clusters`
and `notes`.

```python
console.print(diagram.Dot("digraph { rankdir=LR; build -> test -> ship }"))
try:
    diagram.parse_dot("digraph {\n  a -> b\n  a:out -> c\n}")
except diagram.DotError as error:
    print(error.line, error.construct)
```

```text
╭───────╮  ╭──────╮  ╭──────╮
│ build ├─►│ test ├─►│ ship │
╰───────╯  ╰──────╯  ╰──────╯
3 a node port (`a:…`)
```

From the shell, `rich dot FILE` draws a DOT file and `rich --markdown` draws
```` ```dot ```` fences
([Diagrams guide](https://buchochelliq-labs.github.io/rs-rich-cli/guide/diagram/)).
