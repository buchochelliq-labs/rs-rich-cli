//! `rich.theme`: a named collection of styles for `Console(theme=...)`,
//! `push_theme` and `use_theme`.
//!
//! Owner: the text/style area. The foundation ships the minimal
//! `Theme(styles, inherit=True)`; `Theme.read`, `from_file` and friends
//! belong here too.

use pyo3::exceptions::PyTypeError;
use pyo3::prelude::*;
use pyo3::types::PyDict;

use rich::theme::Theme as CoreTheme;
use rich::StyleType;

use crate::errors::StyleSyntaxError;
use crate::style::Style;

/// `rich.theme.Theme`: style names mapped to styles.
#[pyclass(name = "Theme", module = "rs_rich.theme", frozen, skip_from_py_object)]
#[derive(Clone)]
pub(crate) struct Theme {
    pub(crate) inner: CoreTheme,
}

#[pymethods]
impl Theme {
    /// `styles` maps names to a style definition or a `Style`. With
    /// `inherit` (the default) Rich's default styles come first.
    #[new]
    #[pyo3(signature = (styles=None, inherit=true))]
    fn new(styles: Option<&Bound<'_, PyDict>>, inherit: bool) -> PyResult<Self> {
        let mut pairs: Vec<(String, StyleType)> = Vec::new();
        if let Some(styles) = styles {
            for (name, value) in styles.iter() {
                let name: String = name.extract()?;
                let style = if let Ok(style) = value.extract::<PyRef<'_, Style>>() {
                    StyleType::Style(style.inner.clone())
                } else if let Ok(definition) = value.extract::<String>() {
                    StyleType::Name(definition)
                } else {
                    return Err(PyTypeError::new_err(
                        "Theme styles must be str or Style values",
                    ));
                };
                pairs.push((name, style));
            }
        }
        let inner = CoreTheme::from_styles(pairs, inherit)
            .map_err(|e| StyleSyntaxError::new_err(e.to_string()))?;
        Ok(Theme { inner })
    }

    /// The styles, by name.
    #[getter]
    fn styles<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let dict = PyDict::new(py);
        let mut names: Vec<&str> = self.inner.names().collect();
        names.sort_unstable();
        for name in names {
            if let Some(style) = self.inner.get(name) {
                dict.set_item(name, Style::from_core(style.clone()))?;
            }
        }
        Ok(dict)
    }

    /// The theme as a config file (`[styles]` section), as `Theme.config`.
    #[getter]
    fn config(&self) -> String {
        self.inner.config()
    }
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Theme>()
}
