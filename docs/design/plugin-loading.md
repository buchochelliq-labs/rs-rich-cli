# Plugin loading (#14, #232)

**Status:** implemented in 0.0.13 (plugin API 0.0.2, ext 0.0.11, CLI 0.0.13).
This note records why third-party plugins load the way they do. How to write
one is in [the plugin guide](../PLUGINS.md).

## Summary

A plugin reaches a registry in one of four ways. Each is more opt-in than the
one before it.

| Way | How | Code runs | Off by default? |
|---|---|---|---|
| Explicit | `registry.add_plugin(&MyPlugin)` | in process, compiled in | no: the caller names the plugin |
| Linked | `export_plugin!(MyPlugin)` in the plugin crate, `ExtensionRegistry::with_linked_plugins()` in the host | in process, compiled in | yes: the host must ask for linked plugins |
| Native | a `cdylib` loaded from a path through a C ABI | in process, arbitrary native code | yes: the `dylib-plugins` feature, then a path |
| WASM | a `.wasm` module loaded from a path, run by `wasmi` | in a sandbox | yes: the `wasm-plugins` feature, then a path |

All four produce an ordinary `rich_plugin_api::Plugin`, so
`ExtensionRegistry::add_plugin` checks every one the same way: the name rules,
the API version, no two providers for one capability, all or nothing. Core
`rich` is unchanged and knows about none of this.

## Compile-time plugins: `inventory`

`rich_plugin_api::export_plugin!(expr)` submits a `LinkedPlugin` (a function
that makes the plugin, and the module that exported it) to a link-time
collection. `rich_plugin_api::linked_plugins()` lists them, and
`ExtensionRegistry::add_linked_plugins()` adds them.

We use [`inventory`](https://crates.io/crates/inventory) rather than
[`linkme`](https://crates.io/crates/linkme):

- **No proc macro.** `inventory`'s `submit!` is a declarative macro. `linkme`'s
  `#[distributed_slice]` is an attribute macro, which pulls `syn`, `quote` and
  `proc-macro2` into every plugin's build. The plugin API crate is meant to
  stay small; `inventory` adds one crate (and `rustversion`), with no build
  script.
- **Platform coverage.** `inventory` registers through each platform's
  constructor section (`.init_array`, `__mod_init_func`, `.CRT$XCU`) and
  supports the platforms `rich` does. `linkme` relies on
  linker section start/stop symbols, which some linkers do not provide.
- **The cost is a constructor per plugin** that runs before `main`. It only
  links a node into a list; nothing of the plugin runs until a host asks.

Order is not the linker's. `add_linked_plugins` collects every plugin's id,
sorts them, and refuses the whole set when two share an id
(`PluginError::DuplicatePlugin`), before adding any. So the same binary always
gets the same registry, and a duplicate cannot depend on link order.

Linked plugins are **not** added by `ExtensionRegistry::with_defaults()`:
registration stays explicit, and a host opts in with `with_linked_plugins()`.
The `rich` binary does opt in, so a custom build that adds a plugin crate as a
dependency gets it without code changes.

One caveat applies to every link-time scheme: the linker may drop a crate the
binary depends on but never names. `use my_plugin as _;` keeps it.

## One ABI for native and WASM plugins

Rust's ABI is unstable, and a trait object cannot cross a library boundary
built by another compiler. So runtime plugins get a deliberately small
contract that is **text in, text out** (`rich_plugin_api::abi`):

| Kind | Input | Output | Becomes |
|---|---|---|---|
| `transform` | plain text | plain text | a named `TextTransform` |
| `highlighter` | plain text | `START END STYLE` lines (byte offsets) | a `Highlighter` |
| `fence-markup` | a fence's code, and the width | `rich` markup | a `FenceRenderer` |
| `fence-ansi` | a fence's code, and the width | ANSI SGR text | a `FenceRenderer` |

Every call also receives the width available in cells (0 when unknown).
Renderables, themes, box styles and code highlighters stay compile-time only:
their Rust types cannot cross the boundary, and a serialized form would be a
second rendering API to keep stable.

Both loaders decode a plugin's self-description into one type,
`rich_plugin_api::abi::PluginAbi` (ABI version, name, version, description,
and the capabilities in order), and wrap it with a backend in
`rich_ext::plugin_loading::RuntimePlugin`. One set of adapters in `rich-ext`
turns the capabilities into registry types and sanitizes every output. The
loaders differ only in how they call the plugin.

### Versioning

`ABI_MAJOR.ABI_MINOR` is 1.0. The ABI version is separate from
`PLUGIN_API_VERSION` (which versions the Rust traits) because the two change
for different reasons. A host refuses another major. A newer minor loads if it
uses nothing the host lacks; an unknown capability kind is refused, never
skipped, so a plugin never silently loses half of what it does.

### Native plugins (`dylib-plugins`)

The library exports `rich_plugin_entry`, an `extern "C" fn() -> *const
PluginDescriptor`. The descriptor is `repr(C)`: the ABI version (two `u32`s
that stay first in every version, so a host reads them before trusting the
rest), the name, version and description as pointer and length pairs, the
capability array, and a vtable of two functions: `call` and `free`. Output is
allocated by the plugin and given back to it through `free`, so the two sides
never share an allocator.

A plugin author writes no unsafe code: `export_dylib_plugin!(|| Exports::new(…)
.transform("reverse", reverse))` generates the entry point and the `call`
function, which catches panics so none unwinds across the boundary.

The host (`libloading`) canonicalizes the path, so a bare name is never looked
up in the system library path. The `Library` lives inside the backend, and
every registered capability holds the backend through an `Arc`, so the
library stays loaded as long as anything registered from it exists; it is
never unloaded while a function pointer into it can be called.

### WASM plugins (`wasm-plugins`)

A module exports `memory`, `rich_plugin_alloc`, `rich_plugin_manifest` and
`rich_plugin_call`. The manifest is `PluginAbi` in a line-based text form
(`PluginAbi::to_manifest`), because a WASM module has no C struct layout to
share. Results are packed `ptr << 32 | len`; bit 63 marks an error message.

We use [`wasmi`](https://crates.io/crates/wasmi), a pure-Rust interpreter,
rather than `wasmtime`: no JIT, no C or assembly, far fewer dependencies, and
it builds on the workspace's MSRV. Plugins handle short texts, so an
interpreter's speed is enough. The sandbox:

- **No imports.** A module that imports anything is refused at load time, so
  it has no WASI, file system, network, clock or randomness. It sees only the
  text it is given.
- **Fuel.** Every call (instantiation included) gets a fuel budget, 50 million
  units by default; a module that runs out is stopped with an error.
- **Memory.** An instance may use 64 MiB by default. A module whose memory
  starts above the cap is refused at load time; one that grows past it traps.
  `wasmi`'s strict limits bound the module's own size and structure, and the
  file may be 16 MiB.
- **A fresh instance per call,** so no state or leaked memory carries over,
  and calls cannot interfere with each other.

## Output hygiene

Runtime plugin output reaches a terminal, so the host never trusts it:

- Output over 4 MiB, or not UTF-8, is an error.
- `transform`, `highlighter` and `fence-markup` output goes through
  `sanitize_terminal_controls`: every control character, ESC included, is made
  visible.
- `fence-ansi` output goes through `sanitize_ansi_for_decoder`, the sanitizer
  `rich view` uses: CSI sequences reach the ANSI decoder, which keeps only SGR
  styling, OSC strings are removed, and other controls are made visible.
- A highlighter span that is out of range, not on a character boundary, or
  has a style that does not parse is skipped.
- A failed fence call leaves the fence to render as code, as upstream does.

## The CLI

`rich --plugin PATH` loads a plugin for one run. A config's `plugins = [...]`
does the same, **only from a trusted config**: the user's own
`~/.config/rich/config.toml` or a file given with `--config`. A project's
`./rich.toml` is ignored with a warning, as it is for
`mermaid_backend = "mmdc"`, since a checked-out repository could otherwise
make every `rich` command in it run a native library. Relative paths are
relative to the config file.

Loading fails closed: a missing file (exit 3), a file of an unknown kind, a
build without the loader, a refused ABI or a name another plugin provides
(exit 2) all stop the command with a message naming the file. Nothing panics.

A loaded plugin draws its Markdown fences, adds its highlighters, and its
transforms run with `--transform NAME`: repeatable, in the order given, on
text, `--print` and `--syntax`, after `--filter` and before `--highlight`. The
names are checked once plugins are loaded, so an unknown one is a usage error
listing the names there are.

`rich plugins list` and `rich plugins info NAME` show every plugin with its
source (`built-in`, `linked`, `native`, `wasm`), version, ABI and
capabilities; `--report json` writes the same to stdout.

## Deferred

- **A Rust helper for WASM plugins.** Native plugins get
  `export_dylib_plugin!`; a WASM plugin is written against the documented
  exports (the example is hand-written WAT). A matching macro needs a
  `wasm32` build in CI to test.
- **Manifests without running code, the registry and `rich plugin install`**
  (#232 phases 5 and 6).
- **Declared permissions.** WASM plugins get no host access at all, so there
  is nothing yet to permit.
