//! `rs_rich.ext.theme`: the extended theme (upstream's styles plus
//! `error`/`warning`/`info`/`success` and the styles of every `rich-ext`
//! renderable). The extension registry itself (`ExtensionRegistry`,
//! `install_defaults`) is the plugins area's (`rs_rich.plugins`), and
//! `rs_rich.ext.registry` re-exports it.

use pyo3::prelude::*;

use super::terminal::py_theme;

/// `extended_theme()`: Rich's default theme plus `rich-ext`'s styles, as a
/// `Theme` for `Console(theme=...)` or `push_theme`.
#[pyfunction]
fn extended_theme(py: Python<'_>) -> PyResult<Py<PyAny>> {
    py_theme(py, &rich_ext::extended_theme())
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(pyo3::wrap_pyfunction!(extended_theme, m)?)?;
    m.add("EXTRA_STYLES", rich_ext::EXTRA_STYLES.to_vec())?;
    let tables: Vec<Vec<(&str, &str)>> = rich_ext::theme::STYLE_TABLES
        .iter()
        .map(|table| table.to_vec())
        .collect();
    m.add("STYLE_TABLES", tables)?;
    Ok(())
}
