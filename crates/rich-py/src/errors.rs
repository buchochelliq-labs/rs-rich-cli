//! Rich's exception classes (`rich.errors`, plus `rich.console.CaptureError`
//! and `rich.theme.ThemeStackError`), with upstream's hierarchy.
//!
//! Owner: the foundation. Other areas raise these with `ErrorName::new_err`;
//! an area that needs a new Rich exception adds it here (one line each).

use pyo3::create_exception;
use pyo3::exceptions::PyException;
use pyo3::prelude::*;

create_exception!(_native, ConsoleError, PyException);
create_exception!(_native, StyleError, PyException);
create_exception!(_native, StyleSyntaxError, ConsoleError);
create_exception!(_native, MissingStyle, StyleError);
create_exception!(_native, StyleStackError, ConsoleError);
create_exception!(_native, NotRenderableError, ConsoleError);
create_exception!(_native, MarkupError, ConsoleError);
create_exception!(_native, LiveError, ConsoleError);
create_exception!(_native, NoAltScreen, ConsoleError);
create_exception!(_native, CaptureError, PyException);
create_exception!(_native, ThemeStackError, PyException);

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    let py = m.py();
    m.add("ConsoleError", py.get_type::<ConsoleError>())?;
    m.add("StyleError", py.get_type::<StyleError>())?;
    m.add("StyleSyntaxError", py.get_type::<StyleSyntaxError>())?;
    m.add("MissingStyle", py.get_type::<MissingStyle>())?;
    m.add("StyleStackError", py.get_type::<StyleStackError>())?;
    m.add("NotRenderableError", py.get_type::<NotRenderableError>())?;
    m.add("MarkupError", py.get_type::<MarkupError>())?;
    m.add("LiveError", py.get_type::<LiveError>())?;
    m.add("NoAltScreen", py.get_type::<NoAltScreen>())?;
    m.add("CaptureError", py.get_type::<CaptureError>())?;
    m.add("ThemeStackError", py.get_type::<ThemeStackError>())?;
    Ok(())
}
