//! `rs_rich.ext.a11y.accessible_text` (typed screen-reader text) and the
//! parts of `rich-ext` behind its `testing` feature (`rs_rich.ext.testing`
//! and `rs_rich.ext.qa`: render snapshots, rendered-diff assertions,
//! approved screenshots, stress, lint, explain, profile, fuzz, matrix and
//! benchmarks). Reports come back as Python data plus a printable
//! `QaReport`; assertions raise `AssertionError` with rich-ext's rendered
//! diff.

use std::sync::Arc;
use std::time::Duration;

use pyo3::exceptions::{PyAssertionError, PyOSError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList, PyString};

use rich::protocol::RenderEnvironment;
use rich_ext::capabilities::ColorDepth;
use rich_ext::target::RenderTarget as CoreTarget;

use rich_ext::a11y::AccessibleText;

use super::common;
use super::diagnostic::Diagnostic;
use super::tables::{StreamingTable, TableData};
use super::terminal::RenderTarget;
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

// ---------------------------------------------------------------------------
// `rich_ext::testing` and `rich_ext::diff::assert`

/// A report as Python data: serde's JSON through `json.loads`.
fn to_py_data<T: serde::Serialize>(py: Python<'_>, value: &T) -> PyResult<Py<PyAny>> {
    let json = serde_json::to_string(value).map_err(|e| PyValueError::new_err(e.to_string()))?;
    Ok(py.import("json")?.call_method1("loads", (json,))?.unbind())
}

/// The default target: an 80-column truecolor capture.
fn capture_target(width: usize) -> CoreTarget {
    CoreTarget::new(
        rich_ext::target::TargetKind::Capture,
        rich::protocol::TargetCapabilities {
            width: width.clamp(1, crate::limits::MAX_CONSOLE_WIDTH),
            height: 25,
            color_system: Some(rich::ColorSystem::Truecolor),
            interactive: false,
            unicode: true,
            hyperlinks: true,
            sixel: rich::protocol::Support::Unsupported,
        },
        rich::theme::Theme::default_theme(),
    )
}

fn target_or(target: Option<PyRef<'_, RenderTarget>>, width: usize) -> CoreTarget {
    target.map_or_else(|| capture_target(width), |t| t.inner.clone())
}

/// Run `f` with a Python renderable converted, inside a render scope.
fn with_renderable<T>(
    py: Python<'_>,
    renderable: &Bound<'_, PyAny>,
    width: usize,
    f: impl FnOnce(&dyn rich::Renderable) -> PyResult<T>,
) -> PyResult<T> {
    common::scoped(py, width, 10_000, false, || {
        let value = renderable::to_renderable(renderable, None)?;
        let result = f(value.as_ref())?;
        renderable::check_pending()?;
        Ok(result)
    })
}

/// `AssertionError` as rich-ext's assertions panic: the rendered diff.
fn assertion(what: &str, view: rich_ext::diff::DiffView, message: Option<String>) -> PyErr {
    let rendered = rich_ext::diff::assert::Report::from_env().render(&view);
    PyAssertionError::new_err(match message {
        Some(message) => format!("assertion `{what}` failed: {message}\n{rendered}"),
        None => format!("assertion `{what}` failed\n{rendered}"),
    })
}

/// `render_snapshot(renderable, *, target=None, width=80)`: rich-ext's
/// `RenderSnapshot` of a render (schema version, size, plain and ANSI text,
/// and every segment's text, colours, attributes and link) as a dict.
#[pyfunction]
#[pyo3(signature = (renderable, *, target=None, width=80))]
fn render_snapshot(
    py: Python<'_>,
    renderable: &Bound<'_, PyAny>,
    target: Option<PyRef<'_, RenderTarget>>,
    width: usize,
) -> PyResult<Py<PyAny>> {
    let target = target_or(target, width);
    let width = target.capabilities().width;
    let snapshot = with_renderable(py, renderable, width, |value| {
        Ok(rich_ext::testing::RenderSnapshot::capture(&target, value))
    })?;
    to_py_data(py, &snapshot)
}

/// `assert_str_eq(left, right, message=None)`: raise `AssertionError` with
/// a rendered line diff when the strings differ (`assert_rich_eq!`).
#[pyfunction]
#[pyo3(signature = (left, right, message=None))]
fn assert_str_eq(left: &str, right: &str, message: Option<String>) -> PyResult<()> {
    if left == right {
        return Ok(());
    }
    let view = rich_ext::diff::DiffView::new(left, right).titles("left", "right");
    Err(assertion("left == right", view, message))
}

/// Pretty JSON as serde prints it, keys sorted (a dict's order does not
/// make two values differ, as in Python).
fn pretty_json(py: Python<'_>, value: &Bound<'_, PyAny>) -> PyResult<String> {
    let kwargs = PyDict::new(py);
    kwargs.set_item("sort_keys", true)?;
    let text: String = py
        .import("json")?
        .call_method("dumps", (value,), Some(&kwargs))?
        .extract()?;
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| PyValueError::new_err(e.to_string()))?;
    let pretty =
        serde_json::to_string_pretty(&value).map_err(|e| PyValueError::new_err(e.to_string()))?;
    Ok(pretty + "\n")
}

/// `assert_json_eq(left, right, message=None)`: compare two JSON-able
/// values as pretty JSON (`assert_rich_json_eq!`).
#[pyfunction]
#[pyo3(signature = (left, right, message=None))]
fn assert_json_eq(
    py: Python<'_>,
    left: &Bound<'_, PyAny>,
    right: &Bound<'_, PyAny>,
    message: Option<String>,
) -> PyResult<()> {
    let (left, right) = (pretty_json(py, left)?, pretty_json(py, right)?);
    if left == right {
        return Ok(());
    }
    let view = rich_ext::diff::DiffView::new(&left, &right).titles("left", "right");
    Err(assertion("left == right", view, message))
}

/// `assert_render_eq(renderable, expected, width=80, message=None)`: render
/// plainly at `width` (trailing spaces and blank lines dropped) and compare
/// with `expected` (`assert_render_eq!`).
#[pyfunction]
#[pyo3(signature = (renderable, expected, width=80, message=None))]
fn assert_render_eq(
    py: Python<'_>,
    renderable: &Bound<'_, PyAny>,
    expected: &str,
    width: usize,
    message: Option<String>,
) -> PyResult<()> {
    let actual = with_renderable(py, renderable, width, |value| {
        Ok(rich_ext::diff::assert::render_plain(value, width))
    })?;
    let normalize = |text: &str| {
        let lines: Vec<&str> = text.lines().map(str::trim_end).collect();
        let end = lines
            .iter()
            .rposition(|l| !l.is_empty())
            .map_or(0, |i| i + 1);
        let mut out = lines[..end].join("\n");
        if !out.is_empty() {
            out.push('\n');
        }
        out
    };
    let expected = normalize(expected);
    if actual == expected {
        return Ok(());
    }
    let view = rich_ext::diff::DiffView::new(&expected, &actual).titles("expected", "rendered");
    Err(assertion("rendered == expected", view, message))
}

/// `highlighter_conformance(highlighter, *, all_themes=False, scaling=True)`:
/// rich-ext's code-highlighter conformance checks (a name, a plugin handle
/// or a Python code highlighter). Raises `AssertionError` listing every
/// failure.
#[pyfunction]
#[pyo3(signature = (highlighter, *, all_themes=false, scaling=true))]
fn highlighter_conformance(
    py: Python<'_>,
    highlighter: &Bound<'_, PyAny>,
    all_themes: bool,
    scaling: bool,
) -> PyResult<()> {
    let engine = crate::code::code_highlighter_value(Some(highlighter))?
        .ok_or_else(|| PyValueError::new_err("a code highlighter is required"))?;
    let options = rich_ext::testing::conformance::Options {
        all_themes,
        scaling,
    };
    let result = py.detach(|| rich_ext::testing::conformance::check_with(engine, options));
    result.map_err(|error| PyAssertionError::new_err(error.to_string()))
}

// ---------------------------------------------------------------------------
// `rich_ext::qa`

/// A report's renderable, shared by the Python object.
type SharedReport = Arc<dyn rich::Renderable + Send + Sync>;

/// What a `qa` tool found: `data` (the report as dicts and lists), `ok`,
/// and the Rust report itself when printed.
#[pyclass(name = "QaReport", module = "rs_rich.ext.qa", frozen)]
pub(crate) struct QaReport {
    #[pyo3(get)]
    kind: &'static str,
    #[pyo3(get)]
    ok: bool,
    #[pyo3(get)]
    data: Py<PyAny>,
    view: SharedReport,
}

#[pymethods]
impl QaReport {
    fn __repr__(&self) -> String {
        format!(
            "<QaReport {} ok={}>",
            self.kind,
            if self.ok { "True" } else { "False" }
        )
    }

    fn __bool__(&self) -> bool {
        self.ok
    }
}

impl renderable::AsRenderable for QaReport {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn rich::Renderable>> {
        Ok(Box::new(View(Arc::clone(&self.view))))
    }
}

struct View(SharedReport);

impl rich::Renderable for View {
    fn rich_render(
        &self,
        console: &rich::Console,
        options: &rich::ConsoleOptions,
    ) -> Vec<rich::Segment> {
        self.0.rich_render(console, options)
    }
}

struct OwnedExplanation(rich_ext::qa::explain::Explanation);

impl rich::Renderable for OwnedExplanation {
    fn rich_render(
        &self,
        console: &rich::Console,
        options: &rich::ConsoleOptions,
    ) -> Vec<rich::Segment> {
        rich_ext::qa::explain::ExplanationView::new(&self.0).rich_render(console, options)
    }
}

struct OwnedProfile(rich_ext::qa::profile::Profile);

impl rich::Renderable for OwnedProfile {
    fn rich_render(
        &self,
        console: &rich::Console,
        options: &rich::ConsoleOptions,
    ) -> Vec<rich::Segment> {
        rich_ext::qa::profile::ProfileReport::new(&self.0).rich_render(console, options)
    }
}

/// Plain text as a report view.
struct Lines(String);

impl rich::Renderable for Lines {
    fn rich_render(
        &self,
        console: &rich::Console,
        options: &rich::ConsoleOptions,
    ) -> Vec<rich::Segment> {
        rich::Text::new(self.0.clone()).rich_render(console, options)
    }
}

fn report(
    py: Python<'_>,
    kind: &'static str,
    ok: bool,
    data: Py<PyAny>,
    view: SharedReport,
) -> PyResult<Py<QaReport>> {
    Py::new(
        py,
        QaReport {
            kind,
            ok,
            data,
            view,
        },
    )
}

fn depth(value: &str) -> PyResult<ColorDepth> {
    ColorDepth::parse(value).ok_or_else(|| {
        PyValueError::new_err(format!(
            "invalid color depth {value:?}; expected none, 16, 256 or truecolor"
        ))
    })
}

/// `screenshot(name, renderable, *, widths=(40, 80, 120), color=("truecolor",
/// "none"), unicode=(True, False), hyperlinks=False, height=None,
/// approvals=None, approve=None)`: render every configuration. Returns the
/// shots (`key`, `name`, `width`, `color`, `unicode`, `text`, `contents`);
/// with an `approvals` directory, checks them against the approved files
/// (writing `.new` files, or approving with `approve=True` or
/// `RICH_APPROVE=1`) and raises `AssertionError` on a difference.
#[pyfunction]
#[pyo3(signature = (
    name, renderable, *, widths=None, color=None, unicode=None, hyperlinks=false, height=None,
    approvals=None, approve=None
))]
#[allow(clippy::too_many_arguments)]
fn qa_screenshot(
    py: Python<'_>,
    name: &str,
    renderable: &Bound<'_, PyAny>,
    widths: Option<Vec<usize>>,
    color: Option<Vec<String>>,
    unicode: Option<Vec<bool>>,
    hyperlinks: bool,
    height: Option<usize>,
    approvals: Option<std::path::PathBuf>,
    approve: Option<bool>,
) -> PyResult<Py<PyAny>> {
    let mut matrix = rich_ext::qa::Matrix::default()
        .hyperlinks(hyperlinks)
        .height(height);
    if let Some(widths) = widths {
        matrix = matrix.widths(widths);
    }
    if let Some(color) = color {
        matrix = matrix.color(
            color
                .iter()
                .map(|c| depth(c))
                .collect::<PyResult<Vec<_>>>()?,
        );
    }
    if let Some(unicode) = unicode {
        matrix = matrix.unicode(unicode);
    }
    let widest = matrix.widths.iter().copied().max().unwrap_or(80);
    let shots = with_renderable(py, renderable, widest, |value| {
        Ok(rich_ext::qa::Screenshot::capture(name, value, &matrix))
    })?;
    if let Some(dir) = approvals {
        let mut approvals = rich_ext::qa::Approvals::new(&dir);
        if let Some(approve) = approve {
            approvals = approvals.approving(approve);
        }
        let outcome = approvals
            .check(&shots)
            .map_err(|e| PyOSError::new_err(format!("cannot use {}: {e}", dir.display())))?;
        if !outcome.is_ok() {
            return Err(PyAssertionError::new_err(format!(
                "screenshots `{name}` differ from {}: {}\n\
                 review the .new files, then rerun with RICH_APPROVE=1 to accept them",
                dir.display(),
                outcome.report()
            )));
        }
    }
    let list = PyList::empty(py);
    for shot in &shots {
        let entry = PyDict::new(py);
        entry.set_item("key", &shot.key)?;
        entry.set_item("name", &shot.name)?;
        entry.set_item("width", shot.width)?;
        entry.set_item("color", shot.color.name())?;
        entry.set_item("unicode", shot.unicode)?;
        entry.set_item("text", &shot.text)?;
        entry.set_item("contents", shot.file_contents())?;
        list.append(entry)?;
    }
    Ok(list.into_any().unbind())
}

/// `stress(renderable, *, widths=None, heights=None, unicode=True,
/// line_tolerance=1, strict_minimum=False)`: render at many sizes and report
/// overflow, clipping, unstable wrapping, panics and measure mismatches.
#[pyfunction]
#[pyo3(signature = (
    renderable, *, widths=None, heights=None, unicode=true, line_tolerance=1,
    strict_minimum=false
))]
fn qa_stress(
    py: Python<'_>,
    renderable: &Bound<'_, PyAny>,
    widths: Option<Vec<usize>>,
    heights: Option<Vec<Option<usize>>>,
    unicode: bool,
    line_tolerance: usize,
    strict_minimum: bool,
) -> PyResult<Py<QaReport>> {
    let mut options = rich_ext::qa::stress::StressOptions::default();
    if let Some(widths) = widths {
        options.widths = widths;
    }
    if let Some(heights) = heights {
        options.heights = heights;
    }
    options.unicode = unicode;
    options.line_tolerance = line_tolerance;
    options.strict_minimum = strict_minimum;
    let widest = options.widths.iter().copied().max().unwrap_or(80);
    let found = with_renderable(py, renderable, widest, |value| {
        Ok(rich_ext::qa::stress::stress(value, &options))
    })?;
    let data = to_py_data(py, &found)?;
    report(py, "stress", found.is_clean(), data, Arc::new(found))
}

/// `lint(renderable, *, widths=(20, 40, 80), color="16", unicode=True,
/// hyperlinks=False, layout=True)`: render and markup lints. A `str` is
/// linted as console markup.
#[pyfunction]
#[pyo3(signature = (
    renderable, *, widths=None, color="16", unicode=true, hyperlinks=false, layout=true
))]
fn qa_lint(
    py: Python<'_>,
    renderable: &Bound<'_, PyAny>,
    widths: Option<Vec<usize>>,
    color: &str,
    unicode: bool,
    hyperlinks: bool,
    layout: bool,
) -> PyResult<Py<QaReport>> {
    let mut options = rich_ext::qa::lint::LintOptions::default()
        .color(depth(color)?)
        .unicode(unicode)
        .hyperlinks(hyperlinks)
        .layout(layout);
    if let Some(widths) = widths {
        options = options.widths(widths);
    }
    let findings = if let Ok(markup) = renderable.cast::<PyString>() {
        rich_ext::qa::lint::lint_markup(markup.to_cow()?.as_ref(), &options.theme)
    } else {
        let widest = options.widths.iter().copied().max().unwrap_or(80);
        with_renderable(py, renderable, widest, |value| {
            Ok(rich_ext::qa::lint::lint(value, &options))
        })?
    };
    let found = rich_ext::qa::lint::LintReport::new(findings);
    let data = to_py_data(py, &found)?;
    report(py, "lint", !found.has_errors(), data, Arc::new(found))
}

/// `explain(renderable, target=None, *, width=80)`: why the output looks the
/// way it does on `target` (wrapping, truncation, colour downgrades,
/// fidelity, unicode fallback).
#[pyfunction]
#[pyo3(signature = (renderable, target=None, *, width=80))]
fn qa_explain(
    py: Python<'_>,
    renderable: &Bound<'_, PyAny>,
    target: Option<PyRef<'_, RenderTarget>>,
    width: usize,
) -> PyResult<Py<QaReport>> {
    let target = target_or(target, width);
    let width = target.capabilities().width;
    let found = with_renderable(py, renderable, width, |value| {
        Ok(rich_ext::qa::explain::explain(value, &target))
    })?;
    let data = to_py_data(py, &found)?;
    report(py, "explain", true, data, Arc::new(OwnedExplanation(found)))
}

/// `profile(renderable, *, width=80, iterations=20, warmup=2, frame=None)`:
/// measure and render timings, output size and (with `frame=(width,
/// height)`) the cost of one frame.
#[pyfunction]
#[pyo3(signature = (renderable, *, width=80, iterations=20, warmup=2, frame=None))]
fn qa_profile(
    py: Python<'_>,
    renderable: &Bound<'_, PyAny>,
    width: usize,
    iterations: usize,
    warmup: usize,
    frame: Option<(usize, usize)>,
) -> PyResult<Py<QaReport>> {
    let mut options = rich_ext::qa::profile::ProfileOptions::default()
        .iterations(iterations)
        .width(width);
    options.warmup = warmup;
    if let Some((w, h)) = frame {
        options = options.frame(w, h);
    }
    let console = common::plain_console(width.max(1), true);
    let found = with_renderable(py, renderable, width, |value| {
        Ok(rich_ext::qa::profile::profile(value, &console, &options))
    })?;
    let data = to_py_data(py, &found)?;
    report(py, "profile", true, data, Arc::new(OwnedProfile(found)))
}

/// `fuzz(seed=0, cases=100)`: render seeded random text, tables, columns and
/// trees and check that none panics, overflows, renders differently twice
/// or breaks its measurement. Failures carry a minimised Rust
/// reproduction.
#[pyfunction]
#[pyo3(signature = (seed=0, cases=100))]
fn qa_fuzz(py: Python<'_>, seed: u64, cases: u64) -> PyResult<Py<QaReport>> {
    let invariants = rich_ext::qa::fuzz::Invariants::default();
    let found = py.detach(|| rich_ext::qa::fuzz::fuzz(seed, cases, &invariants));
    let failures = PyList::empty(py);
    for failure in &found.failures {
        let entry = PyDict::new(py);
        entry.set_item("seed", failure.seed)?;
        entry.set_item("case_index", failure.case_index)?;
        entry.set_item("width", failure.width)?;
        entry.set_item("invariant", &failure.invariant)?;
        entry.set_item("description", &failure.description)?;
        entry.set_item("minimized", failure.minimized.as_deref())?;
        failures.append(entry)?;
    }
    let data = PyDict::new(py);
    data.set_item("seed", found.seed)?;
    data.set_item("cases", found.cases)?;
    data.set_item("failures", failures)?;
    let ok = found.is_clean();
    report(py, "fuzz", ok, data.into_any().unbind(), Arc::new(found))
}

thread_local! {
    /// The fixture a `qa.matrix` run is building (its factories are plain
    /// `fn`s, so the Python renderable is handed over here).
    static FIXTURE: std::cell::RefCell<Option<Py<PyAny>>> = const { std::cell::RefCell::new(None) };
}

/// The trampoline `qa.matrix` calls for the current fixture.
fn current_fixture() -> Box<dyn rich::Renderable> {
    Python::attach(|py| {
        let object = FIXTURE.with(|f| f.borrow().as_ref().map(|o| o.clone_ref(py)));
        let Some(object) = object else {
            return Box::new(rich::Text::new("")) as Box<dyn rich::Renderable>;
        };
        let object = object.bind(py);
        let value = if object.is_callable() && !renderable::is_renderable(object).unwrap_or(false) {
            object.call0()
        } else {
            Ok(object.clone())
        };
        match value.and_then(|value| renderable::to_renderable(&value, None)) {
            Ok(value) => value,
            Err(error) => {
                renderable::report_error(py, error);
                Box::new(rich::Text::new(""))
            }
        }
    })
}

/// Fixture names live as long as the process (`qa.matrix` takes
/// `&'static str`); each distinct name is kept once.
fn static_name(name: &str) -> &'static str {
    static NAMES: std::sync::Mutex<Vec<&'static str>> = std::sync::Mutex::new(Vec::new());
    let mut names = NAMES.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(found) = names.iter().find(|n| **n == name) {
        return found;
    }
    let leaked: &'static str = Box::leak(name.to_string().into_boxed_str());
    names.push(leaked);
    leaked
}

fn profile_named(name: &str) -> PyResult<rich_ext::qa::matrix::CapabilityProfile> {
    use rich_ext::qa::matrix::CapabilityProfile;
    CapabilityProfile::standard()
        .into_iter()
        .find(|p| p.name == name)
        .ok_or_else(|| {
            let names: Vec<String> = CapabilityProfile::standard()
                .into_iter()
                .map(|p| p.name)
                .collect();
            PyValueError::new_err(format!(
                "unknown capability profile {name:?}; expected one of {}",
                names.join(", ")
            ))
        })
}

/// `matrix(fixtures, *, profiles=None, width=80)`: render each fixture (a
/// `{name: renderable or zero-argument callable}` dict) under capability
/// profiles (names; default all the standard ones) and check the structure
/// (no colour codes where there is no colour, no links where there are
/// none, ASCII where there is no unicode).
#[pyfunction]
#[pyo3(signature = (fixtures, *, profiles=None, width=80))]
fn qa_matrix(
    py: Python<'_>,
    fixtures: &Bound<'_, PyDict>,
    profiles: Option<Vec<String>>,
    width: usize,
) -> PyResult<Py<QaReport>> {
    let profiles = match profiles {
        Some(names) => names
            .iter()
            .map(|n| profile_named(n))
            .collect::<PyResult<Vec<_>>>()?,
        None => rich_ext::qa::matrix::CapabilityProfile::standard(),
    };
    let mut merged = rich_ext::qa::matrix::MatrixReport {
        width,
        profiles: profiles.iter().map(|p| p.name.clone()).collect(),
        ..Default::default()
    };
    common::scoped(py, width, 10_000, false, || {
        for (name, object) in fixtures.iter() {
            let name: String = name.extract()?;
            FIXTURE.with(|f| *f.borrow_mut() = Some(object.unbind()));
            let fixture: rich_ext::qa::matrix::Fixture = (static_name(&name), current_fixture);
            let part = rich_ext::qa::matrix::regression(&[fixture], &profiles, width);
            FIXTURE.with(|f| *f.borrow_mut() = None);
            renderable::check_pending()?;
            merged.fixtures.extend(part.fixtures);
            merged.cells.extend(part.cells);
        }
        Ok(())
    })?;
    let cells = PyList::empty(py);
    for cell in &merged.cells {
        let entry = PyDict::new(py);
        entry.set_item("fixture", &cell.fixture)?;
        entry.set_item("profile", &cell.profile)?;
        let (status, reasons) = match &cell.status {
            rich_ext::qa::matrix::CellStatus::Pass => ("pass", Vec::new()),
            rich_ext::qa::matrix::CellStatus::Fail(reasons) => ("fail", reasons.clone()),
            rich_ext::qa::matrix::CellStatus::Mismatch => ("mismatch", Vec::new()),
            rich_ext::qa::matrix::CellStatus::Missing => ("missing", Vec::new()),
        };
        entry.set_item("status", status)?;
        entry.set_item("reasons", reasons)?;
        cells.append(entry)?;
    }
    let data = PyDict::new(py);
    data.set_item("width", merged.width)?;
    data.set_item("fixtures", merged.fixtures.clone())?;
    data.set_item("profiles", merged.profiles.clone())?;
    data.set_item("cells", cells)?;
    let ok = merged.is_ok();
    report(py, "matrix", ok, data.into_any().unbind(), Arc::new(merged))
}

/// `bench(name, renderable, *, width=80, warmup=0.1, target_time=1.0,
/// min_samples=10, max_samples=10000)`: time rendering (segments and ANSI
/// encoding). Returns the measurement (nanoseconds per render).
#[pyfunction]
#[pyo3(signature = (
    name, renderable, *, width=80, warmup=0.1, target_time=1.0, min_samples=10,
    max_samples=10000
))]
#[allow(clippy::too_many_arguments)]
fn qa_bench(
    py: Python<'_>,
    name: &str,
    renderable: &Bound<'_, PyAny>,
    width: usize,
    warmup: f64,
    target_time: f64,
    min_samples: usize,
    max_samples: usize,
) -> PyResult<Py<QaReport>> {
    let bench = rich_ext::qa::bench::Bench::new(name)
        .warmup(Duration::from_secs_f64(warmup.max(0.0)))
        .target_time(Duration::from_secs_f64(target_time.max(0.0)))
        .samples(min_samples, max_samples);
    let found = with_renderable(py, renderable, width, |value| {
        Ok(rich_ext::qa::bench::bench_renderable_with(
            &bench, value, width,
        ))
    })?;
    let data = to_py_data(py, &found)?;
    let summary = format!(
        "{}: mean {:.0} ns, median {:.0} ns, p95 {:.0} ns ({} samples)",
        found.name, found.mean, found.median, found.p95, found.samples
    );
    report(py, "bench", true, data, Arc::new(Lines(summary)))
}

fn register_testing(m: &Bound<'_, PyModule>) -> PyResult<()> {
    renderable::add_renderable_class::<QaReport>(m)?;
    m.add_function(pyo3::wrap_pyfunction!(render_snapshot, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(assert_render_eq, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(assert_str_eq, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(assert_json_eq, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(highlighter_conformance, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(qa_screenshot, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(qa_stress, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(qa_lint, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(qa_explain, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(qa_profile, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(qa_fuzz, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(qa_matrix, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(qa_bench, m)?)?;
    Ok(())
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(pyo3::wrap_pyfunction!(accessible_text, m)?)?;
    register_testing(m)
}
