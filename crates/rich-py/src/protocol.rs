//! Rich's render protocol types as Python sees them: `ConsoleOptions`
//! (`rich.console.ConsoleOptions`) and `Measurement` (`rich.measure`).
//!
//! Owner: the foundation. `__rich_console__(console, options)` receives a
//! `ConsoleOptions` built from core's options by [`ConsoleOptions::from_core`];
//! `__rich_measure__` returns a `Measurement` (or anything with `minimum` and
//! `maximum`, or a pair).

use pyo3::exceptions::PyTypeError;
use pyo3::prelude::*;
use pyo3::sync::PyOnceLock;
use pyo3::types::{PyDict, PyTuple, PyType};

use rich::console::ConsoleOptions as CoreOptions;
use rich::measure::Measurement as CoreMeasurement;

use crate::convert;

/// What a Python `ConsoleOptions` holds besides core's options: the fields
/// core's `ConsoleOptions` does not have.
#[derive(Clone, Debug)]
pub(crate) struct OptionsBase {
    pub(crate) size: (usize, usize),
    pub(crate) legacy_windows: bool,
    pub(crate) is_terminal: bool,
    pub(crate) encoding: String,
    pub(crate) max_height: usize,
    pub(crate) highlight: Option<bool>,
    pub(crate) markup: Option<bool>,
}

/// `rich.console.ConsoleOptions`: the space and settings a renderable
/// renders into.
#[pyclass(
    name = "ConsoleOptions",
    module = "rs_rich.console",
    skip_from_py_object
)]
#[derive(Clone)]
pub(crate) struct ConsoleOptions {
    size: (usize, usize),
    #[pyo3(get, set)]
    legacy_windows: bool,
    #[pyo3(get, set)]
    min_width: usize,
    #[pyo3(get, set)]
    max_width: usize,
    #[pyo3(get, set)]
    is_terminal: bool,
    #[pyo3(get, set)]
    encoding: String,
    #[pyo3(get, set)]
    max_height: usize,
    justify: Option<String>,
    overflow: Option<String>,
    #[pyo3(get, set)]
    no_wrap: Option<bool>,
    #[pyo3(get, set)]
    highlight: Option<bool>,
    #[pyo3(get, set)]
    markup: Option<bool>,
    #[pyo3(get, set)]
    height: Option<usize>,
}

static CONSOLE_DIMENSIONS: PyOnceLock<Py<PyType>> = PyOnceLock::new();

/// `rich.console.ConsoleDimensions`: a `(width, height)` named tuple.
pub(crate) fn console_dimensions(py: Python<'_>) -> PyResult<&Bound<'_, PyType>> {
    CONSOLE_DIMENSIONS
        .get_or_try_init(py, || {
            let namedtuple = py.import("collections")?.getattr("namedtuple")?;
            let class = namedtuple.call1(("ConsoleDimensions", ("width", "height")))?;
            class.setattr("__module__", "rs_rich.console")?;
            Ok::<_, PyErr>(class.cast_into::<PyType>()?.unbind())
        })
        .map(|class| class.bind(py))
}

pub(crate) fn dimensions(py: Python<'_>, (width, height): (usize, usize)) -> PyResult<Py<PyAny>> {
    Ok(console_dimensions(py)?.call1((width, height))?.unbind())
}

impl ConsoleOptions {
    pub(crate) fn from_core(options: &CoreOptions, base: &OptionsBase) -> ConsoleOptions {
        ConsoleOptions {
            size: base.size,
            legacy_windows: base.legacy_windows,
            min_width: options.min_width,
            max_width: options.max_width,
            is_terminal: base.is_terminal,
            encoding: base.encoding.clone(),
            max_height: options.height.unwrap_or(base.max_height),
            justify: convert::justify_name(options.justify).map(str::to_string),
            overflow: options
                .overflow
                .map(|overflow| convert::overflow_name(overflow).to_string()),
            no_wrap: options.no_wrap,
            highlight: base.highlight,
            markup: base.markup,
            height: options.height,
        }
    }

    pub(crate) fn to_core(&self) -> PyResult<CoreOptions> {
        use crate::limits::{check_size, MAX_CONSOLE_HEIGHT, MAX_CONSOLE_WIDTH};
        for (what, value, limit) in [
            ("min_width", self.min_width, MAX_CONSOLE_WIDTH),
            ("max_width", self.max_width, MAX_CONSOLE_WIDTH),
            ("width", self.size.0, MAX_CONSOLE_WIDTH),
            ("height", self.height.unwrap_or(0), MAX_CONSOLE_HEIGHT),
            ("max_height", self.max_height, MAX_CONSOLE_HEIGHT),
            ("console height", self.size.1, MAX_CONSOLE_HEIGHT),
        ] {
            check_size(what, value, limit)?;
        }
        Ok(CoreOptions {
            min_width: self.min_width,
            max_width: self.max_width,
            height: self.height,
            justify: convert::justify(self.justify.as_deref())?,
            overflow: self
                .overflow
                .as_deref()
                .map(convert::overflow)
                .transpose()?,
            no_wrap: self.no_wrap,
            highlight: self.highlight,
            markup: self.markup,
            max_height: self.max_height,
            encoding: self.encoding.clone(),
            is_terminal: self.is_terminal,
            legacy_windows: self.legacy_windows,
            size: rich::console::ConsoleDimensions {
                width: self.size.0,
                height: self.size.1,
            },
        })
    }

    pub(crate) fn base(&self) -> OptionsBase {
        OptionsBase {
            size: self.size,
            legacy_windows: self.legacy_windows,
            is_terminal: self.is_terminal,
            encoding: self.encoding.clone(),
            max_height: self.max_height,
            highlight: self.highlight,
            markup: self.markup,
        }
    }
}

#[pymethods]
impl ConsoleOptions {
    #[getter]
    fn size(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        dimensions(py, self.size)
    }

    #[setter]
    fn set_size(&mut self, size: (usize, usize)) {
        self.size = size;
    }

    #[getter]
    fn justify(&self) -> Option<String> {
        self.justify.clone()
    }

    #[setter]
    fn set_justify(&mut self, value: Option<String>) -> PyResult<()> {
        convert::justify(value.as_deref())?;
        self.justify = value;
        Ok(())
    }

    #[getter]
    fn overflow(&self) -> Option<String> {
        self.overflow.clone()
    }

    #[setter]
    fn set_overflow(&mut self, value: Option<String>) -> PyResult<()> {
        if let Some(value) = &value {
            convert::overflow(value)?;
        }
        self.overflow = value;
        Ok(())
    }

    /// Whether renderables should use ASCII only (the encoding is not UTF).
    #[getter]
    fn ascii_only(&self) -> bool {
        !self.encoding.starts_with("utf")
    }

    fn copy(&self) -> ConsoleOptions {
        self.clone()
    }

    /// Update values, returning a copy. Arguments not given keep their
    /// value; `width` sets both `min_width` and `max_width`.
    #[pyo3(signature = (**changes))]
    fn update(&self, changes: Option<&Bound<'_, PyDict>>) -> PyResult<ConsoleOptions> {
        let mut options = self.clone();
        let Some(changes) = changes else {
            return Ok(options);
        };
        for (name, value) in changes.iter() {
            let name: String = name.extract()?;
            match name.as_str() {
                "width" => {
                    let width = value.extract::<isize>()?.max(0) as usize;
                    options.min_width = width;
                    options.max_width = width;
                }
                "min_width" => options.min_width = value.extract()?,
                "max_width" => options.max_width = value.extract()?,
                "justify" => options.set_justify(value.extract()?)?,
                "overflow" => options.set_overflow(value.extract()?)?,
                "no_wrap" => options.no_wrap = value.extract()?,
                "highlight" => options.highlight = value.extract()?,
                "markup" => options.markup = value.extract()?,
                "height" => {
                    let height: Option<isize> = value.extract()?;
                    if let Some(height) = height {
                        options.max_height = height.max(0) as usize;
                    }
                    options.height = height.map(|height| height.max(0) as usize);
                }
                other => {
                    return Err(PyTypeError::new_err(format!(
                        "update() got an unexpected keyword argument '{other}'"
                    )))
                }
            }
        }
        Ok(options)
    }

    /// A copy with both widths set to `width`.
    fn update_width(&self, width: isize) -> ConsoleOptions {
        let mut options = self.clone();
        options.min_width = width.max(0) as usize;
        options.max_width = options.min_width;
        options
    }

    /// A copy with `height` and `max_height` set.
    fn update_height(&self, height: usize) -> ConsoleOptions {
        let mut options = self.clone();
        options.max_height = height;
        options.height = Some(height);
        options
    }

    /// A copy with `height` set to `None`.
    fn reset_height(&self) -> ConsoleOptions {
        let mut options = self.clone();
        options.height = None;
        options
    }

    /// A copy with the width and height set.
    fn update_dimensions(&self, width: isize, height: usize) -> ConsoleOptions {
        let mut options = self.update_width(width);
        options.max_height = height;
        options.height = Some(height);
        options
    }

    fn __repr__(&self) -> String {
        format!(
            "ConsoleOptions(size=ConsoleDimensions(width={}, height={}), legacy_windows={}, \
             min_width={}, max_width={}, is_terminal={}, encoding='{}', max_height={}, \
             justify={}, overflow={}, no_wrap={}, highlight={}, markup={}, height={})",
            self.size.0,
            self.size.1,
            py_bool(self.legacy_windows),
            self.min_width,
            self.max_width,
            py_bool(self.is_terminal),
            self.encoding,
            self.max_height,
            py_str(&self.justify),
            py_str(&self.overflow),
            py_opt_bool(self.no_wrap),
            py_opt_bool(self.highlight),
            py_opt_bool(self.markup),
            self.height.map_or("None".into(), |h| h.to_string()),
        )
    }
}

fn py_bool(value: bool) -> &'static str {
    if value {
        "True"
    } else {
        "False"
    }
}

fn py_opt_bool(value: Option<bool>) -> &'static str {
    value.map_or("None", py_bool)
}

fn py_str(value: &Option<String>) -> String {
    value.as_ref().map_or("None".into(), |v| format!("'{v}'"))
}

// ---------------------------------------------------------------------------

/// `rich.measure.Measurement(minimum, maximum)`: the cells a renderable
/// needs. It behaves like upstream's named tuple.
#[pyclass(
    name = "Measurement",
    module = "rs_rich.measure",
    frozen,
    skip_from_py_object
)]
#[derive(Clone, Copy)]
pub(crate) struct Measurement {
    #[pyo3(get)]
    minimum: i64,
    #[pyo3(get)]
    maximum: i64,
}

impl Measurement {
    pub(crate) fn from_core(measurement: CoreMeasurement) -> Measurement {
        Measurement {
            minimum: measurement.minimum as i64,
            maximum: measurement.maximum as i64,
        }
    }

    /// Core's measurement, negative bounds clamped to 0.
    pub(crate) fn to_core(self) -> CoreMeasurement {
        CoreMeasurement::new(self.minimum.max(0) as usize, self.maximum.max(0) as usize)
    }

    /// What `__rich_measure__` returned: a `Measurement`, anything with
    /// `minimum` and `maximum` (Rich's own), or a pair of ints.
    pub(crate) fn extract_any(value: &Bound<'_, PyAny>) -> PyResult<Measurement> {
        if let Ok(measurement) = value.extract::<PyRef<'_, Measurement>>() {
            return Ok(*measurement);
        }
        if let (Ok(minimum), Ok(maximum)) = (value.getattr("minimum"), value.getattr("maximum")) {
            return Ok(Measurement {
                minimum: minimum.extract()?,
                maximum: maximum.extract()?,
            });
        }
        if let Ok((minimum, maximum)) = value.extract::<(i64, i64)>() {
            return Ok(Measurement { minimum, maximum });
        }
        Err(PyTypeError::new_err(format!(
            "__rich_measure__ must return a Measurement, not {}",
            value.get_type().name()?
        )))
    }

    fn tuple<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
        PyTuple::new(py, [self.minimum, self.maximum])
    }
}

#[pymethods]
impl Measurement {
    #[new]
    fn new(minimum: i64, maximum: i64) -> Self {
        Measurement { minimum, maximum }
    }

    /// `maximum - minimum`.
    #[getter]
    fn span(&self) -> i64 {
        self.maximum - self.minimum
    }

    /// Both bounds non-negative, with `minimum <= maximum`.
    fn normalize(&self) -> Measurement {
        let minimum = self.minimum.min(self.maximum).max(0);
        Measurement {
            minimum,
            maximum: self.maximum.max(minimum).max(0),
        }
    }

    fn with_maximum(&self, width: i64) -> Measurement {
        Measurement {
            minimum: self.minimum.min(width),
            maximum: self.maximum.min(width),
        }
    }

    fn with_minimum(&self, width: i64) -> Measurement {
        let width = width.max(0);
        Measurement {
            minimum: self.minimum.max(width),
            maximum: self.maximum.max(width),
        }
    }

    #[pyo3(signature = (min_width=None, max_width=None))]
    fn clamp(&self, min_width: Option<i64>, max_width: Option<i64>) -> Measurement {
        let mut measurement = *self;
        if let Some(width) = min_width {
            measurement = measurement.with_minimum(width);
        }
        if let Some(width) = max_width {
            measurement = measurement.with_maximum(width);
        }
        measurement
    }

    /// `Measurement.get(console, options, renderable)`.
    #[classmethod]
    fn get(
        _cls: &Bound<'_, PyType>,
        console: &Bound<'_, PyAny>,
        options: &Bound<'_, PyAny>,
        renderable: &Bound<'_, PyAny>,
    ) -> PyResult<Py<PyAny>> {
        let kwargs = PyDict::new(console.py());
        kwargs.set_item("options", options)?;
        Ok(console
            .call_method("measure", (renderable,), Some(&kwargs))?
            .unbind())
    }

    fn __len__(&self) -> usize {
        2
    }

    fn __getitem__(&self, index: isize) -> PyResult<i64> {
        match index {
            0 | -2 => Ok(self.minimum),
            1 | -1 => Ok(self.maximum),
            _ => Err(pyo3::exceptions::PyIndexError::new_err(
                "tuple index out of range",
            )),
        }
    }

    fn __iter__<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        Ok(self.tuple(py)?.try_iter()?.into_any())
    }

    fn __eq__(&self, py: Python<'_>, other: &Bound<'_, PyAny>) -> PyResult<bool> {
        match other.extract::<PyRef<'_, Measurement>>() {
            Ok(other) => Ok(self.minimum == other.minimum && self.maximum == other.maximum),
            Err(_) => self.tuple(py)?.eq(other),
        }
    }

    fn __hash__(&self, py: Python<'_>) -> PyResult<isize> {
        self.tuple(py)?.hash()
    }

    fn __repr__(&self) -> String {
        format!(
            "Measurement(minimum={}, maximum={})",
            self.minimum, self.maximum
        )
    }
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<ConsoleOptions>()?;
    m.add_class::<Measurement>()?;
    m.add("ConsoleDimensions", console_dimensions(m.py())?)?;
    Ok(())
}
