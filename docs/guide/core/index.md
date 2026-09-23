# The core library

`rs-rich` (imported as `rich`) is the core of the project: a module-for-module
port of Python [`rich`](https://github.com/Textualize/rich). Everything that
draws in the terminal — styled text, tables, panels, trees, progress bars,
Markdown, syntax highlighting — lives here. The other crates build on it.

```bash
cargo add rs-rich     # the package name; the library is `rich`
```

```rust
use rich::{Console, Table, Panel};
```

This section of the guide covers the core crate only. Extensions (the plugin
registry, extra highlighters, log handlers) are in
[`rs-rich-ext`](https://docs.rs/rs-rich-ext/latest/rich_ext/); image rendering
is in [`rs-rich-art`](https://docs.rs/rs-rich-art/latest/rich_art/).

## What is in the crate

| Area | Types | Page |
|---|---|---|
| Output | `Console`, `ConsoleOptions`, `Segment`, `Measurement`, the `Renderable` trait | [Console and printing](console.md) |
| Text | `Text`, `Style`, `Color`, markup, emoji, highlighters, `Theme` | [Text and style](text-and-style.md) |
| Tables | `Table`, `ColumnOptions`, `Cell`, box styles | [Tables](tables.md) |
| Layout | `Panel`, `Padding`, `Align`, `Columns`, `Rule`, `Layout`, `Constrain`, `Styled` | [Layout](layout.md) |
| Hierarchies | `Tree` | [Tree](tree.md) |
| Running work | `Progress`, `track`, `Live`, `Status`, `Spinner`, `ProgressBar` | [Progress and live displays](progress-and-live.md) |
| Documents | `Syntax`, `Markdown`, `Json`, `Pretty` | [Code and data](code-and-data.md) |
| Diagnostics | `LogRecord`, `LogRender`, `Traceback` | [Logging and errors](logging-and-errors.md) |
| Input | `Prompt`, `Confirm`, `IntPrompt`, `FloatPrompt` | [Prompts](prompts.md) |
| Output files | HTML, SVG and text export, `TerminalTheme` | [Exporting](export.md) |

The most-used names are re-exported at the crate root (`rich::Table`,
`rich::Panel`, …). Less common ones stay in their module: `rich::markdown::Markdown`,
`rich::prompt::Prompt`, `rich::r#box::ROUNDED` (`box` is a Rust keyword, hence
the `r#`).

## How rendering works

Every visible thing goes through the same four steps. Knowing them explains
most of the API.

```text
 your value ──► Renderable ──► measure ──► render ──► Vec<Segment> ──► Console
 (Table, Text,   (a trait)     (min/max    (fit into   (text + style    writes ANSI,
  Panel, …)                     width)      options)    pieces)         or exports
                                                                        HTML/SVG/text
```

1. **`Renderable`.** Anything printable implements
   [`Renderable`](https://docs.rs/rs-rich/latest/rich/protocol/trait.Renderable.html).
   It is the Rust form of upstream's `__rich_console__` protocol, and you can
   implement it for your own types ([example](console.md#writing-your-own-renderable)).
2. **Measure.** Containers ask their children how wide they want to be — a
   [`Measurement`](https://docs.rs/rs-rich/latest/rich/measure/struct.Measurement.html)
   with a minimum and maximum cell width. This is how a table sizes its
   columns and how `Align::center` knows what to centre.
3. **Render.** The renderable receives
   [`ConsoleOptions`](https://docs.rs/rs-rich/latest/rich/console/struct.ConsoleOptions.html)
   (the width it must fit, an optional height, justify/overflow overrides) and
   returns a flat list of
   [`Segment`](https://docs.rs/rs-rich/latest/rich/segment/struct.Segment.html)s:
   a string plus an optional `Style`. Newlines are segments too.
4. **Output.** The `Console` turns segments into bytes for the terminal
   (downgrading colours to what the terminal supports), or records them so they
   can be exported as HTML, SVG or plain text.

Containers such as `Panel` and `Table` render their children through the same
protocol, so everything nests: a table in a panel in a layout.

## How it mirrors upstream

- **Same modules, same names.** `rich/table.py` is `crates/rich/src/table.rs`;
  `Table.add_column` is `Table::add_column`. Where Python uses keyword
  arguments, Rust uses builder methods (`Panel::new(x).title("t")`) or an
  options struct (`ColumnOptions`).
- **Same output.** Golden tests compare the bytes this crate writes against
  real Python rich output for the pinned upstream version (see
  [Parity](../../parity.md)). The screenshots in this guide are real output,
  exported by the guide's example programs.
- **Documented differences.** Where Rust cannot follow Python — there is no
  `repr()` to pretty-print, no Python traceback, no Pygments — the port says so
  in [Divergences](../../DIVERGENCES.md). Some upstream options are not ported
  yet; each page lists what is missing under "Not yet ported".
- **No extras in the core.** Features that upstream does not have belong in
  `rs-rich-ext`, so the core stays a faithful mirror.

## Running the examples

Every snippet in this guide is cut from an example program in
[`crates/rich/examples/`](https://github.com/buchochelliq-labs/rs-rich-cli/tree/main/crates/rich/examples),
so it compiles and runs:

```bash
cargo run -p rs-rich --example guide_tables              # print to your terminal
cargo run -p rs-rich --example guide_tables -- --svg out # write the screenshots
```

## Pages

- [Console and printing](console.md)
- [Text and style](text-and-style.md)
- [Tables](tables.md)
- [Layout](layout.md)
- [Tree](tree.md)
- [Progress and live displays](progress-and-live.md)
- [Code and data](code-and-data.md)
- [Logging and errors](logging-and-errors.md)
- [Prompts](prompts.md)
- [Exporting](export.md)

API reference: [docs.rs/rs-rich](https://docs.rs/rs-rich/latest/rich/).
