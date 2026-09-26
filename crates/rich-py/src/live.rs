//! Live and interactive: `Live`, `Progress` (columns, `track`, `wrap_file`,
//! `open`), `Status`, `Screen`, `Pager`, prompts and the logging handler
//! (`rs_rich.live`, `rs_rich.progress`, `rs_rich.status`, `rs_rich.screen`,
//! `rs_rich.pager`, `rs_rich.prompt`, `rs_rich.logging`), and the Console
//! methods `status`, `pager` and `screen`.
//!
//! Owner: the live area. `console.rs` calls the three hooks below.
//!
//! # Threads
//!
//! A `Live` (and so a `Progress` or `Status`) with `auto_refresh` redraws
//! from a `threading.Thread` (a daemon, as upstream's): its target is a Rust
//! closure that holds the GIL while it runs and waits on a `threading.Event`
//! (which releases it). Each redraw is a `Console.print`, which renders in
//! its own `renderable::scope` on that thread. The display's lock is a
//! Python `RLock`, so waiting for it never blocks another thread's Python
//! code. Threads still running at interpreter exit are stopped by an
//! `atexit` hook.
//!
//! # Modules
//!
//! | Module | Rich |
//! |---|---|
//! | `live_display` | `rich.live.Live`, `rich.file_proxy.FileProxy` |
//! | `live_render` | `rich.live_render.LiveRender` |
//! | `progress`, `progress_bar` | `rich.progress`, `rich.progress_bar` |
//! | `status` | `rich.status.Status`, `Console.status` |
//! | `screen` | `rich.screen.Screen`, `rich.pager`, `Console.screen`, `Console.pager` |
//! | `prompt` | `rich.prompt` |
//! | `logging` | `rich.logging.RichHandler`'s rendering, `rich._log_render` |
//! | `glue.py` | the few classes that must be Python (`RichHandler`, ...) |

use std::ffi::CString;

use pyo3::prelude::*;
use pyo3::sync::PyOnceLock;
use pyo3::types::{PyDict, PyTuple};

use crate::console::Console;

mod live_display;
mod live_render;
mod logging;
mod progress;
mod progress_bar;
mod prompt;
mod screen;
mod status;
mod util;

static GLUE: PyOnceLock<Py<PyModule>> = PyOnceLock::new();

/// The live area's Python glue module (see `live/glue.py`).
pub(crate) fn glue_module(py: Python<'_>) -> PyResult<Bound<'_, PyAny>> {
    GLUE.get_or_try_init(py, || {
        let code = CString::new(include_str!("live/glue.py"))
            .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?;
        let module =
            PyModule::from_code(py, &code, c"rs_rich/_live_glue.py", c"rs_rich._live_glue")?;
        Ok::<_, PyErr>(module.unbind())
    })
    .map(|module| module.bind(py).clone().into_any())
}

/// `Console.status(status, *, spinner="dots", ...)`.
pub(crate) fn console_status(
    console: &Bound<'_, Console>,
    args: &Bound<'_, PyTuple>,
    kwargs: Option<&Bound<'_, PyDict>>,
) -> PyResult<Py<PyAny>> {
    status::console_status(console, args, kwargs)
}

/// `Console.pager(pager=None, styles=False, links=False)`.
pub(crate) fn console_pager(
    console: &Bound<'_, Console>,
    args: &Bound<'_, PyTuple>,
    kwargs: Option<&Bound<'_, PyDict>>,
) -> PyResult<Py<PyAny>> {
    screen::console_pager(console, args, kwargs)
}

/// `Console.screen(hide_cursor=True, style=None)`.
pub(crate) fn console_screen(
    console: &Bound<'_, Console>,
    args: &Bound<'_, PyTuple>,
    kwargs: Option<&Bound<'_, PyDict>>,
) -> PyResult<Py<PyAny>> {
    screen::console_screen(console, args, kwargs)
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    util::register(m)?;
    live_render::register(m)?;
    live_display::register(m)?;
    progress_bar::register(m)?;
    progress::register(m)?;
    status::register(m)?;
    screen::register(m)?;
    prompt::register(m)?;
    logging::register(m)?;
    let glue = glue_module(m.py())?;
    for name in ["RichHandler", "PromptError", "InvalidResponse"] {
        m.add(name, glue.getattr(name)?)?;
    }
    Ok(())
}
