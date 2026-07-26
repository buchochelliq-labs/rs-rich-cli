# rs-rich-cli

A **Rust port of the Python [`rich`](https://github.com/Textualize/rich)** library
and the [`rich-cli`](https://github.com/Textualize/rich-cli) tool — for rich text,
color, and beautiful formatting in the terminal.

> **Version-locked to upstream.** The core crate's version *is* the upstream
> `rich` version it mirrors, so the number tells you exactly which features you
> get. Our own additions live in a separate crate and never move that number.
> See [AGENTS.md](AGENTS.md).

Currently tracking **`rich` 15.0.0** and **`rich-cli` 1.8.1**
(see [`UPSTREAM.toml`](UPSTREAM.toml)).

## Workspace

| crate | what it is | version |
|-------|------------|---------|
| [`crates/rich`](crates/rich) | faithful port of the `rich` library | `15.0.0` (mirrors upstream) |
| [`crates/rich-ext`](crates/rich-ext) | our additions + the internal plugin registry | `0.1.0` (independent) |
| [`crates/rich-cli`](crates/rich-cli) | the `rich` command-line tool | `1.8.1` (mirrors upstream) |

The dependency arrow only ever points one way: `rich-cli → rich-ext → rich`. This
keeps the core a clean mirror and makes upstream syncs a mechanical diff-and-port.
See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## Status

Early. The **first vertical slice** is implemented and parity-tested against real
`rich` 15.0.0: `color` · `style` · `cells` · `segment` · `markup` · `text` ·
`theme` · `console` · extension points (`protocol`). The full roadmap is tracked in
GitHub issues and [docs/PORTING.md](docs/PORTING.md).

## Try it

```bash
cargo run -p rich-cli            # capability demo (markup, color, extensions)
cargo run -p rich-cli -- --help  # planned subcommands
cargo run -p rich-cli -- FILE    # print a file
```

Library usage:

```rust
use rich::{Console, ColorSystem};
use rich_ext::ConsoleExt;

let mut console = Console::new();
console.install_extensions();                 // optional: our highlighters etc.
console.print_str("[bold red]Hello[/] [green]World[/] — 42");
```

## Develop

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test
```

Golden parity fixtures are captured from the real Python library:

```bash
pip install "rich==15.0.0"
python scripts/capture_golden.py
```

## Extending

Add your own highlighters/renderables in `rich-ext` and register them onto a
`Console` — the core never needs to know. See [docs/PLUGINS.md](docs/PLUGINS.md).

## Contributing / maintaining

The maintenance contract — the mirror/ext boundary, versioning, the parity
workflow, and how to sync a new upstream release — is in
[AGENTS.md](AGENTS.md) and [CONTRIBUTING.md](CONTRIBUTING.md). There are two
skills to drive the common flows: `sync-upstream` and `port-module`.

## License

MIT — matching upstream `rich`. See [LICENSE](LICENSE).
