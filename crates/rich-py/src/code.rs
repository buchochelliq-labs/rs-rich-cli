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
//! | `layout` | small renderables the others compose with |

use pyo3::exceptions::PyNotImplementedError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyTuple};

use rich::protocol::Renderable;

use crate::console::Console;

mod highlighter;
mod json;
mod layout;
mod markdown;
mod pretty;
mod syntax;

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
    _console: &Bound<'_, Console>,
    _args: &Bound<'_, PyTuple>,
    _kwargs: Option<&Bound<'_, PyDict>>,
) -> PyResult<Py<PyAny>> {
    Err(PyNotImplementedError::new_err(
        "rs_rich cannot print exceptions yet: Traceback is not implemented",
    ))
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    highlighter::register(m)?;
    pretty::register(m)?;
    json::register(m)?;
    markdown::register(m)?;
    syntax::register(m)?;
    Ok(())
}
