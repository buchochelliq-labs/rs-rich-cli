//! Code and data: `Markdown`, `Syntax` (with the code-highlighter choice),
//! `JSON`, `Pretty`, `pretty.install`, `inspect`, `Traceback`,
//! `Console.print_exception`, and the highlighters (`rs_rich.markdown`,
//! `rs_rich.syntax`, `rs_rich.json`, `rs_rich.pretty`, `rs_rich.traceback`,
//! `rs_rich.highlighter`).
//!
//! Owner: the code area. Placeholder: the two hooks below are what
//! `console.rs` calls; replace their bodies, keep their signatures. Add
//! classes in [`register`] (submodules under `code/` are fine).

use pyo3::exceptions::PyNotImplementedError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyTuple};

use rich::protocol::Renderable;

use crate::console::Console;

/// What `Console.print` renders for a container, dataclass or other
/// object Rich pretty-prints (`rich.pretty.Pretty(obj, highlighter=...)`).
/// `highlight` is the print's effective highlight setting.
pub(crate) fn pretty_for_print(
    value: &Bound<'_, PyAny>,
    _highlight: bool,
) -> PyResult<Box<dyn Renderable>> {
    Err(PyNotImplementedError::new_err(format!(
        "rs_rich cannot render {} yet: pretty printing (rs_rich.pretty) is not implemented",
        value.get_type().name()?
    )))
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

pub(crate) fn register(_m: &Bound<'_, PyModule>) -> PyResult<()> {
    Ok(())
}
