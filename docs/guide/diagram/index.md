# Diagrams (rs-rich-diagram)

`rs-rich-diagram` (`rich_diagram`, new in 0.0.15) draws graphs in the
terminal: a graph model, a layered layout, and a `Diagram` renderable that
draws the result with box-drawing characters, or ASCII. `rich` draws no
graphs, so this crate is an rs-rich addition, not a port; it depends on core
`rs-rich` only, and core is unchanged.

The layout is the one [`rs-rich-mermaid`](https://crates.io/crates/rs-rich-mermaid)
has drawn flowcharts with since 0.0.12, lifted out so that every diagram
source draws the same way. A Mermaid flowchart is now converted into a
`rich_diagram::Graph` and drawn here, and a graph built in code draws exactly
as the equivalent Mermaid source does.

## Building a graph in code

`Graph`'s builder chains. `node` and `edge` add items; `label` changes
whichever was added last, `shape` the last node, and `stroke`, `heads` and
`min_length` the last edge. An edge to an id with no node yet adds one,
labelled with its id.

```rust
use rich::Console;
use rich_diagram::{Diagram, Direction, Graph, Shape, Stroke};

let graph = Graph::new(Direction::TopDown)
    .node("web", "Browser").shape(Shape::Round)
    .node("api", "API")
    .node("db", "Postgres").shape(Shape::Cylinder)
    .node("jobs", "Job queue")
    .node("worker", "Worker").shape(Shape::Subroutine)
    .edge("web", "api").label("HTTPS")
    .edge("api", "db").label("reads")
    .edge("api", "jobs").stroke(Stroke::Dotted)
    .edge("jobs", "worker")
    .edge("worker", "db").stroke(Stroke::Thick).label("writes");

Console::new().print(&Diagram::new(graph));
```

`cargo run -p rs-rich-diagram --example services` prints:

```text
      ╭─────────╮
      │ Browser │
      ╰────┬────╯
           │
         HTTPS
           ▼
        ┌─────┐
        │ API │
        └─┬─┬─┘
          ┆ │
      ┌┄┄┄┘ └──┐
      ▼        │
┌───────────┐  │
│ Job queue │  │
└─────┬─────┘  │
      │        │
      ▼        │
┌┬─────────┬┐  │
││ Worker  ││  │
└┴────┬────┴┘  │
      ┃        │
      └━━━┐ ┌──┘
   writes ┃ │ reads
          ▼ ▼
     ╭───────────╮
     │ Postgres  │
     ╰───────────╯
```

`Graph::from_parts(direction, nodes, edges)` takes finished `Node`s and
`Edge`s instead, as a parser builds them.

## The model

- **Direction**: `TopDown`, `BottomUp`, `LeftRight` or `RightLeft`.
- **Nodes**: an id, a label (`\n` breaks a line) and a `Shape`: `Rect`,
  `Round`, `Stadium`, `Subroutine`, `Cylinder`, `Circle`, `DoubleCircle`,
  `Asymmetric` (a flag), `Rhombus` (a decision diamond), `Hexagon`,
  `Parallelogram`, `ParallelogramAlt`, `Trapezoid` and `TrapezoidAlt`. Each
  is a box whose corners and sides suggest the shape.
- **Edges**: a source and a target, an optional label, a `Stroke` (`Solid`,
  `Thick`, `Dotted`, or `Invisible`: laid out but not drawn), a `Head` at each
  end (`None`, `Arrow`, `Circle`, `Cross`), and a minimum number of ranks to
  span (at most 10). `edge` adds an arrow; `link` adds an undirected edge,
  with no heads; `heads(Head::Arrow, Head::Arrow)` makes it two-way.

Clusters (subgraph frames) are not drawn yet: the layout lays every node out
in one graph, as Mermaid's subgraphs always have been.

## The layout

`rich_diagram::draw(&graph, ascii)` returns a `Drawing`: plain lines of text
and their width. The layout is layered (Sugiyama-style):

1. cycles are broken by reversing the edges a depth-first search finds going
   back, and nodes are ranked by longest path;
2. edges spanning several ranks get one-cell dummy points;
3. each rank is ordered by barycentre sweeps, keeping the order with the
   fewest crossings;
4. nodes are placed along the rank by averaging their neighbours' centres;
5. every edge is routed orthogonally, with its own port on a node and its own
   track in the gap between two ranks.

A self-loop is marked `↻` (`@` in ASCII). The layout refuses graphs needing
more than 5000 points (nodes, plus one per rank a long edge crosses) and
drawings over 2 million cells; `Diagram` shows a one-line note instead.

## Width and ASCII

The layout takes no width: a drawing is as wide as the graph needs. `Diagram`
measures to that width, so it sits in a `Table` cell, a `Panel` or
`Columns` like any renderable, and given less it **crops**: each line is cut
at the right edge and rows left empty are dropped. The output never exceeds
the width it is given, and the same graph at the same width always crops the
same way. `Diagram::drawing` gives the whole drawing when you want to scroll
or page it instead. (Mermaid adds a note saying how much it cropped; the
lines above the note are the same.)

ASCII follows the console (`Console::ascii_only`, set for a non-UTF
encoding) unless you choose with `Diagram::ascii(true)`. The same graph, with
`cargo run -p rs-rich-diagram --example services -- 80 ascii`:

```text
      .---------.
      | Browser |
      '----+----'
           |
         HTTPS
           v
        +-----+
        | API |
        +-+-+-+
          : |
      +...+ +--+
      v        |
+-----------+  |
| Job queue |  |
+-----+-----+  |
      |        |
      v        |
++---------++  |
|| Worker  ||  |
++----+----++  |
      |        |
      +===+ +--+
   writes | | reads
          v v
     .-----------.
     | Postgres  |
     '-----------'
```

Labels have their control characters removed before drawing, so a label
from untrusted input cannot reach the terminal as an escape sequence.

## From Mermaid

The Mermaid source for the first graph draws the same lines:

```text
graph TD
  web(Browser) -->|HTTPS| api[API]
  api -->|reads| db[(Postgres)]
  api -.-> jobs[Job queue]
  jobs --> worker[[Worker]]
  worker ==>|writes| db
```

`rich_mermaid::Flowchart::to_graph` gives the `Graph` a parsed flowchart
draws through. Mermaid's tests check both directions: its snapshots built
with the builder, and random graphs written both ways.
