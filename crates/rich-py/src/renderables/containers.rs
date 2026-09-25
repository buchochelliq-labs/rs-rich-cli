//! `rich.console.Group` and `group`, and `rich.containers.Renderables`
//! (ports of upstream `rich/console.py`'s `Group` and `rich/containers.py`;
//! `Lines` belongs to the text area), plus
//! `rich.measure.measure_renderables`.

use pyo3::prelude::*;
use pyo3::types::{PyCFunction, PyDict, PyList, PyTuple};
use pyo3::{PyTraverseError, PyVisit};

use rich::console::{Console as CoreConsole, ConsoleOptions as CoreOptions};
use rich::measure::Measurement as CoreMeasurement;
use rich::protocol::Renderable;
use rich::segment::Segment as CoreSegment;

use crate::protocol::Measurement;
use crate::renderable::{self, AsRenderable};

use super::render;

/// Render each child in turn, as a `__rich_console__` that yields them
/// does: each child's lines end with a newline, and children do not reset
/// the options' height.
pub(crate) struct Stack {
    pub(crate) children: Vec<Box<dyn Renderable>>,
    pub(crate) measure: Measure,
}

/// How a [`Stack`] measures.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Measure {
    /// `Group(fit=True)`: `measure_renderables`, `(0, 0)` when empty.
    Fit,
    /// `Group(fit=False)`: the whole width.
    Fill,
    /// `Renderables`: as `Fit`, but `(1, 1)` when empty.
    Renderables,
}

impl Stack {
    fn from_list(list: &Bound<'_, PyList>, measure: Measure) -> PyResult<Stack> {
        let children = list
            .iter()
            .map(|child| renderable::to_renderable(&child, None))
            .collect::<PyResult<_>>()?;
        Ok(Stack { children, measure })
    }
}

impl Renderable for Stack {
    fn rich_render(&self, console: &CoreConsole, options: &CoreOptions) -> Vec<CoreSegment> {
        let mut child_options = options.clone();
        child_options.height = None;
        let mut segments = Vec::new();
        for child in &self.children {
            segments.extend(renderable::terminated(render(
                console,
                child.as_ref(),
                &child_options,
            )));
        }
        renderable::unterminated(segments)
    }

    fn measure(&self, console: &CoreConsole, options: &CoreOptions) -> CoreMeasurement {
        match self.measure {
            Measure::Fill => return CoreMeasurement::new(options.max_width, options.max_width),
            Measure::Fit if self.children.is_empty() => return CoreMeasurement::new(0, 0),
            Measure::Renderables if self.children.is_empty() => return CoreMeasurement::new(1, 1),
            _ => {}
        }
        let mut minimum = 0;
        let mut maximum = 0;
        for child in &self.children {
            let measured = CoreMeasurement::get(console, options, child.as_ref());
            minimum = minimum.max(measured.minimum);
            maximum = maximum.max(measured.maximum);
        }
        CoreMeasurement::new(minimum, maximum)
    }
}

/// `rich.console.Group`: render several renderables one after another.
#[pyclass(name = "Group", module = "rs_rich.console")]
pub(crate) struct Group {
    renderables: Py<PyList>,
    #[pyo3(get, set)]
    fit: bool,
}

impl AsRenderable for Group {
    fn to_renderable(&self, py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(Stack::from_list(
            self.renderables.bind(py),
            if self.fit {
                Measure::Fit
            } else {
                Measure::Fill
            },
        )?))
    }
}

#[pymethods]
impl Group {
    #[new]
    #[pyo3(signature = (*renderables, fit=true))]
    fn new(renderables: &Bound<'_, PyTuple>, fit: bool) -> PyResult<Self> {
        Ok(Group {
            renderables: PyList::new(renderables.py(), renderables.iter())?.unbind(),
            fit,
        })
    }

    /// The renderables, as a list you may change.
    #[getter]
    fn renderables(&self, py: Python<'_>) -> Py<PyList> {
        self.renderables.clone_ref(py)
    }

    fn __traverse__(&self, visit: PyVisit<'_>) -> Result<(), PyTraverseError> {
        visit.call(&self.renderables)
    }
}

/// Python glue for `group`: `functools.wraps` needs a Python function to
/// copy the decorated function's name and docstring onto.
const GROUP_GLUE: &std::ffi::CStr = c"
import functools

def decorate(method, make_group):
    @functools.wraps(method)
    def _replace(*args, **kwargs):
        return make_group(method(*args, **kwargs))

    return _replace
";

/// `rich.console.group(fit=True)`: a decorator that turns a function
/// returning renderables into one returning a `Group` of them.
#[pyfunction]
#[pyo3(signature = (fit=true))]
fn group(py: Python<'_>, fit: bool) -> PyResult<Bound<'_, PyCFunction>> {
    static GLUE: pyo3::sync::PyOnceLock<Py<PyAny>> = pyo3::sync::PyOnceLock::new();
    let decorate = GLUE
        .get_or_try_init(py, || {
            let module =
                PyModule::from_code(py, GROUP_GLUE, c"rs_rich/console.py", c"rs_rich._group")?;
            Ok::<_, PyErr>(module.getattr("decorate")?.unbind())
        })?
        .clone_ref(py);
    PyCFunction::new_closure(
        py,
        Some(c"decorator"),
        None,
        move |args: &Bound<'_, PyTuple>,
              _kwargs: Option<&Bound<'_, PyDict>>|
              -> PyResult<Py<PyAny>> {
            let py = args.py();
            let method = args.get_item(0)?;
            let make_group = PyCFunction::new_closure(
                py,
                Some(c"make_group"),
                None,
                move |args: &Bound<'_, PyTuple>,
                      _kwargs: Option<&Bound<'_, PyDict>>|
                      -> PyResult<Py<Group>> {
                    let py = args.py();
                    let list = PyList::empty(py);
                    for item in args.get_item(0)?.try_iter()? {
                        list.append(item?)?;
                    }
                    Py::new(
                        py,
                        Group {
                            renderables: list.unbind(),
                            fit,
                        },
                    )
                },
            )?;
            Ok(decorate.bind(py).call1((method, make_group))?.unbind())
        },
    )
}

/// `rich.containers.Renderables`: a list of renderables that renders them
/// one after another.
#[pyclass(name = "Renderables", module = "rs_rich.containers")]
pub(crate) struct Renderables {
    renderables: Py<PyList>,
}

impl AsRenderable for Renderables {
    fn to_renderable(&self, py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(Stack::from_list(
            self.renderables.bind(py),
            Measure::Renderables,
        )?))
    }
}

#[pymethods]
impl Renderables {
    #[new]
    #[pyo3(signature = (renderables=None))]
    fn new(py: Python<'_>, renderables: Option<&Bound<'_, PyAny>>) -> PyResult<Self> {
        let list = PyList::empty(py);
        if let Some(renderables) = renderables.filter(|r| !r.is_none()) {
            for item in renderables.try_iter()? {
                list.append(item?)?;
            }
        }
        Ok(Renderables {
            renderables: list.unbind(),
        })
    }

    fn append(&self, py: Python<'_>, renderable: Py<PyAny>) -> PyResult<()> {
        self.renderables.bind(py).append(renderable)
    }

    fn __iter__<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        Ok(self.renderables.bind(py).try_iter()?.into_any())
    }

    fn __traverse__(&self, visit: PyVisit<'_>) -> Result<(), PyTraverseError> {
        visit.call(&self.renderables)
    }
}

/// `rich.measure.measure_renderables`: the widest minimum and maximum of
/// several renderables.
#[pyfunction]
fn measure_renderables(
    console: &Bound<'_, PyAny>,
    options: &Bound<'_, PyAny>,
    renderables: &Bound<'_, PyAny>,
) -> PyResult<Measurement> {
    let mut measured: Option<(i64, i64)> = None;
    let kwargs = PyDict::new(console.py());
    kwargs.set_item("options", options)?;
    for renderable in renderables.try_iter()? {
        let measurement = console.call_method("measure", (renderable?,), Some(&kwargs))?;
        let measurement = Measurement::extract_any(&measurement)?.to_core();
        let (minimum, maximum) = measured.unwrap_or((0, 0));
        measured = Some((
            minimum.max(measurement.minimum as i64),
            maximum.max(measurement.maximum as i64),
        ));
    }
    let (minimum, maximum) = measured.unwrap_or((0, 0));
    Ok(Measurement::from_core(CoreMeasurement::new(
        minimum as usize,
        maximum as usize,
    )))
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    renderable::add_renderable_class::<Group>(m)?;
    renderable::add_renderable_class::<Renderables>(m)?;
    m.add_function(pyo3::wrap_pyfunction!(group, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(measure_renderables, m)?)?;
    Ok(())
}
