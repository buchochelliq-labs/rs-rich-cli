# rich

A **faithful** Rust port of the Python [`rich`](https://github.com/Textualize/rich)
terminal-rendering library.

```rust
use rich::{Console, Panel, Table, Text};

let console = Console::builder().build();
console.print_str("[bold magenta]Hello[/] from rich");
```

## Faithful means byte-parity

This crate mirrors upstream module-for-module, and correctness is verified by
**golden tests captured from real Python `rich`** — the Rust output is compared
byte-for-byte against what upstream produces for the same input. CI regenerates
those fixtures from the pinned upstream release on every run and fails if they
drift.

Its **version tracks the upstream release it reflects** (currently `15.0.0`),
so the version number tells you which upstream features exist. It is bumped
only when syncing upstream — never to ship a local feature.

Every intentional deviation is recorded in
[`docs/DIVERGENCES.md`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/docs/DIVERGENCES.md).

## What's here

Console + styling
: `Console`, `Style`, `Color` (16/256/truecolor with downgrade), `Segment`,
  console markup (`[bold red]…[/]`), `Theme`, control codes, `Screen`.

Text
: `Text` with spans, word wrapping, justification, overflow (crop / fold /
  ellipsis), emoji shortcodes, and an ANSI decoder that round-trips SGR and
  OSC 8 hyperlinks back into styled text.

Renderables
: `Panel`, `Table`, `Tree`, `Layout`, `Columns`, `Align`, `Padding`,
  `Constrain`, `Rule`, `Bar`, `ProgressBar`, `Progress`, `Spinner`, `Status`,
  `Markdown` (including GFM tables), `Syntax`, `Json`, `Pretty`, `Traceback`.

Live output
: `Live` for in-place redraw — both a deterministic manual-refresh flow and a
  background auto-refresh thread.

Export
: `export_text`, `export_html` (inline **and** CSS-class forms), and
  `export_svg` — a self-contained SVG of a terminal window.

## Extending it

The core ships only upstream's own behaviour. Local features and the plugin
registry live in [`rich-ext`](../rich-ext); ASCII art lives in
[`rich-art`](../rich-art). That boundary is what keeps upstream syncs from
turning into merge conflicts — see
[`AGENTS.md`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/AGENTS.md).

## Licence

MIT.
