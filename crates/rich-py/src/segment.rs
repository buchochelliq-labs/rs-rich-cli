//! `rich.segment.Segment`: a piece of text with one style, the unit
//! `__rich_console__` may yield and `Console.render` returns.
//!
//! Owner: the foundation ships the class and its conversions to and from
//! core segments; the static-renderables area adds Rich's `Segment` class
//! methods (`apply_style`, `split_lines`, `simplify`, ...) here.

use pyo3::prelude::*;
use pyo3::types::PyTuple;
use pyo3::{PyTraverseError, PyVisit};

use rich::segment::Segment as CoreSegment;
use rich::Style as CoreStyle;

use crate::style::{resolved_style, Style};

/// `rich.segment.Segment(text, style=None, control=None)`.
///
/// `control` is kept as given; core only knows whether a segment is a
/// control segment, so one that came from core reports `True`.
#[pyclass(
    name = "Segment",
    module = "rs_rich.segment",
    frozen,
    skip_from_py_object
)]
pub(crate) struct Segment {
    pub(crate) text: String,
    pub(crate) style: Option<CoreStyle>,
    control: Option<Py<PyAny>>,
    is_control: bool,
}

impl Segment {
    /// A Python segment for a core one.
    pub(crate) fn from_core(py: Python<'_>, segment: &CoreSegment) -> Segment {
        Segment {
            text: segment.text.clone(),
            style: segment.style.clone(),
            control: segment.control.then(|| {
                pyo3::types::PyBool::new(py, true)
                    .to_owned()
                    .into_any()
                    .unbind()
            }),
            is_control: segment.control,
        }
    }

    /// This segment as a core one, text unchanged.
    pub(crate) fn to_core(&self) -> CoreSegment {
        CoreSegment {
            text: self.text.clone(),
            style: self.style.clone(),
            control: self.is_control,
        }
    }

    fn style_object(&self) -> Option<Style> {
        self.style.clone().map(Style::from_core)
    }
}

#[pymethods]
impl Segment {
    #[new]
    #[pyo3(signature = (text="", style=None, control=None))]
    fn new(
        text: &str,
        style: Option<&Bound<'_, PyAny>>,
        control: Option<Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        let control = control.filter(|value| !value.is_none());
        let is_control = match &control {
            Some(value) => value.is_truthy()?,
            None => false,
        };
        Ok(Segment {
            text: text.to_string(),
            style: resolved_style(style)?,
            control: control.map(Bound::unbind),
            is_control,
        })
    }

    /// `Segment.line()`: a newline.
    #[classmethod]
    fn line(_cls: &Bound<'_, pyo3::types::PyType>) -> Segment {
        Segment {
            text: "\n".into(),
            style: None,
            control: None,
            is_control: false,
        }
    }

    #[getter]
    fn text(&self) -> &str {
        &self.text
    }

    #[getter]
    fn style(&self) -> Option<Style> {
        self.style_object()
    }

    #[getter]
    fn control(&self, py: Python<'_>) -> Option<Py<PyAny>> {
        self.control.as_ref().map(|value| value.clone_ref(py))
    }

    /// The cells the text occupies (0 for a control segment).
    #[getter]
    fn cell_length(&self) -> usize {
        if self.is_control {
            0
        } else {
            rich::cells::cell_len(&self.text)
        }
    }

    #[getter]
    fn is_control(&self) -> bool {
        self.is_control
    }

    fn __bool__(&self) -> bool {
        !self.text.is_empty()
    }

    fn __len__(&self) -> usize {
        3
    }

    fn __getitem__(&self, py: Python<'_>, index: isize) -> PyResult<Py<PyAny>> {
        self.as_tuple(py)?
            .get_item(if index < 0 { index + 3 } else { index } as usize)
            .map(Bound::unbind)
    }

    fn __iter__<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        Ok(self.as_tuple(py)?.try_iter()?.into_any())
    }

    fn __eq__(&self, py: Python<'_>, other: &Bound<'_, PyAny>) -> PyResult<bool> {
        match other.extract::<PyRef<'_, Segment>>() {
            Ok(other) => Ok(self.text == other.text
                && self.style == other.style
                && self.is_control == other.is_control),
            Err(_) => self.as_tuple(py)?.eq(other),
        }
    }

    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        let style = match self.style_object() {
            Some(style) => format!("Style.parse({:?})", style.definition),
            None => "None".into(),
        };
        let control = match &self.control {
            Some(control) => control.bind(py).repr()?.to_string(),
            None => "None".into(),
        };
        Ok(format!(
            "Segment(text={}, style={style}, control={control})",
            pyo3::types::PyString::new(py, &self.text).repr()?
        ))
    }

    fn __traverse__(&self, visit: PyVisit<'_>) -> Result<(), PyTraverseError> {
        if let Some(control) = &self.control {
            visit.call(control)?;
        }
        Ok(())
    }
}

impl Segment {
    fn as_tuple<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
        PyTuple::new(
            py,
            [
                self.text.clone().into_pyobject(py)?.into_any(),
                self.style_object().into_pyobject(py)?.into_any(),
                match &self.control {
                    Some(control) => control.bind(py).clone(),
                    None => py.None().into_bound(py),
                },
            ],
        )
    }
}

/// Split newlines inside segments out into `"\n"` segments of their own,
/// as core's line handling expects (upstream's `Segment.split_lines` and
/// `split_and_crop_lines` do the same when they print).
pub(crate) fn split_newlines(segments: Vec<CoreSegment>) -> Vec<CoreSegment> {
    if !segments
        .iter()
        .any(|s| !s.control && s.text != "\n" && s.text.contains('\n'))
    {
        return segments;
    }
    let mut out = Vec::with_capacity(segments.len() + 4);
    for segment in segments {
        if segment.control || segment.text == "\n" || !segment.text.contains('\n') {
            out.push(segment);
            continue;
        }
        let mut parts = segment.text.split('\n').peekable();
        while let Some(part) = parts.next() {
            if !part.is_empty() {
                out.push(CoreSegment::new(part, segment.style.clone()));
            }
            if parts.peek().is_some() {
                out.push(CoreSegment::line());
            }
        }
    }
    out
}

/// Core segments as a Python list of `Segment`s.
pub(crate) fn to_python<'py>(
    py: Python<'py>,
    segments: &[CoreSegment],
) -> PyResult<Bound<'py, pyo3::types::PyList>> {
    pyo3::types::PyList::new(
        py,
        segments
            .iter()
            .map(|segment| Segment::from_core(py, segment))
            .collect::<Vec<_>>(),
    )
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Segment>()
}
