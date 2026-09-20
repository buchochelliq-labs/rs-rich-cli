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

**[Watch the rich-art videos](demos.md)** · [Browse the gallery](gallery.md) ·
[Start with the CLI](cli.md) · [Learn the library](tutorial/index.md)

The preview shows crop anchors from the optimized CLI 0.0.8 preparation build.
See the [new workflow videos](demos.md#cli-008-workflows) for batch dry-run,
parallel exports, config inspection and reproduction commands.

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

[CLI 0.0.8 preparation](releases/0.0.8.md) follows the [shipped 0.0.7 release](releases/0.0.7.md).
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
| [`rs-rich-art`](https://crates.io/crates/rs-rich-art) | [![rs-rich-art](https://img.shields.io/crates/v/rs-rich-art.svg)](https://crates.io/crates/rs-rich-art) | [docs.rs](https://docs.rs/rs-rich-art) | FIGlet text, image→ASCII, GIFs |

<a id="versions-prepared-in-this-checkout"></a>

### Versions in this checkout

These manifest versions describe the source tree, not publication status.
Each crate versions independently; see [Branching and releases](BRANCHING.md).

<!-- BEGIN MANIFEST VERSIONS -->
| Package | Manifest version |
|---|---|
| [`rs-rich`](https://crates.io/crates/rs-rich) | `0.0.4` |
| [`rs-rich-ext`](https://crates.io/crates/rs-rich-ext) | `0.0.6` |
| [`rs-rich-cli`](https://crates.io/crates/rs-rich-cli) | `0.0.8` |
| [`rs-rich-art`](https://crates.io/crates/rs-rich-art) | `0.0.6` |
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
