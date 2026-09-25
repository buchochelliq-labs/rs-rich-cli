//! `rich.rule`: `Rule`. Port of upstream `rich/rule.py`.
//!
//! The Python class keeps what it was given; core's `Rule` renders it
//! (markup or `Text` titles, `end`, the ASCII fallback).

use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyString;
use pyo3::{PyTraverseError, PyVisit};

use rich::align::HorizontalAlign;
use rich::cells::cell_len;
use rich::protocol::Renderable;
use rich::style::StyleType;
use rich::{Rule as CoreRule, Text as CoreText};

use crate::renderable::{self, AsRenderable};
use crate::style::style_type;
use crate::text::Text;

fn check_align(align: &str) -> PyResult<()> {
    if !matches!(align, "left" | "center" | "right") {
        return Err(PyValueError::new_err(format!(
            "invalid value for align, expected \"left\", \"center\", \"right\" (not '{align}')"
        )));
    }
    Ok(())
}

fn check_characters(characters: &str) -> PyResult<()> {
    if cell_len(characters) < 1 {
        return Err(PyValueError::new_err(
            "'characters' argument must have a cell width of at least 1",
        ));
    }
    Ok(())
}

/// `rich.rule.Rule`: a horizontal line, optionally with a title.
#[pyclass(name = "Rule", module = "rs_rich.rule")]
pub(crate) struct Rule {
    title: Py<PyAny>,
    characters: String,
    style: Py<PyAny>,
    #[pyo3(get, set)]
    end: String,
    align: String,
}

impl AsRenderable for Rule {
    fn to_renderable(&self, py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        let title = self.title.bind(py);
        let rule = if let Ok(text) = title.extract::<PyRef<'_, Text>>() {
            CoreRule::with_title_text(text.inner.clone())
        } else if let Ok(markup) = title.cast::<PyString>() {
            let markup = markup.to_cow()?.into_owned();
            CoreText::from_markup(&markup).map_err(crate::color::markup::markup_error)?;
            if markup.is_empty() {
                CoreRule::line()
            } else {
                CoreRule::new(markup)
            }
        } else if title.is_none() || !title.is_truthy()? {
            CoreRule::line()
        } else {
            return Err(PyTypeError::new_err("a Rule title must be a str or a Text"));
        };
        let style = style_type(Some(self.style.bind(py)))?
            .unwrap_or_else(|| StyleType::Style(Default::default()));
        let align = match self.align.as_str() {
            "left" => HorizontalAlign::Left,
            "right" => HorizontalAlign::Right,
            _ => HorizontalAlign::Center,
        };
        Ok(Box::new(
            rule.characters(self.characters.clone())
                .style(style)
                .end(self.end.clone())
                .align(align),
        ))
    }
}

#[pymethods]
impl Rule {
    #[new]
    #[pyo3(signature = (title=None, *, characters="─", style=None, end="\n", align="center"))]
    fn new(
        py: Python<'_>,
        title: Option<Py<PyAny>>,
        characters: &str,
        style: Option<Py<PyAny>>,
        end: &str,
        align: &str,
    ) -> PyResult<Self> {
        check_characters(characters)?;
        check_align(align)?;
        let style = style.unwrap_or_else(|| PyString::new(py, "rule.line").into_any().unbind());
        style_type(Some(style.bind(py)))?;
        Ok(Rule {
            title: title.unwrap_or_else(|| PyString::new(py, "").into_any().unbind()),
            characters: characters.to_string(),
            style,
            end: end.to_string(),
            align: align.to_string(),
        })
    }

    #[getter]
    fn title(&self, py: Python<'_>) -> Py<PyAny> {
        self.title.clone_ref(py)
    }

    #[setter]
    fn set_title(&mut self, title: Py<PyAny>) {
        self.title = title;
    }

    #[getter]
    fn characters(&self) -> String {
        self.characters.clone()
    }

    #[setter]
    fn set_characters(&mut self, characters: &str) -> PyResult<()> {
        check_characters(characters)?;
        self.characters = characters.to_string();
        Ok(())
    }

    #[getter]
    fn style(&self, py: Python<'_>) -> Py<PyAny> {
        self.style.clone_ref(py)
    }

    #[setter]
    fn set_style(&mut self, style: Bound<'_, PyAny>) -> PyResult<()> {
        style_type(Some(&style))?;
        self.style = style.unbind();
        Ok(())
    }

    #[getter]
    fn align(&self) -> String {
        self.align.clone()
    }

    #[setter]
    fn set_align(&mut self, align: &str) -> PyResult<()> {
        check_align(align)?;
        self.align = align.to_string();
        Ok(())
    }

    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        Ok(format!(
            "Rule({}, {})",
            self.title.bind(py).repr()?,
            PyString::new(py, &self.characters).repr()?
        ))
    }

    fn __traverse__(&self, visit: PyVisit<'_>) -> Result<(), PyTraverseError> {
        visit.call(&self.title)?;
        visit.call(&self.style)
    }
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    renderable::add_renderable_class::<Rule>(m)
}

/// Whether `value` is a titled `Rule` whose `end` does not end its line
/// (Rich then prints nothing after it: `Rule("x", end="")` shares its line
/// with what follows). Rich's rule without a title ignores `end`.
pub(crate) fn ends_inline(value: &Bound<'_, PyAny>) -> bool {
    let Ok(rule) = value.cast::<Rule>() else {
        return false;
    };
    let rule = rule.borrow();
    !rule.end.ends_with('\n') && rule.title.bind(value.py()).is_truthy().unwrap_or(false)
}
