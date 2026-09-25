# Plugin & extension design

This port keeps its core a faithful mirror of upstream `rich`. Everything we add
lives on the *outside* of that core, through a small set of **extension points**.
This document describes how that works today (internal-facing) and the intended
path to opening it up to third-party plugins.

## Why

If our own features were woven into `crates/rich`, every upstream sync would be a
manual three-way merge. Instead:

- `crates/rich` defines **extension-point traits** and calls them, but ships only
  upstream's built-in implementations. It has **no knowledge** of `rich-ext`.
- `crates/rich-ext` provides extra implementations and **registers** them onto a
  `Console`. Syncing upstream never touches `rich-ext`.

## Extension points (today)

Defined in [`crates/rich/src/protocol.rs`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/rich/src/protocol.rs):

| trait          | upstream analogue          | purpose |
|----------------|----------------------------|---------|
| `Renderable`   | `__rich_console__` protocol | make a custom type printable by `Console` |
| `LineRenderable` | incremental consumption of rendering generators | stream styled lines without collecting the full rendered output; implemented by `Table` |
| `Highlighter`  | `Highlighter` ABC          | add style spans to `Text` (numbers, URLs, syntax, …) |
| `FenceRenderer` | none (upstream always highlights a fence) | draw ```` ```lang ```` fences in `Markdown` instead of highlighting them, via `Markdown::fence_renderer`; with none added, Markdown is unchanged (0.0.12) |
| `CodeHighlighter` | Pygments behind `Syntax` | the syntax-highlighting engine for `Syntax` and Markdown code; `SyntectHighlighter` is the default, and `Syntax::highlighter` / `Markdown::highlighter` take any other (0.0.12) |

More seams (custom `Box` sets, spinners, themes) are added here as the
corresponding modules are ported — always as a trait the core calls, never as an
`if cfg!(feature = "ours")` branch inside core logic.

## Registration (explicit, not magic)

We deliberately use **explicit registration** rather than compile-time
auto-discovery (`inventory`/`linkme`): it is easier to debug, reason about, and
test, and it keeps the install order deterministic.

```rust
use rich::{Console, ColorSystem};
use rich_ext::{ExtensionRegistry, ConsoleExt};

// Option A: the convenience trait
let mut console = Console::new();
console.install_extensions();

// Option B: a registry you compose yourself
let mut console = Console::new();
let mut registry = ExtensionRegistry::new();
registry.register_highlighter(|| Box::new(rich_ext::NumberHighlighter::new()));
registry.install(&mut console);
```

The registry ([`crates/rich-ext/src/registry.rs`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/rich-ext/src/registry.rs))
holds *factories* so one registry can be installed onto many consoles.

## Writing a plugin (0.0.12)

The plugin contract is its own crate, `rs-rich-plugin-api` (imported as
`rich_plugin_api`, and re-exported as `rich_ext::plugin`). It depends only on
core `rich`, so a plugin crate never depends on `rich-ext`, and every
first-party plugin goes through the same contract.

A plugin implements `Plugin`: `metadata()` says who it is, and `register()`
adds capabilities through a `PluginRegistrar`:

| registrar method | adds |
|---|---|
| `highlighter(factory)` | a regex `Highlighter` for printed text |
| `code_highlighter(name, engine)` | a `CodeHighlighter`, selectable by name |
| `theme(name, theme)` | a named `Theme` |
| `box_style(name, box)` | a named table/panel box style |
| `renderer(name, renderer)` | a `SourceRenderer` that turns source text into a renderable |
| `fence_renderer(language, renderer)` | a `FenceRenderer` for Markdown fences in that language |

```rust
use rich_ext::plugin::{Plugin, PluginError, PluginMetadata, PluginRegistrar};
use rich_ext::ExtensionRegistry;

struct Solarized;

impl Plugin for Solarized {
    fn metadata(&self) -> PluginMetadata {
        PluginMetadata::new("solarized", "Solarized themes", env!("CARGO_PKG_VERSION"))
    }
    fn register(&self, registrar: &mut dyn PluginRegistrar) -> Result<(), PluginError> {
        let theme = rich::Theme::from_styles([("repr.number", "#268bd2")], true)
            .map_err(|e| PluginError::Other(e.to_string()))?;
        registrar.theme("solarized", theme);
        Ok(())
    }
}

let mut registry = ExtensionRegistry::with_defaults();
registry.add_plugin(&Solarized)?;
```

`ExtensionRegistry::add_plugin` is the host. It is all-or-nothing: a plugin
whose `register` fails, or that breaks a rule below, leaves the registry as it
was.

- **API version.** `PluginMetadata::new` records `PLUGIN_API_VERSION`. A host
  refuses a plugin built for another version (`PluginError::IncompatibleApi`).
  At 0.0.x the contract still changes, and every breaking change bumps it.
- **Names.** The plugin id and every capability name use lowercase letters,
  digits, `-`, `_` and `.` (at most 64 bytes), so they are safe to print and to
  use as CLI values.
- **No silent overrides.** Two plugins may not register the same capability
  under the same name, and a plugin id may be added once (`Conflict`,
  `DuplicatePlugin`).

`ExtensionRegistry::with_defaults()` adds the built-in `rich-ext` plugin, which
provides the number highlighter and the `syntect` code highlighter. Query what
is registered with `plugins()`, `code_highlighter(name)`, `theme(name)`,
`box_style(name)`, `renderer(name)`, `fence_renderer(language)` and
`provided_by(capability)`. `fences()` combines every registered fence renderer
into one for `Markdown::fence_renderer`, routed by language.

`rs-rich-lumis` (`LumisPlugin`) registers the code highlighter `"lumis"`:
tree-sitter grammars with lumis's Neovim themes, and `ansi_dark`/`ansi_light`
mapped from tree-sitter capture names to upstream's Pygments token styles.

The first plugin built this way is `rs-rich-mermaid` (`MermaidPlugin`): a
`mermaid` fence renderer and source renderer, with flowcharts drawn as text and
every diagram type through `mmdc` behind its `mmdc` feature.
`rich doctor` lists the registered plugins and the API version (and includes
them in `--json`).

## Roadmap: from internal to public

1. **Done — internal.** `rich-ext` was the only registrant.
2. **Now — a public contract (0.0.12).** `rs-rich-plugin-api` defines what a
   plugin is; the ext registry hosts it. Not yet a stability promise: see the
   API version above.
3. **Later — third-party plugin loading.** Evaluate compile-time aggregation
   (`inventory`/`linkme`) for "just add the dependency" registration, and/or a
   dynamic/WASM boundary for runtime plugins. Tracked as its own roadmap issue;
   not built until the trait surface has settled.

Whatever we add, the invariant holds: **the core never learns about a specific
extension.**

`rich_ext::layout` offers bounded composition independently of core Layout.
`allocate` validates constraints and returns sizes, padding and relaxed request
indices. `fit_segments` shares Unicode-aware overflow across extension renderables.
See `cargo run -p rs-rich-ext --example layout -- 20` for a narrow sidebar layout.

### Explicit rendering environment

The sanctioned `protocol::RenderEnvironment` trait and optional
`protocol::ConsoleEnvironment` let a Console carry an immutable capability snapshot
through nested renderables. This is the only new core seam; a Console without it
keeps upstream-compatible behavior. `rich-ext::target::RenderTarget` implements
destination policy and creates configured consoles. Core has no dependency on ext,
art or CLI. Prefer one observation at the application boundary followed by explicit
capabilities; never redetect stdout inside an explicit render.

`TargetKind` covers Terminal, PlainStream, Capture, Html, Svg and Custom.
Detection provenance is separate from the rendering snapshot. `Support::Inferred`
is a conservative hint; protocol rendering can require `Confirmed`. Unicode and
hyperlink policy are declared by the caller. Legacy renderers may still have their
own sizing choices; wrap them in bounded `LayoutNode` containers for strict cells.
`layout::Overflowing` gives any renderable one explicit `OverflowPolicy`: Syntax,
JSON and Text are rendered at their measured natural width and every line is then
wrapped, folded, cropped or ellipsised to the cell width (wide glyphs never split);
output that already fits is returned byte-identical to core.
`LayoutNode` leaves receive their region's height, like upstream `Layout`, so a
`Panel` leaf fills its region; opt into `.content_height()` for natural height.

Enable `rs-rich-ext` features `testing`, `log` and `tracing` independently. The
snapshot helper has no process environment dependency or assertion-framework
requirement. See the `snapshot`, `diagnostic`, `log_adapter`, `tracing_adapter`,
`layout`, `live_regions` and `expanded_release` examples in the crate.
