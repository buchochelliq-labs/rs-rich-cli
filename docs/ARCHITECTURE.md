# Architecture

A thirteen-crate Cargo workspace, plus the Python bindings beside it, with a
strict, one-directional dependency rule.
Each crate versions independently. The [manifest-version table](index.md#versions-in-this-checkout)
tracks this checkout; registry badges show published versions.

```
┌────────────┐     ┌────────────┐     ┌───────────────────────────┐
│  rich-cli  │ ──▶ │  rich-ext  │ ──▶ │  rich (faithful core)     │
│ (bin:rich) │     │ (our code) │     │  mirrors upstream `rich`  │
│rs-rich-cli │     │rs-rich-ext │     │  rs-rich (crates.io name) │
└────────────┘     └────────────┘     └───────────────────────────┘
      │                  │                         ▲    ▲
      │                  ├──────▶ rich-plugin-api ─┤    │
      │                  │  rs-rich-plugin-api     │    │
      │                  │ macros feature          │    │
      │                  ▼                         │    │
      │            ┌──────────────┐                │    │
      │            │ rich-macros  │ ───────────────┘    │
      │            │rs-rich-macros│  (proc-macros)      │
      │            └──────────────┘                     │
      │            ┌──────────────┐                     │
      └──────────▶ │  rich-art    │ ────────────────────┘
                   │ rs-rich-art  │  (FIGlet, images, GIFs)
                   └──────────────┘
        every arrow points towards the core; core depends on none of them
```

The diagram shows the original crates. The others follow the same rule:
`rich-mermaid` → `rich-diagram` → `rich` (with `rich-plugin-api` behind
features), `rich-lumis` → `rich-plugin-api`, and `rich-record`,
`rich-interact`, `rich-micro` and `rich-data` → `rich-ext`. The full graph is in
[AGENTS.md](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/AGENTS.md).

- **`crates/rich`** — the faithful port of the Python `rich` *library*. Mirrors
  upstream module-for-module. Which upstream release it reflects is recorded in
  `UPSTREAM.toml`, not in the crate version.
  Depends on nothing in this workspace and knows nothing about the other crates.
- **`crates/rich-ext`** — everything that is *ours*: extra highlighters,
  renderables (charts among them), the extension registry that hosts
  plugins, and the runtime plugin loaders. Independent SemVer. Talks to core
  only through public APIs and the extension traits.
- **`crates/rich-plugin-api`** — the plugin contract: `Plugin`,
  `PluginMetadata`, `PluginRegistrar`, the plugin-facing traits
  (`SourceRenderer`, `TextTransform`) and `PLUGIN_API_VERSION`. Depends on
  `rich` only, so a plugin never needs `rich-ext`; `rich-ext`'s
  `ExtensionRegistry` is the host that loads plugins. Independent SemVer. See
  [PLUGINS](PLUGINS.md).
- **`crates/rich-diagram`** — graph diagrams: a graph model with a builder,
  the layered layout, a `Diagram` renderable, and a native DOT (Graphviz)
  parser with a `Dot` renderable. Behind its `plugin` feature, a `dot` fence
  and source renderer; behind `graphviz`, Graphviz's own SVG through the
  `dot` program. Depends on `rich` (and `rich-plugin-api` for `plugin`),
  never on `rich-ext`. The CLI uses it for `rich dot`, ```` ```dot ````
  fences and `rich deps --graph`. Independent SemVer.
- **`crates/rich-data`** — tabular data: a row source contract (column
  names, an optional schema in `rich_ext::schema`'s model, rows of
  `rich_ext::table::Value`), CSV/TSV (the port of Python's `csv` sniffer and
  reader that `rich --csv` and `rich chart` use), JSON Lines and serde
  adapters, Arrow `RecordBatch`es behind its off-by-default `arrow` feature,
  opt-in type inference with evidence, and column statistics. Depends on
  `rich` and `rich-ext`'s public API, never on core internals. Independent
  SemVer.
- **`crates/rich-mermaid`** — Mermaid diagrams as a plugin: flowcharts drawn
  as text through `rich-diagram`'s layout, every diagram type through
  Mermaid's CLI (`mmdc`) behind its `mmdc` feature. Depends on `rich`,
  `rich-plugin-api`, `rich-diagram` and (for `mmdc`) `rich-art`, never on
  `rich-ext`. The CLI registers it for `rich mermaid` and Markdown fences.
  Independent SemVer.
- **`crates/rich-lumis`** — the lumis (tree-sitter) syntax highlighter as a
  `CodeHighlighter`, with lumis's Neovim themes and upstream's ANSI themes, and
  a plugin that registers it as `"lumis"`. Its own crate because lumis links
  tree-sitter's C runtime and grammars; it declares its own minimum Rust
  (lumis's). Depends on `rich` and `rich-plugin-api` only. Independent SemVer.
- **`crates/rich-record`** — tapes: scripted terminal sessions run in `bash`
  on a PTY (`portable-pty`), followed by a VT emulator (`vt100`), and rendered
  as PNG, SVG and text screenshots, asciinema casts, GIF, MP4 and an HTML
  page, with an embedded DejaVu Sans Mono. The screen becomes a
  `rich_ext::frame::Frame`, which gives the text grids, the SVGs
  (`Frame::to_svg`) and the page's HTML (`Frame::to_html_with`).
  Behind `rich record` in the CLI (the default `record` feature). Depends on
  `rich` and `rich-ext`. Independent SemVer.
- **`crates/rich-interact`** — interactive components between printing and a
  full TUI: a `Component` state machine, a blocking `run` and an `EventLoop`,
  a terminal `Session` restored on every way out, a painter over
  `rich_ext::frame` cell diffs, a headless driver for tests, and a policy
  that degrades to line prompts without a terminal. Ready-made pickers,
  input, confirm, forms, a pager, a text area, explorers and containers,
  with overlays and a keymap. Behind `rich choose`, `filter`, `input`,
  `confirm`, `pager`, `write`, `file`, `color`, `asset` and `explore` in the
  CLI (the default `interact` feature), which paint on stderr. Depends on
  `rich` and `rich-ext` (and `rich-micro` behind its `micro` feature).
  Independent SemVer.
- **`crates/rich-micro`** — micro assets: emoji-sized inline images written
  `:micro:name:`, `.richmicro` packages, a layered registry, an image
  pipeline, a built-in library, and drawing with Kitty, iTerm2, Sixel or
  half-blocks. Behind `rich micro` (the CLI's default `art` feature).
  Depends on `rich`, `rich-ext`, `rich-plugin-api` and `rich-art`.
  Independent SemVer.
- **`crates/rich-py`** — the Python bindings (`rs-rich` on PyPI, `import
  rs_rich`), built with PyO3 and maturin. Rich's Python API over core
  `rich`, plus the port's own crates (`rs_rich.ext`, `chart`, `diagram`,
  `mermaid`, `art`, `interact`, `micro`, `plugins`) and the `rich` command
  line: the compiled module converts arguments and writes output, and the
  Rust crates render. Outside the Cargo workspace (it needs a Python interpreter to
  build), with its own CI (`python.yml`) and release (`pypi-release.yml`).
  Never published to crates.io; see [Python bindings](https://buchochelliq-labs.github.io/rs-rich-cli/python/).
- **`crates/rich-macros`** — procedural macros (`richf!`, `style!`,
  `theme_key!`, `markup!`, `#[derive(Rich)]`) that check markup and styles at
  compile time. Depends on `rich` only (to parse markup and styles while
  expanding); users reach it through `rich-ext`'s optional `macros` feature.
  Independent SemVer.
- **`crates/rich-art`** — FIGlet banners, images as ASCII, Braille, blocks,
  quadrants or Sixel, animated GIFs and perceptual image diffs. Depends on
  `rich` only. Independent SemVer.
- **`crates/rich-cli`** — the binary mirroring the Python `rich-cli` tool (a
  separate upstream project with its own version). Built on `rich`,
  `rich-ext` and `rich-diagram`, and behind its default features `rich-art`
  and `rich-micro` (`art`), `rich-mermaid`, `rich-record` and
  `rich-interact`; its own commands are documented binary-boundary
  conveniences in [PORTING](PORTING.md).

Why the split: it makes upstream syncs a mechanical diff-and-port of `crates/rich`
only, and guarantees our features can never make that harder. See
[AGENTS.md](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/AGENTS.md).

## Render pipeline

```
&str (markup)
   │  markup::render + Theme          → Text (plain string + Style spans)
   ▼
Text                                   crates/rich/src/text.rs
   │  Highlighter(s) add spans         (rich-ext, via Console)
   │  Text::render(base, system)       → Vec<Segment>
   ▼
Segment { text, style, control }       crates/rich/src/segment.rs
   │  Console applies color system     (downgrade + SGR)
   ▼
ANSI bytes → stdout                    crates/rich/src/console.rs
```

`ColorSystem` (Standard / EightBit / Truecolor / Windows) is detected by the
`Console` and applied last: styles carry rich color information and are *downgraded*
to the target system at the final step via the redmean nearest-color search
(`color::match_color`, ported from `rich.palette`).

## Key types

| type | file | upstream |
|------|------|----------|
| `Color`, `ColorTriplet`, `ColorSystem` | `color.rs` | `rich.color` |
| `Style` | `style.rs` | `rich.style` |
| `Text`, `Span` | `text.rs` | `rich.text` |
| `Segment` | `segment.rs` | `rich.segment` |
| `Console`, `ConsoleBuilder` | `console.rs` | `rich.console` |
| `Renderable`, `Highlighter` (extension points) | `protocol.rs` | `rich.protocol` / `rich.abc` |
| `Theme` | `theme.rs` | `rich.theme` |

See [PORTING.md](PORTING.md) for the full module map and per-module status, and
[PLUGINS.md](PLUGINS.md) for the extension model.
