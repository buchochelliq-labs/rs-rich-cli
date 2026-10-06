# rs-rich

<div class="project-badges" markdown="1">

[![CI](https://github.com/buchochelliq-labs/rs-rich-cli/actions/workflows/ci.yml/badge.svg)](https://github.com/buchochelliq-labs/rs-rich-cli/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/rs-rich.svg)](https://crates.io/crates/rs-rich)
[![docs.rs](https://img.shields.io/docsrs/rs-rich)](https://docs.rs/rs-rich)
[![MSRV](https://img.shields.io/crates/msrv/rs-rich.svg)](https://github.com/buchochelliq-labs/rs-rich-cli#develop)
[![License](https://img.shields.io/crates/l/rs-rich.svg)](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/LICENSE)
[![GitHub](https://img.shields.io/badge/github-repo-blue)](https://github.com/buchochelliq-labs/rs-rich-cli)

</div>

A Rust port of Python's [`rich`](https://github.com/Textualize/rich) — rich text,
colour, tables, markdown and progress bars in the terminal — plus a port of the
[`rich-cli`](https://github.com/Textualize/rich-cli) tool.

![Rich-art cover cropping with nine anchors](assets/demos/v8-crop-anchors.gif)

**[Watch the CLI and art videos](demos.md)** · [Browse the gallery](gallery.md) ·
[Start with the CLI](cli.md) · [Learn the library](tutorial/index.md) · [Read the full guide](guide/index.md)

The preview shows `rich-art` cover cropping, recorded with CLI 0.0.8. The
[0.0.11 demo tour](demos.md) walks through the newer tools: `inspect`, `diff`,
`view`, the `hex`, `unicode` and `ansi explain` inspectors, a theme file, a
redacted `capture`, and the new image modes.

```bash
cargo install rs-rich-cli
rich --print '[bold magenta]Hello[/] [green]World[/]'
```

For Rust applications: `cargo add rs-rich`. Read [Getting started](getting-started.md)
for a complete library example.

## Output in motion

![Progress frames exported by rs-rich](assets/demos/progress.gif)

Progress and spinner animations replay exported library frames. The
[gallery](gallery.md) pairs output with links to the relevant guides.

## Release history and development

The [0.0.9 cohort](releases/0.0.9-expanded.md) is published. The
[0.0.10 cohort](releases/0.0.10.md) is published too. It adds
Progress time, rate and spinner columns, upstream's theme stack, `~~~` strikethrough
parity, multi-file watch, and quadrant, ANSI16 and grayscale image modes.
The [0.0.11 cohort](releases/0.0.11.md) (core 0.0.7, macros 0.0.1 (new),
ext 0.0.9, art 0.0.9, CLI 0.0.11) is published. It adds
diagnostics, structured data, checked-markup macros, `clap` and `tracing`
integration, workflow renderables, and the `inspect`, `diff`, `view`, `hex`,
`unicode`, `env` and `capture` commands.
The [0.0.12 cohort](releases/0.0.12.md) added the plugin platform and the
first Python package; [0.0.13](releases/0.0.13.md) interactive components and
`rich record`; [0.0.14](releases/0.0.14.md) composable interactive views,
`rich explore` and micro assets; and [0.0.15](releases/0.0.15.md) terminal
charts, `rs-rich-diagram` with DOT, `rich chart`, `rich dot`, `rich deps` and
`rich schema`. All four are published.
The [roadmap](ROADMAP.md) tracks subsequent work. Manifest versions below
identify this checkout; the crates.io badges identify published packages.

## What it does

<div class="grid cards" markdown>

- **Console markup**

    `[bold red]text[/]` — nested tags, themes, emoji shortcodes, and automatic
    highlighting of numbers, paths, URLs and booleans.

- **Layout**

    Tables, panels, trees, columns, rules, alignment and padding, all of which
    measure and wrap correctly against terminal cell widths.

- **Documents**

    Markdown, JSON and syntax-highlighted source, rendered to the terminal.

- **Live output**

    Progress bars, spinners and status displays that refresh in place.

- **The `rich` command**

    Render files from the shell, plus tools: `inspect` structured data, `diff`
    text and patches, `view`, `hex`, `unicode`, `env`, `capture` and `doctor`;
    `chart`, `dot`, `mermaid`, `deps` and `schema` to draw data and structure;
    `choose`, `input`, `pager` and `explore` for scripts.
    See the [CLI guide](guide/cli/walkthrough.md).

- **Charts**

    Sparklines, bars, histograms, Braille line and scatter charts, gauges,
    heatmaps, status matrices, KPI cards and timelines, each with an ASCII
    form and a reading that does not depend on colour. See
    [Charts](guide/ext/charts.md).

- **Diagrams**

    Graphs drawn with box-drawing characters through one layered layout, from
    code, Mermaid flowcharts or DOT (Graphviz) sources; Cargo dependency
    graphs and JSON Schemas. See [Diagrams](guide/diagram/index.md) and
    [Dependency graphs and JSON Schemas](guide/ext/sources.md).

- **Interactive components**

    Fuzzy pickers, input, forms, pagers and explorers, composable into your
    own views. See [Interactive](guide/interact/index.md).

- **Micro assets**

    Emoji-sized inline images written `:micro:name:`, drawn with Kitty,
    iTerm2 or Sixel graphics, with a text fallback everywhere else. See
    [Micro assets](guide/micro/index.md).

- **Python**

    `rs_rich`, Rich's API over the Rust core, with the port's extensions,
    charts and diagrams. See the
    [Python documentation](https://buchochelliq-labs.github.io/rs-rich-cli/python/).

- **Rust ergonomics**

    Diagnostics and stack traces, checked-markup macros, `clap` help, `tracing`
    and `log` handlers, and serde-driven structured data, in `rs-rich-ext`.

- **Workflow renderables**

    Command steps, task trees, transfers, countdowns, badges, size bars and
    formatters for build and deploy tools. See [Workflows](guide/ext/workflows.md).

- **Art**

    FIGlet banners, images and GIFs as ASCII, Braille, blocks, quadrants or
    Sixel, with dithering and alpha backgrounds. See [Art](guide/art/index.md).

- **Accessibility and capabilities**

    Detect what the terminal supports, degrade to a fidelity level, render
    decoration-free text for screen readers and check theme contrast. See
    [Capabilities](guide/ext/capabilities.md) and
    [Accessibility](guide/ext/accessibility.md).

</div>

## Byte-parity with Python rich

Golden fixtures compare covered output against Python `rich` 15.0.0 in CI.
Coverage and known differences are documented in [Module status](PORTING.md)
and [Divergences](DIVERGENCES.md); not every implemented feature is byte-identical.

That promise is why the honest bits matter:

!!! warning "Early releases with independent crate versions"

    The API takes breaking changes regularly, and the port is deliberately
    incomplete. What is implemented is parity-tested; what isn't is listed
    plainly in [Module status](PORTING.md), and what deliberately differs is in
    [Divergences](DIVERGENCES.md) — most notably syntax highlighting, which uses
    `syntect` rather than Pygments and is therefore *not* byte-identical.

## Released crates

The badges below show the versions currently available on crates.io.

| crate | version | docs | what it is |
|---|---|---|---|
| [`rs-rich`](https://crates.io/crates/rs-rich) | [![rs-rich](https://img.shields.io/crates/v/rs-rich.svg)](https://crates.io/crates/rs-rich) | [docs.rs](https://docs.rs/rs-rich) | the library — `use rich::…` |
| [`rs-rich-cli`](https://crates.io/crates/rs-rich-cli) | [![rs-rich-cli](https://img.shields.io/crates/v/rs-rich-cli.svg)](https://crates.io/crates/rs-rich-cli) | — | the `rich` command |
| [`rs-rich-ext`](https://crates.io/crates/rs-rich-ext) | [![rs-rich-ext](https://img.shields.io/crates/v/rs-rich-ext.svg)](https://crates.io/crates/rs-rich-ext) | [docs.rs](https://docs.rs/rs-rich-ext) | extensions + plugin registry |
| [`rs-rich-macros`](https://crates.io/crates/rs-rich-macros) | [![rs-rich-macros](https://img.shields.io/crates/v/rs-rich-macros.svg)](https://crates.io/crates/rs-rich-macros) | [docs.rs](https://docs.rs/rs-rich-macros) | checked markup and derive macros, used through `rs-rich-ext` |
| [`rs-rich-art`](https://crates.io/crates/rs-rich-art) | [![rs-rich-art](https://img.shields.io/crates/v/rs-rich-art.svg)](https://crates.io/crates/rs-rich-art) | [docs.rs](https://docs.rs/rs-rich-art) | FIGlet text, image→ASCII, GIFs |
| [`rs-rich-plugin-api`](https://crates.io/crates/rs-rich-plugin-api) | [![rs-rich-plugin-api](https://img.shields.io/crates/v/rs-rich-plugin-api.svg)](https://crates.io/crates/rs-rich-plugin-api) | [docs.rs](https://docs.rs/rs-rich-plugin-api) | the plugin contract |
| [`rs-rich-mermaid`](https://crates.io/crates/rs-rich-mermaid) | [![rs-rich-mermaid](https://img.shields.io/crates/v/rs-rich-mermaid.svg)](https://crates.io/crates/rs-rich-mermaid) | [docs.rs](https://docs.rs/rs-rich-mermaid) | Mermaid flowcharts as text |
| [`rs-rich-lumis`](https://crates.io/crates/rs-rich-lumis) | [![rs-rich-lumis](https://img.shields.io/crates/v/rs-rich-lumis.svg)](https://crates.io/crates/rs-rich-lumis) | [docs.rs](https://docs.rs/rs-rich-lumis) | the lumis (tree-sitter) highlighter |
| [`rs-rich-record`](https://crates.io/crates/rs-rich-record) | [![rs-rich-record](https://img.shields.io/crates/v/rs-rich-record.svg)](https://crates.io/crates/rs-rich-record) | [docs.rs](https://docs.rs/rs-rich-record) | scripted terminal recordings |
| [`rs-rich-interact`](https://crates.io/crates/rs-rich-interact) | [![rs-rich-interact](https://img.shields.io/crates/v/rs-rich-interact.svg)](https://crates.io/crates/rs-rich-interact) | [docs.rs](https://docs.rs/rs-rich-interact) | interactive components |
| [`rs-rich-micro`](https://crates.io/crates/rs-rich-micro) | [![rs-rich-micro](https://img.shields.io/crates/v/rs-rich-micro.svg)](https://crates.io/crates/rs-rich-micro) | [docs.rs](https://docs.rs/rs-rich-micro) | micro assets |
| [`rs-rich-diagram`](https://crates.io/crates/rs-rich-diagram) | [![rs-rich-diagram](https://img.shields.io/crates/v/rs-rich-diagram.svg)](https://crates.io/crates/rs-rich-diagram) | [docs.rs](https://docs.rs/rs-rich-diagram) | graph diagrams and DOT (new in 0.0.15; published with the cohort) |
| [`rs-rich-data`](https://crates.io/crates/rs-rich-data) | [![rs-rich-data](https://img.shields.io/crates/v/rs-rich-data.svg)](https://crates.io/crates/rs-rich-data) | [docs.rs](https://docs.rs/rs-rich-data) | tabular data: row sources, inference and statistics (new in 0.0.16) |

<a id="versions-prepared-in-this-checkout"></a>

### Versions in this checkout

These manifest versions describe the source tree, not publication status.
Each crate versions independently; see [Branching and releases](BRANCHING.md).

<!-- BEGIN MANIFEST VERSIONS -->
| Package | Manifest version |
|---|---|
| [`rs-rich`](https://crates.io/crates/rs-rich) | `0.0.9` |
| [`rs-rich-plugin-api`](https://crates.io/crates/rs-rich-plugin-api) | `0.0.3` |
| [`rs-rich-macros`](https://crates.io/crates/rs-rich-macros) | `0.0.3` |
| [`rs-rich-ext`](https://crates.io/crates/rs-rich-ext) | `0.0.14` |
| [`rs-rich-cli`](https://crates.io/crates/rs-rich-cli) | `0.0.16` |
| [`rs-rich-art`](https://crates.io/crates/rs-rich-art) | `0.0.12` |
| [`rs-rich-mermaid`](https://crates.io/crates/rs-rich-mermaid) | `0.0.4` |
| [`rs-rich-lumis`](https://crates.io/crates/rs-rich-lumis) | `0.0.3` |
| [`rs-rich-record`](https://crates.io/crates/rs-rich-record) | `0.0.4` |
| [`rs-rich-interact`](https://crates.io/crates/rs-rich-interact) | `0.0.4` |
| [`rs-rich-micro`](https://crates.io/crates/rs-rich-micro) | `0.0.3` |
| [`rs-rich-diagram`](https://crates.io/crates/rs-rich-diagram) | `0.0.1` |
| [`rs-rich-data`](https://crates.io/crates/rs-rich-data) | `0.0.1` |
<!-- END MANIFEST VERSIONS -->

## Install

=== "Library"

    ```bash
    cargo add rs-rich
    ```

    The package is `rs-rich` because `rich` was taken on crates.io. The library
    target keeps the short name, so you write:

    ```rust
    use rich::{Console, Table};
    ```

=== "CLI"

    ```bash
    cargo install rs-rich-cli
    ```

    Installs a binary called `rich`.

## A first look

```rust
use rich::Console;

let console = Console::new();
console.print_str("[bold magenta]Hello[/] [green]World[/] — 42");
```

Then read the [tutorial](tutorial/index.md), or skim the
[gallery](gallery.md) to see what is available.
