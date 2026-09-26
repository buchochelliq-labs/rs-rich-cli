//! `rs_rich.ext.transform`: composable transforms and pipelines.
//!
//! The text transforms (`KeepLines`, `HighlightMatches`) are `rich-ext`'s;
//! data (`rs_rich.ext.data`), table (`rs_rich.ext.table`) and patch
//! (`rs_rich.ext.diff`) transforms live with what they transform. A
//! `Pipeline` runs named stages in order and names the stage that failed,
//! as `rich_ext::transform::Pipeline` does; a stage is any of those
//! transforms, another pipeline, or a Python callable.

use pyo3::prelude::*;
use pyo3::types::PyTuple;

use rich_ext::transform::TextTransform;
use rich_ext::transform::{HighlightMatches as CoreHighlight, KeepLines as CoreKeepLines};

use super::common::{self, PipelineError, TransformError};
use crate::text::Text;

fn transform_error(error: rich_ext::transform::TransformError) -> PyErr {
    TransformError::new_err(error.message().to_string())
}

fn plugin_error(error: rich_ext::plugin::PluginError) -> PyErr {
    TransformError::new_err(error.to_string())
}

/// Apply a text transform to a `str` or `Text` argument, returning a `Text`.
pub(crate) fn apply_text(
    py: Python<'_>,
    transform: &dyn TextTransform,
    text: &Bound<'_, PyAny>,
) -> PyResult<Py<Text>> {
    let text = common::text_arg(text)?;
    let text = transform.transform(text).map_err(plugin_error)?;
    common::py_text(py, text)
}

/// `KeepLines(pattern, *, invert=False)`: keep the lines a regular
/// expression matches (or, inverted, the others), with their styles.
#[pyclass(name = "KeepLines", module = "rs_rich.ext.transform", frozen)]
pub(crate) struct KeepLines {
    pub(crate) inner: CoreKeepLines,
    #[pyo3(get)]
    pattern: String,
    #[pyo3(get)]
    invert: bool,
}

#[pymethods]
impl KeepLines {
    #[new]
    #[pyo3(signature = (pattern, *, invert=false))]
    fn new(pattern: String, invert: bool) -> PyResult<Self> {
        Ok(KeepLines {
            inner: CoreKeepLines::new(&pattern)
                .map_err(transform_error)?
                .invert(invert),
            pattern,
            invert,
        })
    }

    /// The text with only the kept lines.
    fn apply(&self, py: Python<'_>, text: &Bound<'_, PyAny>) -> PyResult<Py<Text>> {
        apply_text(py, &self.inner, text)
    }

    fn __call__(&self, py: Python<'_>, text: &Bound<'_, PyAny>) -> PyResult<Py<Text>> {
        self.apply(py, text)
    }
}

/// `HighlightMatches(pattern, style)`: style every match of a regular
/// expression.
#[pyclass(name = "HighlightMatches", module = "rs_rich.ext.transform", frozen)]
pub(crate) struct HighlightMatches {
    pub(crate) inner: CoreHighlight,
    #[pyo3(get)]
    pattern: String,
}

#[pymethods]
impl HighlightMatches {
    #[new]
    fn new(pattern: String, style: &Bound<'_, PyAny>) -> PyResult<Self> {
        let style = common::required_style(style)?;
        Ok(HighlightMatches {
            inner: CoreHighlight::new(&pattern, style).map_err(transform_error)?,
            pattern,
        })
    }

    fn apply(&self, py: Python<'_>, text: &Bound<'_, PyAny>) -> PyResult<Py<Text>> {
        apply_text(py, &self.inner, text)
    }

    fn __call__(&self, py: Python<'_>, text: &Bound<'_, PyAny>) -> PyResult<Py<Text>> {
        self.apply(py, text)
    }
}

/// `Pipeline(stages=())`: named stages run in order, each given the one
/// before's output. `then(name, stage)` adds one and returns the pipeline.
#[pyclass(name = "Pipeline", module = "rs_rich.ext.transform")]
pub(crate) struct Pipeline {
    stages: Vec<(String, Py<PyAny>)>,
}

#[pymethods]
impl Pipeline {
    #[new]
    #[pyo3(signature = (stages=None))]
    fn new(stages: Option<&Bound<'_, PyAny>>) -> PyResult<Self> {
        let mut pipeline = Pipeline { stages: Vec::new() };
        if let Some(stages) = stages {
            for item in stages.try_iter()? {
                let (name, stage): (String, Bound<'_, PyAny>) = item?.extract()?;
                pipeline.stages.push((name, stage.unbind()));
            }
        }
        Ok(pipeline)
    }

    /// Add a stage. Returns the pipeline.
    fn then<'py>(
        mut slf: PyRefMut<'py, Self>,
        name: String,
        stage: &Bound<'py, PyAny>,
    ) -> PyResult<PyRefMut<'py, Self>> {
        if !stage.hasattr("apply")? && !stage.is_callable() {
            return Err(pyo3::exceptions::PyTypeError::new_err(
                "a stage must be a transform (with .apply) or a callable",
            ));
        }
        slf.stages.push((name, stage.clone().unbind()));
        Ok(slf)
    }

    /// The stage names, in order.
    fn names(&self) -> Vec<String> {
        self.stages.iter().map(|(name, _)| name.clone()).collect()
    }

    fn __len__(&self) -> usize {
        self.stages.len()
    }

    /// Run every stage on `value`. A stage raising `TransformError` stops
    /// the pipeline with a `PipelineError` naming it (`stage` attribute).
    fn apply(&self, py: Python<'_>, value: Py<PyAny>) -> PyResult<Py<PyAny>> {
        let mut value = value;
        for (name, stage) in &self.stages {
            let stage = stage.bind(py);
            let args = PyTuple::new(py, [value.bind(py)])?;
            let result = match stage.getattr_opt("apply")? {
                Some(apply) => apply.call1(args),
                None => stage.call1(args),
            };
            value = match result {
                Ok(next) => next.unbind(),
                Err(error) if error.is_instance_of::<TransformError>(py) => {
                    let reason = error.value(py).str()?.to_string();
                    let wrapped = PipelineError::new_err(format!("{name}: {reason}"));
                    let instance = wrapped.value(py);
                    instance.setattr("stage", name)?;
                    instance.setattr("reason", reason)?;
                    wrapped.set_cause(py, Some(error));
                    return Err(wrapped);
                }
                Err(error) => return Err(error),
            };
        }
        Ok(value)
    }

    fn __call__(&self, py: Python<'_>, value: Py<PyAny>) -> PyResult<Py<PyAny>> {
        self.apply(py, value)
    }

    fn __repr__(&self) -> String {
        format!("Pipeline({:?})", self.names())
    }

    fn __traverse__(&self, visit: pyo3::PyVisit<'_>) -> Result<(), pyo3::PyTraverseError> {
        for (_, stage) in &self.stages {
            visit.call(stage)?;
        }
        Ok(())
    }

    fn __clear__(&mut self) {
        self.stages.clear();
    }
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<KeepLines>()?;
    m.add_class::<HighlightMatches>()?;
    m.add_class::<Pipeline>()?;
    Ok(())
}
