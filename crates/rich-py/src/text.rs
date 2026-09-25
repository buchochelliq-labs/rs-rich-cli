//! `rich.text`: the `Text` class.
//!
//! Owner: the text/style area (with `style.rs`, `theme.rs` and `color.rs`).

use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;

use rich::protocol::Renderable;
use rich::Text as CoreText;

use crate::convert::{self, Index};
use crate::errors::MarkupError;
use crate::renderable::{self, AsRenderable};
use crate::style::style_type;

/// `rich.text.Text`: a string with styled spans.
#[pyclass(name = "Text", module = "rs_rich.text", skip_from_py_object)]
#[derive(Clone)]
pub(crate) struct Text {
    pub(crate) inner: CoreText,
}

impl Text {
    /// The byte offset of character `index` (negative counts from the end,
    /// as in Python), clamped to the text.
    fn byte_offset(&self, index: isize) -> usize {
        let plain = self.inner.plain();
        let chars = plain.chars().count() as isize;
        let index = if index < 0 { chars + index } else { index }.clamp(0, chars) as usize;
        plain
            .char_indices()
            .nth(index)
            .map_or(plain.len(), |(offset, _)| offset)
    }
}

impl AsRenderable for Text {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(self.inner.clone()))
    }
}

#[pymethods]
impl Text {
    #[new]
    #[pyo3(signature = (text="", style=None, *, justify=None, overflow=None, no_wrap=None))]
    fn new(
        text: &str,
        style: Option<&Bound<'_, PyAny>>,
        justify: Option<&str>,
        overflow: Option<&str>,
        no_wrap: Option<bool>,
    ) -> PyResult<Self> {
        let mut inner = match style_type(style)? {
            Some(style) => CoreText::styled(text, style),
            None => CoreText::new(text),
        };
        inner.set_justify(convert::justify(justify)?);
        if let Some(value) = overflow {
            inner.set_overflow(Some(convert::overflow(value)?));
        }
        inner.set_no_wrap(no_wrap);
        Ok(Text { inner })
    }

    /// `Text.from_markup("[bold]hi[/]")`.
    #[classmethod]
    #[pyo3(signature = (text, *, style=None, justify=None))]
    fn from_markup(
        _cls: &Bound<'_, pyo3::types::PyType>,
        text: &str,
        style: Option<&Bound<'_, PyAny>>,
        justify: Option<&str>,
    ) -> PyResult<Self> {
        let mut inner =
            CoreText::from_markup(text).map_err(|e| MarkupError::new_err(e.to_string()))?;
        if let Some(style) = style_type(style)? {
            inner.set_base_style(style);
        }
        inner.set_justify(convert::justify(justify)?);
        Ok(Text { inner })
    }

    #[getter]
    fn plain(&self) -> String {
        self.inner.plain().to_string()
    }

    /// Append a string (with an optional style) or another `Text`.
    #[pyo3(signature = (text, style=None))]
    fn append<'py>(
        mut slf: PyRefMut<'py, Self>,
        text: &Bound<'py, PyAny>,
        style: Option<&Bound<'py, PyAny>>,
    ) -> PyResult<PyRefMut<'py, Self>> {
        // `t.append(t)`: `t` is already borrowed mutably here, so it cannot
        // be extracted again; append a copy of itself, as upstream does.
        if text.as_ptr() == slf.as_ptr() {
            if style_type(style)?.is_some() {
                return Err(PyValueError::new_err(
                    "style must not be set when appending a Text instance",
                ));
            }
            let copy = slf.inner.clone();
            let joined = slf.inner.clone().append_text(&copy);
            slf.inner = joined;
        } else if let Ok(other) = text.extract::<PyRef<'_, Text>>() {
            if style_type(style)?.is_some() {
                return Err(PyValueError::new_err(
                    "style must not be set when appending a Text instance",
                ));
            }
            let joined = slf.inner.clone().append_text(&other.inner);
            slf.inner = joined;
        } else if let Ok(string) = text.extract::<String>() {
            let style = style_type(style)?;
            slf.inner.append(&string, style);
        } else {
            return Err(PyTypeError::new_err(
                "Only str or Text can be appended to Text",
            ));
        }
        Ok(slf)
    }

    /// Style characters `start..end` (Python indices; negative from the end).
    /// Offsets are any Python `int`; ones past the text clamp to it.
    #[pyo3(signature = (style, start=Index(0), end=None))]
    fn stylize(
        &mut self,
        style: &Bound<'_, PyAny>,
        start: Index,
        end: Option<Index>,
    ) -> PyResult<()> {
        let (start, end) = (start.0, end.map(|end| end.0));
        let Some(style) = style_type(Some(style))? else {
            return Ok(());
        };
        let start = self.byte_offset(start);
        let end = end.map_or(self.inner.plain().len(), |end| self.byte_offset(end));
        self.inner.stylize(style, start, end);
        Ok(())
    }

    fn __len__(&self) -> usize {
        self.inner.plain().chars().count()
    }

    fn __str__(&self) -> String {
        self.plain()
    }

    fn __repr__(&self) -> String {
        format!("<text {:?}>", self.inner.plain())
    }
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    renderable::add_renderable_class::<Text>(m)
}
