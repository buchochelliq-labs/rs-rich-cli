//! Live and interactive: `Live`, `Progress` (columns, `track`, `wrap_file`,
//! `open`), `Status`, `Screen`, `Pager`, prompts and the logging handler
//! (`rs_rich.live`, `rs_rich.progress`, `rs_rich.status`, `rs_rich.screen`,
//! `rs_rich.pager`, `rs_rich.prompt`, `rs_rich.logging`), and the Console
//! methods `status`, `pager` and `screen`.
//!
//! Owner: the live area. Placeholder: the three hooks below are what
//! `console.rs` calls; replace their bodies, keep their signatures. A render
//! on a refresh thread must enter its own `renderable::scope`.

use pyo3::exceptions::PyNotImplementedError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyTuple};

use crate::console::Console;

/// `Console.status(status, *, spinner="dots", ...)`.
pub(crate) fn console_status(
    _console: &Bound<'_, Console>,
    _args: &Bound<'_, PyTuple>,
    _kwargs: Option<&Bound<'_, PyDict>>,
) -> PyResult<Py<PyAny>> {
    Err(PyNotImplementedError::new_err(
        "rs_rich has no Console.status yet: Status is not implemented",
    ))
}

/// `Console.pager(pager=None, styles=False, links=False)`.
pub(crate) fn console_pager(
    _console: &Bound<'_, Console>,
    _args: &Bound<'_, PyTuple>,
    _kwargs: Option<&Bound<'_, PyDict>>,
) -> PyResult<Py<PyAny>> {
    Err(PyNotImplementedError::new_err(
        "rs_rich has no Console.pager yet: Pager is not implemented",
    ))
}

/// `Console.screen(hide_cursor=True, style=None)`.
pub(crate) fn console_screen(
    _console: &Bound<'_, Console>,
    _args: &Bound<'_, PyTuple>,
    _kwargs: Option<&Bound<'_, PyDict>>,
) -> PyResult<Py<PyAny>> {
    Err(PyNotImplementedError::new_err(
        "rs_rich has no Console.screen yet: Screen is not implemented",
    ))
}

pub(crate) fn register(_m: &Bound<'_, PyModule>) -> PyResult<()> {
    Ok(())
}
