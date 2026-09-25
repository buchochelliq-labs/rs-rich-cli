//! `rich.theme`: `Theme`, a named collection of styles for
//! `Console(theme=...)`, `push_theme` and `use_theme`, and `ThemeStack`.
//!
//! Owner: the text/style area.

use pyo3::exceptions::PyTypeError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyType};

use rich::theme::{Theme as CoreTheme, DEFAULT_STYLES};
use rich::Style as CoreStyle;

use crate::errors::ThemeStackError;
use crate::style::{parse_style, Style};

/// `rich.theme.Theme`: style names mapped to styles.
#[pyclass(name = "Theme", module = "rs_rich.theme", frozen, skip_from_py_object)]
#[derive(Clone)]
pub(crate) struct Theme {
    pub(crate) inner: CoreTheme,
    /// The names in Rich's dict order: the default styles first (with
    /// `inherit`), then new names in the order they were given.
    order: Vec<String>,
}

impl Theme {
    /// A theme from `(name, style)` pairs, as `Theme(styles, inherit)`.
    fn build(pairs: Vec<(String, CoreStyle)>, inherit: bool) -> Theme {
        let (mut inner, mut order) = if inherit {
            (
                CoreTheme::default_theme(),
                DEFAULT_STYLES
                    .iter()
                    .map(|(name, _)| name.to_string())
                    .collect::<Vec<_>>(),
            )
        } else {
            (CoreTheme::new(), Vec::new())
        };
        for (name, style) in pairs {
            if inner.get(&name).is_none() && !order.contains(&name) {
                order.push(name.clone());
            }
            inner.insert(name, style);
        }
        order.retain(|name| inner.get(name).is_some());
        Theme { inner, order }
    }

    fn styles_dict<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let dict = PyDict::new(py);
        for name in &self.order {
            if let Some(style) = self.inner.get(name) {
                dict.set_item(name, Style::from_core(style.clone()))?;
            }
        }
        Ok(dict)
    }
}

/// `(name, style)` pairs from a mapping of names to definitions or `Style`s.
fn pairs(styles: &Bound<'_, PyAny>) -> PyResult<Vec<(String, CoreStyle)>> {
    let mut pairs = Vec::new();
    for item in styles.call_method0("items")?.try_iter()? {
        let (name, value): (String, Bound<'_, PyAny>) = item?.extract()?;
        let style = if let Ok(style) = value.cast::<Style>() {
            style.get().inner.clone()
        } else if let Ok(definition) = value.extract::<String>() {
            parse_style(&definition)?
        } else {
            return Err(PyTypeError::new_err(
                "Theme styles must be str or Style values",
            ));
        };
        pairs.push((name, style));
    }
    Ok(pairs)
}

#[pymethods]
impl Theme {
    /// `styles` maps names to a style definition or a `Style`. With
    /// `inherit` (the default) Rich's default styles come first.
    #[new]
    #[pyo3(signature = (styles=None, inherit=true))]
    fn new(styles: Option<&Bound<'_, PyAny>>, inherit: bool) -> PyResult<Self> {
        let pairs = match styles.filter(|s| !s.is_none()) {
            Some(styles) => pairs(styles)?,
            None => Vec::new(),
        };
        Ok(Theme::build(pairs, inherit))
    }

    /// The styles, by name, in Rich's order (a new dict each time).
    #[getter]
    fn styles<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        self.styles_dict(py)
    }

    /// The theme as a config file (`[styles]` section), as `Theme.config`.
    #[getter]
    fn config(&self) -> String {
        self.inner.config()
    }

    /// Load a theme from an open config file. The file is read with Python's
    /// `configparser`, as Rich does, so its errors are the same.
    #[classmethod]
    #[pyo3(signature = (config_file, source=None, inherit=true))]
    fn from_file(
        _cls: &Bound<'_, PyType>,
        config_file: &Bound<'_, PyAny>,
        source: Option<&str>,
        inherit: bool,
    ) -> PyResult<Theme> {
        let py = config_file.py();
        let config = py.import("configparser")?.call_method0("ConfigParser")?;
        let kwargs = PyDict::new(py);
        kwargs.set_item("source", source)?;
        config.call_method("read_file", (config_file,), Some(&kwargs))?;
        let mut pairs = Vec::new();
        for item in config.call_method1("items", ("styles",))?.try_iter()? {
            let (name, value): (String, String) = item?.extract()?;
            pairs.push((name, parse_style(&value)?));
        }
        Ok(Theme::build(pairs, inherit))
    }

    /// Read a theme from a config file at `path`.
    #[classmethod]
    #[pyo3(signature = (path, inherit=true, encoding=None))]
    fn read(
        cls: &Bound<'_, PyType>,
        path: &Bound<'_, PyAny>,
        inherit: bool,
        encoding: Option<&str>,
    ) -> PyResult<Theme> {
        let py = cls.py();
        let kwargs = PyDict::new(py);
        kwargs.set_item("encoding", encoding)?;
        let file = py
            .import("builtins")?
            .getattr("open")?
            .call((path,), Some(&kwargs))?;
        let source: Option<String> = path.str().ok().map(|s| s.to_string());
        let result = Theme::from_file(cls, &file, source.as_deref(), inherit);
        file.call_method0("close")?;
        result
    }
}

/// `rich.theme.ThemeStack`: a stack of themes, each (with `inherit`)
/// layered over the one below.
#[pyclass(name = "ThemeStack", module = "rs_rich.theme")]
pub(crate) struct ThemeStack {
    entries: Vec<CoreTheme>,
}

#[pymethods]
impl ThemeStack {
    #[new]
    fn new(theme: PyRef<'_, Theme>) -> Self {
        ThemeStack {
            entries: vec![theme.inner.clone()],
        }
    }

    /// The style called `name` in the top theme, or `default`.
    #[pyo3(signature = (name, default=None))]
    fn get<'py>(
        &self,
        py: Python<'py>,
        name: &str,
        default: Option<Bound<'py, PyAny>>,
    ) -> PyResult<Option<Bound<'py, PyAny>>> {
        match self.entries.last().and_then(|theme| theme.get(name)) {
            Some(style) => Ok(Some(
                Bound::new(py, Style::from_core(style.clone()))?.into_any(),
            )),
            None => Ok(default),
        }
    }

    /// Push `theme`; with `inherit` it adds to the current styles.
    #[pyo3(signature = (theme, inherit=true))]
    fn push_theme(&mut self, theme: PyRef<'_, Theme>, inherit: bool) {
        let styles = match (inherit, self.entries.last()) {
            (true, Some(top)) => {
                let mut merged = top.clone();
                merged.extend_from(&theme.inner);
                merged
            }
            _ => theme.inner.clone(),
        };
        self.entries.push(styles);
    }

    /// Pop the top theme; the base theme cannot be popped.
    fn pop_theme(&mut self) -> PyResult<()> {
        if self.entries.len() == 1 {
            return Err(ThemeStackError::new_err("Unable to pop base theme"));
        }
        self.entries.pop();
        Ok(())
    }
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Theme>()?;
    m.add_class::<ThemeStack>()
}
