# rs-rich

[![CI](https://github.com/buchochelliq-labs/rs-rich-cli/actions/workflows/ci.yml/badge.svg)](https://github.com/buchochelliq-labs/rs-rich-cli/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/rs-rich.svg)](https://crates.io/crates/rs-rich)
[![docs.rs](https://img.shields.io/docsrs/rs-rich)](https://docs.rs/rs-rich)
[![MSRV](https://img.shields.io/crates/msrv/rs-rich.svg)](https://github.com/buchochelliq-labs/rs-rich-cli#develop)
[![License](https://img.shields.io/crates/l/rs-rich.svg)](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/LICENSE)
[![GitHub](https://img.shields.io/badge/github-repo-blue)](https://github.com/buchochelliq-labs/rs-rich-cli)

A Rust port of Python's [`rich`](https://github.com/Textualize/rich) — rich text,
colour, tables, markdown and progress bars in the terminal — plus a port of the
[`rich-cli`](https://github.com/Textualize/rich-cli) tool.

![Markup](assets/markup.svg)

Renderings on this site come from the library or CLI itself. SVG exports and
browser screenshots are committed snapshots of actual output.

---

## 0.0.4 is released

All four crates are published at 0.0.4 and passed exact-version registry
verification. The [release notes](releases/0.0.4.md) cover explicit text encoding,
GIF half-block rendering, measured CSV/wrapping improvements and an optional
syntax cache, with actual CLI screenshots and the release workflow results.

The [0.0.5 preparation plan](plans/0.0.5.md) proposes four focused workstreams:
targeted goldens, differential fuzzing, library benchmarks and opt-in input
sanitization. Implementation and version changes are still ahead.

## What changed in 0.0.3

Graphical diff exports retain color with redirected stdout; notebooks support
panels and alignment; styled titles render correctly; Windows paging uses
`more.com`. JSON precision and GIF/CSV redirection also improve.

Read the [0.0.3 release notes](releases/0.0.3.md) for actual CLI screenshots,
per-package changes and remaining limitations. Package badges below show what
is currently available on crates.io.

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

</div>

## Byte-parity with Python rich

This is a port, not a re-imagining. The promise is that output is **byte-identical**
to Python `rich` 15.0.0 for everything implemented — enforced by golden fixtures
captured from the real library and asserted in CI.

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
| [`rs-rich-art`](https://crates.io/crates/rs-rich-art) | [![rs-rich-art](https://img.shields.io/crates/v/rs-rich-art.svg)](https://crates.io/crates/rs-rich-art) | [docs.rs](https://docs.rs/rs-rich-art) | FIGlet text, image→ASCII, GIFs |

<a id="versions-prepared-in-this-checkout"></a>

### Versions in this checkout

These manifest versions describe the source tree, not publication status.
Each crate versions independently; see [Branching and releases](BRANCHING.md).

<!-- BEGIN MANIFEST VERSIONS -->
| Package | Manifest version |
|---|---|
| [`rs-rich`](https://crates.io/crates/rs-rich) | `0.0.4` |
| [`rs-rich-ext`](https://crates.io/crates/rs-rich-ext) | `0.0.4` |
| [`rs-rich-cli`](https://crates.io/crates/rs-rich-cli) | `0.0.4` |
| [`rs-rich-art`](https://crates.io/crates/rs-rich-art) | `0.0.4` |
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
