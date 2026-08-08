# rs-rich-cli

[![CI](https://github.com/buchochelliq-labs/rs-rich-cli/actions/workflows/ci.yml/badge.svg)](https://github.com/buchochelliq-labs/rs-rich-cli/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/rs-rich.svg)](https://crates.io/crates/rs-rich)
[![docs.rs](https://img.shields.io/docsrs/rs-rich)](https://docs.rs/rs-rich)
[![MSRV](https://img.shields.io/crates/msrv/rs-rich.svg)](https://github.com/buchochelliq-labs/rs-rich-cli#develop)
[![License](https://img.shields.io/crates/l/rs-rich.svg)](LICENSE)
[![Docs site](https://img.shields.io/badge/docs-site-blue)](https://buchochelliq-labs.github.io/rs-rich-cli/)

A **Rust port of the Python [`rich`](https://github.com/Textualize/rich)** library
and the [`rich-cli`](https://github.com/Textualize/rich-cli) tool — for rich text,
color, and beautiful formatting in the terminal.

**📖 [Documentation, tutorial and gallery](https://buchochelliq-labs.github.io/rs-rich-cli/)**

Currently tracking **`rich` 15.0.0** and **`rich-cli` 1.8.1**
(see [`UPSTREAM.toml`](UPSTREAM.toml)).

![Console markup](docs/assets/markup.svg)

> **`0.0.1` — early, and the version says so.** The crates version independently
> by ordinary SemVer; the number is *not* tied to the upstream release. Expect
> breaking API changes. Which upstream version is tracked lives in
> [`UPSTREAM.toml`](UPSTREAM.toml) and the line above. See [AGENTS.md](AGENTS.md)
> for why the version is not mirrored.

## Workspace

**All four are published on crates.io at `0.0.1`.**

| crate | crates.io | docs | `use` as | what it is |
|-------|-----------|------|----------|------------|
| [`crates/rich`](crates/rich) | [![rs-rich](https://img.shields.io/crates/v/rs-rich.svg)](https://crates.io/crates/rs-rich) | [docs.rs](https://docs.rs/rs-rich) | `rich` | faithful port of the `rich` library |
| [`crates/rich-ext`](crates/rich-ext) | [![rs-rich-ext](https://img.shields.io/crates/v/rs-rich-ext.svg)](https://crates.io/crates/rs-rich-ext) | [docs.rs](https://docs.rs/rs-rich-ext) | `rich_ext` | our additions + the plugin registry |
| [`crates/rich-cli`](crates/rich-cli) | [![rs-rich-cli](https://img.shields.io/crates/v/rs-rich-cli.svg)](https://crates.io/crates/rs-rich-cli) | — *(binary)* | *(binary `rich`)* | the `rich` command-line tool |
| [`crates/rich-art`](crates/rich-art) | [![rs-rich-art](https://img.shields.io/crates/v/rs-rich-art.svg)](https://crates.io/crates/rs-rich-art) | [docs.rs](https://docs.rs/rs-rich-art) | `rich_art` | FIGlet text, image→ASCII, animated GIFs |

The published package names carry an `rs-` prefix because `rich` is already taken
on crates.io by an unrelated crate. The library targets keep the short names, so
you still write `use rich::…`.

The dependency arrow only ever points one way: `rich-cli → rich-ext → rich`
(`rich-art` also depends only on `rich`). This keeps the core a clean mirror and
makes upstream syncs a mechanical diff-and-port. See
[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## Status

**Usable, not complete.** Read that literally — the parts listed as done are
byte-parity tested against real Python `rich` 15.0.0, and the parts that aren't
listed genuinely aren't there.

**Ported and parity-tested:** colour (truecolor/256/standard downgrade) · styles ·
markup · `Text` (wrapping, justification incl. full, overflow, spans) ·
`Console` (capture, export to HTML/SVG, paging, control codes) · `Segment` ·
themes · highlighters · `Table` · `Panel` · `Rule` · `Align` · `Padding` ·
`Columns` · `Constrain` · `Tree` · `Layout` · `Screen` · `Markdown` · `Syntax` ·
`JSON` · `Pretty` · `Traceback` · `Progress` · `Spinner` · `Status` · `Live` ·
`Bar` · `Prompt` · emoji · filesize.

**Not ported:** Windows legacy-console support (so pre-Windows-10 terminals fall
back to plain output), Jupyter integration, `inspect`/`repr` of Python objects
(no Rust equivalent — see [#19](https://github.com/buchochelliq-labs/rs-rich-cli/issues/19)),
and a `log`/`tracing` handler.

**Known to differ from upstream**, deliberately and with reasons, in
[docs/DIVERGENCES.md](docs/DIVERGENCES.md) — most notably syntax highlighting uses
`syntect` rather than Pygments, so highlighted code is *not* byte-identical.

Per-module detail is in [docs/PORTING.md](docs/PORTING.md); the roadmap is in
GitHub issues.

## Install

```bash
cargo add rs-rich                      # the library — then `use rich::…`
cargo install rs-rich-cli              # the `rich` command
```

## Try it

```bash
cargo run -p rs-rich-cli            # capability demo (markup, color, extensions)
cargo run -p rs-rich-cli -- --help  # every supported flag
cargo run -p rs-rich-cli -- FILE    # print a file (type auto-detected)
```

The CLI covers `--markdown` · `--syntax` · `--json` · `--csv` · `--ipynb` ·
`--print` · `--rule` · `--panel` · `--padding` · `--pager` · `--export-html` ·
`--export-svg` · alignment and width flags, plus fetching an `http(s)` URL
directly.

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
