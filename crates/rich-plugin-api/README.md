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

This crate depends only on `rs-rich`. A plugin never depends on `rs-rich-ext`,
which is where plugins are hosted:

```rust,ignore
let mut registry = rich_ext::ExtensionRegistry::with_defaults();
registry.add_plugin(&my_plugin::MyPlugin)?;
registry.install(&mut console);
```

Registration is explicit. There is no auto-discovery and no dynamic loading.
Each plugin declares the `PLUGIN_API_VERSION` it was built for, and the host
refuses a plugin built for a different one.

The API is at 0.0.x and still moving. See the
[plugin guide](https://buchochelliq-labs.github.io/rs-rich-cli/PLUGINS/).
