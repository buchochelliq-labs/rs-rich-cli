# rs-rich

<div class="project-badges" markdown="1">

[![CI](https://github.com/buchochelliq-labs/rs-rich-cli/actions/workflows/ci.yml/badge.svg)](https://github.com/buchochelliq-labs/rs-rich-cli/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/rs-rich.svg)](https://crates.io/crates/rs-rich)
[![docs.rs](https://img.shields.io/docsrs/rs-rich)](https://docs.rs/rs-rich)
[![MSRV](https://img.shields.io/crates/msrv/rs-rich.svg)](https://github.com/buchochelliq-labs/rs-rich-cli#develop)
[![License](https://img.shields.io/crates/l/rs-rich.svg)](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/LICENSE)
[![GitHub](https://img.shields.io/badge/github-repo-blue)](https://github.com/buchochelliq-labs/rs-rich-cli)

</div>

Python's [`rich`](https://github.com/Textualize/rich), ported to Rust and
held to it byte for byte: rich text, colour, tables, Markdown and progress
bars in the terminal, with the [`rich-cli`](https://github.com/Textualize/rich-cli)
tool. On top of it: charts and diagrams, pickers and forms, intuiTUIve (a
reactive framework for terminal apps), a terminal recorder, and images in
the terminal.

![rich rendering Markdown, a CSV table and Python source in a real terminal](media/tapes/hero/hero.gif)

**[Play the terminal recordings](recordings.md)** · [Browse the gallery](gallery.md) ·
[Start with the CLI](cli.md) · [Learn the library](tutorial/index.md) · [Read the full guide](guide/index.md)

```bash
cargo install rs-rich-cli
rich --print '[bold magenta]Hello[/] [green]World[/]'
```

For Rust applications: `cargo add rs-rich`. Read [Getting started](getting-started.md)
for a complete library example.

## New in 0.0.20

| A raycaster on intuiTUIve | Recorded with held keys |
|---|---|
| ![The raycaster example: a first-person maze in half blocks](media/tapes/raycast/start.png) | ![The raycaster's map, recorded from a tape](media/tapes/raycast/map.png) |

0.0.20 is the intuiTUIve demo loop's release: eleven well-known terminal
apps rebuilt on [intuiTUIve](guide/intuituive/index.md) (Yazi, scope-tui,
gitui, bottom, Slumber, csvlens, diskonaut, termusic, taskwarrior-tui, a
falling-block game and a Doom-style raycaster), what each lacked added to
the framework, and each rebuilt again on it.
[What the demos are missing](design/intuituive-demo-gaps.md) tells the
story round by round.

- **Games and pixels:** sprites on a stage, RGBA pixel buffers, held keys
  and key releases, key repeat at the app's own rate, timers you can pause
  and fixed steps with catch-up. See [a game, played with held
  keys](recordings.md#a-game-played-with-held-keys).
- **Apps:** tables, trees and lists that follow state, input completion and
  history, a command line, a query language over records, focus scopes,
  treemaps.
- **Dates and zones:** `tomorrow`, `eow` and `3d` read by
  [`dates::parse`](guide/interact/index.md#natural-dates), and
  [time zones](guide/interact/index.md#time-zones) with summer time.
- **Recording games:** `rich record` follows Kitty, Sixel and iTerm2
  images, keeps every frame at a set rate, holds keys and writes an
  animated PNG. See [Record your own](recordings.md#record-your-own).
- **Core:** [`Text::markup()`](guide/core/text-and-style.md#back-to-markup),
  a port of upstream's `Text.markup`.

Earlier releases are under [Releases](releases/0.0.19.md) in the
navigation; the [roadmap](ROADMAP.md) and the
[0.0.21 plan](plans/0.0.21.md) say what comes next.

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

- **Terminal apps**

    intuiTUIve: signals, a retained tree, layout, focus, tables, trees,
    a command line, sprites and pixel buffers, tested headless and served
    to a browser. See [Terminal apps](guide/intuituive/index.md).

- **Terminal recordings**

    `rich record` runs a tape in a real terminal and writes screenshots,
    a cast, a GIF, an animated PNG and an MP4, images and held keys
    included. See [Terminal recordings](recordings.md).

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

The badges below show the versions currently available on crates.io. The
packages carry an `rs-` prefix because `rich` was taken on crates.io; the
library targets keep the short names, so you write `use rich::Console`.

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
| [`rs-rich-ratatui`](https://crates.io/crates/rs-rich-ratatui) | [![rs-rich-ratatui](https://img.shields.io/crates/v/rs-rich-ratatui.svg)](https://crates.io/crates/rs-rich-ratatui) | [docs.rs](https://docs.rs/rs-rich-ratatui) | ratatui interop: rich renderables as ratatui widgets (new in 0.0.17) |
| [`rs-rich-intuituive`](https://crates.io/crates/rs-rich-intuituive) | [![rs-rich-intuituive](https://img.shields.io/crates/v/rs-rich-intuituive.svg)](https://crates.io/crates/rs-rich-intuituive) | [docs.rs](https://docs.rs/rs-rich-intuituive) | intuiTUIve: a reactive, retained TUI framework (new in 0.0.17) |
| [`rs-rich-web`](https://crates.io/crates/rs-rich-web) | [![rs-rich-web](https://img.shields.io/crates/v/rs-rich-web.svg)](https://crates.io/crates/rs-rich-web) | [docs.rs](https://docs.rs/rs-rich-web) | serving intuiTUIve apps and terminal programs to a browser (new in 0.0.18) |
| [`rs-rich-embed`](https://crates.io/crates/rs-rich-embed) | [![rs-rich-embed](https://img.shields.io/crates/v/rs-rich-embed.svg)](https://crates.io/crates/rs-rich-embed) | [docs.rs](https://docs.rs/rs-rich-embed) | a terminal pane and a web view for intuiTUIve apps (new in 0.0.18) |

<a id="versions-prepared-in-this-checkout"></a>

### Versions in this checkout

These manifest versions describe the source tree, not publication status.
Each crate versions independently; see [Branching and releases](BRANCHING.md).

<!-- BEGIN MANIFEST VERSIONS -->
| Package | Manifest version |
|---|---|
| [`rs-rich`](https://crates.io/crates/rs-rich) | `0.0.10` |
| [`rs-rich-plugin-api`](https://crates.io/crates/rs-rich-plugin-api) | `0.0.4` |
| [`rs-rich-macros`](https://crates.io/crates/rs-rich-macros) | `0.0.4` |
| [`rs-rich-ext`](https://crates.io/crates/rs-rich-ext) | `0.0.17` |
| [`rs-rich-cli`](https://crates.io/crates/rs-rich-cli) | `0.0.20` |
| [`rs-rich-art`](https://crates.io/crates/rs-rich-art) | `0.0.13` |
| [`rs-rich-mermaid`](https://crates.io/crates/rs-rich-mermaid) | `0.0.6` |
| [`rs-rich-lumis`](https://crates.io/crates/rs-rich-lumis) | `0.0.4` |
| [`rs-rich-record`](https://crates.io/crates/rs-rich-record) | `0.0.8` |
| [`rs-rich-interact`](https://crates.io/crates/rs-rich-interact) | `0.0.8` |
| [`rs-rich-micro`](https://crates.io/crates/rs-rich-micro) | `0.0.6` |
| [`rs-rich-diagram`](https://crates.io/crates/rs-rich-diagram) | `0.0.3` |
| [`rs-rich-data`](https://crates.io/crates/rs-rich-data) | `0.0.4` |
| [`rs-rich-ratatui`](https://crates.io/crates/rs-rich-ratatui) | `0.0.5` |
| [`rs-rich-intuituive`](https://crates.io/crates/rs-rich-intuituive) | `0.0.5` |
| [`rs-rich-web`](https://crates.io/crates/rs-rich-web) | `0.0.3` |
| [`rs-rich-embed`](https://crates.io/crates/rs-rich-embed) | `0.0.3` |
<!-- END MANIFEST VERSIONS -->

## Where next

- [Getting started](getting-started.md): install, check it worked, and a
  first program.
- The [tutorial](tutorial/index.md) builds up from a line of text to a
  live progress display.
- The [gallery](gallery.md) and the [recordings](recordings.md) show what
  exists; the [guide](guide/index.md) covers every crate.
