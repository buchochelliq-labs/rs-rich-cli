//! `rich.box`: the box styles tables and panels draw with.
//!
//! Owner: the foundation. `BoxArg` is the `box=` argument every boxed
//! renderable takes.

use pyo3::exceptions::PyTypeError;
use pyo3::prelude::*;

use rich::r#box::Box as CoreBox;

/// A box style for tables and panels (`rs_rich.box.ROUNDED`, …).
#[pyclass(name = "Box", module = "rs_rich.box", frozen)]
pub(crate) struct PyBox {
    name: &'static str,
    pub(crate) inner: CoreBox,
}

#[pymethods]
impl PyBox {
    fn __repr__(&self) -> String {
        format!("box.{}", self.name)
    }
}

const BOXES: &[(&str, CoreBox)] = {
    use rich::r#box::*;
    &[
        ("ASCII", ASCII),
        ("ASCII2", ASCII2),
        ("ASCII_DOUBLE_HEAD", ASCII_DOUBLE_HEAD),
        ("SQUARE", SQUARE),
        ("SQUARE_DOUBLE_HEAD", SQUARE_DOUBLE_HEAD),
        ("MINIMAL", MINIMAL),
        ("MINIMAL_HEAVY_HEAD", MINIMAL_HEAVY_HEAD),
        ("MINIMAL_DOUBLE_HEAD", MINIMAL_DOUBLE_HEAD),
        ("SIMPLE", SIMPLE),
        ("SIMPLE_HEAD", SIMPLE_HEAD),
        ("SIMPLE_HEAVY", SIMPLE_HEAVY),
        ("HORIZONTALS", HORIZONTALS),
        ("ROUNDED", ROUNDED),
        ("HEAVY", HEAVY),
        ("HEAVY_EDGE", HEAVY_EDGE),
        ("HEAVY_HEAD", HEAVY_HEAD),
        ("DOUBLE", DOUBLE),
        ("DOUBLE_EDGE", DOUBLE_EDGE),
        ("MARKDOWN", MARKDOWN),
    ]
};

/// A core box as Python's `rs_rich.box` constant (a new `Box` for one that
/// is not a constant).
pub(crate) fn to_py(py: Python<'_>, inner: CoreBox) -> PyResult<Py<PyAny>> {
    if let Some((name, _)) = BOXES.iter().find(|(_, known)| *known == inner) {
        return Ok(py.import("rs_rich.box")?.getattr(*name)?.unbind());
    }
    Ok(Py::new(py, PyBox { name: "Box", inner })?.into_any())
}

/// A `box=` argument: not given, an explicit `None` (no box), or a box.
/// PyO3 maps both a missing argument and `None` to `Option::None`, so this
/// type tells them apart.
pub(crate) enum BoxArg {
    Default,
    NoBox,
    Box(CoreBox),
}

impl<'a, 'py> FromPyObject<'a, 'py> for BoxArg {
    type Error = PyErr;

    fn extract(value: pyo3::Borrowed<'a, 'py, PyAny>) -> PyResult<Self> {
        if value.is_none() {
            return Ok(BoxArg::NoBox);
        }
        value
            .extract::<PyRef<'_, PyBox>>()
            .map(|b| BoxArg::Box(b.inner))
            .map_err(|_| PyTypeError::new_err("box must be one of rs_rich.box's constants or None"))
    }
}

impl BoxArg {
    pub(crate) fn or(self, default: CoreBox) -> Option<CoreBox> {
        match self {
            BoxArg::Default => Some(default),
            BoxArg::NoBox => None,
            BoxArg::Box(inner) => Some(inner),
        }
    }
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyBox>()?;
    for (name, inner) in BOXES {
        m.add(
            *name,
            PyBox {
                name,
                inner: *inner,
            },
        )?;
    }
    Ok(())
}
