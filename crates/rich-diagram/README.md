# rs-rich-diagram

Graph diagrams for [rs-rich](https://github.com/buchochelliq-labs/rs-rich-cli),
the Rust port of Python's `rich`: a graph model, a layered layout, and a
renderable that draws the result with box-drawing characters, or ASCII. This
crate is an rs-rich addition, not a port: `rich` draws no graphs. It depends
on core `rs-rich` only.

The layout is the one
[`rs-rich-mermaid`](https://crates.io/crates/rs-rich-mermaid) has always drawn
flowcharts with, lifted out so every diagram source draws the same way.
Mermaid flowcharts now render through this crate, unchanged.

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

## The model

- **`Graph`**: nodes and edges flowing in a `Direction` (`TopDown`,
  `BottomUp`, `LeftRight`, `RightLeft`). The builder's `node` and `edge` add
  items; `label` changes whichever was added last, `shape` the last node,
  and `stroke`, `heads` and `min_length` the last edge. An edge to an id with
  no node yet adds one, labelled with its id. `Graph::from_parts` takes
  finished nodes and edges, as a parser builds them.
- **`Node`**: an id, a label (`\n` breaks a line) and a `Shape`: a box,
  rounded, stadium, subroutine, cylinder, circle, flag, diamond (`Rhombus`),
  hexagon, parallelograms and trapezoids.
- **`Edge`**: a source and a target, an optional label, a `Stroke` (solid,
  thick, dotted, or invisible: laid out but not drawn), a `Head` at each end
  (none, arrow, circle, cross; none at both ends is an undirected edge), and a
  minimum number of ranks to span.

## Drawing

`draw(&graph, ascii)` lays the graph out and returns a `Drawing`: plain lines
of text and their width. The layout breaks cycles, ranks nodes by longest
path, splits long edges with dummy points, orders each rank to cut crossings,
and routes every edge orthogonally with its own port and track. A self-loop
is marked `↻`.

`Diagram` is the drawing as a renderable. It measures to the drawing's width
and, given less, crops each line at the right edge, so it never overflows a
`Table` cell, a `Panel` or the console, and crops the same way every time.
ASCII follows the console (`Console::ascii_only`) unless set with
`Diagram::ascii`; the ASCII form uses `+-|` junctions, `=` and `:`/`.` for
thick and dotted lines, and `v^<>` heads.

Labels have their control characters removed before drawing, so a label from
untrusted input cannot reach the terminal as an escape sequence.

## DOT (Graphviz)

`rich_diagram::dot::parse` reads the DOT people write by hand (`graph` and
`digraph`, `strict`, node and edge statements with chains and `{ … }` groups,
`label`/`shape`/`style` and the other common attributes, `node`/`edge`/`graph`
defaults, subgraphs and `cluster…` clusters, `rank=same`, quoted IDs,
comments) into a `Graph`, and the `Dot` renderable draws a source through the
layout:

```rust
use rich::Console;
use rich_diagram::Dot;

Console::new().print(&Dot::new("digraph { rankdir=LR; web -> api -> db }"));
```

What the drawing cannot represent (node ports, HTML-like labels, `record`
shapes, several graphs in one file) is refused with a `DotError` naming the
construct and its line, never drawn partially.

Features, both off by default:

- `plugin`: `plugin::DotPlugin`, a `dot` (and `graphviz`) fence renderer and
  a `dot` source renderer through `rs-rich-plugin-api`.
- `graphviz`: `graphviz::render_svg`, which runs Graphviz's own `dot -Tsvg`
  (installed separately) with the source on stdin, a timeout and size caps.

## Clusters and same-rank groups

A `Cluster` (a DOT `subgraph cluster_…`, or `Graph::cluster` /
`Graph::add_cluster` in code) is drawn as a dashed frame around its nodes,
its label in the top border; nested clusters draw nested frames. The layout
keeps a cluster's nodes together in every rank and moves what is outside a
frame clear of it; edges cross frames whole. A same-rank group (DOT
`{ rank=same; a; b }`, or `Graph::same_rank`) is drawn in one rank, unless an
edge joins two of its nodes: a layered drawing has no edges within a rank,
so that group is dropped with a note in `Drawing::notes`. A graph with
neither draws exactly as before.

```text
         ┌╌ Backend ╌╌╌╌╌╌┐
┌─────┐  ╎ ┌─────┐  ┌────┐╎
│ web ├───►│ api ├─►│ db │╎
└─────┘  ╎ └─────┘  └────┘╎
         └╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┘
```

## ER diagrams

`rich_diagram::er` draws entity-relationship diagrams from an `ErModel`
built in code: entities with columns (type, `PK`, `FK`, `UQ`, nullable),
relationships from one entity's columns to another's with an optional
cardinality, and groups, drawn as frames. The model knows no source format,
so a schema or SQL DDL reader fills it.

```rust
use rich::Console;
use rich_diagram::er::{Cardinality, Column, Entity, ErDiagram, ErModel, Relationship};

let model = ErModel::new()
    .entity(Entity::new("users")
        .column(Column::new("id").data_type("int").primary_key())
        .column(Column::new("email").data_type("text").unique()))
    .entity(Entity::new("orders")
        .column(Column::new("id").data_type("int").primary_key())
        .column(Column::new("user_id").data_type("int").foreign_key())
        .column(Column::new("note").data_type("text").nullable()))
    .relationship(Relationship::new("orders", "users")
        .columns(["user_id"], ["id"])
        .cardinality(Cardinality::ManyToOne));
Console::new().print(&ErDiagram::new(model));
```

```text
┌────────────────────┐                       ┌─────────────────┐
│       orders       │                       │      users      │
├────────────────────┤                       ├─────────────────┤
│ id       int    PK │ ┌─user_id → id (N:1)─►│ id     int   PK │
│ user_id  int    FK ├─┘                     │ email  text  UQ │
│ note     text?     │                       └─────────────────┘
└────────────────────┘
```

## Limits

The layout refuses (`DrawError`; `Diagram` shows a one-line note instead)
graphs needing more than 5000 points (nodes plus one per rank a long edge
crosses) and drawings over 2 million cells. An edge spans at most 10 ranks.
Mermaid's subgraphs are not passed on as clusters yet, so they are drawn
without frames.
