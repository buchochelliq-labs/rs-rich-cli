//! `rich.json`: `JSON`. As upstream: Python's `json` encodes, and
//! `JSONHighlighter` styles the result, which renders as a no-wrap `Text`.

use pyo3::prelude::*;
use pyo3::types::{PyDict, PyType};

use rich::protocol::Renderable;
use rich::Text as CoreText;

use super::highlighter::new_text;
use crate::renderable::{self, AsRenderable};
use crate::text::Text;

/// `rich.json.JSON`: pretty-printed, highlighted JSON.
#[pyclass(name = "JSON", module = "rs_rich.json")]
pub(crate) struct Json {
    text: CoreText,
}

impl AsRenderable for Json {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(self.text.clone()))
    }
}

/// `indent=`: upstream's default is 2; `None` means compact.
pub(crate) enum Indent {
    Two,
    Given(Py<PyAny>),
}

impl<'a, 'py> FromPyObject<'a, 'py> for Indent {
    type Error = PyErr;

    fn extract(value: pyo3::Borrowed<'a, 'py, PyAny>) -> PyResult<Self> {
        Ok(Indent::Given(value.to_owned().unbind()))
    }
}

impl Indent {
    fn object<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        Ok(match self {
            Indent::Two => 2i32.into_pyobject(py)?.into_any(),
            Indent::Given(value) => value.bind(py).clone(),
        })
    }
}

/// `json.dumps` with upstream's arguments, then the highlighter.
#[allow(clippy::too_many_arguments)]
fn encode(
    data: &Bound<'_, PyAny>,
    indent: &Bound<'_, PyAny>,
    highlight: bool,
    skip_keys: bool,
    ensure_ascii: bool,
    check_circular: bool,
    allow_nan: bool,
    default: Option<&Bound<'_, PyAny>>,
    sort_keys: bool,
) -> PyResult<CoreText> {
    let py = data.py();
    let kwargs = PyDict::new(py);
    kwargs.set_item("indent", indent)?;
    kwargs.set_item("skipkeys", skip_keys)?;
    kwargs.set_item("ensure_ascii", ensure_ascii)?;
    kwargs.set_item("check_circular", check_circular)?;
    kwargs.set_item("allow_nan", allow_nan)?;
    kwargs.set_item("default", default)?;
    kwargs.set_item("sort_keys", sort_keys)?;
    let json: String = py
        .import("json")?
        .call_method("dumps", (data,), Some(&kwargs))?
        .extract()?;
    let mut text = CoreText::new(json);
    if highlight {
        super::highlighter::json(py, &mut text)?;
    }
    text.set_no_wrap(Some(true));
    text.set_overflow(None);
    Ok(text)
}

#[pymethods]
impl Json {
    #[new]
    #[pyo3(signature = (
        json, indent=Indent::Two, highlight=true,
        skip_keys=false, ensure_ascii=false, check_circular=true, allow_nan=true, default=None,
        sort_keys=false
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        json: &Bound<'_, PyAny>,
        indent: Indent,
        highlight: bool,
        skip_keys: bool,
        ensure_ascii: bool,
        check_circular: bool,
        allow_nan: bool,
        default: Option<&Bound<'_, PyAny>>,
        sort_keys: bool,
    ) -> PyResult<Self> {
        let data_py = json.py();
        let data = data_py.import("json")?.call_method1("loads", (json,))?;
        Ok(Json {
            text: encode(
                &data,
                &indent.object(data_py)?,
                highlight,
                skip_keys,
                ensure_ascii,
                check_circular,
                allow_nan,
                default,
                sort_keys,
            )?,
        })
    }

    /// Encode any Json-able `data`.
    #[classmethod]
    #[pyo3(signature = (
        data, indent=Indent::Two, highlight=true,
        skip_keys=false, ensure_ascii=false, check_circular=true, allow_nan=true, default=None,
        sort_keys=false
    ))]
    #[allow(clippy::too_many_arguments)]
    fn from_data(
        _cls: &Bound<'_, PyType>,
        data: &Bound<'_, PyAny>,
        indent: Indent,
        highlight: bool,
        skip_keys: bool,
        ensure_ascii: bool,
        check_circular: bool,
        allow_nan: bool,
        default: Option<&Bound<'_, PyAny>>,
        sort_keys: bool,
    ) -> PyResult<Self> {
        let data_py = data.py();
        Ok(Json {
            text: encode(
                data,
                &indent.object(data_py)?,
                highlight,
                skip_keys,
                ensure_ascii,
                check_circular,
                allow_nan,
                default,
                sort_keys,
            )?,
        })
    }

    /// The highlighted text (a copy).
    #[getter]
    fn text<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, Text>> {
        new_text(py, self.text.clone())
    }

    fn __rich__<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, Text>> {
        self.text(py)
    }
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    renderable::add_renderable_class::<Json>(m)
}
