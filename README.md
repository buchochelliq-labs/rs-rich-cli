# rs-rich-cli

[![CI](https://github.com/buchochelliq-labs/rs-rich-cli/actions/workflows/ci.yml/badge.svg)](https://github.com/buchochelliq-labs/rs-rich-cli/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/rs-rich.svg)](https://crates.io/crates/rs-rich)
[![docs.rs](https://img.shields.io/docsrs/rs-rich)](https://docs.rs/rs-rich)
[![MSRV](https://img.shields.io/crates/msrv/rs-rich.svg)](https://github.com/buchochelliq-labs/rs-rich-cli#develop)
[![License](https://img.shields.io/crates/l/rs-rich.svg)](LICENSE)
[![Docs site](https://img.shields.io/badge/docs-site-blue)](https://buchochelliq-labs.github.io/rs-rich-cli/)

**Python's [`rich`](https://github.com/Textualize/rich), ported to Rust and held to
it byte for byte**, with the [`rich-cli`](https://github.com/Textualize/rich-cli)
tool and a stack built on top: charts and diagrams, pickers and forms, a
reactive terminal UI framework (intuiTUIve), a terminal recorder, and images
in the terminal. Tracking **`rich` 15.0.0** and **`rich-cli` 1.8.1**
([`UPSTREAM.toml`](UPSTREAM.toml)); every parity claim is a golden test
captured from the real Python library.

**📖 [Documentation, tutorial and gallery](https://buchochelliq-labs.github.io/rs-rich-cli/)**

![rich rendering Markdown, a CSV table and Python source in a real terminal](docs/media/tapes/hero/hero.gif)

| A game on intuiTUIve | A Yazi-style file manager |
|---|---|
| ![The raycaster example: a first-person maze drawn in half blocks, with a status bar of digits](docs/media/tapes/raycast/start.png) | ![The files example previewing a Rust file in three columns](docs/media/tapes/files/files.png) |

Every picture here is a [terminal recording](https://buchochelliq-labs.github.io/rs-rich-cli/recordings/)
made by `rich record` from a script in [`docs/tapes/`](docs/tapes/), re-run
by CI on every change.

## Install and try

```bash
cargo install rs-rich-cli
rich --print '[bold magenta]Hello[/] [green]World[/]'
rich README.md          # Markdown, JSON, CSV, source, notebooks, images…
rich --demo             # the guided tour; Ctrl+C stops
```

In Rust, `cargo add rs-rich` and `use rich::Console`:

```rust
use rich::Console;

let console = Console::new();
console.print_str("[bold red]Hello[/] [green]World[/] — 42");
```

[Getting started](docs/getting-started.md) has a complete program; the
[tutorial](docs/tutorial/index.md) builds up from there.

## New in 0.0.20

The intuiTUIve demo loop: eleven well-known terminal apps rebuilt on
intuiTUIve (Yazi, scope-tui, gitui, bottom, Slumber, csvlens, diskonaut,
termusic, taskwarrior-tui, a falling-block game and a Doom-style raycaster),
each gap they found added to the framework, and each clone rebuilt on it
([the loop](docs/design/intuituive-demo-gaps.md)). What it added:

- **Games and pixels:** sprites on a stage, RGBA pixel buffers drawn in half
  blocks, quadrants or Braille, held keys and key releases, DAS/ARR key
  repeat, timers with a handle and fixed steps with catch-up, frame pacing.
- **Apps:** tables, trees and lists that follow state, rows from a source
  filtered on a worker thread, input completion and history, a command
  line, a query language over records, focus scopes and directional focus,
  treemaps, and natural dates (`tomorrow`, `eow`, `3d`) with time zones.
- **`rich record`** follows Kitty, Sixel and iTerm2 images, records every
  frame at a set rate (`Set Framerate 35`), holds keys (`KeyDown`, `KeyUp`,
  `Hold`) with the kitty keyboard protocol, and writes an animated PNG.
- **Core:** `Text::markup()`, a port of upstream's `Text.markup`.

The [changelog](CHANGELOG.md) lists every change.

> **Early releases, and the versions say so.** The crates version independently
> by ordinary SemVer; the number is *not* tied to the upstream release. Expect
> breaking API changes. Which upstream version is tracked lives in
> [`UPSTREAM.toml`](UPSTREAM.toml). See [AGENTS.md](AGENTS.md) for why the
> version is not mirrored.

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
| [`rs-rich`](https://crates.io/crates/rs-rich) | `0.0.11` |
| [`rs-rich-plugin-api`](https://crates.io/crates/rs-rich-plugin-api) | `0.0.5` |
| [`rs-rich-macros`](https://crates.io/crates/rs-rich-macros) | `0.0.5` |
| [`rs-rich-ext`](https://crates.io/crates/rs-rich-ext) | `0.0.18` |
| [`rs-rich-cli`](https://crates.io/crates/rs-rich-cli) | `0.0.21` |
| [`rs-rich-art`](https://crates.io/crates/rs-rich-art) | `0.0.14` |
| [`rs-rich-mermaid`](https://crates.io/crates/rs-rich-mermaid) | `0.0.7` |
| [`rs-rich-lumis`](https://crates.io/crates/rs-rich-lumis) | `0.0.5` |
| [`rs-rich-record`](https://crates.io/crates/rs-rich-record) | `0.0.9` |
| [`rs-rich-interact`](https://crates.io/crates/rs-rich-interact) | `0.0.9` |
| [`rs-rich-micro`](https://crates.io/crates/rs-rich-micro) | `0.0.7` |
| [`rs-rich-diagram`](https://crates.io/crates/rs-rich-diagram) | `0.0.4` |
| [`rs-rich-data`](https://crates.io/crates/rs-rich-data) | `0.0.5` |
| [`rs-rich-ratatui`](https://crates.io/crates/rs-rich-ratatui) | `0.0.6` |
| [`rs-rich-intuituive`](https://crates.io/crates/rs-rich-intuituive) | `0.0.6` |
| [`rs-rich-web`](https://crates.io/crates/rs-rich-web) | `0.0.4` |
| [`rs-rich-embed`](https://crates.io/crates/rs-rich-embed) | `0.0.4` |
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
| `rs-rich-intuituive` | intuiTUIve: a reactive, retained terminal UI framework (signals, a retained tree, widgets, sprites and pixel buffers) on rich-interact | `use intuituive` |
| `rs-rich-web` | serving intuiTUIve apps, or any terminal program, to a web browser over a WebSocket, drawn by a vendored xterm.js or an accessible DOM renderer | `use rich_web` |
| `rs-rich-embed` | a terminal pane and a web view for intuiTUIve apps, over swappable PTY and browser backends | `use rich_embed` |
| `rs-rich-lumis` | the lumis (tree-sitter) code highlighter | `use rich_lumis` |
| `rs-rich-record` | scripted terminal recordings (`rich record`): screenshots, casts, GIF, APNG and MP4, with terminal graphics and held keys | `use rich_record` |
| `rs-rich-interact` | interactive components: pickers, input, forms, pagers, explorers; the terminal session, keys, natural dates and time zones | `use rich_interact` |
| `rs-rich-micro` | micro assets: emoji-sized inline images written `:micro:name:` | `use rich_micro` |
| `rs-rich` (PyPI) | Python bindings: Rich's API over the Rust core, plus the port's crates (`rs_rich.ext`, `rs_rich.chart`, `rs_rich.diagram`, `rs_rich.interact`, `rs_rich.tui`, ...) | `import rs_rich` |

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
markup · `Text` (wrapping, justification incl. full, overflow, spans, `markup()`) ·
`Console` (capture, export to HTML/SVG, paging, control codes) · `Segment` ·
themes · highlighters · `Table` · `Panel` · `Rule` · `Align` · `Padding` ·
`Columns` · `Constrain` · `Tree` · `Layout` · `Screen` · `Markdown` · `Syntax` ·
`JSON` · `Pretty` · `Traceback` · `Progress` (incl. `track()` and
`RenderableColumn`) · `Spinner` · `Status` · `Live` · `LogRender` · `Bar` ·
`Prompt` · emoji · filesize.

**Not ported:** Windows legacy-console support (a console without VT
processing shows raw escape sequences; use Windows Terminal), Jupyter integration, and `inspect`/`repr` of Python objects
(no Rust equivalent — see [#19](https://github.com/buchochelliq-labs/rs-rich-cli/issues/19)).
A `log`/`tracing` handler (`RichHandler`) lives in `rs-rich-ext`.

**Known to differ from upstream**, deliberately and with reasons, in
[docs/DIVERGENCES.md](docs/DIVERGENCES.md) — most notably syntax highlighting uses
`syntect` rather than Pygments, so highlighted code is *not* byte-identical.

Per-module detail is in [docs/PORTING.md](docs/PORTING.md). What comes next, and
why, is in [docs/ROADMAP.md](docs/ROADMAP.md); the tracking epic is
[#16](https://github.com/buchochelliq-labs/rs-rich-cli/issues/16).

## Releases

Each release has notes in [`docs/releases/`](docs/releases/) (the newest
published one is [0.0.19](docs/releases/0.0.19.md)), and
[`CHANGELOG.md`](CHANGELOG.md) lists every change. What comes next is in the
[roadmap](docs/ROADMAP.md) and the [0.0.21 plan](docs/plans/0.0.21.md). The
manifest table above describes this checkout; crates.io is the source for
published versions.

## Leaner builds

`Syntax` (syntect) and `Markdown` (pulldown-cmark) are default features of
`rs-rich` and `rs-rich-ext`. If you use neither, turn them off and drop
syntect, its bincode 1.x and a second `fancy-regex` from your build:

```toml
rs-rich = { version = "0.0.10", default-features = false }
rs-rich-ext = { version = "0.0.17", default-features = false }  # if you use it
```

## The `rich` command

The CLI covers upstream's `--markdown` · `--rst` · `--syntax` · `--json` · `--csv` ·
`--ipynb` · `--print` · `--rule` · `--panel` · `--padding` · `--pager` ·
`--export-html` · `--export-svg`, the alignment and width flags, and every
other rich-cli 1.8.1 option. Beyond upstream it has subcommands to:

- read and compare: `inspect`, `view`, `diff` (text, patches, images and
  merge conflicts), `hex`, `unicode`, `ansi explain`, `log`, `profile`;
- draw data and structure: `chart`, `dot`, `mermaid`, `deps`, `schema`;
- ask in a script: `choose`, `filter`, `input`, `confirm`, `write`, `file`,
  `color`, `date`, `pager`, `explore`, `approve`, `jump`;
- record and present: `record` (tapes into PNG, SVG, GIF, APNG, MP4 and
  casts), `capture`, `banner`, `shell`, `micro`, and `serve` (a program in a
  browser, behind the off-by-default `serve` feature).

See [Using the CLI](docs/cli.md), the [CLI walkthrough](docs/guide/cli/walkthrough.md)
and the [CLI reference](docs/cli-reference.md).

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
