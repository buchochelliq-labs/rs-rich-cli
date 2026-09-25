//! Code highlighters: `CodeHighlighter` (the base a Python engine subclasses,
//! and the handle of a Rust one), the `HighlightedCode` it returns, and the
//! adapter that lets Rust call a Python engine.
//!
//! Python offsets are *character* indices into each line; Rust's are byte
//! ranges. The adapter converts, then validates the result as core's
//! `Syntax` does (see [`validate`]): out-of-range, overlapping or reversed
//! spans are dropped, missing lines are unstyled, and styles lose their
//! links. A Python engine raises `UnknownThemeError(theme)` for a theme it
//! does not have; any other exception is an engine failure.

use std::path::Path;
use std::sync::Arc;

use pyo3::exceptions::{PyNotImplementedError, PyTypeError};
use pyo3::prelude::*;
use pyo3::types::{PyString, PyTuple};

use rich::color::Color as CoreColor;
use rich::protocol::{
    CodeHighlighter as CoreCodeHighlighter, HighlightError as CoreHighlightError,
    HighlightSpan as CoreSpan, HighlightedCode as CoreCode, HighlightedLine as CoreLine,
};
use rich::Style as CoreStyle;

use super::errors::{self, callback_failed, UnknownThemeError};
use crate::style::{resolved_style, Style};

// ---------------------------------------------------------------------------
// The data: spans, lines and the whole result

/// One styled run of a line: characters `start..end` (Python indices).
#[pyclass(
    name = "HighlightSpan",
    module = "rs_rich.plugins",
    frozen,
    skip_from_py_object
)]
#[derive(Clone)]
pub(crate) struct HighlightSpan {
    start: usize,
    end: usize,
    style: CoreStyle,
}

#[pymethods]
impl HighlightSpan {
    #[new]
    fn new(start: usize, end: usize, style: &Bound<'_, PyAny>) -> PyResult<Self> {
        Ok(HighlightSpan {
            start,
            end,
            style: resolved_style(Some(style))?.unwrap_or_default(),
        })
    }

    #[getter]
    fn start(&self) -> usize {
        self.start
    }

    #[getter]
    fn end(&self) -> usize {
        self.end
    }

    #[getter]
    fn style(&self) -> Style {
        Style::from_core(self.style.clone())
    }

    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        other
            .extract::<PyRef<'_, HighlightSpan>>()
            .is_ok_and(|o| o.start == self.start && o.end == self.end && o.style == self.style)
    }

    fn __repr__(&self) -> String {
        format!(
            "HighlightSpan({}, {}, {:?})",
            self.start,
            self.end,
            if self.style.is_null() {
                String::new()
            } else {
                self.style.definition()
            }
        )
    }
}

/// The spans of one line, and the style of the line break after it.
#[pyclass(
    name = "HighlightedLine",
    module = "rs_rich.plugins",
    frozen,
    skip_from_py_object
)]
#[derive(Clone, Default)]
pub(crate) struct HighlightedLine {
    spans: Vec<HighlightSpan>,
    newline_style: Option<CoreStyle>,
}

#[pymethods]
impl HighlightedLine {
    #[new]
    #[pyo3(signature = (spans=None, newline_style=None))]
    fn new(
        spans: Option<&Bound<'_, PyAny>>,
        newline_style: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        let mut line = HighlightedLine {
            spans: Vec::new(),
            newline_style: resolved_style(newline_style)?,
        };
        if let Some(spans) = spans {
            for span in spans.try_iter()? {
                line.spans.push(span_arg(&span?)?);
            }
        }
        Ok(line)
    }

    #[getter]
    fn spans(&self) -> Vec<HighlightSpan> {
        self.spans.clone()
    }

    #[getter]
    fn newline_style(&self) -> Option<Style> {
        self.newline_style.clone().map(Style::from_core)
    }

    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        other
            .extract::<PyRef<'_, HighlightedLine>>()
            .is_ok_and(|o| {
                o.newline_style == self.newline_style
                    && o.spans.len() == self.spans.len()
                    && o.spans
                        .iter()
                        .zip(&self.spans)
                        .all(|(a, b)| a.start == b.start && a.end == b.end && a.style == b.style)
            })
    }

    fn __repr__(&self) -> String {
        let spans: Vec<String> = self.spans.iter().map(HighlightSpan::__repr__).collect();
        format!("HighlightedLine([{}])", spans.join(", "))
    }
}

/// What a code highlighter returns: one line per element of
/// `code.split("\n")`, the theme's background colour (if it has one) and
/// the style for text no span covers.
#[pyclass(
    name = "HighlightedCode",
    module = "rs_rich.plugins",
    frozen,
    skip_from_py_object
)]
#[derive(Clone, Default)]
pub(crate) struct HighlightedCode {
    lines: Vec<HighlightedLine>,
    background: Option<CoreColor>,
    default_style: CoreStyle,
}

#[pymethods]
impl HighlightedCode {
    #[new]
    #[pyo3(signature = (lines=None, background=None, default_style=None))]
    fn new(
        lines: Option<&Bound<'_, PyAny>>,
        background: Option<&str>,
        default_style: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        let mut code = HighlightedCode {
            lines: Vec::new(),
            background: background.map(parse_color).transpose()?,
            default_style: resolved_style(default_style)?.unwrap_or_default(),
        };
        if let Some(lines) = lines {
            for line in lines.try_iter()? {
                code.lines.push(line_arg(&line?)?);
            }
        }
        Ok(code)
    }

    #[getter]
    fn lines(&self) -> Vec<HighlightedLine> {
        self.lines.clone()
    }

    /// The background colour's name, as it was given (`"#272822"`).
    #[getter]
    fn background(&self) -> Option<String> {
        self.background.as_ref().map(|c| c.name.clone())
    }

    #[getter]
    fn default_style(&self) -> Style {
        Style::from_core(self.default_style.clone())
    }

    fn __len__(&self) -> usize {
        self.lines.len()
    }

    fn __eq__(&self, other: &Bound<'_, PyAny>) -> PyResult<bool> {
        let Ok(other) = other.extract::<PyRef<'_, HighlightedCode>>() else {
            return Ok(false);
        };
        Ok(other.background == self.background
            && other.default_style == self.default_style
            && other.lines.len() == self.lines.len()
            && other.lines.iter().zip(&self.lines).all(|(a, b)| {
                a.newline_style == b.newline_style
                    && a.spans.len() == b.spans.len()
                    && a.spans
                        .iter()
                        .zip(&b.spans)
                        .all(|(x, y)| x.start == y.start && x.end == y.end && x.style == y.style)
            }))
    }

    fn __repr__(&self) -> String {
        format!("<HighlightedCode lines={}>", self.lines.len())
    }
}

fn parse_color(name: &str) -> PyResult<CoreColor> {
    CoreColor::parse(name).map_err(|e| crate::errors::StyleSyntaxError::new_err(e.to_string()))
}

/// A span: a `HighlightSpan` or a `(start, end, style)` tuple.
fn span_arg(value: &Bound<'_, PyAny>) -> PyResult<HighlightSpan> {
    if let Ok(span) = value.extract::<PyRef<'_, HighlightSpan>>() {
        return Ok(span.clone());
    }
    let tuple = value.cast::<PyTuple>().map_err(|_| {
        PyTypeError::new_err("a span must be a HighlightSpan or a (start, end, style) tuple")
    })?;
    if tuple.len() != 3 {
        return Err(PyTypeError::new_err(
            "a span tuple must be (start, end, style)",
        ));
    }
    // Negative offsets are invalid spans, dropped by validation like
    // out-of-range ones: map them to an empty range.
    let offset = |index: usize| -> PyResult<Option<usize>> {
        let value: i64 = tuple.get_item(index)?.extract()?;
        Ok(usize::try_from(value).ok())
    };
    let (start, end) = match (offset(0)?, offset(1)?) {
        (Some(start), Some(end)) => (start, end),
        _ => (0, 0),
    };
    let style = tuple.get_item(2)?;
    Ok(HighlightSpan {
        start,
        end,
        style: resolved_style(Some(&style))?.unwrap_or_default(),
    })
}

/// A line: a `HighlightedLine` or an iterable of spans.
fn line_arg(value: &Bound<'_, PyAny>) -> PyResult<HighlightedLine> {
    if let Ok(line) = value.extract::<PyRef<'_, HighlightedLine>>() {
        return Ok(line.clone());
    }
    if value.is_instance_of::<PyString>() {
        return Err(PyTypeError::new_err(
            "a line must be a HighlightedLine or a list of spans",
        ));
    }
    let mut line = HighlightedLine::default();
    for span in value.try_iter()? {
        line.spans.push(span_arg(&span?)?);
    }
    Ok(line)
}

/// A highlighter's result: a `HighlightedCode` or an iterable of lines.
fn code_arg(value: &Bound<'_, PyAny>) -> PyResult<HighlightedCode> {
    if let Ok(code) = value.extract::<PyRef<'_, HighlightedCode>>() {
        return Ok(code.clone());
    }
    let mut code = HighlightedCode::default();
    for line in value.try_iter()? {
        code.lines.push(line_arg(&line?)?);
    }
    Ok(code)
}

// ---------------------------------------------------------------------------
// Character offsets and byte ranges

/// The byte offset of character `index` in `line`, or `None` past its end.
fn byte_offset(line: &str, index: usize) -> Option<usize> {
    if index == 0 {
        return Some(0);
    }
    match line.char_indices().nth(index) {
        Some((offset, _)) => Some(offset),
        None if line.chars().count() == index => Some(line.len()),
        None => None,
    }
}

/// A Python result as Rust's, against the source it highlights.
fn to_core(code: &str, highlighted: HighlightedCode) -> CoreCode {
    let sources: Vec<&str> = code.split('\n').collect();
    let lines = highlighted
        .lines
        .into_iter()
        .enumerate()
        .map(|(index, line)| {
            let source = sources.get(index).copied().unwrap_or("");
            CoreLine {
                spans: line
                    .spans
                    .into_iter()
                    .map(|span| {
                        // A range past the line becomes one validation drops.
                        let range = match (
                            byte_offset(source, span.start),
                            byte_offset(source, span.end),
                        ) {
                            (Some(start), Some(end)) => start..end,
                            _ => usize::MAX..usize::MAX,
                        };
                        CoreSpan {
                            range,
                            style: span.style,
                        }
                    })
                    .collect(),
                newline_style: line.newline_style,
            }
        })
        .collect();
    CoreCode {
        lines,
        background: highlighted.background,
        default_style: highlighted.default_style,
    }
}

/// A Rust result for Python, with character offsets.
fn from_core(code: &str, highlighted: &CoreCode) -> HighlightedCode {
    let sources: Vec<&str> = code.split('\n').collect();
    let lines = highlighted
        .lines
        .iter()
        .enumerate()
        .map(|(index, line)| {
            let source = sources.get(index).copied().unwrap_or("");
            let chars = |byte: usize| source.get(..byte).map_or(0, |s| s.chars().count());
            HighlightedLine {
                spans: line
                    .spans
                    .iter()
                    .map(|span| HighlightSpan {
                        start: chars(span.range.start),
                        end: chars(span.range.end),
                        style: span.style.clone(),
                    })
                    .collect(),
                newline_style: line.newline_style.clone(),
            }
        })
        .collect();
    HighlightedCode {
        lines,
        background: highlighted.background.clone(),
        default_style: highlighted.default_style.clone(),
    }
}

/// Core's `Syntax` validation (`rich::syntax::validate`, which is private),
/// so a result read from Python is as safe as one core renders: one line per
/// `code.split('\n')` element; spans non-empty, sorted, non-overlapping,
/// inside their line and on character boundaries; no hyperlinks.
pub(crate) fn validate(code: &str, mut highlighted: CoreCode) -> CoreCode {
    let sources: Vec<&str> = code.split('\n').collect();
    highlighted
        .lines
        .resize_with(sources.len(), CoreLine::default);
    for (line, source) in highlighted.lines.iter_mut().zip(&sources) {
        let mut end = 0usize;
        line.spans.retain(|span| {
            let keep = span.range.start < span.range.end
                && span.range.start >= end
                && span.range.end <= source.len()
                && source.is_char_boundary(span.range.start)
                && source.is_char_boundary(span.range.end);
            if keep {
                end = span.range.end;
            }
            keep
        });
        for span in &mut line.spans {
            span.style = span.style.update_link(None);
        }
        line.newline_style = line.newline_style.as_ref().map(|s| s.update_link(None));
    }
    highlighted.default_style = highlighted.default_style.update_link(None);
    highlighted
}

// ---------------------------------------------------------------------------
// The Python-facing class

/// A syntax-highlighting engine for `Syntax` and Markdown code blocks.
///
/// Subclass it (or write any class with the same methods) to write an
/// engine in Python: `highlight(code, language, theme)` returns one list of
/// `(start, end, style)` spans per line of `code.split("\n")` (or a
/// `HighlightedCode`), `default_theme()` and `themes()` name the themes, and
/// `languages()` and `language_for_path(path)` are optional. The instances
/// `ExtensionRegistry.code_highlighter(name)` returns wrap a Rust engine.
#[pyclass(
    name = "CodeHighlighter",
    module = "rs_rich.plugins",
    subclass,
    frozen,
    skip_from_py_object
)]
pub(crate) struct CodeHighlighter {
    pub(crate) inner: Option<Arc<dyn CoreCodeHighlighter>>,
}

impl CodeHighlighter {
    pub(crate) fn wrap(inner: Arc<dyn CoreCodeHighlighter>) -> CodeHighlighter {
        CodeHighlighter { inner: Some(inner) }
    }

    fn engine(&self) -> PyResult<&Arc<dyn CoreCodeHighlighter>> {
        self.inner.as_ref().ok_or_else(|| {
            PyNotImplementedError::new_err(
                "a CodeHighlighter subclass must implement highlight, default_theme and themes",
            )
        })
    }
}

#[pymethods]
impl CodeHighlighter {
    #[new]
    #[pyo3(signature = (*_args, **_kwargs))]
    fn new(_args: &Bound<'_, PyTuple>, _kwargs: Option<&Bound<'_, pyo3::types::PyDict>>) -> Self {
        CodeHighlighter { inner: None }
    }

    /// Highlight `code` (`language`: a name or extension, `None` for plain
    /// text; `theme`: one of `themes()`, `None` for the default). The result
    /// is validated against `code`.
    #[pyo3(signature = (code, language=None, theme=None))]
    fn highlight(
        &self,
        py: Python<'_>,
        code: &str,
        language: Option<&str>,
        theme: Option<&str>,
    ) -> PyResult<HighlightedCode> {
        let engine = self.engine()?.clone();
        let theme = theme.map_or_else(|| engine.default_theme().to_string(), str::to_string);
        let result = errors::direct(|| engine.highlight(code, language, &theme));
        match result {
            Ok(highlighted) => Ok(from_core(code, &validate(code, highlighted))),
            Err(error) => Err(errors::highlight_error(py, &error)),
        }
    }

    fn default_theme(&self) -> PyResult<String> {
        Ok(self.engine()?.default_theme().to_string())
    }

    fn themes(&self) -> PyResult<Vec<String>> {
        let engine = self.engine()?.clone();
        Ok(errors::direct(|| engine.themes()))
    }

    fn languages(&self) -> PyResult<Vec<String>> {
        let engine = self.engine()?.clone();
        Ok(errors::direct(|| engine.languages()))
    }

    fn language_for_path(&self, path: std::path::PathBuf) -> PyResult<Option<String>> {
        let engine = self.engine()?.clone();
        Ok(errors::direct(|| engine.language_for_path(&path)))
    }

    fn __repr__(slf: &Bound<'_, Self>) -> PyResult<String> {
        let this = slf.get();
        Ok(match &this.inner {
            Some(engine) => format!(
                "<CodeHighlighter default_theme={:?}>",
                engine.default_theme()
            ),
            None => format!("<{} (Python)>", slf.get_type().name()?),
        })
    }
}

// ---------------------------------------------------------------------------
// The adapter: a Python engine behind Rust's trait

/// A Python code highlighter as a Rust one. `default_theme` is read once,
/// when the adapter is made (Rust returns it by reference).
struct PyCodeHighlighter {
    object: Py<PyAny>,
    default_theme: String,
}

/// A method's result, or an attribute's value when it is not callable.
fn call_or_get<'py>(object: &Bound<'py, PyAny>, name: &str) -> PyResult<Option<Bound<'py, PyAny>>> {
    let Some(attribute) = object.getattr_opt(name)? else {
        return Ok(None);
    };
    if attribute.is_callable() {
        attribute.call0().map(Some)
    } else {
        Ok(Some(attribute))
    }
}

impl PyCodeHighlighter {
    fn strings(&self, name: &str) -> Vec<String> {
        Python::attach(|py| {
            let result = call_or_get(self.object.bind(py), name).and_then(|value| match value {
                None => Ok(Vec::new()),
                Some(value) => value
                    .try_iter()?
                    .map(|item| item?.extract::<String>())
                    .collect(),
            });
            result.unwrap_or_else(|error| {
                callback_failed(py, error);
                Vec::new()
            })
        })
    }
}

impl CoreCodeHighlighter for PyCodeHighlighter {
    fn highlight(
        &self,
        code: &str,
        language: Option<&str>,
        theme: &str,
    ) -> Result<CoreCode, CoreHighlightError> {
        Python::attach(|py| {
            let object = self.object.bind(py);
            let result = object
                .call_method1("highlight", (code, language, theme))
                .and_then(|value| code_arg(&value));
            match result {
                Ok(highlighted) => Ok(validate(code, to_core(code, highlighted))),
                Err(error) if error.is_instance_of::<UnknownThemeError>(py) => {
                    let name = error
                        .value(py)
                        .getattr("args")
                        .ok()
                        .and_then(|args| args.get_item(0).ok())
                        .and_then(|name| name.extract::<String>().ok())
                        .unwrap_or_else(|| theme.to_string());
                    Err(CoreHighlightError::UnknownTheme(name))
                }
                Err(error) => Err(CoreHighlightError::Engine(callback_failed(py, error))),
            }
        })
    }

    fn default_theme(&self) -> &str {
        &self.default_theme
    }

    fn themes(&self) -> Vec<String> {
        self.strings("themes")
    }

    fn languages(&self) -> Vec<String> {
        self.strings("languages")
    }

    fn language_for_path(&self, path: &Path) -> Option<String> {
        Python::attach(|py| {
            let object = self.object.bind(py);
            let result = match object.getattr_opt("language_for_path") {
                Ok(Some(method)) => method
                    .call1((path,))
                    .and_then(|value| value.extract::<Option<String>>()),
                Ok(None) => Ok(None),
                Err(error) => Err(error),
            };
            result.unwrap_or_else(|error| {
                callback_failed(py, error);
                None
            })
        })
    }
}

/// A `CodeHighlighter` argument as Rust's: a handle's own engine, or any
/// Python object with `highlight`, `default_theme` and `themes` behind the
/// adapter. For other areas too (`Syntax(highlighter=...)`).
pub(crate) fn code_highlighter_arg(
    value: &Bound<'_, PyAny>,
) -> PyResult<Arc<dyn CoreCodeHighlighter>> {
    if let Ok(handle) = value.extract::<PyRef<'_, CodeHighlighter>>() {
        if let Some(inner) = &handle.inner {
            return Ok(inner.clone());
        }
    }
    if !value
        .getattr_opt("highlight")?
        .is_some_and(|m| m.is_callable())
    {
        return Err(PyTypeError::new_err(format!(
            "a code highlighter needs highlight(code, language, theme), default_theme() and \
             themes(); got {}",
            value.repr()?
        )));
    }
    let default_theme = match call_or_get(value, "default_theme")? {
        Some(theme) => theme.extract::<String>().map_err(|_| {
            PyTypeError::new_err("a code highlighter's default_theme() must return a str")
        })?,
        None => {
            return Err(PyTypeError::new_err(
                "a code highlighter needs default_theme()",
            ))
        }
    };
    Ok(Arc::new(PyCodeHighlighter {
        object: value.clone().unbind(),
        default_theme,
    }))
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<HighlightSpan>()?;
    m.add_class::<HighlightedLine>()?;
    m.add_class::<HighlightedCode>()?;
    m.add_class::<CodeHighlighter>()?;
    Ok(())
}
