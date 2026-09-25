//! `rs_rich._native`: the Python bindings' only compiled module.
//!
//! Everything that renders is core `rich` (and, for their areas, the other
//! Rust crates). This crate converts Python values into core types and back,
//! and nothing more: a Python `Table` stores what it was given and builds a
//! core `Table` when printed, so nested objects can still change until then,
//! as they can in Python `rich`.
//!
//! # Layout
//!
//! One module per area, each with `pub(crate) fn register(m)` that adds its
//! classes to `_native`; this file only calls them. An area edits its own
//! module (and may add submodules below it), its Python modules under
//! `python/rs_rich/`, its section of `_native.pyi`, its docs page and its
//! tests, and nothing shared. See `renderable.rs` for making a class
//! renderable.
//!
//! | Module | Area |
//! |---|---|
//! | `errors`, `limits`, `convert`, `renderable`, `protocol`, `segment`, `console`, `terminal_theme`, `boxes`, `table`, `panel` | foundation |
//! | `text`, `style`, `theme`, `color` | text, style, colour, emoji, themes |
//! | `renderables` | static renderables |
//! | `code` | Markdown, Syntax, JSON, Pretty, inspect, Traceback, highlighters |
//! | `live` | Live, Progress, Status, Screen, Pager, prompts, logging |
//! | `ext` | `rich-ext` |
//! | `art` | `rich-art` and Mermaid |
//! | `plugins` | the plugin API from Python |
//! | `cli` | the `rich` CLI from Python |

use pyo3::prelude::*;

mod art;
mod boxes;
mod cli;
mod code;
mod color;
mod console;
mod convert;
mod errors;
mod ext;
mod limits;
mod live;
mod panel;
mod plugins;
mod protocol;
mod renderable;
mod renderables;
mod segment;
mod style;
mod table;
mod terminal_theme;
mod text;
mod theme;

#[pymodule]
fn _native(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    m.add("escape", pyo3::wrap_pyfunction!(escape, m)?)?;
    // Foundation.
    errors::register(m)?;
    protocol::register(m)?;
    segment::register(m)?;
    boxes::register(m)?;
    terminal_theme::register(m)?;
    console::register(m)?;
    table::register(m)?;
    panel::register(m)?;
    // Areas.
    text::register(m)?;
    style::register(m)?;
    theme::register(m)?;
    color::register(m)?;
    renderables::register(m)?;
    code::register(m)?;
    live::register(m)?;
    ext::register(m)?;
    art::register(m)?;
    plugins::register(m)?;
    cli::register(m)?;
    Ok(())
}

/// `rich.markup.escape`: backslash-escape `[` so text is not read as markup.
#[pyfunction]
fn escape(markup: &str) -> String {
    rich::markup::escape(markup)
}
