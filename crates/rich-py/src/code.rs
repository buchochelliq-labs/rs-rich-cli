//! Code and data: `Markdown`, `Syntax` (with the code-highlighter choice),
//! `JSON`, `Pretty`, `pretty.install`, `inspect`, `Traceback`,
//! `Console.print_exception`, and the highlighters (`rs_rich.markdown`,
//! `rs_rich.syntax`, `rs_rich.json`, `rs_rich.pretty`, `rs_rich.traceback`,
//! `rs_rich.highlighter`).
//!
//! Owner: the code area. `console.rs` calls the two hooks below.
//!
//! | Submodule | Rich module |
//! |---|---|
//! | `highlighter` | `rich.highlighter` |
//! | `pretty` | `rich.pretty` |
//! | `json` | `rich.json` |
//! | `markdown` | `rich.markdown` |
//! | `syntax` | `rich.syntax` (and the code-highlighter choice) |
//! | `inspect` | `rich._inspect` and `rich.inspect` |
//! | `traceback` | `rich.traceback` and `Console.print_exception` |
//! | `layout` | small renderables the others compose with |

use pyo3::prelude::*;
use pyo3::types::{PyDict, PyTuple};

use rich::protocol::Renderable;

use crate::console::Console;

mod highlighter;
mod inspect;
mod json;
mod layout;
mod markdown;
mod pretty;
mod syntax;
mod traceback;

pub(crate) use syntax::code_highlighter_value;

/// What `Console.print` renders for a container, dataclass or other
/// object Rich pretty-prints (`rich.pretty.Pretty(obj, highlighter=...)`).
/// `highlight` is the print's effective highlight setting.
pub(crate) fn pretty_for_print(
    value: &Bound<'_, PyAny>,
    highlight: bool,
) -> PyResult<Box<dyn Renderable>> {
    pretty::for_print(value, highlight)
}

/// `Console.print_exception(...)`, with Rich's arguments.
pub(crate) fn console_print_exception(
    console: &Bound<'_, Console>,
    args: &Bound<'_, PyTuple>,
    kwargs: Option<&Bound<'_, PyDict>>,
) -> PyResult<Py<PyAny>> {
    traceback::print_exception(console, args, kwargs)
}

/// For `Console(highlighter=...)`: highlight printed text with a Python
/// highlighter object, calling no Python code for the built-in ones.
pub(crate) fn highlight_with(
    highlighter: &Bound<'_, PyAny>,
    text: rich::Text,
) -> PyResult<rich::Text> {
    highlighter::Highlight::from_arg(Some(highlighter))?.apply(highlighter.py(), text)
}

/// For `Console.log(log_locals=True)`: upstream's
/// `render_scope(locals, title="[i]locals")`.
pub(crate) fn render_scope(
    scope: &Bound<'_, PyDict>,
    title: Option<String>,
) -> PyResult<std::sync::Arc<dyn Renderable + Send + Sync>> {
    traceback::render_scope(scope, title, false, pretty::Limits::default(), None)
}

/// For `Console.print_json`: `JSON.from_data(data, indent=..., highlight=...,
/// ...)`'s text.
#[allow(clippy::too_many_arguments)]
pub(crate) fn json_text(
    data: &Bound<'_, PyAny>,
    indent: &Bound<'_, PyAny>,
    highlight: bool,
    skip_keys: bool,
    ensure_ascii: bool,
    check_circular: bool,
    allow_nan: bool,
    default: Option<&Bound<'_, PyAny>>,
    sort_keys: bool,
) -> PyResult<rich::Text> {
    json::encode(
        data,
        indent,
        highlight,
        skip_keys,
        ensure_ascii,
        check_circular,
        allow_nan,
        default,
        sort_keys,
    )
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    highlighter::register(m)?;
    pretty::register(m)?;
    json::register(m)?;
    markdown::register(m)?;
    syntax::register(m)?;
    inspect::register(m)?;
    traceback::register(m)?;
    Ok(())
}
