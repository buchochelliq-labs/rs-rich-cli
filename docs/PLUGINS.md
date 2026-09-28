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

## Registration (explicit by default)

Registration is **explicit** by default: a host names each plugin it adds, which
is easy to debug, reason about and test, and keeps the install order
deterministic. Linked and runtime plugins (below) are opt-in on top of that.

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
| `transform(name, transform)` | a `TextTransform`, chained by name into a text pipeline (0.0.12; see [Transforms](guide/ext/transforms.md)) |

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
  digits, `-`, `_` and `.` (at most 64 bytes), and start with a letter or
  digit, so they are safe to print and to use as CLI values.
- **No silent overrides.** Two plugins may not register the same capability
  under the same name, and a plugin id may be added once (`Conflict`,
  `DuplicatePlugin`).

`ExtensionRegistry::with_defaults()` adds the built-in `rich-ext` plugin, which
provides the number highlighter and the `syntect` code highlighter. Query what
is registered with `plugins()`, `code_highlighter(name)`, `theme(name)`,
`box_style(name)`, `renderer(name)`, `fence_renderer(language)`,
`transform(name)`, `transform_names()` and `provided_by(capability)`.
`text_pipeline(names)` chains registered text transforms, in the order given,
into a `rich_ext::transform::Pipeline`. `fences()` combines every registered fence renderer
into one for `Markdown::fence_renderer`, routed by language.

**A default code highlighter.** `ExtensionRegistry::set_default_code_highlighter(name,
theme)` chooses a registered code highlighter (and optionally one of its
themes) for every console the registry is installed onto, through core's
`ConsoleCodeHighlighting`. `Syntax`, Markdown code, `source_view` and the diff
views without a highlighter of their own use it. An unknown name or theme is a
`HighlighterChoiceError` listing the choices. The CLI exposes this as
`--highlighter` and `--code-theme`.

`rs-rich-lumis` (`LumisPlugin`) registers the code highlighter `"lumis"`:
tree-sitter grammars with lumis's Neovim themes, and `ansi_dark`/`ansi_light`
mapped from tree-sitter capture names to upstream's Pygments token styles.

The first plugin built this way is `rs-rich-mermaid` (`MermaidPlugin`): a
`mermaid` fence renderer and source renderer, with flowcharts drawn as text and
every diagram type through `mmdc` behind its `mmdc` feature.
`rich doctor` lists the registered plugins and the API version (and includes
them in `--json`).

## Third-party plugins (0.0.13)

Beyond `add_plugin`, a plugin can reach a registry in three more ways, each
more opt-in than the last. Why they work this way is in the design note,
[Plugin loading](design/plugin-loading.md).

### Linked: add the dependency

A plugin crate registers itself once:

```rust
rich_plugin_api::export_plugin!(MyPlugin);
```

A host that wants every linked plugin asks for them:

```rust
let registry = rich_ext::ExtensionRegistry::with_linked_plugins()?;
```

They are collected at link time (with `inventory`) and added **sorted by
plugin id**, never in link order; two with the same id are an error
(`PluginError::DuplicatePlugin`) and none is added. `with_defaults()` does not
add them. The `rich` binary does, so a custom build that depends on a plugin
crate gets it. If the binary never names the crate, the linker may drop it:
write `use my_plugin as _;`.

### Runtime: native libraries and WASM modules

Runtime plugins are loaded from a path, behind `rs-rich-ext` features that are
off by default: `dylib-plugins` for native libraries (`.so`, `.dylib`,
`.dll`) and `wasm-plugins` for WASM modules (`.wasm`).

```rust
use rich_ext::plugin_loading::{load, LoadOptions};

let plugin = load(path, &LoadOptions::default())?; // a LoadError names the path
registry.add_plugin(&plugin)?;
```

Rust types cannot cross a library boundary, so a runtime plugin exchanges UTF-8
text only, through the ABI in `rich_plugin_api::abi`. It may contribute:

| kind | input | output |
|---|---|---|
| `transform` | plain text | plain text, as a named `TextTransform` |
| `highlighter` | plain text | `START END STYLE` lines (byte offsets), as a `Highlighter` |
| `fence-markup` | a fence's code and the width | `rich` markup, as a `FenceRenderer` |
| `fence-ansi` | a fence's code and the width | ANSI SGR text, as a `FenceRenderer` |

Themes, box styles, renderables and code highlighters stay compile-time only.

- **A native plugin** is a `cdylib` that depends on `rs-rich-plugin-api` and
  calls `export_dylib_plugin!` with an `abi::Exports` description; it needs no
  unsafe code. See
  [`examples/dylib-plugin`](https://github.com/buchochelliq-labs/rs-rich-cli/tree/main/crates/rich-plugin-api/examples/dylib-plugin).
- **A WASM plugin** exports `memory`, `rich_plugin_alloc`,
  `rich_plugin_manifest` and `rich_plugin_call` (see `abi::wasm`) and imports
  nothing. See the hand-written
  [`examples/wasm/shout.wat`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/rich-plugin-api/examples/wasm/shout.wat).

The ABI is versioned separately from `PLUGIN_API_VERSION` (it is 1.0). A host
refuses a plugin built for another major, and one that declares a capability
kind the host does not know.

### In the `rich` CLI

`rich plugins list` shows every plugin with its source (`built-in`, `linked`,
`native`, `wasm`), version, ABI and capabilities, and `rich plugins info NAME`
shows one; both take `--report json`. In a build with `dylib-plugins` or
`wasm-plugins` (`cargo install rs-rich-cli --features wasm-plugins`),
`--plugin PATH` loads a runtime plugin for one run, and a `plugins = [...]`
list in `~/.config/rich/config.toml` or a file given with `--config` loads
them every time. Loaded plugins draw Markdown fences and highlight printed
text.

## Security: threat model

Plugins are code from someone else. What each kind can do, and what stands in
its way:

- **Nothing loads unless the user opts in.** Compile-time plugins are chosen
  by whoever builds the binary. Runtime plugins need a Cargo feature that is
  off by default, and then a path: on the command line, or in the user's own
  config. A project's `./rich.toml` may **not** list plugins; `rich` ignores
  the list with a warning, so cloning a repository and running `rich` in it
  never loads that repository's code. (The same rule keeps a project config
  from choosing `mermaid_backend = "mmdc"` or files to write.)
- **Native plugins run arbitrary code, so they are trust-only.** Loading a
  library runs its initialisers, and every call runs in the `rich` process
  with the user's permissions. The ABI makes the boundary well-defined (only
  `repr(C)` data crosses it, another ABI major is refused before the rest of
  the descriptor is read, and the functions `export_dylib_plugin!` generates
  turn a panic into an error), but it is not a sandbox. Load only a library you
  would run as a program.
- **WASM plugins are sandboxed.** A module that imports anything is refused,
  so it has no WASI, file system, network, clock or randomness. Each call runs
  in a fresh instance with a fuel limit (a loop that never ends is stopped)
  and a memory cap (a module that starts or grows past it is refused or
  trapped). A WASM plugin can waste at most one call's fuel and cannot see
  anything but its input.
- **Output is sanitized, whatever the kind.** Output over 4 MiB or not UTF-8
  is an error. Every control character in transform, highlighter and markup
  output, ESC included, is made visible. Only `fence-ansi` may carry ANSI, and
  it goes through the same sanitizer `rich view` uses: SGR styling survives,
  OSC strings (titles, clipboard, hyperlinks) are removed, and every other
  control is made visible. A highlighter span that does not fit its text is
  skipped. A failed fence renders as a code block.
- **Names cannot collide or spoof.** Every plugin, compiled in or loaded, goes
  through `add_plugin`: its id and capability names are restricted to
  lowercase letters, digits, `-`, `_` and `.`, and a name another plugin
  already provides is refused, so a runtime plugin cannot replace a built-in.

## Roadmap: from internal to public

1. **Done — internal.** `rich-ext` was the only registrant.
2. **Done — a public contract (0.0.12).** `rs-rich-plugin-api` defines what a
   plugin is; the ext registry hosts it. Not yet a stability promise: see the
   API version above.
3. **Done — third-party loading (0.0.13).** Linked plugins, native and WASM
   runtime plugins, and `rich plugins list/info`, as above.
4. **Later.** Plugin manifests that can be read without running code, a
   Git-backed index, `rich plugin install` (#232), and CLI flags that apply a
   runtime plugin's transforms.

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
