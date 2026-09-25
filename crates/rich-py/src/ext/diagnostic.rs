//! `rs_rich.ext.diagnostic`, `.stacktrace`, `.dashboard`, `.hyperlink`,
//! `.event`, `.log_handler` and `.highlighter`: compiler-style diagnostics,
//! stack traces, hyperlinks, structured log events and their handler.
//!
//! Offsets are Python's: character indices into the source (Rust's are byte
//! ranges), converted here.

use std::collections::HashSet;

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList, PyTuple, PyType};

use rich::console::{Console as CoreConsole, ConsoleOptions as CoreOptions};
use rich::measure::Measurement as CoreMeasurement;
use rich::protocol::{Highlighter, Renderable};
use rich::segment::Segment as CoreSegment;
use rich_ext::dashboard::DiagnosticsDashboard as CoreDashboard;
use rich_ext::diagnostic::{
    Diagnostic as CoreDiagnostic, Level, Location as CoreLocation,
    SourceSnippet as CoreSnippet, Suggestion as CoreSuggestion,
};
use rich_ext::event::{
    EventContext, EventView, Message, Severity, SourceLocation, SpanContext, SpanEvent,
    StructuredEvent as CoreEvent,
};
use rich_ext::hyperlink::Hyperlinker as CoreHyperlinker;
use rich_ext::layout::OverflowPolicy;
use rich_ext::log_handler::{RichHandler, SpanView};
use rich_ext::stacktrace::{
    CauseKind, Frame as CoreFrame, Language, Parsers, StackTrace as CoreTrace, TraceParser,
};

use super::common::{self, names, DiagnosticSpanError};
use crate::renderable::{self, AsRenderable};
use crate::text::Text;

names!(level, level_name, Level, "level", {
    "error" => Level::Error,
    "warning" => Level::Warning,
    "info" => Level::Info,
    "note" => Level::Note,
    "help" => Level::Help,
});

names!(view, view_name, EventView, "view", {
    "compact" => EventView::Compact,
    "expanded" => EventView::Expanded,
});

names!(overflow_policy, overflow_policy_name, OverflowPolicy, "overflow", {
    "wrap" => OverflowPolicy::Wrap,
    "fold" => OverflowPolicy::Fold,
    "crop" => OverflowPolicy::Crop,
    "ellipsis" => OverflowPolicy::Ellipsis,
    "visible" => OverflowPolicy::Visible,
});

names!(severity, severity_name, Severity, "severity", {
    "trace" => Severity::Trace,
    "debug" => Severity::Debug,
    "info" => Severity::Info,
    "warn" => Severity::Warn,
    "warning" => Severity::Warn,
    "error" => Severity::Error,
    "fatal" => Severity::Fatal,
    "critical" => Severity::Fatal,
});

names!(span_view, span_view_name, SpanView, "span view", {
    "inline" => SpanView::Inline,
    "tree" => SpanView::Tree,
    "hidden" => SpanView::Hidden,
});

names!(cause_kind, cause_kind_name, CauseKind, "cause kind", {
    "caused_by" => CauseKind::CausedBy,
    "during_handling" => CauseKind::DuringHandling,
});

fn language(name: &str) -> Language {
    match name.to_ascii_lowercase().as_str() {
        "rust" => Language::Rust,
        "python" => Language::Python,
        "java" => Language::Java,
        "javascript" | "js" => Language::JavaScript,
        _ => Language::Other(name.to_string()),
    }
}

fn language_name(language: &Language) -> String {
    match language {
        Language::Rust => "rust".into(),
        Language::Python => "python".into(),
        Language::Java => "java".into(),
        Language::JavaScript => "javascript".into(),
        Language::Other(name) => name.clone(),
    }
}

// ---------------------------------------------------------------------------
// Hyperlinker

/// `Hyperlinker`: OSC 8 links for URLs, paths (`path:line:col`) and issue
/// references (`#123`), with editor URL templates.
#[pyclass(name = "Hyperlinker", module = "rs_rich.ext.hyperlink", skip_from_py_object)]
#[derive(Clone)]
pub(crate) struct Hyperlinker {
    pub(crate) inner: CoreHyperlinker,
}

/// A link [`Hyperlinker.find`] found: character offsets and the URL.
#[pyclass(name = "FoundLink", module = "rs_rich.ext.hyperlink", frozen, skip_from_py_object)]
#[derive(Clone)]
pub(crate) struct Link {
    #[pyo3(get)]
    start: usize,
    #[pyo3(get)]
    end: usize,
    #[pyo3(get)]
    url: String,
}

#[pymethods]
impl Link {
    fn __repr__(&self) -> String {
        format!("FoundLink(start={}, end={}, url={:?})", self.start, self.end, self.url)
    }

    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        other
            .extract::<PyRef<'_, Link>>()
            .is_ok_and(|o| o.start == self.start && o.end == self.end && o.url == self.url)
    }
}

pub(crate) fn hyperlinker_arg(value: Option<&Bound<'_, PyAny>>) -> PyResult<Option<CoreHyperlinker>> {
    let Some(value) = value.filter(|v| !v.is_none()) else {
        return Ok(None);
    };
    if let Ok(flag) = value.extract::<bool>() {
        return Ok(Some(if flag {
            CoreHyperlinker::new()
        } else {
            CoreHyperlinker::disabled()
        }));
    }
    Ok(Some(value.extract::<PyRef<'_, Hyperlinker>>()?.inner.clone()))
}

#[pymethods]
impl Hyperlinker {
    #[new]
    #[pyo3(signature = (*, enabled=true, urls=true, paths=true, base_dir=None, editor=None, repository=None))]
    fn new(
        enabled: bool,
        urls: bool,
        paths: bool,
        base_dir: Option<std::path::PathBuf>,
        editor: Option<String>,
        repository: Option<String>,
    ) -> Self {
        let mut inner = CoreHyperlinker::new()
            .enabled(enabled)
            .urls(urls)
            .paths(paths);
        if let Some(dir) = base_dir {
            inner = inner.base_dir(dir);
        }
        if let Some(template) = editor {
            inner = inner.editor(template);
        }
        if let Some(url) = repository {
            inner = inner.repository(url);
        }
        Hyperlinker { inner }
    }

    /// A hyperlinker that links nothing.
    #[staticmethod]
    fn disabled() -> Self {
        Hyperlinker {
            inner: CoreHyperlinker::disabled(),
        }
    }

    #[getter]
    fn enabled(&self) -> bool {
        self.inner.is_enabled()
    }

    /// The URL for `path` (and a line and column): the editor template
    /// filled in, or a `file://` URL. `None` when disabled.
    #[pyo3(signature = (path, line=None, column=None))]
    fn file_url(&self, path: &str, line: Option<usize>, column: Option<usize>) -> Option<String> {
        self.inner.file_url(path, line, column)
    }

    /// The issue URL for `#number` (or `owner/repo#number`) in the repository.
    #[pyo3(signature = (number, repo=None))]
    fn reference_url(&self, number: u64, repo: Option<&str>) -> Option<String> {
        self.inner.reference_url(repo, number)
    }

    /// Every link in `text`, with character offsets.
    fn find(&self, text: &str) -> Vec<Link> {
        self.inner
            .find(text)
            .into_iter()
            .map(|link| Link {
                start: common::char_index(text, link.start),
                end: common::char_index(text, link.end),
                url: link.url,
            })
            .collect()
    }

    /// Add link styles to a `Text`, in place (Rich's `Highlighter.highlight`).
    fn highlight(&self, mut text: PyRefMut<'_, Text>) {
        self.inner.link(&mut text.inner);
    }

    /// A new linked `Text` from a `str` or `Text` (Rich's `Highlighter.__call__`).
    fn __call__(&self, py: Python<'_>, text: &Bound<'_, PyAny>) -> PyResult<Py<Text>> {
        let mut text = common::text_arg(text)?;
        self.inner.link(&mut text);
        common::py_text(py, text)
    }

    /// `path:line:column` as a `Text` linked to the location.
    #[pyo3(signature = (path, line=None, column=None, style=""))]
    fn location(
        &self,
        py: Python<'_>,
        path: &str,
        line: Option<usize>,
        column: Option<usize>,
        style: &str,
    ) -> PyResult<Py<Text>> {
        common::py_text(py, self.inner.location(path, line, column, style))
    }

    fn __repr__(&self) -> String {
        format!("{:?}", self.inner)
    }
}

/// `NumberHighlighter`: styles numbers in text (the default extension).
#[pyclass(name = "NumberHighlighter", module = "rs_rich.ext.highlighter", frozen)]
pub(crate) struct NumberHighlighter;

#[pymethods]
impl NumberHighlighter {
    #[new]
    fn new() -> Self {
        NumberHighlighter
    }

    fn highlight(&self, mut text: PyRefMut<'_, Text>) {
        rich_ext::NumberHighlighter::new().highlight(&mut text.inner);
    }

    fn __call__(&self, py: Python<'_>, text: &Bound<'_, PyAny>) -> PyResult<Py<Text>> {
        let mut text = common::text_arg(text)?;
        rich_ext::NumberHighlighter::new().highlight(&mut text);
        common::py_text(py, text)
    }
}

// ---------------------------------------------------------------------------
// Locations, snippets and suggestions

/// `Location(path, line=None, column=None)`: `path:line:column`.
#[pyclass(name = "Location", module = "rs_rich.ext.diagnostic", frozen, skip_from_py_object)]
#[derive(Clone)]
pub(crate) struct Location {
    pub(crate) inner: CoreLocation,
}

#[pymethods]
impl Location {
    #[new]
    #[pyo3(signature = (path, line=None, column=None))]
    fn new(path: String, line: Option<usize>, column: Option<usize>) -> Self {
        Location {
            inner: CoreLocation::new(path, line, column),
        }
    }

    #[getter]
    fn path(&self) -> &str {
        &self.inner.path
    }

    #[getter]
    fn line(&self) -> Option<usize> {
        self.inner.line
    }

    #[getter]
    fn column(&self) -> Option<usize> {
        self.inner.column
    }

    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        other
            .extract::<PyRef<'_, Location>>()
            .is_ok_and(|o| o.inner == self.inner)
    }

    fn __hash__(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.inner.hash(&mut hasher);
        hasher.finish()
    }

    fn __repr__(&self) -> String {
        format!(
            "Location({:?}, {}, {})",
            self.inner.path,
            self.inner.line.map_or("None".into(), |v| v.to_string()),
            self.inner.column.map_or("None".into(), |v| v.to_string())
        )
    }
}

fn location_arg(value: &Bound<'_, PyAny>) -> PyResult<CoreLocation> {
    if let Ok(location) = value.extract::<PyRef<'_, Location>>() {
        return Ok(location.inner.clone());
    }
    if let Ok(path) = value.extract::<String>() {
        return Ok(CoreLocation::new(path, None, None));
    }
    let (path, line, column): (String, Option<usize>, Option<usize>) = match value.len()? {
        2 => {
            let (path, line) = value.extract::<(String, Option<usize>)>()?;
            (path, line, None)
        }
        _ => value.extract()?,
    };
    Ok(CoreLocation::new(path, line, column))
}

fn span_error(error: rich_ext::diagnostic::DiagnosticError) -> PyErr {
    DiagnosticSpanError::new_err(error.to_string())
}

/// `SourceSnippet(name, source, start, end, *, context_lines=1, label=None)`:
/// quoted source with `^^^` under characters `start..end`.
#[pyclass(name = "SourceSnippet", module = "rs_rich.ext.diagnostic", skip_from_py_object)]
#[derive(Clone)]
pub(crate) struct SourceSnippet {
    inner: CoreSnippet,
    source: String,
}

#[pymethods]
impl SourceSnippet {
    #[new]
    #[pyo3(signature = (name, source, start, end, *, context_lines=1, label=None))]
    fn new(
        name: String,
        source: String,
        start: isize,
        end: isize,
        context_lines: usize,
        label: Option<String>,
    ) -> PyResult<Self> {
        let span = common::byte_range(&source, start, end);
        let mut inner =
            CoreSnippet::new(name, source.clone(), span, context_lines).map_err(span_error)?;
        if let Some(label) = label {
            inner = inner.primary_label(label);
        }
        Ok(SourceSnippet { inner, source })
    }

    /// Mark another span `^^^` with a label. Returns the snippet.
    #[pyo3(signature = (start, end, label=""))]
    fn primary<'py>(
        mut slf: PyRefMut<'py, Self>,
        start: isize,
        end: isize,
        label: &str,
    ) -> PyResult<PyRefMut<'py, Self>> {
        let span = common::byte_range(&slf.source, start, end);
        slf.inner = slf.inner.clone().primary(span, label).map_err(span_error)?;
        Ok(slf)
    }

    /// Mark related code `---` with a label. Returns the snippet.
    #[pyo3(signature = (start, end, label=""))]
    fn secondary<'py>(
        mut slf: PyRefMut<'py, Self>,
        start: isize,
        end: isize,
        label: &str,
    ) -> PyResult<PyRefMut<'py, Self>> {
        let span = common::byte_range(&slf.source, start, end);
        slf.inner = slf.inner.clone().secondary(span, label).map_err(span_error)?;
        Ok(slf)
    }

    #[getter]
    fn name(&self) -> &str {
        self.inner.name()
    }

    /// Where the primary span starts (1-based line and character column).
    #[getter]
    fn location(&self) -> Location {
        Location {
            inner: self.inner.location(),
        }
    }
}

/// `Suggestion(message)`, or `Suggestion.replace(message, source, start,
/// end, replacement)` to show the edited line.
#[pyclass(name = "Suggestion", module = "rs_rich.ext.diagnostic", frozen, skip_from_py_object)]
#[derive(Clone)]
pub(crate) struct Suggestion {
    inner: CoreSuggestion,
}

#[pymethods]
impl Suggestion {
    #[new]
    fn new(message: String) -> Self {
        Suggestion {
            inner: CoreSuggestion::new(message),
        }
    }

    #[classmethod]
    fn replace(
        _cls: &Bound<'_, PyType>,
        message: String,
        source: &str,
        start: isize,
        end: isize,
        replacement: &str,
    ) -> PyResult<Self> {
        let span = common::byte_range(source, start, end);
        Ok(Suggestion {
            inner: CoreSuggestion::replace(message, source, span, replacement)
                .map_err(span_error)?,
        })
    }

    #[getter]
    fn message(&self) -> &str {
        self.inner.message()
    }
}

// ---------------------------------------------------------------------------
// Stack traces

/// One stack frame.
#[pyclass(name = "TraceFrame", module = "rs_rich.ext.stacktrace", skip_from_py_object)]
#[derive(Clone)]
pub(crate) struct Frame {
    inner: CoreFrame,
}

#[pymethods]
impl Frame {
    #[new]
    #[pyo3(signature = (*, function=None, path=None, line=None, column=None, source=None, metadata=None, library=false))]
    fn new(
        function: Option<String>,
        path: Option<String>,
        line: Option<usize>,
        column: Option<usize>,
        source: Option<String>,
        metadata: Option<&Bound<'_, PyAny>>,
        library: bool,
    ) -> PyResult<Self> {
        let metadata = match metadata {
            Some(value) => common::pairs(value)?
                .into_iter()
                .map(|(key, value)| Ok((key, value.str()?.to_string())))
                .collect::<PyResult<Vec<_>>>()?,
            None => Vec::new(),
        };
        Ok(Frame {
            inner: CoreFrame {
                function,
                path,
                line,
                column,
                source,
                metadata,
                library,
            },
        })
    }

    #[getter]
    fn function(&self) -> Option<String> {
        self.inner.function.clone()
    }
    #[getter]
    fn path(&self) -> Option<String> {
        self.inner.path.clone()
    }
    #[getter]
    fn line(&self) -> Option<usize> {
        self.inner.line
    }
    #[getter]
    fn column(&self) -> Option<usize> {
        self.inner.column
    }
    #[getter]
    fn source(&self) -> Option<String> {
        self.inner.source.clone()
    }
    #[getter]
    fn metadata(&self) -> Vec<(String, String)> {
        self.inner.metadata.clone()
    }
    #[getter]
    fn library(&self) -> bool {
        self.inner.library
    }

    fn __repr__(&self) -> String {
        format!(
            "TraceFrame(function={:?}, path={:?}, line={:?}, library={})",
            self.inner.function,
            self.inner.path,
            self.inner.line,
            if self.inner.library { "True" } else { "False" }
        )
    }
}

fn frame_arg(value: &Bound<'_, PyAny>) -> PyResult<CoreFrame> {
    Ok(value.extract::<PyRef<'_, Frame>>()?.inner.clone())
}

/// A normalised stack trace from Rust, Python, Java or JavaScript; renders
/// with its causes first and library frames collapsed.
#[pyclass(name = "StackTrace", module = "rs_rich.ext.stacktrace", skip_from_py_object)]
#[derive(Clone)]
pub(crate) struct StackTrace {
    pub(crate) inner: CoreTrace,
    linker: Option<CoreHyperlinker>,
    #[pyo3(get, set)]
    show_library: bool,
}

impl StackTrace {
    pub(crate) fn from_core(inner: CoreTrace) -> Self {
        StackTrace {
            inner,
            linker: None,
            show_library: false,
        }
    }
}

struct TraceView {
    trace: CoreTrace,
    linker: Option<CoreHyperlinker>,
    show_library: bool,
}

impl Renderable for TraceView {
    fn rich_render(&self, console: &CoreConsole, options: &CoreOptions) -> Vec<CoreSegment> {
        let mut view = self.trace.render_options().show_library(self.show_library);
        if let Some(linker) = &self.linker {
            view = view.hyperlinker(linker.clone());
        }
        view.rich_render(console, options)
    }

    fn measure(&self, console: &CoreConsole, options: &CoreOptions) -> CoreMeasurement {
        let mut view = self.trace.render_options().show_library(self.show_library);
        if let Some(linker) = &self.linker {
            view = view.hyperlinker(linker.clone());
        }
        view.measure(console, options)
    }
}

impl AsRenderable for StackTrace {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(TraceView {
            trace: self.inner.clone(),
            linker: self.linker.clone(),
            show_library: self.show_library,
        }))
    }
}

/// A trace parser written in Python: an object with `detect(text) -> bool`
/// and `parse(text) -> StackTrace | None`.
struct PyParser(Py<PyAny>);

impl TraceParser for PyParser {
    fn detect(&self, text: &str) -> bool {
        Python::attach(|py| {
            self.0
                .bind(py)
                .call_method1("detect", (text,))
                .and_then(|r| r.is_truthy())
                .unwrap_or_else(|error| {
                    error.write_unraisable(py, Some(self.0.bind(py)));
                    false
                })
        })
    }

    fn parse(&self, text: &str) -> Option<CoreTrace> {
        Python::attach(|py| {
            let result = self.0.bind(py).call_method1("parse", (text,));
            match result {
                Ok(value) if value.is_none() => None,
                Ok(value) => match value.extract::<PyRef<'_, StackTrace>>() {
                    Ok(trace) => Some(trace.inner.clone()),
                    Err(error) => {
                        PyErr::from(error).write_unraisable(py, Some(self.0.bind(py)));
                        None
                    }
                },
                Err(error) => {
                    error.write_unraisable(py, Some(self.0.bind(py)));
                    None
                }
            }
        })
    }
}

#[pymethods]
impl StackTrace {
    #[new]
    #[pyo3(signature = (language="python", *, kind=None, message=None, frames=None, cause=None, cause_kind="caused_by", location=None, omitted_causes=0, hyperlinker=None, show_library=false))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        language: &str,
        kind: Option<String>,
        message: Option<String>,
        frames: Option<&Bound<'_, PyAny>>,
        cause: Option<PyRef<'_, StackTrace>>,
        cause_kind: &str,
        location: Option<&Bound<'_, PyAny>>,
        omitted_causes: usize,
        hyperlinker: Option<&Bound<'_, PyAny>>,
        show_library: bool,
    ) -> PyResult<Self> {
        let mut inner = CoreTrace::new(self::language(language));
        inner.kind = kind;
        inner.message = message;
        if let Some(frames) = frames {
            for frame in frames.try_iter()? {
                inner.frames.push(frame_arg(&frame?)?);
            }
        }
        inner.cause = cause.map(|cause| Box::new(cause.inner.clone()));
        inner.cause_kind = self::cause_kind(cause_kind)?;
        inner.location = location.filter(|v| !v.is_none()).map(frame_arg).transpose()?;
        inner.omitted_causes = omitted_causes;
        Ok(StackTrace {
            inner,
            linker: common_linker(hyperlinker)?,
            show_library,
        })
    }

    /// Parse a Rust, Python, Java or JavaScript trace; `None` when no parser
    /// recognises it. `parsers` are tried first: objects with
    /// `detect(text)` and `parse(text)`.
    #[staticmethod]
    #[pyo3(signature = (text, *, parsers=None))]
    fn parse(text: &str, parsers: Option<&Bound<'_, PyAny>>) -> PyResult<Option<StackTrace>> {
        let mut all = Parsers::new();
        if let Some(parsers) = parsers {
            let custom: Vec<_> = parsers.try_iter()?.collect::<PyResult<Vec<_>>>()?;
            // `with_parser` puts each first, so add them last to first.
            for parser in custom.into_iter().rev() {
                all = all.with_parser(PyParser(parser.unbind()));
            }
        }
        Ok(all.parse(text).map(StackTrace::from_core))
    }

    /// The trace of a Python exception (with its `__cause__` and
    /// `__context__` chain), through `traceback.format_exception`.
    #[staticmethod]
    fn from_exception(exception: &Bound<'_, PyAny>) -> PyResult<Option<StackTrace>> {
        let py = exception.py();
        let lines = py
            .import("traceback")?
            .getattr("format_exception")?
            .call1((exception.get_type(), exception, exception.getattr("__traceback__")?))?;
        let text: String = PyTuple::new(py, [""])?
            .get_item(0)?
            .call_method1("join", (lines,))?
            .extract()?;
        Ok(rich_ext::stacktrace::parse(&text).map(StackTrace::from_core))
    }

    #[getter]
    fn language(&self) -> String {
        language_name(&self.inner.language)
    }
    #[getter]
    fn kind(&self) -> Option<String> {
        self.inner.kind.clone()
    }
    #[getter]
    fn message(&self) -> Option<String> {
        self.inner.message.clone()
    }
    #[getter]
    fn frames(&self) -> Vec<Frame> {
        self.inner
            .frames
            .iter()
            .map(|frame| Frame {
                inner: frame.clone(),
            })
            .collect()
    }
    #[getter]
    fn cause(&self) -> Option<StackTrace> {
        self.inner
            .cause
            .as_deref()
            .map(|cause| StackTrace::from_core(cause.clone()))
    }
    #[getter]
    fn cause_kind(&self) -> &'static str {
        cause_kind_name(self.inner.cause_kind)
    }
    #[getter]
    fn omitted_causes(&self) -> usize {
        self.inner.omitted_causes
    }
    #[getter]
    fn location(&self) -> Option<Frame> {
        self.inner.location.clone().map(|inner| Frame { inner })
    }

    /// The most recent application (non-library) frame: where to look first.
    #[getter]
    fn origin(&self) -> Option<Frame> {
        self.inner.origin().cloned().map(|inner| Frame { inner })
    }

    /// This trace, then its causes.
    fn chain(&self) -> Vec<StackTrace> {
        self.inner
            .chain()
            .map(|trace| {
                let mut copy = trace.clone();
                copy.cause = None;
                StackTrace::from_core(copy)
            })
            .collect()
    }

    #[getter]
    fn get_hyperlinker(&self) -> Option<Hyperlinker> {
        self.linker.clone().map(|inner| Hyperlinker { inner })
    }

    #[setter]
    fn set_hyperlinker(&mut self, value: Option<&Bound<'_, PyAny>>) -> PyResult<()> {
        self.linker = common_linker(value)?;
        Ok(())
    }

    fn __repr__(&self) -> String {
        format!(
            "<StackTrace {} {:?}: {:?}, {} frames>",
            language_name(&self.inner.language),
            self.inner.kind.as_deref().unwrap_or(""),
            self.inner.message.as_deref().unwrap_or(""),
            self.inner.frames.len()
        )
    }
}

fn common_linker(value: Option<&Bound<'_, PyAny>>) -> PyResult<Option<CoreHyperlinker>> {
    hyperlinker_arg(value)
}

/// `parse_stacktrace(text)`: `StackTrace.parse`.
#[pyfunction]
#[pyo3(signature = (text, *, parsers=None))]
fn parse_stacktrace(text: &str, parsers: Option<&Bound<'_, PyAny>>) -> PyResult<Option<StackTrace>> {
    StackTrace::parse(text, parsers)
}

// ---------------------------------------------------------------------------
// Diagnostic

/// `Diagnostic(message, *, level=None, code=None, ...)`: a compiler-style
/// error, `error[E0308]: mismatched types`, with location, causes, snippets,
/// notes, help, suggestions and a stack trace.
#[pyclass(name = "Diagnostic", module = "rs_rich.ext.diagnostic", skip_from_py_object)]
#[derive(Clone)]
pub(crate) struct Diagnostic {
    pub(crate) inner: CoreDiagnostic,
    view: EventView,
    overflow: OverflowPolicy,
    linker: Option<CoreHyperlinker>,
}

impl Diagnostic {
    /// Wrap a `rich-ext` diagnostic (one a parser or a command record made).
    pub(crate) fn from_core(inner: CoreDiagnostic, view: EventView) -> Diagnostic {
        Diagnostic {
            inner,
            view,
            overflow: OverflowPolicy::Fold,
            linker: None,
        }
    }

    fn update(&mut self, f: impl FnOnce(CoreDiagnostic) -> CoreDiagnostic) {
        self.inner = f(self.inner.clone());
    }
}

pub(crate) fn diagnostic_arg(value: &Bound<'_, PyAny>) -> PyResult<CoreDiagnostic> {
    Ok(value.extract::<PyRef<'_, Diagnostic>>()?.inner.clone())
}

impl AsRenderable for Diagnostic {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(self.inner.clone()))
    }
}

/// A Python exception chain as a Rust error chain, for `Diagnostic::from_error`.
#[derive(Debug)]
struct ChainError {
    message: String,
    source: Option<Box<ChainError>>,
}

impl std::fmt::Display for ChainError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ChainError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source
            .as_deref()
            .map(|e| e as &(dyn std::error::Error + 'static))
    }
}

fn exception_message(exception: &Bound<'_, PyAny>) -> PyResult<String> {
    let text = exception.str()?.to_string();
    Ok(if text.is_empty() {
        exception.get_type().name()?.to_string()
    } else {
        text
    })
}

fn suggestion_arg(value: &Bound<'_, PyAny>) -> PyResult<CoreSuggestion> {
    Ok(match value.extract::<String>() {
        Ok(message) => CoreSuggestion::new(message),
        Err(_) => value.extract::<PyRef<'_, Suggestion>>()?.inner.clone(),
    })
}

#[pymethods]
impl Diagnostic {
    #[new]
    #[pyo3(signature = (
        message, *, level=None, code=None, code_url=None, location=None, causes=None, notes=None,
        help=None, suggestions=None, labels=None, snippets=None, metadata=None, trace=None,
        hyperlinker=None, view="compact", overflow="fold"
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        message: String,
        level: Option<&str>,
        code: Option<String>,
        code_url: Option<String>,
        location: Option<&Bound<'_, PyAny>>,
        causes: Option<&Bound<'_, PyAny>>,
        notes: Option<&Bound<'_, PyAny>>,
        help: Option<&Bound<'_, PyAny>>,
        suggestions: Option<&Bound<'_, PyAny>>,
        labels: Option<&Bound<'_, PyAny>>,
        snippets: Option<&Bound<'_, PyAny>>,
        metadata: Option<&Bound<'_, PyAny>>,
        trace: Option<PyRef<'_, StackTrace>>,
        hyperlinker: Option<&Bound<'_, PyAny>>,
        view: &str,
        overflow: &str,
    ) -> PyResult<Self> {
        let mut d = CoreDiagnostic::new(message);
        if let Some(level) = level {
            d = d.level(self::level(level)?);
        }
        if let Some(code) = code {
            d = d.code(code);
        }
        if let Some(url) = code_url {
            d = d.code_url(url);
        }
        if let Some(location) = location.filter(|v| !v.is_none()) {
            d = d.location(location_arg(location)?);
        }
        for cause in causes.map(common::strings).transpose()?.unwrap_or_default() {
            d = d.cause(cause);
        }
        for note in notes.map(common::strings).transpose()?.unwrap_or_default() {
            d = d.note(note);
        }
        for text in help.map(common::strings).transpose()?.unwrap_or_default() {
            d = d.help(text);
        }
        if let Some(items) = suggestions {
            for item in items.try_iter()? {
                d = d.suggestion(suggestion_arg(&item?)?);
            }
        }
        for label in labels.map(common::strings).transpose()?.unwrap_or_default() {
            d = d.label(label);
        }
        if let Some(items) = snippets {
            for item in items.try_iter()? {
                d = d.snippet(item?.extract::<PyRef<'_, SourceSnippet>>()?.inner.clone());
            }
        }
        if let Some(metadata) = metadata {
            for (key, value) in common::pairs(metadata)? {
                d = d.metadata(key, common::event_value(&value)?);
            }
        }
        if let Some(trace) = trace {
            d = d.trace(trace.inner.clone());
        }
        let linker = hyperlinker_arg(hyperlinker)?;
        if let Some(linker) = &linker {
            d = d.hyperlinker(linker.clone());
        }
        let view = self::view(view)?;
        let overflow = overflow_policy(overflow)?;
        Ok(Diagnostic {
            inner: d.view(view).overflow(overflow),
            view,
            overflow,
            linker,
        })
    }

    /// An error-level diagnostic.
    #[classmethod]
    #[pyo3(signature = (message, **kwargs))]
    fn error(
        cls: &Bound<'_, PyType>,
        message: String,
        kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Py<PyAny>> {
        with_level(cls, message, "error", kwargs)
    }

    /// A warning-level diagnostic.
    #[classmethod]
    #[pyo3(signature = (message, **kwargs))]
    fn warning(
        cls: &Bound<'_, PyType>,
        message: String,
        kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Py<PyAny>> {
        with_level(cls, message, "warning", kwargs)
    }

    /// A diagnostic from a Python exception: its message, then its
    /// `__cause__` / `__context__` chain as causes (at most `max_depth`,
    /// then `[truncated]`; a loop ends with `[cycle]`). `trace=True`
    /// attaches its stack trace (shown in the expanded view).
    #[classmethod]
    #[pyo3(signature = (exception, *, max_depth=16, level=Some("error"), trace=false, view="compact"))]
    fn from_exception(
        _cls: &Bound<'_, PyType>,
        exception: &Bound<'_, PyAny>,
        max_depth: usize,
        level: Option<&str>,
        trace: bool,
        view: &str,
    ) -> PyResult<Self> {
        let mut messages = Vec::new();
        let mut seen: HashSet<usize> = HashSet::new();
        let mut cycle = false;
        let mut current = Some(exception.clone());
        while let Some(error) = current {
            if !seen.insert(error.as_ptr() as usize) {
                cycle = true;
                break;
            }
            messages.push(exception_message(&error)?);
            if messages.len() > max_depth + 1 {
                break;
            }
            let cause = error.getattr("__cause__")?;
            current = if !cause.is_none() {
                Some(cause)
            } else if error.getattr("__suppress_context__")?.is_truthy()? {
                None
            } else {
                Some(error.getattr("__context__")?).filter(|c| !c.is_none())
            };
        }
        let mut chain: Option<Box<ChainError>> = None;
        for message in messages.into_iter().rev() {
            chain = Some(Box::new(ChainError {
                message,
                source: chain,
            }));
        }
        let chain = chain.expect("an exception has a message");
        let mut d = CoreDiagnostic::from_error(chain.as_ref(), max_depth);
        if cycle && d.causes().len() < max_depth {
            d = d.cause("[cycle]");
        }
        if let Some(level) = level {
            d = d.level(self::level(level)?);
        }
        if trace {
            if let Some(trace) = StackTrace::from_exception(exception)? {
                d = d.trace(trace.inner);
            }
        }
        let view = self::view(view)?;
        Ok(Diagnostic::from_core(d.view(view), view))
    }

    #[getter]
    fn message(&self) -> String {
        self.inner.message().to_string()
    }

    #[getter]
    fn get_level(&self) -> Option<&'static str> {
        self.inner.get_level().map(level_name)
    }

    #[setter]
    fn set_level(&mut self, value: &str) -> PyResult<()> {
        let level = level(value)?;
        self.update(|d| d.level(level));
        Ok(())
    }

    #[getter]
    fn get_code(&self) -> Option<String> {
        self.inner.get_code().map(str::to_string)
    }

    #[setter]
    fn set_code(&mut self, value: String) {
        self.update(|d| d.code(value));
    }

    #[getter]
    fn get_code_url(&self) -> Option<String> {
        self.inner.get_code_url().map(str::to_string)
    }

    #[setter]
    fn set_code_url(&mut self, value: String) {
        self.update(|d| d.code_url(value));
    }

    /// The location, or the first snippet's when none was set.
    #[getter]
    fn get_location(&self) -> Option<Location> {
        self.inner.get_location().map(|inner| Location { inner })
    }

    #[setter]
    fn set_location(&mut self, value: &Bound<'_, PyAny>) -> PyResult<()> {
        let location = location_arg(value)?;
        self.update(|d| d.location(location));
        Ok(())
    }

    #[getter]
    fn causes(&self) -> Vec<String> {
        self.inner.causes().to_vec()
    }

    #[getter]
    fn notes(&self) -> Vec<String> {
        self.inner.notes().to_vec()
    }

    #[getter]
    fn help(&self) -> Vec<String> {
        self.inner.help_messages().to_vec()
    }

    #[getter]
    fn labels(&self) -> Vec<String> {
        self.inner.labels().to_vec()
    }

    #[getter]
    fn suggestions(&self) -> Vec<Suggestion> {
        self.inner
            .suggestions()
            .iter()
            .map(|inner| Suggestion {
                inner: inner.clone(),
            })
            .collect()
    }

    #[getter]
    fn get_trace(&self) -> Option<StackTrace> {
        self.inner.get_trace().cloned().map(StackTrace::from_core)
    }

    #[setter]
    fn set_trace(&mut self, value: PyRef<'_, StackTrace>) {
        let trace = value.inner.clone();
        self.update(|d| d.trace(trace));
    }

    #[getter]
    fn get_view(&self) -> &'static str {
        view_name(self.view)
    }

    #[setter]
    fn set_view(&mut self, value: &str) -> PyResult<()> {
        let view = view(value)?;
        self.view = view;
        self.update(|d| d.view(view));
        Ok(())
    }

    #[getter]
    fn get_overflow(&self) -> &'static str {
        overflow_policy_name(self.overflow)
    }

    #[setter]
    fn set_overflow(&mut self, value: &str) -> PyResult<()> {
        let overflow = overflow_policy(value)?;
        self.overflow = overflow;
        self.update(|d| d.overflow(overflow));
        Ok(())
    }

    #[getter]
    fn get_hyperlinker(&self) -> Option<Hyperlinker> {
        self.linker.clone().map(|inner| Hyperlinker { inner })
    }

    #[setter]
    fn set_hyperlinker(&mut self, value: &Bound<'_, PyAny>) -> PyResult<()> {
        if let Some(linker) = hyperlinker_arg(Some(value))? {
            self.linker = Some(linker.clone());
            self.update(|d| d.hyperlinker(linker));
        }
        Ok(())
    }

    /// Add a `caused by:` line. Returns the diagnostic.
    fn add_cause(mut slf: PyRefMut<'_, Self>, message: String) -> PyRefMut<'_, Self> {
        slf.update(|d| d.cause(message));
        slf
    }

    /// Add a `note:` line. Returns the diagnostic.
    fn add_note(mut slf: PyRefMut<'_, Self>, message: String) -> PyRefMut<'_, Self> {
        slf.update(|d| d.note(message));
        slf
    }

    /// Add a `help:` line. Returns the diagnostic.
    fn add_help(mut slf: PyRefMut<'_, Self>, message: String) -> PyRefMut<'_, Self> {
        slf.update(|d| d.help(message));
        slf
    }

    /// Add a free-standing label line (expanded view). Returns the diagnostic.
    fn add_label(mut slf: PyRefMut<'_, Self>, message: String) -> PyRefMut<'_, Self> {
        slf.update(|d| d.label(message));
        slf
    }

    /// Add a suggestion (a `Suggestion` or a message). Returns the diagnostic.
    fn add_suggestion<'py>(
        mut slf: PyRefMut<'py, Self>,
        suggestion: &Bound<'py, PyAny>,
    ) -> PyResult<PyRefMut<'py, Self>> {
        let suggestion = suggestion_arg(suggestion)?;
        slf.update(|d| d.suggestion(suggestion));
        Ok(slf)
    }

    /// Add a source snippet. Returns the diagnostic.
    fn add_snippet<'py>(
        mut slf: PyRefMut<'py, Self>,
        snippet: PyRef<'py, SourceSnippet>,
    ) -> PyRefMut<'py, Self> {
        let snippet = snippet.inner.clone();
        slf.update(|d| d.snippet(snippet));
        slf
    }

    /// Add `key=value` metadata (expanded view). Returns the diagnostic.
    fn add_metadata<'py>(
        mut slf: PyRefMut<'py, Self>,
        key: String,
        value: &Bound<'py, PyAny>,
    ) -> PyResult<PyRefMut<'py, Self>> {
        let value = common::event_value(value)?;
        slf.update(|d| d.metadata(key, value));
        Ok(slf)
    }

    fn __repr__(&self) -> String {
        format!(
            "<Diagnostic {}{:?}>",
            self.inner
                .get_level()
                .map_or(String::new(), |l| format!("{} ", level_name(l))),
            self.inner.message()
        )
    }
}

fn with_level(
    cls: &Bound<'_, PyType>,
    message: String,
    level: &str,
    kwargs: Option<&Bound<'_, PyDict>>,
) -> PyResult<Py<PyAny>> {
    let kwargs = match kwargs {
        Some(kwargs) => kwargs.copy()?,
        None => PyDict::new(cls.py()),
    };
    if kwargs.contains("level")? {
        return Err(PyValueError::new_err(format!(
            "Diagnostic.{level}() sets the level; do not pass level="
        )));
    }
    kwargs.set_item("level", level)?;
    Ok(cls.call((message,), Some(&kwargs))?.unbind())
}


/// `DiagnosticsDashboard(diagnostics=(), *, min_level=None, top_codes=5,
/// hyperlinker=None)`: counts by level, the most frequent codes, then every
/// diagnostic grouped by file.
#[pyclass(name = "DiagnosticsDashboard", module = "rs_rich.ext.dashboard")]
pub(crate) struct DiagnosticsDashboard {
    diagnostics: Vec<CoreDiagnostic>,
    #[pyo3(get, set)]
    top_codes: usize,
    min_level: Option<Level>,
    linker: Option<CoreHyperlinker>,
}

impl DiagnosticsDashboard {
    fn build(&self) -> CoreDashboard {
        let mut dashboard = CoreDashboard::new().top_codes(self.top_codes);
        if let Some(level) = self.min_level {
            dashboard = dashboard.min_level(level);
        }
        if let Some(linker) = &self.linker {
            dashboard = dashboard.hyperlinker(linker.clone());
        }
        dashboard.extend(self.diagnostics.iter().cloned());
        dashboard
    }
}

impl AsRenderable for DiagnosticsDashboard {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(self.build()))
    }
}

#[pymethods]
impl DiagnosticsDashboard {
    #[new]
    #[pyo3(signature = (diagnostics=None, *, min_level=None, top_codes=5, hyperlinker=None))]
    fn new(
        diagnostics: Option<&Bound<'_, PyAny>>,
        min_level: Option<&str>,
        top_codes: usize,
        hyperlinker: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        let mut dashboard = DiagnosticsDashboard {
            diagnostics: Vec::new(),
            top_codes,
            min_level: min_level.map(level).transpose()?,
            linker: hyperlinker_arg(hyperlinker)?,
        };
        if let Some(items) = diagnostics {
            dashboard.extend(items)?;
        }
        Ok(dashboard)
    }

    /// Add one diagnostic.
    fn push(&mut self, diagnostic: &Bound<'_, PyAny>) -> PyResult<()> {
        self.diagnostics.push(diagnostic_arg(diagnostic)?);
        Ok(())
    }

    /// Add many diagnostics.
    fn extend(&mut self, diagnostics: &Bound<'_, PyAny>) -> PyResult<()> {
        for item in diagnostics.try_iter()? {
            self.diagnostics.push(diagnostic_arg(&item?)?);
        }
        Ok(())
    }

    #[getter]
    fn get_min_level(&self) -> Option<&'static str> {
        self.min_level.map(level_name)
    }

    #[setter]
    fn set_min_level(&mut self, value: Option<&str>) -> PyResult<()> {
        self.min_level = value.map(level).transpose()?;
        Ok(())
    }

    /// Diagnostics shown per level (`{"error": 2, ...}`); ones without a
    /// level count as errors.
    fn counts(&self) -> Vec<(&'static str, usize)> {
        self.build()
            .counts()
            .into_iter()
            .map(|(level, count)| (level_name(level), count))
            .collect()
    }

    fn __len__(&self) -> usize {
        self.diagnostics.len()
    }
}

// ---------------------------------------------------------------------------
// Structured events

/// `StructuredEvent(message, *, fields=None, ...)`: a typed log record that
/// renders compact (`message key=value`) or expanded.
#[pyclass(name = "StructuredEvent", module = "rs_rich.ext.event", skip_from_py_object)]
#[derive(Clone)]
pub(crate) struct StructuredEvent {
    pub(crate) inner: CoreEvent,
}

fn span_arg(value: &Bound<'_, PyAny>) -> PyResult<SpanContext> {
    if let Ok(name) = value.extract::<String>() {
        return Ok(SpanContext::new(name));
    }
    let (name, fields): (String, Bound<'_, PyAny>) = value.extract()?;
    let mut span = SpanContext::new(name);
    for (key, value) in common::pairs(&fields)? {
        span = span.field(key, common::event_value(&value)?);
    }
    Ok(span)
}

#[pymethods]
impl StructuredEvent {
    #[new]
    #[pyo3(signature = (
        message, *, markup=false, fields=None, field_order=None, hide_fields=None, view="compact",
        overflow="fold", timestamp=None, severity=None, target=None, module=None, path=None,
        line=None, column=None, thread=None, task=None, correlation_id=None, spans=None,
        span_event=None, elapsed=None, diagnostics=None
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        message: String,
        markup: bool,
        fields: Option<&Bound<'_, PyAny>>,
        field_order: Option<&Bound<'_, PyAny>>,
        hide_fields: Option<&Bound<'_, PyAny>>,
        view: &str,
        overflow: &str,
        timestamp: Option<String>,
        severity: Option<&str>,
        target: Option<String>,
        module: Option<String>,
        path: Option<String>,
        line: Option<usize>,
        column: Option<usize>,
        thread: Option<String>,
        task: Option<String>,
        correlation_id: Option<String>,
        spans: Option<&Bound<'_, PyAny>>,
        span_event: Option<&str>,
        elapsed: Option<&Bound<'_, PyAny>>,
        diagnostics: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        let message = if markup {
            Message::Markup(message)
        } else {
            Message::Literal(message)
        };
        let mut event = CoreEvent::new(message);
        if let Some(fields) = fields {
            for (key, value) in common::pairs(fields)? {
                event = event.field(key, common::event_value(&value)?);
            }
        }
        if let Some(order) = field_order {
            event = event.field_order(common::strings(order)?);
        }
        if let Some(hidden) = hide_fields {
            event = event.hide_fields(common::strings(hidden)?);
        }
        let source = match path {
            Some(path) => Some(SourceLocation {
                path,
                line: line.unwrap_or(0),
                column,
            }),
            None => None,
        };
        event = event
            .view(self::view(view)?)
            .overflow(overflow_policy(overflow)?)
            .context(EventContext {
                timestamp,
                severity: severity.map(self::severity).transpose()?,
                target,
                module,
                source,
                thread,
                task,
                correlation_id,
            });
        if let Some(spans) = spans {
            let spans: PyResult<Vec<_>> = spans.try_iter()?.map(|s| span_arg(&s?)).collect();
            event = event.spans(spans?);
        }
        match span_event {
            None => {}
            Some("open") => event = event.span_event(SpanEvent::Open),
            Some("close") => {
                event = event.span_event(SpanEvent::Close {
                    elapsed: common::opt_seconds(elapsed)?.unwrap_or_default(),
                })
            }
            Some(other) => {
                return Err(PyValueError::new_err(format!(
                    "invalid span_event {other:?}; expected open or close"
                )))
            }
        }
        if let Some(items) = diagnostics {
            for item in items.try_iter()? {
                event = event.diagnostic(diagnostic_arg(&item?)?);
            }
        }
        Ok(StructuredEvent { inner: event })
    }

    #[getter]
    fn message(&self) -> String {
        match &self.inner.message {
            Message::Literal(text) | Message::Markup(text) => text.clone(),
        }
    }

    #[getter]
    fn fields<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let dict = PyDict::new(py);
        for (key, value) in &self.inner.fields {
            dict.set_item(key, common::event_value_to_py(py, value)?)?;
        }
        Ok(dict)
    }

    #[getter]
    fn severity(&self) -> Option<&'static str> {
        self.inner.context.severity.map(severity_name)
    }

    /// Set a field (replacing one of the same name). Returns the event.
    fn field<'py>(
        mut slf: PyRefMut<'py, Self>,
        key: String,
        value: &Bound<'py, PyAny>,
    ) -> PyResult<PyRefMut<'py, Self>> {
        let value = common::event_value(value)?;
        slf.inner = slf.inner.clone().field(key, value);
        Ok(slf)
    }

    /// Attach a diagnostic, shown under the message. Returns the event.
    fn add_diagnostic<'py>(
        mut slf: PyRefMut<'py, Self>,
        diagnostic: &Bound<'py, PyAny>,
    ) -> PyResult<PyRefMut<'py, Self>> {
        let diagnostic = diagnostic_arg(diagnostic)?;
        slf.inner = slf.inner.clone().diagnostic(diagnostic);
        Ok(slf)
    }
}

impl AsRenderable for StructuredEvent {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(self.inner.clone()))
    }
}

fn event_arg(value: &Bound<'_, PyAny>) -> PyResult<CoreEvent> {
    Ok(value.extract::<PyRef<'_, StructuredEvent>>()?.inner.clone())
}

/// `EventHandler(...)`: lays out `StructuredEvent`s like Rich's logging
/// `RichHandler` (time, level, message, path), with spans. One handler
/// remembers the last time it showed, to blank repeats.
#[pyclass(name = "EventHandler", module = "rs_rich.ext.log_handler", frozen)]
pub(crate) struct EventHandler {
    handler: std::sync::Arc<RichHandler>,
}

/// A laid-out event: `EventHandler.render(event)`'s result.
#[pyclass(name = "HandledEvent", module = "rs_rich.ext.log_handler", frozen)]
pub(crate) struct PyHandledEvent {
    table: std::sync::Arc<std::sync::Mutex<rich::Table>>,
}

struct SharedTable(std::sync::Arc<std::sync::Mutex<rich::Table>>);

impl Renderable for SharedTable {
    fn rich_render(&self, console: &CoreConsole, options: &CoreOptions) -> Vec<CoreSegment> {
        let table = self.0.lock().unwrap_or_else(|e| e.into_inner());
        table.rich_render(console, options)
    }

    fn measure(&self, console: &CoreConsole, options: &CoreOptions) -> CoreMeasurement {
        let table = self.0.lock().unwrap_or_else(|e| e.into_inner());
        table.measure(console, options)
    }
}

impl AsRenderable for PyHandledEvent {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(SharedTable(self.table.clone())))
    }
}

#[pymethods]
impl EventHandler {
    #[new]
    #[pyo3(signature = (
        *, show_time=true, show_level=true, show_path=true, omit_repeated_times=true,
        level_width=Some(8), markup=false, highlight=true, keywords=None, enable_link_path=true,
        span_view="inline", time=None
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        show_time: bool,
        show_level: bool,
        show_path: bool,
        omit_repeated_times: bool,
        level_width: Option<usize>,
        markup: bool,
        highlight: bool,
        keywords: Option<&Bound<'_, PyAny>>,
        enable_link_path: bool,
        span_view: &str,
        time: Option<Py<PyAny>>,
    ) -> PyResult<Self> {
        let mut handler = RichHandler::new(CoreConsole::builder().build())
            .show_time(show_time)
            .show_level(show_level)
            .show_path(show_path)
            .omit_repeated_times(omit_repeated_times)
            .level_width(level_width)
            .markup(markup)
            .enable_link_path(enable_link_path)
            .span_view(self::span_view(span_view)?);
        if !highlight {
            handler = handler.highlighter(None);
        }
        if let Some(keywords) = keywords {
            handler = handler.keywords(common::strings(keywords)?);
        }
        if let Some(time) = time {
            // A fixed string, or a callable returning one for each event.
            handler = handler.time_format(move || {
                Python::attach(|py| {
                    let time = time.bind(py);
                    let value = if time.is_callable() {
                        time.call0()
                    } else {
                        Ok(time.clone())
                    };
                    value
                        .and_then(|v| v.str().map(|s| s.to_string()))
                        .unwrap_or_default()
                })
            });
        }
        Ok(EventHandler {
            handler: std::sync::Arc::new(handler),
        })
    }

    /// The event laid out as a renderable (a grid, as Rich's handler makes).
    fn render(&self, event: &Bound<'_, PyAny>) -> PyResult<PyHandledEvent> {
        let event = event_arg(event)?;
        Ok(PyHandledEvent {
            table: std::sync::Arc::new(std::sync::Mutex::new(self.handler.render(&event))),
        })
    }

    /// The message column as a `Text`.
    fn render_message(&self, py: Python<'_>, event: &Bound<'_, PyAny>) -> PyResult<Py<Text>> {
        let event = event_arg(event)?;
        common::py_text(py, self.handler.render_message(&event))
    }

    /// Print the event to `console`.
    fn emit(
        &self,
        console: &Bound<'_, crate::console::Console>,
        event: &Bound<'_, PyAny>,
    ) -> PyResult<()> {
        let table = self.render(event)?.table;
        crate::console::Console::print_core(console, Box::new(SharedTable(table)))
    }

    /// The level column for a severity (`"INFO    "` in `logging.level.info`).
    #[staticmethod]
    fn level_text(py: Python<'_>, severity: &str) -> PyResult<Py<Text>> {
        common::py_text(py, RichHandler::level_text(self::severity(severity)?))
    }
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Hyperlinker>()?;
    m.add_class::<Link>()?;
    m.add_class::<NumberHighlighter>()?;
    m.add_class::<Location>()?;
    m.add_class::<SourceSnippet>()?;
    m.add_class::<Suggestion>()?;
    m.add_class::<Frame>()?;
    renderable::add_renderable_class::<StackTrace>(m)?;
    m.add_function(pyo3::wrap_pyfunction!(parse_stacktrace, m)?)?;
    renderable::add_renderable_class::<Diagnostic>(m)?;
    renderable::add_renderable_class::<DiagnosticsDashboard>(m)?;
    renderable::add_renderable_class::<StructuredEvent>(m)?;
    m.add_class::<EventHandler>()?;
    renderable::add_renderable_class::<PyHandledEvent>(m)?;
    m.add("EVENT_KEYWORDS", rich_ext::log_handler::KEYWORDS.to_vec())?;
    m.add("MAX_TRACE_CAUSES", rich_ext::stacktrace::MAX_CAUSES)?;
    Ok(())
}
