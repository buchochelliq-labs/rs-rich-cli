# rs-rich-mermaid

<img src="https://raw.githubusercontent.com/buchochelliq-labs/rs-rich-cli/main/docs/assets/intuituive/mascot/crates/rs-rich-mermaid.svg" width="96" align="right" alt="Memo the elephant drawing a flowchart, a fish tail on her trunk">

Mermaid diagrams for [`rs-rich`](https://crates.io/crates/rs-rich), the Rust port
of Python's `rich`.

- **Flowcharts as text.** `graph` / `flowchart` diagrams in every direction
  (`TD`, `TB`, `BT`, `LR`, `RL`), the common node shapes, and solid, thick,
  dotted and labelled edges, drawn with box-drawing characters (or ASCII). No
  external tools. Since 0.0.4 the layout lives in
  [`rs-rich-diagram`](https://crates.io/crates/rs-rich-diagram), which also
  draws graphs built in code and DOT sources the same way;
  `Flowchart::to_graph` gives a parsed flowchart as its `Graph`.
- **Sequence diagrams as text** (0.0.7). Participants and actors on
  lifelines, every arrow (`->>`, `-->>`, `->`, `-->`, `-x`, `--x`, `-)`,
  `--)`, `<<->>`), self-messages, notes, activation bars, `loop` / `alt` /
  `opt` / `par` / `critical` / `break` / `rect` frames, `autonumber` and
  `title`, with columns sized to their text (`rich_mermaid::sequence`).
- **ER diagrams as text** (0.0.7). Entities as table boxes (type, name,
  `PK`/`FK`/`UK`, comment), identifying and non-identifying relationships
  through the same layered layout, and the cardinality at each end as text
  (`1`, `0..1`, `0..N`, `1..N`) next to the line (`rich_mermaid::er`).
- **Every diagram type through `mmdc`.** With the off-by-default `mmdc`
  feature and `Backend::Mmdc`, Mermaid's own CLI
  ([`@mermaid-js/mermaid-cli`](https://github.com/mermaid-js/mermaid-cli), Node
  and headless Chromium, installed separately) renders the diagram, shown
  through `rs-rich-art`: Sixel where the terminal supports it, otherwise
  quadrant blocks. The kinds drawn as text still prefer text when there are no
  real pixels.
- **Never an error on screen.** Anything that cannot be drawn is shown as its
  source in a code block, under a one-line note saying why.

```rust
use rich::Console;
use rich_mermaid::Mermaid;

let console = Console::new();
console.print(&Mermaid::new("graph LR\n  A[Write] --> B{Tests pass?}\n  B -->|yes| C[Ship]"));
```

In Markdown, register the plugin (or `MermaidFences` directly) so
```` ```mermaid ```` blocks are drawn:

```rust
use std::sync::Arc;
use rich::markdown::Markdown;
use rich_mermaid::MermaidFences;

let markdown = Markdown::new("```mermaid\ngraph TD\n  A --> B\n```")
    .fence_renderer(Arc::new(MermaidFences::default()));
```

`MermaidPlugin` registers the same fence renderer and a `mermaid` source
renderer through [`rs-rich-plugin-api`](https://crates.io/crates/rs-rich-plugin-api).

## Limits

The text renderer refuses (showing the source under a note) diagrams over
64 KiB, flowcharts and ER diagrams with over 500 nodes (entities) or 2000
edges (relationships), sequence diagrams with over 500 participants, 2000
messages, notes and block lines, or blocks nested over 32 deep, layouts needing more than 5000 points (nodes
plus one per rank a long edge crosses), and drawings over 2 million cells. A
link lengthened with extra dashes (`---->`) spans at most 10 ranks; longer
runs are drawn at that length.

## `mmdc` safety

`mmdc` starts a browser, so it only runs when asked for. The diagram goes
through a file in a private temporary directory, never a shell command line;
the process is stopped after a timeout (20 s by default); sources over 64 KiB
and images over 16 MiB (`MmdcOptions::max_output`) are refused, and at most
4 KiB of its output is read for an error message. `mmdc` stays in the
caller's process group, so Ctrl-C stops it too (Puppeteer then closes
Chromium); on a timeout it gets SIGTERM, then SIGKILL two seconds later. On
Unix a small `sh` watchdog removes the temporary directory even if the caller
is killed first.
