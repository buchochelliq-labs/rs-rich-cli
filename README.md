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

![rich rendering Markdown, a CSV table and Python source in a real terminal](docs/media/tapes/hero/hero.gif)

**[Play the terminal recordings](https://buchochelliq-labs.github.io/rs-rich-cli/recordings/)**:
real sessions you can pause and copy text from, re-run by CI on every change.

![The guided tour of the rich CLI, recorded by rich record](docs/media/tapes/tour/tour.gif)

**[Watch the CLI and art videos](https://buchochelliq-labs.github.io/rs-rich-cli/demos/)** ·
[User guide](docs/guide/index.md) · [Output gallery](docs/gallery.md) · [CLI guide](docs/cli.md)

This is the guided tour (`rich --demo`), recorded from a tape by `rich record`
in a real terminal: inspectors, diffs, Mermaid flowcharts drawn as text, code
themes, `--filter` and `--highlight`, rich-cli's options, reStructuredText, a
`rich choose` picker, banners and images.
[Tour and reproduction](docs/demos.md) ·
[0.0.15 release notes](docs/releases/0.0.15.md).

## Install and try

```bash
cargo install rs-rich-cli
rich --print '[bold magenta]Hello[/] [green]World[/]'
rich --help
rich --demo  # guided suite tour; Ctrl+C stops
```

For Rust applications, use `cargo add rs-rich` and import `rich::Console`.
See [Getting started](docs/getting-started.md) for library examples.

![Progress output exported by rs-rich](docs/assets/demos/progress.gif)

Progress and spinner GIFs replay exported library frames; the gallery includes
[tables, panels, trees, Markdown and JSON](docs/gallery.md).

> **Early releases, and the versions say so.** The crates version independently
> by ordinary SemVer; the number is *not* tied to the upstream release. Expect
> breaking API changes. Which upstream version is tracked lives in
> [`UPSTREAM.toml`](UPSTREAM.toml) and the line above. See [AGENTS.md](AGENTS.md)
> for why the version is not mirrored.

## Workspace

Each package follows independent SemVer. The Python releases being tracked are
recorded separately in [`UPSTREAM.toml`](UPSTREAM.toml).

A `vX.Y.Z` tag selects a coordinated workspace release; a `<crate>-vX.Y.Z`
tag selects only that crate. See [the release documentation](docs/BRANCHING.md#releases).

These are the manifest versions in this checkout. Publication status is recorded
in the release notes; the crates.io links show available packages.

<!-- BEGIN MANIFEST VERSIONS -->
| Package | Manifest version |
|---|---|
| [`rs-rich`](https://crates.io/crates/rs-rich) | `0.0.9` |
| [`rs-rich-plugin-api`](https://crates.io/crates/rs-rich-plugin-api) | `0.0.3` |
| [`rs-rich-macros`](https://crates.io/crates/rs-rich-macros) | `0.0.3` |
| [`rs-rich-ext`](https://crates.io/crates/rs-rich-ext) | `0.0.14` |
| [`rs-rich-cli`](https://crates.io/crates/rs-rich-cli) | `0.0.16` |
| [`rs-rich-art`](https://crates.io/crates/rs-rich-art) | `0.0.12` |
| [`rs-rich-mermaid`](https://crates.io/crates/rs-rich-mermaid) | `0.0.5` |
| [`rs-rich-lumis`](https://crates.io/crates/rs-rich-lumis) | `0.0.3` |
| [`rs-rich-record`](https://crates.io/crates/rs-rich-record) | `0.0.4` |
| [`rs-rich-interact`](https://crates.io/crates/rs-rich-interact) | `0.0.4` |
| [`rs-rich-micro`](https://crates.io/crates/rs-rich-micro) | `0.0.3` |
| [`rs-rich-diagram`](https://crates.io/crates/rs-rich-diagram) | `0.0.2` |
| [`rs-rich-data`](https://crates.io/crates/rs-rich-data) | `0.0.1` |
| [`rs-rich-ratatui`](https://crates.io/crates/rs-rich-ratatui) | `0.0.1` |
<!-- END MANIFEST VERSIONS -->

| Package | Role | Import / installed name |
|---|---|---|
| `rs-rich` | faithful port of Python rich 15.0.0 | `use rich` |
| `rs-rich-ext` | additions and plugin registry | `use rich_ext` |
| `rs-rich-macros` | checked markup and derive macros (via `rs-rich-ext`'s `macros` feature) | `use rich_ext::richf` |
| `rs-rich-cli` | CLI tracking Python rich-cli 1.8.1 | executable `rich` |
| `rs-rich-art` | FIGlet, image→ASCII, animated GIFs | `use rich_art` |
| `rs-rich-plugin-api` | the plugin contract: highlighters, themes, boxes, renderers, fence renderers, components | `use rich_plugin_api` |
| `rs-rich-mermaid` | Mermaid flowcharts drawn as text, ```` ```mermaid ```` fences | `use rich_mermaid` |
| `rs-rich-diagram` | graph model, layered layout and `Diagram` renderable; DOT (Graphviz) sources and ```` ```dot ```` fences | `use rich_diagram` |
| `rs-rich-data` | tabular data: CSV/TSV, JSONL, serde and Arrow row sources, type inference and column statistics | `use rich_data` |
| `rs-rich-ratatui` | ratatui interop: rich renderables as ratatui widgets, buffer and style conversions, ratatui widgets in rich-interact | `use rich_ratatui` |
| `rs-rich-lumis` | the lumis (tree-sitter) code highlighter | `use rich_lumis` |
| `rs-rich-record` | scripted terminal recordings (`rich record`) | `use rich_record` |
| `rs-rich-interact` | interactive components: pickers, input, forms, pagers, explorers | `use rich_interact` |
| `rs-rich-micro` | micro assets: emoji-sized inline images written `:micro:name:` | `use rich_micro` |
| `rs-rich` (PyPI) | Python bindings: Rich's API over the Rust core, plus the port's crates (`rs_rich.ext`, `rs_rich.chart`, `rs_rich.diagram`, `rs_rich.interact`, ...) | `import rs_rich` |

The published package names carry an `rs-` prefix because `rich` is already taken
on crates.io by an unrelated crate. The library targets keep the short names, so
you still write `use rich::…`.

The dependency arrow only ever points one way: `rich-cli → rich-ext → rich` and
`rich-cli → rich-art → rich`, with `rich-ext → rich-macros` behind ext's optional
`macros` feature; the other crates sit beside them (the full graph is in
[AGENTS.md](AGENTS.md)). Core depends on none of them, and nothing depends back
on the CLI. This keeps the core a clean mirror and
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
`JSON` · `Pretty` · `Traceback` · `Progress` (incl. `track()` and
`RenderableColumn`) · `Spinner` · `Status` · `Live` · `LogRender` · `Bar` ·
`Prompt` · emoji · filesize.

**Not ported:** Windows legacy-console support (so pre-Windows-10 terminals fall
back to plain output), Jupyter integration, and `inspect`/`repr` of Python objects
(no Rust equivalent — see [#19](https://github.com/buchochelliq-labs/rs-rich-cli/issues/19)).
A `log`/`tracing` handler (`RichHandler`) lives in `rs-rich-ext`.

**Known to differ from upstream**, deliberately and with reasons, in
[docs/DIVERGENCES.md](docs/DIVERGENCES.md) — most notably syntax highlighting uses
`syntect` rather than Pygments, so highlighted code is *not* byte-identical.

Per-module detail is in [docs/PORTING.md](docs/PORTING.md). What comes next, and
why, is in [docs/ROADMAP.md](docs/ROADMAP.md); the tracking epic is
[#16](https://github.com/buchochelliq-labs/rs-rich-cli/issues/16).

## Release history and development

The [0.0.15 release notes](docs/releases/0.0.15.md) document the newest
published cohort, charts and diagrams; older notes are in
[`docs/releases/`](docs/releases/), and [`CHANGELOG.md`](CHANGELOG.md) lists
every change. Later source changes are tracked in the [roadmap](docs/ROADMAP.md)
and development plans. The manifest table above describes this checkout;
crates.io is the source for available published versions.

The 0.0.9 cohort (core 0.0.5, ext 0.0.7, art 0.0.7, CLI 0.0.9) is also
[published](docs/releases/0.0.9-expanded.md). The [0.0.10 cohort](docs/releases/0.0.10.md)
(core 0.0.6, ext 0.0.8, art 0.0.8, CLI 0.0.10) is published: Progress time, rate and spinner columns, upstream's theme stack, `~~~`
strikethrough parity, multi-file watch, and quadrant, ANSI16 and grayscale image
modes with tone adjustments. The [0.0.11 cohort](docs/releases/0.0.11.md)
(core 0.0.7, macros 0.0.1 (new), ext 0.0.9, art 0.0.9, CLI 0.0.11) is
published: diagnostics, structured data, checked-markup macros,
`clap` and `tracing` integration, workflow renderables, and the `inspect`, `diff`,
`view`, `hex`, `unicode`, `env` and `capture` commands. The
[0.0.12 cohort](docs/releases/0.0.12.md) added the plugin platform
(`rs-rich-plugin-api`, `rs-rich-mermaid`, `rs-rich-lumis`) and the first
`rs-rich` Python package; [0.0.13](docs/releases/0.0.13.md) interactive
components (`rs-rich-interact`), third-party plugins and `rich record`; and
[0.0.14](docs/releases/0.0.14.md) composable interactive views, `rich explore`
and micro assets (`rs-rich-micro`); and [0.0.15](docs/releases/0.0.15.md)
terminal charts and diagrams (`rs-rich-diagram`, `rich chart`, `rich dot`,
`rich deps` and `rich schema`). See [copyable workflows](docs/recipes.md).

## Install

```bash
cargo add rs-rich                      # the library — then `use rich::…`
cargo install rs-rich-cli              # the `rich` command
```

`Syntax` (syntect) and `Markdown` (pulldown-cmark) are default features of
`rs-rich` and `rs-rich-ext`. If you use neither, turn them off and drop
syntect, its bincode 1.x and a second `fancy-regex` from your build:

```toml
rs-rich = { version = "0.0.9", default-features = false }
rs-rich-ext = { version = "0.0.13", default-features = false }  # if you use it
```

## Try it

```bash
cargo run -p rs-rich-cli            # capability demo (markup, color, extensions)
cargo run -p rs-rich-cli -- --help  # every supported flag
cargo run -p rs-rich-cli -- FILE    # print a file (type auto-detected)
```

The CLI covers upstream's `--markdown` · `--rst` · `--syntax` · `--json` · `--csv` ·
`--ipynb` · `--print` · `--rule` · `--panel` · `--padding` · `--pager` ·
`--export-html` · `--export-svg` and alignment and width flags, plus fetching an
`http(s)` URL directly. It adds `--jsonl` · `--log` · `--image` · `--gif` ·
`--diff` · `--sanitize` · `--report json` · `--watch` · `--batch` ·
`--theme-file` · `--format`. Preferred subcommands such as `rich json`,
`rich markdown`, `rich csv`, `rich jsonl` and `rich log` coexist with the legacy
flat flags.

0.0.11 adds tool commands: `inspect`
(JSON/YAML/TOML/XML/INI/CSV with `--select`, `--compare` and experimental
`--redact`), `diff` for text and patches, `view`, `hex` (alias `hexdump`),
`unicode`, `env`, `capture` (with experimental `--redact`), `ansi explain`,
`doctor`, `bench compare`, `completions`, `docs`, `config explain|reference`,
and rich-rendered help (`rich COMMAND --help`). 0.0.13 adds the rest of
rich-cli 1.8.1's options (`--head`/`--tail`, `-n`, `--guides`, `--lexer`,
`--emoji`, `--soft`, `--no-wrap`, `--max-width`, the `--text-*` alignments,
the rule options, `--force-terminal` and `--rst`), interactive commands for
scripts (`rich choose`, `filter`, `input`, `confirm` and `pager`) and
`rich record` for scripted terminal recordings. 0.0.14 adds `rich explore`
and `rich micro`. 0.0.15 draws data and structure: `rich chart` (CSV, JSON or
stdin as a sparkline, bars, lines, points or a heatmap), `rich dot` and
```` ```dot ```` fences (DOT drawn natively), `rich deps` (Cargo dependency
trees and graphs) and `rich schema` (a JSON Schema, SQL DDL or Arrow schema as
a tree, what changed between two, or an ER diagram with `--er`). See the [CLI guide](docs/guide/cli/walkthrough.md),
[Charts](docs/guide/ext/charts.md), [Diagrams](docs/guide/diagram/index.md)
and the [CLI reference](docs/cli-reference.md).

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
