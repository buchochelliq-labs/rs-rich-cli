//! Helpers shared by the live area: the global console, Python locks,
//! segment conversions and the small private renderables the area prints.

use std::sync::Arc;

use pyo3::exceptions::PyTypeError;
use pyo3::prelude::*;
use pyo3::types::{PyList, PyString};

use rich::console::{Console as CoreConsole, ConsoleOptions as CoreOptions};
use rich::measure::Measurement as CoreMeasurement;
use rich::protocol::Renderable;
use rich::segment::Segment as CoreSegment;
use rich::Text as CoreText;

use crate::renderable::{self, AsRenderable, PyRenderable};
use crate::segment::Segment;
use crate::text::Text;

/// `rich.get_console()`: the global console.
pub(crate) fn get_console(py: Python<'_>) -> PyResult<Bound<'_, PyAny>> {
    py.import("rs_rich")?.call_method0("get_console")
}

/// The console given, else the global one (Rich's `console or get_console()`).
pub(crate) fn console_or_global<'py>(
    py: Python<'py>,
    console: Option<Bound<'py, PyAny>>,
) -> PyResult<Bound<'py, PyAny>> {
    match console {
        Some(console) if !console.is_none() => Ok(console),
        _ => get_console(py),
    }
}

/// A Python `Text` holding a core text.
pub(crate) fn new_text(py: Python<'_>, inner: CoreText) -> PyResult<Py<PyAny>> {
    Ok(Py::new(py, Text { inner })?.into_any())
}

/// The core text of a Python `Text`, if it is one.
pub(crate) fn core_text(value: &Bound<'_, PyAny>) -> Option<CoreText> {
    value
        .extract::<PyRef<'_, Text>>()
        .ok()
        .map(|text| text.inner.clone())
}

/// A new `threading.RLock()`.
pub(crate) fn rlock(py: Python<'_>) -> PyResult<Py<PyAny>> {
    Ok(py.import("threading")?.call_method0("RLock")?.unbind())
}

/// A Python lock (`RLock`) held until dropped. Acquiring releases the GIL
/// while it waits, so it never deadlocks with another thread's Python code.
pub(crate) struct Held<'py>(Bound<'py, PyAny>);

pub(crate) fn hold<'py>(lock: &Bound<'py, PyAny>) -> PyResult<Held<'py>> {
    lock.call_method0("acquire")?;
    Ok(Held(lock.clone()))
}

impl Drop for Held<'_> {
    fn drop(&mut self) {
        if let Err(error) = self.0.call_method0("release") {
            error.write_unraisable(self.0.py(), None);
        }
    }
}

/// Core segments from a Python iterable of `Segment`s.
pub(crate) fn core_segments(value: &Bound<'_, PyAny>) -> PyResult<Vec<CoreSegment>> {
    let mut segments = Vec::new();
    for item in value.try_iter()? {
        let item = item?;
        let segment = item.extract::<PyRef<'_, Segment>>().map_err(|_| {
            PyTypeError::new_err(format!(
                "expected a Segment, got {}",
                item.get_type().name().map(|n| n.to_string()).unwrap_or_default()
            ))
        })?;
        segments.push(segment.to_core());
    }
    Ok(segments)
}

/// Lines of core segments from `Console.render_lines`' list of lists.
pub(crate) fn core_lines(value: &Bound<'_, PyAny>) -> PyResult<Vec<Vec<CoreSegment>>> {
    value
        .try_iter()?
        .map(|line| core_segments(&line?))
        .collect()
}

/// `Segment.get_shape`: the widest line's cell length and the line count.
pub(crate) fn get_shape(lines: &[Vec<CoreSegment>]) -> (usize, usize) {
    let width = lines
        .iter()
        .map(|line| line.iter().map(CoreSegment::cell_length).sum::<usize>())
        .max()
        .unwrap_or(0);
    (width, lines.len())
}

/// `console.is_<flag>` as a bool.
pub(crate) fn flag(console: &Bound<'_, PyAny>, name: &str) -> PyResult<bool> {
    console.getattr(name)?.is_truthy()
}

/// Write control codes the way `Console.control` does: skipped on a dumb
/// terminal, and never through a render hook.
pub(crate) fn control(console: &Bound<'_, PyAny>, codes: &str) -> PyResult<()> {
    if flag(console, "is_dumb_terminal")? {
        return Ok(());
    }
    let py = console.py();
    let item = Renderables::new(vec![Item::Segments(vec![CoreSegment::control(codes)])]);
    console.call_method1("print", (Py::new(py, item)?,))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Private renderables

/// One piece of a [`Renderables`] frame.
pub(crate) enum Item {
    /// Ready segments (control codes).
    Segments(Vec<CoreSegment>),
    /// A renderable, rendered with the frame's options.
    Object(Py<PyAny>),
}

/// What a print through Rich's render hook prints: control codes and
/// renderables in order, with no newline added after the last (Rich's
/// `Console.print` of a `Control` and a `LiveRender`).
#[pyclass(name = "_Renderables", module = "rs_rich.live", frozen)]
pub(crate) struct Renderables {
    items: Vec<Item>,
}

impl Renderables {
    pub(crate) fn new(items: Vec<Item>) -> Renderables {
        Renderables { items }
    }
}

#[pymethods]
impl Renderables {
    fn __rich_console__<'py>(
        &self,
        py: Python<'py>,
        _console: &Bound<'py, PyAny>,
        _options: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyList>> {
        let list = PyList::empty(py);
        for item in &self.items {
            match item {
                Item::Segments(segments) => {
                    for segment in segments {
                        list.append(Segment::from_core(py, segment))?;
                    }
                }
                Item::Object(object) => list.append(object.bind(py))?,
            }
        }
        Ok(list)
    }
}

/// `rich.control.Control`, as `LiveRender.position_cursor()` returns it: a
/// control segment that prints its codes, and `str()` of them.
#[pyclass(name = "_Control", module = "rs_rich.live", frozen)]
pub(crate) struct Control {
    pub(crate) codes: String,
}

#[pymethods]
impl Control {
    #[getter]
    fn segment(&self, py: Python<'_>) -> Segment {
        Segment::from_core(py, &CoreSegment::control(self.codes.clone()))
    }

    fn __str__(&self) -> String {
        self.codes.clone()
    }

    fn __repr__(&self) -> String {
        format!("<control {:?}>", self.codes)
    }

    fn __rich_console__<'py>(
        &self,
        py: Python<'py>,
        _console: &Bound<'py, PyAny>,
        _options: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyList>> {
        PyList::new(py, [self.segment(py)])
    }
}

/// A shared core renderable behind a `Box`.
struct Shared(Arc<dyn Renderable + Send + Sync>);

impl Renderable for Shared {
    fn rich_render(&self, console: &CoreConsole, options: &CoreOptions) -> Vec<CoreSegment> {
        self.0.rich_render(console, options)
    }

    fn measure(&self, console: &CoreConsole, options: &CoreOptions) -> CoreMeasurement {
        self.0.measure(console, options)
    }

    fn fit_to_measurement(&self) -> bool {
        self.0.fit_to_measurement()
    }
}

/// A core renderable built by the live area (a tasks grid, a log row), as a
/// Python object that prints like any of the bindings' classes.
#[pyclass(name = "_Renderable", module = "rs_rich.live", frozen)]
pub(crate) struct CoreRenderable {
    pub(crate) inner: Arc<dyn Renderable + Send + Sync>,
}

impl AsRenderable for CoreRenderable {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(Shared(self.inner.clone())))
    }
}

pub(crate) fn core_renderable(
    py: Python<'_>,
    inner: Arc<dyn Renderable + Send + Sync>,
) -> PyResult<Py<PyAny>> {
    Ok(Py::new(py, CoreRenderable { inner })?.into_any())
}

/// Render a core renderable through the Python console with `options`,
/// in core's convention (no newline after the last line).
pub(crate) fn render_core(
    console: &Bound<'_, PyAny>,
    options: &Bound<'_, PyAny>,
    inner: Arc<dyn Renderable + Send + Sync>,
) -> PyResult<Vec<CoreSegment>> {
    let py = console.py();
    let object = core_renderable(py, inner)?;
    let rendered = console.call_method1("render", (object, options))?;
    Ok(renderable::unterminated(core_segments(&rendered)?)
        .into_iter()
        .filter(|segment| !(segment.text.is_empty() && !segment.control))
        .collect())
}

/// Rich's `Group`: children rendered one after another.
struct Sequence(Vec<Arc<dyn Renderable + Send + Sync>>);

impl Renderable for Sequence {
    fn rich_render(&self, console: &CoreConsole, options: &CoreOptions) -> Vec<CoreSegment> {
        let mut segments = Vec::new();
        for child in &self.0 {
            let rendered = child.rich_render(console, options);
            if rendered.is_empty() {
                continue;
            }
            if !segments.is_empty() {
                segments.push(CoreSegment::line());
            }
            segments.extend(rendered);
        }
        segments
    }

    fn measure(&self, console: &CoreConsole, options: &CoreOptions) -> CoreMeasurement {
        let mut minimum = 0;
        let mut maximum = 0;
        for child in &self.0 {
            let measured = CoreMeasurement::get(console, options, child.as_ref());
            minimum = minimum.max(measured.minimum);
            maximum = maximum.max(measured.maximum);
        }
        CoreMeasurement::new(minimum, maximum)
    }
}

/// `Group(*children)`; one child stands for itself.
pub(crate) fn group(py: Python<'_>, children: Vec<Bound<'_, PyAny>>) -> PyResult<Py<PyAny>> {
    if children.len() == 1 {
        return Ok(children.into_iter().next().expect("one child").unbind());
    }
    let mut shared = Vec::new();
    for child in children {
        if !renderable::is_renderable(&child)? {
            return Err(crate::errors::NotRenderableError::new_err(format!(
                "Unable to render {}; A str, Segment or object with __rich_console__ method is required",
                child.repr()?
            )));
        }
        shared.push(PyRenderable::shared(child.unbind(), None));
    }
    core_renderable(py, Arc::new(Sequence(shared)))
}

/// `str(value)` as a Rust string.
pub(crate) fn to_str(value: &Bound<'_, PyAny>) -> PyResult<String> {
    Ok(value.str()?.to_cow()?.into_owned())
}

/// Whether `value` is a `str`.
pub(crate) fn is_str(value: &Bound<'_, PyAny>) -> bool {
    value.is_instance_of::<PyString>()
}

/// An argument whose default differs from an explicit `None` (Rich's
/// `total=100.0` next to `total=None`, `default=...`).
pub(crate) enum Arg {
    Missing,
    Given(Py<PyAny>),
}

impl<'a, 'py> FromPyObject<'a, 'py> for Arg {
    type Error = PyErr;

    fn extract(value: pyo3::Borrowed<'a, 'py, PyAny>) -> PyResult<Self> {
        Ok(Arg::Given(value.to_owned().unbind()))
    }
}

impl Arg {
    /// The value given, else `default()`.
    pub(crate) fn or_else(self, default: impl FnOnce() -> Py<PyAny>) -> Py<PyAny> {
        match self {
            Arg::Missing => default(),
            Arg::Given(value) => value,
        }
    }
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Renderables>()?;
    m.add_class::<Control>()?;
    renderable::add_renderable_class::<CoreRenderable>(m)
}
