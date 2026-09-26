//! `rs_rich.plugins`: the `rs-rich-plugin-api` contract from Python, and
//! rich-ext's plugin host (`ExtensionRegistry`).
//!
//! Owner: the plugins area. Python classes act as highlighters, code
//! highlighters, themes, box styles, renderers, fence renderers and text
//! transforms, and register with the host exactly as Rust plugins do:
//! everything a Python plugin adds goes through the Rust registrar and is
//! checked by the Rust host.
//!
//! | Submodule | What |
//! |---|---|
//! | `types` | `PLUGIN_API_VERSION`, `PluginMetadata`, `Capability`, `RegisteredPlugin`, `is_valid_name` |
//! | `errors` | `PluginError`, `HighlightError`, `UnknownThemeError`, `HighlighterChoiceError`; Python exceptions raised in callbacks |
//! | `code` | `CodeHighlighter`, `HighlightedCode` and the Python engine adapter |
//! | `adapters` | highlighter, transform, renderer and fence adapters; `TextTransform`, `TextPipeline`, `SourceRenderer`, `FenceRenderer` |
//! | `plugin` | `Plugin`, `PluginRegistrar`, the built-in plugins |
//! | `registry` | `ExtensionRegistry`, `install_defaults`, installed extensions |
//!
//! # For other areas
//!
//! - [`code_highlighter_arg`] turns a Python argument (a `CodeHighlighter`
//!   handle or any engine object) into an `Arc<dyn CodeHighlighter>`, for
//!   `Syntax(highlighter=...)` / `Markdown(code_highlighter=...)`.
//! - [`fence_renderer_arg`] does the same for fence renderers.
//! - [`installed`] is what `ExtensionRegistry.install(console)` left on a
//!   console, for the console to apply to each core console it builds.

use pyo3::prelude::*;

mod adapters;
mod code;
mod errors;
mod plugin;
mod registry;
mod types;

#[allow(unused_imports)] // for other areas (`Syntax`, `Markdown`)
pub(crate) use adapters::fence_renderer_arg;
#[allow(unused_imports)] // for other areas (`Syntax`, `Markdown`)
pub(crate) use code::code_highlighter_arg;
#[allow(unused_imports)] // for `console.rs`
pub(crate) use registry::{installed, Installed};

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    types::register(m)?;
    errors::register(m)?;
    code::register(m)?;
    adapters::register(m)?;
    plugin::register(m)?;
    registry::register(m)?;
    Ok(())
}
