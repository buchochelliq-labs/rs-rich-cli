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

The crate has its **own independent SemVer** (`0.0.x` while the API still
takes breaking changes); the version does not mirror upstream. The upstream
release it reflects (currently `rich` 15.0.0) is recorded in
[`UPSTREAM.toml`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/UPSTREAM.toml) and in the changelog.

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
  `Markdown` (including GFM tables), `Syntax`, `Json`, `Pretty`, `Traceback`,
  `LogRender`.

Live output
: `Live` for in-place redraw — both a deterministic manual-refresh flow and a
  background auto-refresh thread. `Progress` has upstream's columns (including
  `RenderableColumn`), a Live-driven display and `track()` over an iterator.

Input
: `Prompt`, `Confirm`, `IntPrompt` and `FloatPrompt`.

Export
: `export_text`, `export_html` (inline **and** CSS-class forms, self-contained),
  and `export_svg` — an SVG of a terminal window that loads its font from a
  CDN, as upstream's does.

## Extending it

The core ships only upstream's own behaviour. Local features and the plugin
registry live in [`rs-rich-ext`](https://crates.io/crates/rs-rich-ext); ASCII
art lives in [`rs-rich-art`](https://crates.io/crates/rs-rich-art). That boundary is what keeps upstream syncs from
turning into merge conflicts — see
[`AGENTS.md`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/AGENTS.md).

## Licence

MIT.

The optional `protocol::ConsoleEnvironment` extension seam carries an immutable
`RenderEnvironment` through nested renderables. Legacy consoles have no attached
environment and preserve their default behavior. Destination policy implementations
live in `rs-rich-ext`, keeping this crate independent of extensions and CLI code.
