# rs-rich-plugin-api

The plugin contract for [`rs-rich`](https://crates.io/crates/rs-rich), the Rust
port of Python's [`rich`](https://github.com/Textualize/rich).

A plugin is a type that implements [`Plugin`]: it describes itself with
`PluginMetadata` and registers what it adds through a `PluginRegistrar`:

- regex highlighters, applied to printed text;
- code highlighters (`rich::CodeHighlighter`), selectable by name;
- named themes and box styles;
- named source renderers, which turn text (a diagram, a data file) into a
  renderable;
- renderers for Markdown code fences of a given language;
- named text transforms (`TextTransform`), which a host chains into a
  pipeline.
- named interactive components (`PluginComponent`, in the `component`
  module), which an app mounts beside the built-in components of
  `rs-rich-interact`.

This crate depends only on `rs-rich`. A plugin never depends on `rs-rich-ext`,
which is where plugins are hosted:

```rust,ignore
let mut registry = rich_ext::ExtensionRegistry::with_defaults();
registry.add_plugin(&my_plugin::MyPlugin)?;
registry.install(&mut console);
```

Registration is explicit by default. Each plugin declares the
`PLUGIN_API_VERSION` it was built for, and the host refuses a plugin built for
a different one. Two more ways in are opt-in:

- **Linked plugins.** `rich_plugin_api::export_plugin!(MyPlugin);` registers a
  plugin for link-time collection (through `inventory`). A host that calls
  `ExtensionRegistry::with_linked_plugins()` adds every linked plugin, sorted
  by id; a duplicate id is an error.
- **Runtime plugins.** The `abi` module is a small, versioned, text-only ABI
  for plugins loaded from a file: a native library (`export_dylib_plugin!`
  writes its C entry point, with no unsafe code in the plugin) or a sandboxed
  WASM module. They may contribute transforms, highlighters and Markdown fence
  renderers that return markup or ANSI (not components, which keep state);
  the host sanitizes everything they return. Loading is in `rs-rich-ext`, behind its `dylib-plugins` and
  `wasm-plugins` features. See `examples/dylib-plugin` and
  `examples/wasm/shout.wat` in the repository.

Native plugins run arbitrary code in the host process: load one only if you
would run it as a program.

The API is at 0.0.x and still moving. See the
[plugin guide](https://buchochelliq-labs.github.io/rs-rich-cli/PLUGINS/).
