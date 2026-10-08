# The rs-rich guide

rs-rich brings Python's [rich](https://github.com/Textualize/rich) to Rust:
styled text, tables, trees, progress bars, syntax highlighting, Markdown and more
in the terminal, plus exports to HTML and SVG. On top of that faithful core it
adds Rust-first extensions (diagnostics, structured data, CLI authoring,
diffs, test tooling, capability detection, accessibility, tracing spans,
workflow renderables and charts), graph diagrams, interactive components,
micro assets, image and GIF art, and the `rich` command-line tool.

This guide walks through all of it. Every code sample on these pages is taken
from an example program that CI compiles, and every screenshot is that program's
real output, exported to SVG by the library itself.

## The pieces

| Crate | Import as | What it gives you | Guide |
|---|---|---|---|
| [`rs-rich`](https://crates.io/crates/rs-rich) | `rich` | The core: `Console`, markup, `Text`, `Style`, tables, panels, trees, columns, layout, progress, live displays, Markdown, syntax, JSON, logging, prompts, export. A line-for-line port of rich 15.0.0, proven byte-for-byte against it. | [Core](core/index.md) |
| [`rs-rich-ext`](https://crates.io/crates/rs-rich-ext) | `rich_ext` | Everything that is not in upstream rich: diagnostics, hyperlinks, logging adapters and tracing spans, structured data, CLI authoring, diffs, test and QA tooling, capability detection, accessibility, workflow renderables (commands, task trees, transfers, countdowns, notifications, tables, badges, size bars, formatters, experimental redaction), charts (sparklines, bars, line charts, gauges, heatmaps, status matrices, KPI cards, timelines), and Cargo dependency and JSON Schema trees. Mostly opt-in features. | [Extensions](ext/index.md), [Charts](ext/charts.md) |
| `rs-rich-plugin-api` (new in 0.0.12) | `rich_plugin_api`, or `rich_ext::plugin` | The plugin contract: implement `Plugin` to add highlighters, code highlighters, themes, box styles, source and fence renderers, transforms and components. Depends on core only. | [Plugins](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/docs/PLUGINS.md) |
| `rs-rich-lumis` (new in 0.0.12) | `rich_lumis` | The lumis syntax highlighter (tree-sitter, over 100 languages, 250+ Neovim themes, plus `ansi_dark`/`ansi_light`), for `Syntax`, Markdown or the plugin host. | [Plugins](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/docs/PLUGINS.md) |
| `rs-rich-mermaid` (new in 0.0.12) | `rich_mermaid` | Mermaid diagrams: flowcharts drawn as text in every direction (through `rs-rich-diagram`'s layout), every diagram type through `mmdc` (optional), and ```` ```mermaid ```` fences in Markdown. | [Plugins](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/docs/PLUGINS.md) |
| `rs-rich-diagram` (new in 0.0.15) | `rich_diagram` | Graph diagrams: a graph model with a chaining builder, the layered layout Mermaid's flowcharts draw through, and a `Diagram` renderable in box drawing or ASCII that crops to the width it is given; DOT (Graphviz) sources parsed natively, a `dot` fence plugin (`plugin` feature) and Graphviz's own SVG (`graphviz` feature). | [Diagrams](diagram/index.md) |
| `rs-rich-data` (new in 0.0.16) | `rich_data` | Tabular data: CSV, TSV, JSON Lines, serde and Arrow row sources, type inference, column statistics, bounded profiles, data-quality reports, SQL result sets and windows over long inputs, and ER diagrams of a schema. | [Tabular data](data/index.md), [Schemas](data/schemas.md), [Developer views](data/developer-views.md) |
| [`rs-rich-macros`](https://crates.io/crates/rs-rich-macros) | through `rich_ext` | Compile-time checked markup (`richf!`), `#[derive(Rich)]` and print macros. Enabled by ext's `macros` feature. | [Macros](ext/macros.md) |
| [`rs-rich-art`](https://crates.io/crates/rs-rich-art) | `rich_art` | FIGlet banners, images as ASCII, Braille, blocks, quadrants or Sixel, animated GIFs, perceptual image diffs. | [Art](art/index.md) |
| `rs-rich-interact` (new in 0.0.13) | `rich_interact` | Interactive components: fuzzy pickers, input, confirm, forms, pagers, a text area, file, colour and asset pickers and data explorers, composable with containers, overlays and a keymap. | [Interactive](interact/index.md) |
| `rs-rich-intuituive` (new in 0.0.17) | `intuituive` | intuiTUIve, a framework for full-screen terminal apps: signals and memos, a retained tree of nodes (text, rich renderables, rich-interact components, logs), focus, keys and clicks, and only the changed cells sent to the terminal. | [Terminal apps](intuituive/index.md) |
| `rs-rich-web` (new in 0.0.18) | `rich_web` | Serving intuiTUIve apps to a web browser: one app per tab over a WebSocket, drawn by a vendored xterm.js page, behind a URL token, an origin check and a session cap. | [Serving an app to a browser](intuituive/web.md) |
| `rs-rich-ratatui` (new in 0.0.17) | `rich_ratatui` | ratatui interop: any rich renderable as a ratatui widget, ratatui widgets as rich-interact components (`interact` feature), and the style, colour, line and buffer-cell conversions underneath. | [Using rich with ratatui](ratatui.md) |
| `rs-rich-record` (new in 0.0.13) | `rich_record` | Scripted terminal recordings (tapes) in a real PTY, rendered as PNG, SVG, casts, GIF and MP4; behind `rich record`. | [Terminal recordings](../recordings.md) |
| `rs-rich-micro` (new in 0.0.14) | `rich_micro` | Micro assets: emoji-sized inline images written `:micro:name:`, `.richmicro` packages, a layered registry (built-in, user, trusted project, inline), placeholder cells that keep layouts exact, and drawing with Kitty, iTerm2, Sixel or half-blocks, with an emoji or text fallback. | [Micro assets](micro/index.md) |
| [`rs-rich-cli`](https://crates.io/crates/rs-rich-cli) | the `rich` binary | Upstream rich-cli's commands, plus image, gif, inspect, diff, view, hex, unicode, env, capture, ANSI explain, mermaid, dot, deps, schema, chart, the interactive commands (choose, filter, input, confirm, pager, write, file, color, asset, explore), micro, record, plugins, bench compare, batch, watch, config, completions, docs and doctor. | [CLI](cli/index.md) |

Each crate versions independently; see [the home page](../index.md) for the
current numbers.

## How it fits together

```text
                        your program                         the `rich` binary
                            │                                      │
            ┌───────────────┼───────────────┐                      │ composes the
            ▼               ▼               ▼                      │ libraries below
       rich_ext  ───────▶  rich  ◀─────── rich_art                  │
   (extensions,        (faithful       (banners, images,     ◀─────┘
    features)           core)           GIFs, image diff)
       │  ▲
       │  └── rich_macros (proc-macros behind ext's `macros` feature)
       ▼
   the extension registry, extended theme and capability data are
   installed onto a core Console; core never depends on ext
```

The dependency graph only points inwards: `rich` knows nothing about the other
crates. That is what keeps the core a faithful mirror of upstream rich while
everything new lives in `rich-ext` (see [AGENTS.md](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/AGENTS.md)).

### One render, step by step

Everything you print goes through the same pipeline:

1. **A renderable.** Anything implementing `rich::Renderable`: `Text`, a
   `Table`, a `Panel`, your own type, or a rich-ext view such as a diagnostic.
2. **Measure.** The console asks the renderable for its minimum and maximum
   width (`Measurement`), so containers can lay children out.
3. **Render to segments.** Given `ConsoleOptions` (width, height, colour system,
   capabilities), the renderable returns `Segment`s: runs of text, each with a
   `Style`, plus control codes.
4. **Write or export.** The `Console` turns segments into ANSI for a terminal, or
   records them for HTML, SVG or plain-text export.

Because steps 2 and 3 only depend on the options you pass in, rendering is
deterministic: the same renderable at the same width and capabilities gives the
same bytes. The screenshot tests, the golden parity tests and this guide's
images all rely on that.

## Features at a glance

| Crate | Feature | Turns on |
|---|---|---|
| `rs-rich` | *(default)* `syntax`, `markdown` | `Syntax` (syntect) and `Markdown` (pulldown-cmark; implies `syntax`). With `default-features = false` everything else in the faithful core remains, without syntect, its bincode 1.x or pulldown-cmark. |
| | `syntax-cache`, `json-escape-safe` | Opt-in divergences from upstream, documented in [Divergences](../DIVERGENCES.md). |
| | `onig` | Oniguruma instead of pure-Rust `fancy-regex` for syntax highlighting: 2–4× faster, same output, needs a C compiler ([Divergences #26](../DIVERGENCES.md)). |
| `rs-rich-ext` | *(default)* `syntax`, `markdown` | Core's two features, plus what needs them: `rst`, `cli_doc::markdown_view`, the `syntect` registry entry and `testing::conformance`. Without `syntax`, `source_view`, `SourceDiff` and `PatchView` render unhighlighted. |
| | *(always)* | Registry, highlighters, hyperlinks, diagnostics, stack traces, dashboard, live coordinator, `RichHandler` and `SpanView`, layouts, targets, capabilities, fidelity, accessibility, ANSI explain, diffs, CLI authoring model, and the workflow modules: `workflow`, `transfer`, `countdown`, `notify`, `cancel`, `table`, `badge`, `size_bar`, `format`, `redact`, plus the inspector views `source_view`, `hex`, `unicode_inspect` and `env_inspect`. |
| | `macros` | `richf!`, `#[derive(Rich)]` and the print macros. |
| | `anyhow` | `Diagnostic::from_anyhow`. |
| | `log`, `tracing` | Logging adapters. |
| | `data`, `yaml`, `toml`, `xml`, `jsonpath` | Structured data parsing and views. |
| | `clap` | Build help, errors and completions from a `clap::Command`. |
| | `testing` | Snapshots, assertion macros and the QA tools. |
| | `test-report` | JUnit and libtest result reports. |
| | `serde` | `Serialize` for capability, accessibility and ANSI reports. |
| `rs-rich-art` | `image`, `gif`, `sixel` | Image art, GIF playback, Sixel graphics. |
| `rs-rich-diagram` | `plugin`, `graphviz` | The `dot` fence and source renderer plugin; Graphviz's own SVG through the `dot` program. |
| `rs-rich-cli` | `fetch`, `art`, `mermaid`, `record`, `interact` (default) | URLs; images, GIFs, image diffs and micro assets; Mermaid; `rich record`; the interactive commands. `lumis`, `mmdc`, `dylib-plugins` and `wasm-plugins` are opt-in. |

## Where to start

- **Printing nicer output from a Rust program:** read [Console and printing](core/console.md),
  then [Text, markup and style](core/text-and-style.md) and [Tables](core/tables.md).
  The shorter [tutorial](../tutorial/index.md) covers the same ground in six steps.
- **Reporting errors well:** [Diagnostics](ext/diagnostics.md) and
  [Logging](ext/logging.md).
- **Showing config files or API responses:** [Structured data](ext/structured-data.md).
- **Drawing data or structure:** [Charts](ext/charts.md),
  [Diagrams](diagram/index.md) and
  [Dependency graphs and JSON Schemas](ext/sources.md).
- **Tables from data files, profiles and schemas:** [Tabular data](data/index.md),
  [Schemas](data/schemas.md) and [Developer views](data/developer-views.md).
- **Writing a CLI:** [CLI authoring](ext/cli-authoring.md) for help, errors,
  completions and man pages, with or without clap.
- **Testing terminal output:** [Diffs and test reports](ext/diffs-and-test-reports.md)
  and [QA tooling](ext/qa.md).
- **Adapting to whatever terminal you are on:** [Capabilities and fidelity](ext/capabilities.md)
  and [Accessibility](ext/accessibility.md).
- **Using the `rich` command:** the [CLI guide](cli/index.md), then the
  [walkthrough](cli/walkthrough.md). The [smoke test](cli/smoke.md) runs every
  command end to end.

## Running the examples yourself

Every example named on these pages lives in a crate's `examples/` folder and
starts with the command that runs it, for instance:

```bash
cargo run -p rs-rich --example guide_tables
cargo run -p rs-rich-ext --example guide_data --features data,yaml,toml,xml,jsonpath
```

Add `-- --svg DIR` to write the example's screenshots into `DIR` instead of
printing them. `python3 scripts/capture_guide.py` regenerates every guide
screenshot this way, and `python3 scripts/smoke_cli.py --screenshots
docs/media/guide` regenerates the CLI ones.
