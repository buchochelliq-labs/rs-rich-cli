# rs-rich-mermaid

Mermaid diagrams for [`rs-rich`](https://crates.io/crates/rs-rich), the Rust port
of Python's `rich`.

- **Flowcharts as text.** `graph` / `flowchart` diagrams in every direction
  (`TD`, `TB`, `BT`, `LR`, `RL`), the common node shapes, and solid, thick,
  dotted and labelled edges, drawn with box-drawing characters (or ASCII). No
  external tools.
- **Every diagram type through `mmdc`.** With the off-by-default `mmdc`
  feature and `Backend::Mmdc`, Mermaid's own CLI
  ([`@mermaid-js/mermaid-cli`](https://github.com/mermaid-js/mermaid-cli), Node
  and headless Chromium, installed separately) renders the diagram, shown
  through `rs-rich-art`: Sixel where the terminal supports it, otherwise
  quadrant blocks. A flowchart still prefers text when there are no real pixels.
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

## `mmdc` safety

`mmdc` starts a browser, so it only runs when asked for. The diagram goes
through a file in a private temporary directory, never a shell; the process is
stopped after a timeout (20 s by default); and sources over 64 KiB are refused.
