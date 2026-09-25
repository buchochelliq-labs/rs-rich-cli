//! `rs_rich.ext.a11y.accessible_text` (typed screen-reader text) and the
//! parts of `rich-ext` behind its `testing` feature (`rs_rich.ext.testing`
//! and `rs_rich.ext.qa`: render snapshots, rendered-diff assertions,
//! approved screenshots, stress, lint, explain, profile, fuzz, matrix and
//! benchmarks). The wheel builds `rs-rich-ext` without `testing`, so those
//! raise `NotImplementedError` naming what is missing.

use pyo3::exceptions::PyNotImplementedError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyString, PyTuple};

use rich_ext::a11y::AccessibleText;

use super::common;
use super::diagnostic::Diagnostic;
use super::tables::{StreamingTable, TableData};
use super::widgets::{Badge, Badges};
use crate::renderable;
use crate::text::Text;

/// `accessible_text(renderable, width=80)`: linear text for a screen
/// reader. Tables become `header: value` records, badges and diagnostics
/// keep their words; anything else is rendered plainly with decoration
/// dropped (`semantic_text`).
#[pyfunction]
#[pyo3(signature = (renderable, width=80))]
fn accessible_text(
    py: Python<'_>,
    renderable: &Bound<'_, PyAny>,
    width: usize,
) -> PyResult<String> {
    if let Ok(text) = renderable.cast::<PyString>() {
        return Ok(text.to_cow()?.as_ref().accessible_text(width));
    }
    if let Ok(text) = renderable.extract::<PyRef<'_, Text>>() {
        return Ok(text.inner.accessible_text(width));
    }
    if let Ok(value) = renderable.extract::<PyRef<'_, TableData>>() {
        return Ok(value.inner.accessible_text(width));
    }
    if let Ok(value) = renderable.extract::<PyRef<'_, StreamingTable>>() {
        let table = value.inner.lock().unwrap_or_else(|e| e.into_inner());
        return Ok(table.accessible_text(width));
    }
    if let Ok(value) = renderable.extract::<PyRef<'_, Badge>>() {
        return Ok(value.inner.accessible_text(width));
    }
    if let Ok(value) = renderable.extract::<PyRef<'_, Badges>>() {
        return Ok(value.inner.accessible_text(width));
    }
    if let Ok(value) = renderable.extract::<PyRef<'_, Diagnostic>>() {
        return Ok(value.inner.accessible_text(width));
    }
    common::scoped(py, width, 10_000, false, || {
        let value = renderable::to_renderable(renderable, None)?;
        Ok(rich_ext::a11y::semantic_text(value.as_ref(), width))
    })
}

fn unavailable(name: &str) -> PyErr {
    PyNotImplementedError::new_err(format!(
        "rs_rich.ext {name} needs rs-rich-ext's `testing` feature, which this build of rs_rich \
         does not enable"
    ))
}

macro_rules! testing_only {
    ($($name:ident => $label:literal),+ $(,)?) => {
        $(
            #[doc = concat!("`", $label, "`: needs rs-rich-ext's `testing` feature (raises `NotImplementedError`).")]
            #[pyfunction]
            #[pyo3(signature = (*_args, **_kwargs))]
            fn $name(_args: &Bound<'_, PyTuple>, _kwargs: Option<&Bound<'_, PyDict>>) -> PyResult<()> {
                Err(unavailable($label))
            }
        )+
        fn register_testing(m: &Bound<'_, PyModule>) -> PyResult<()> {
            $(m.add_function(pyo3::wrap_pyfunction!($name, m)?)?;)+
            Ok(())
        }
    };
}

testing_only! {
    render_snapshot => "render_snapshot (RenderSnapshot)",
    assert_render_eq => "assert_render_eq",
    assert_str_eq => "assert_str_eq",
    assert_json_eq => "assert_json_eq",
    qa_screenshot => "qa.screenshot",
    qa_stress => "qa.stress",
    qa_lint => "qa.lint",
    qa_explain => "qa.explain",
    qa_profile => "qa.profile",
    qa_fuzz => "qa.fuzz",
    qa_matrix => "qa.matrix",
    qa_bench => "qa.bench",
    highlighter_conformance => "testing.conformance",
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(pyo3::wrap_pyfunction!(accessible_text, m)?)?;
    register_testing(m)
}
