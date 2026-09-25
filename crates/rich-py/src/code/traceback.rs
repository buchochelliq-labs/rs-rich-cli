//! `rich.traceback`: `Traceback`, its data (`Trace`, `Stack`, `Frame`,
//! `_SyntaxError`), `install`, and `Console.print_exception`.
//!
//! Core's `Traceback` renders a Rust error chain; a Python exception's
//! frames, source lines and locals are Python's to give, so `extract` walks
//! them here, and the report is built from core `Panel`s and `Text`, this
//! area's `Syntax` (line numbers, highlighted line, error range) and
//! `Pretty`, in upstream's layout.

use std::sync::{Arc, OnceLock};

use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList, PyString, PyTuple, PyType};

use rich::console::{Console as CoreConsole, ConsoleOptions as CoreOptions};
use rich::measure::Measurement as CoreMeasurement;
use rich::protocol::Renderable;
use rich::segment::Segment as CoreSegment;
use rich::style::StyleType;
use rich::table::{Cell, ColumnOptions, Table};
use rich::{Justify, Text as CoreText};

use super::highlighter::{highlight_regex, Highlight};
use super::layout::{join_lines, split_keep, Blank, NamedPanel, Stack};
use super::pretty::{shared_repr, traverse, Layout, Limits, PyNode};
use super::syntax::{Render as SyntaxRender, Spec};
use crate::console::Console;
use crate::convert;
use crate::renderable::{self, AsRenderable};

const LOCALS_MAX_LENGTH: usize = 10;
const LOCALS_MAX_STRING: usize = 80;

type Child = Box<dyn Renderable + Send + Sync>;
type SharedChild = Arc<dyn Renderable + Send + Sync>;

/// Upstream's `rich._IMPORT_CWD`: the working directory when `rs_rich`
/// was imported, which relative frame filenames are joined to.
static IMPORT_CWD: OnceLock<String> = OnceLock::new();

fn named(style: &str) -> Option<StyleType> {
    Some(StyleType::Name(style.to_string()))
}

fn styled(text: &str, style: &str) -> CoreText {
    CoreText::styled(text, StyleType::Name(style.to_string()))
}

fn repr_text(py: Python<'_>, text: &str) -> PyResult<CoreText> {
    Highlight::Repr.apply(py, CoreText::new(text))
}

fn markup(text: &str) -> PyResult<CoreText> {
    CoreText::from_markup(text).map_err(crate::color::markup::markup_error)
}

// ---------------------------------------------------------------------------
// The extracted data

/// `rich.traceback.Frame`.
#[pyclass(name = "Frame", module = "rs_rich.traceback", get_all, set_all)]
pub(crate) struct Frame {
    filename: String,
    lineno: isize,
    name: String,
    line: String,
    locals: Option<Py<PyDict>>,
    last_instruction: Option<((isize, isize), (isize, isize))>,
}

#[pymethods]
impl Frame {
    #[new]
    #[pyo3(signature = (filename, lineno, name, line=String::new(), locals=None, last_instruction=None))]
    fn new(
        filename: String,
        lineno: isize,
        name: String,
        line: String,
        locals: Option<Py<PyDict>>,
        last_instruction: Option<((isize, isize), (isize, isize))>,
    ) -> Self {
        Frame {
            filename,
            lineno,
            name,
            line,
            locals,
            last_instruction,
        }
    }

    fn __repr__(&self) -> String {
        format!(
            "Frame(filename={:?}, lineno={}, name={:?})",
            self.filename, self.lineno, self.name
        )
    }
}

/// `rich.traceback._SyntaxError`.
#[pyclass(name = "_SyntaxError", module = "rs_rich.traceback", get_all, set_all)]
pub(crate) struct SyntaxErrorInfo {
    offset: isize,
    filename: String,
    line: String,
    lineno: isize,
    msg: String,
    notes: Vec<String>,
}

#[pymethods]
impl SyntaxErrorInfo {
    #[new]
    #[pyo3(signature = (offset, filename, line, lineno, msg, notes=Vec::new()))]
    fn new(
        offset: isize,
        filename: String,
        line: String,
        lineno: isize,
        msg: String,
        notes: Vec<String>,
    ) -> Self {
        SyntaxErrorInfo {
            offset,
            filename,
            line,
            lineno,
            msg,
            notes,
        }
    }
}

/// `rich.traceback.Stack`: one exception and its frames.
#[pyclass(name = "Stack", module = "rs_rich.traceback", get_all, set_all)]
pub(crate) struct ExcStack {
    exc_type: String,
    exc_value: String,
    syntax_error: Option<Py<SyntaxErrorInfo>>,
    is_cause: bool,
    frames: Py<PyList>,
    notes: Vec<String>,
    is_group: bool,
    exceptions: Py<PyList>,
}

#[pymethods]
impl ExcStack {
    #[new]
    #[pyo3(signature = (
        exc_type, exc_value, syntax_error=None, is_cause=false, frames=None, notes=Vec::new(),
        is_group=false, exceptions=None
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        py: Python<'_>,
        exc_type: String,
        exc_value: String,
        syntax_error: Option<Py<SyntaxErrorInfo>>,
        is_cause: bool,
        frames: Option<Py<PyList>>,
        notes: Vec<String>,
        is_group: bool,
        exceptions: Option<Py<PyList>>,
    ) -> Self {
        ExcStack {
            exc_type,
            exc_value,
            syntax_error,
            is_cause,
            frames: frames.unwrap_or_else(|| PyList::empty(py).unbind()),
            notes,
            is_group,
            exceptions: exceptions.unwrap_or_else(|| PyList::empty(py).unbind()),
        }
    }
}

/// `rich.traceback.Trace`: the stacks of an exception and its causes.
#[pyclass(name = "Trace", module = "rs_rich.traceback", get_all, set_all)]
pub(crate) struct Trace {
    stacks: Py<PyList>,
}

#[pymethods]
impl Trace {
    #[new]
    fn new(stacks: Py<PyList>) -> Self {
        Trace { stacks }
    }
}

/// The locals options of `extract`.
#[derive(Clone, Copy)]
struct LocalsOptions {
    show: bool,
    limits: Limits,
    hide_dunder: bool,
    hide_sunder: bool,
}

fn safe_str(object: &Bound<'_, PyAny>) -> String {
    object
        .str()
        .map(|s| s.to_string())
        .unwrap_or_else(|_| "<exception str() failed>".to_string())
}

/// Upstream's `Traceback.extract`.
fn extract(
    py: Python<'_>,
    exc_type: &Bound<'_, PyAny>,
    exc_value: &Bound<'_, PyAny>,
    traceback: &Bound<'_, PyAny>,
    locals: LocalsOptions,
    visited: Option<&Bound<'_, PyAny>>,
) -> PyResult<Py<Trace>> {
    let stacks = PyList::empty(py);
    let mut is_cause = false;
    let notes: Vec<String> = match exc_value.getattr_opt("__notes__")? {
        Some(notes) if notes.is_truthy()? => notes.extract()?,
        _ => Vec::new(),
    };
    let grouped = match visited {
        Some(visited) => visited.clone(),
        None => py.import("builtins")?.getattr("set")?.call0()?,
    };
    let builtins = py.import("builtins")?;
    let base_group = builtins.getattr_opt("BaseExceptionGroup")?;
    let syntax_error_class = builtins.getattr("SyntaxError")?;
    let os_path = py.import("os")?.getattr("path")?;
    let inspect = py.import("inspect")?;
    let walk_tb = py.import("traceback")?.getattr("walk_tb")?;
    let version_info = py.import("sys")?.getattr("version_info")?;
    let version: (u8, u8) = (
        version_info.get_item(0)?.extract()?,
        version_info.get_item(1)?.extract()?,
    );
    let cwd = IMPORT_CWD.get().cloned().unwrap_or_default();

    let (mut exc_type, mut exc_value, mut traceback) =
        (exc_type.clone(), exc_value.clone(), traceback.clone());
    loop {
        let exceptions = PyList::empty(py);
        let mut is_group = false;
        if let Some(base_group) = &base_group {
            if exc_value.is_instance(base_group)? {
                is_group = true;
                for exception in exc_value.getattr("exceptions")?.try_iter()? {
                    let exception = exception?;
                    if grouped.contains(&exception)? {
                        continue;
                    }
                    grouped.call_method1("add", (&exception,))?;
                    let sub = extract(
                        py,
                        exception.get_type().as_any(),
                        &exception,
                        &exception.getattr("__traceback__")?,
                        LocalsOptions {
                            limits: Limits {
                                max_string: Some(LOCALS_MAX_STRING),
                                max_depth: None,
                                ..locals.limits
                            },
                            ..locals
                        },
                        Some(&grouped),
                    )?;
                    exceptions.append(sub)?;
                }
            }
        }
        let syntax_error = if exc_value.is_instance(&syntax_error_class)? {
            fn or<'py>(
                value: &Bound<'py, PyAny>,
                name: &str,
                default: Bound<'py, PyAny>,
            ) -> PyResult<Bound<'py, PyAny>> {
                let value = value.getattr(name)?;
                Ok(if value.is_truthy()? { value } else { default })
            }
            Some(Py::new(
                py,
                SyntaxErrorInfo {
                    offset: or(&exc_value, "offset", 0i32.into_pyobject(py)?.into_any())?
                        .extract()?,
                    filename: or(&exc_value, "filename", PyString::new(py, "?").into_any())?
                        .extract()?,
                    lineno: or(&exc_value, "lineno", 0i32.into_pyobject(py)?.into_any())?
                        .extract()?,
                    line: or(&exc_value, "text", PyString::new(py, "").into_any())?.extract()?,
                    msg: exc_value.getattr("msg")?.str()?.to_string(),
                    notes: notes.clone(),
                },
            )?)
        } else {
            None
        };
        let frames = PyList::empty(py);
        if !traceback.is_none() {
            for item in walk_tb.call1((&traceback,))?.try_iter()? {
                let (frame, line_no): (Bound<'_, PyAny>, isize) = item?.extract()?;
                let code = frame.getattr("f_code")?;
                let mut filename: String = code.getattr("co_filename")?.extract()?;
                let mut last_instruction = None;
                if version >= (3, 11) {
                    let index: usize = frame.getattr("f_lasti")?.extract::<usize>()? / 2;
                    let positions = code.call_method0("co_positions")?;
                    if let Some(position) = positions.try_iter()?.nth(index) {
                        let (start_line, end_line, start_column, end_column): (
                            Option<isize>,
                            Option<isize>,
                            Option<isize>,
                            Option<isize>,
                        ) = position?.extract()?;
                        if let (Some(a), Some(b), Some(c), Some(d)) =
                            (start_line, end_line, start_column, end_column)
                        {
                            last_instruction = Some(((a, c), (b, d)));
                        }
                    }
                }
                if !filename.is_empty()
                    && !filename.starts_with('<')
                    && !os_path.call_method1("isabs", (&filename,))?.is_truthy()?
                {
                    filename = os_path.call_method1("join", (&cwd, &filename))?.extract()?;
                }
                let f_locals = frame.getattr("f_locals")?;
                let flag = |name: &str| -> PyResult<bool> {
                    f_locals.call_method1("get", (name, false))?.is_truthy()
                };
                if flag("_rich_traceback_omit")? {
                    continue;
                }
                let frame_locals = if locals.show {
                    let dict = PyDict::new(py);
                    for item in f_locals.call_method0("items")?.try_iter()? {
                        let (key, value): (String, Bound<'_, PyAny>) = item?.extract()?;
                        if locals.hide_dunder && key.starts_with("__") {
                            continue;
                        }
                        if locals.hide_sunder && key.starts_with('_') {
                            continue;
                        }
                        if inspect.call_method1("isfunction", (&value,))?.is_truthy()?
                            || inspect.call_method1("isclass", (&value,))?.is_truthy()?
                        {
                            continue;
                        }
                        let node = PyNode {
                            inner: traverse(&value, locals.limits)?,
                        };
                        dict.set_item(key, Py::new(py, node)?)?;
                    }
                    Some(dict.unbind())
                } else {
                    None
                };
                let name: String = code.getattr("co_name")?.extract()?;
                frames.append(Py::new(
                    py,
                    Frame {
                        filename: if filename.is_empty() {
                            "?".to_string()
                        } else {
                            filename
                        },
                        lineno: line_no,
                        name,
                        line: String::new(),
                        locals: frame_locals,
                        last_instruction,
                    },
                )?)?;
                if flag("_rich_traceback_guard")? {
                    frames.call_method0("clear")?;
                }
            }
        }
        stacks.append(Py::new(
            py,
            ExcStack {
                exc_type: safe_str(&exc_type.getattr("__name__")?),
                exc_value: safe_str(&exc_value),
                syntax_error,
                is_cause,
                frames: frames.unbind(),
                notes: notes.clone(),
                is_group,
                exceptions: exceptions.unbind(),
            },
        )?)?;

        if !grouped.is_truthy()? {
            if let Some(cause) = exc_value.getattr_opt("__cause__")? {
                if !cause.is_none() && !cause.is(&exc_value) {
                    exc_type = cause.get_type().into_any();
                    traceback = cause.getattr("__traceback__")?;
                    exc_value = cause;
                    is_cause = true;
                    continue;
                }
            }
            let context = exc_value.getattr("__context__")?;
            let suppress = exc_value
                .getattr_opt("__suppress_context__")?
                .map(|s| s.is_truthy())
                .transpose()?
                .unwrap_or(false);
            if !context.is_none() && !suppress {
                exc_type = context.get_type().into_any();
                traceback = context.getattr("__traceback__")?;
                exc_value = context;
                is_cause = false;
                continue;
            }
        }
        break;
    }
    Py::new(
        py,
        Trace {
            stacks: stacks.unbind(),
        },
    )
}

// ---------------------------------------------------------------------------
// Traceback

/// The rendering options of a `Traceback`.
#[derive(Clone)]
struct View {
    width: Option<usize>,
    code_width: Option<usize>,
    extra_lines: isize,
    theme: String,
    word_wrap: bool,
    indent_guides: bool,
    locals_max_length: Option<usize>,
    locals_max_string: Option<usize>,
    locals_max_depth: Option<usize>,
    locals_overflow: Option<rich::Overflow>,
    suppress: Vec<String>,
    max_frames: usize,
}

/// `rich.traceback.Traceback`: a rendered Python exception.
#[pyclass(name = "Traceback", module = "rs_rich.traceback")]
pub(crate) struct Traceback {
    #[pyo3(get, set)]
    trace: Py<Trace>,
    view: View,
    #[pyo3(get)]
    show_locals: bool,
    #[pyo3(get)]
    locals_hide_dunder: bool,
    #[pyo3(get)]
    locals_hide_sunder: bool,
}

/// Upstream's `PathHighlighter`: the directory dim, the file name bold.
fn path_highlight(py: Python<'_>, text: &mut CoreText) -> PyResult<()> {
    let pattern = PyString::new(py, r"(?P<dim>.*/)(?P<bold>.+)");
    highlight_regex(py, text, pattern.as_any(), "")
}

/// Upstream's `_guess_lexer`: by extension (Python's own, then the default
/// engine's), a Python hashbang, else plain text.
fn guess_lexer(py: Python<'_>, filename: &str, code: &str) -> PyResult<String> {
    let ext: String = py
        .import("os")?
        .getattr("path")?
        .call_method1("splitext", (filename,))?
        .get_item(1)?
        .extract()?;
    if ext.is_empty() {
        let Some(newline) = code.find('\n') else {
            return Err(PyValueError::new_err("substring not found"));
        };
        let first_line = &code[..newline];
        if first_line.starts_with("#!") && first_line.to_lowercase().contains("python") {
            return Ok("python".to_string());
        }
    }
    Ok(match ext.as_str() {
        "" => "text".to_string(),
        ".py" => "python".to_string(),
        ".pxd" | ".pyx" => "cython".to_string(),
        ".pxi" => "pyrex".to_string(),
        _ => rich::SyntectHighlighter::shared()
            .language_for_path(std::path::Path::new(filename))
            .unwrap_or_else(|| "text".to_string()),
    })
}

/// A Python list index (negative from the end), as `list[index]` would.
fn py_index<T>(items: &[T], index: isize) -> Option<&T> {
    let index = if index < 0 {
        items.len() as isize + index
    } else {
        index
    };
    usize::try_from(index).ok().and_then(|i| items.get(i))
}

/// Upstream's `_iter_syntax_lines`.
fn syntax_lines(start: (isize, isize), end: (isize, isize)) -> Vec<(isize, isize, isize)> {
    let ((line1, column1), (line2, column2)) = (start, end);
    if line1 == line2 {
        return vec![(line1, column1, column2)];
    }
    let count = (line2 - line1 + 1).max(0);
    (0..count)
        .map(|index| {
            let line_no = line1 + index;
            if index == 0 {
                (line_no, column1, -1)
            } else if index == count - 1 {
                (line_no, 0, column2)
            } else {
                (line_no, 0, -1)
            }
        })
        .collect()
}

/// Upstream's `Columns([syntax, locals], padding=1)` for a frame: side by
/// side when both fit, else one above the other with a blank line between.
struct FrameColumns {
    items: Vec<SharedChild>,
}

impl Renderable for FrameColumns {
    fn rich_render(&self, console: &CoreConsole, options: &CoreOptions) -> Vec<CoreSegment> {
        let widths: Vec<usize> = self
            .items
            .iter()
            .map(|item| CoreMeasurement::get(console, options, item.as_ref()).maximum)
            .collect();
        let total = widths.iter().sum::<usize>() + widths.len().saturating_sub(1);
        if total <= options.max_width {
            let cells = self
                .items
                .iter()
                .map(|item| Cell::Renderable(item.clone()))
                .collect();
            return rich::Columns::from_cells(cells).rich_render(console, options);
        }
        let column = widths
            .iter()
            .copied()
            .max()
            .unwrap_or(0)
            .min(options.max_width);
        let child_options = options.update_width(column);
        let mut lines = Vec::new();
        for (index, item) in self.items.iter().enumerate() {
            if index > 0 {
                lines.push(Vec::new());
            }
            lines.extend(split_keep(&item.rich_render(console, &child_options)));
        }
        join_lines(lines)
    }

    fn measure(&self, console: &CoreConsole, options: &CoreOptions) -> CoreMeasurement {
        let widths: Vec<CoreMeasurement> = self
            .items
            .iter()
            .map(|item| CoreMeasurement::get(console, options, item.as_ref()))
            .collect();
        let maximum =
            widths.iter().map(|m| m.maximum).sum::<usize>() + widths.len().saturating_sub(1);
        let minimum = widths.iter().map(|m| m.minimum).max().unwrap_or(0);
        CoreMeasurement::new(minimum, maximum.min(options.max_width))
    }
}

/// `Constrain(renderable, width)` over a shared child.
struct Constrained {
    child: SharedChild,
    width: Option<usize>,
}

impl Renderable for Constrained {
    fn rich_render(&self, console: &CoreConsole, options: &CoreOptions) -> Vec<CoreSegment> {
        let width = self
            .width
            .map_or(options.max_width, |w| w.min(options.max_width));
        self.child
            .rich_render(console, &options.update_width(width))
    }

    fn measure(&self, console: &CoreConsole, options: &CoreOptions) -> CoreMeasurement {
        let options = match self.width {
            Some(width) => options.update_width(width.min(options.max_width)),
            None => options.clone(),
        };
        CoreMeasurement::get(console, &options, self.child.as_ref())
    }
}

/// Upstream's `rich.scope.render_scope(scope, title=title, sort_keys=True,
/// indent_guides=..., max_length=..., max_string=..., max_depth=...,
/// overflow=...)`: a fitted panel of names and pretty-printed values (a
/// value may already be a `Node`).
pub(crate) fn render_scope(
    scope: &Bound<'_, PyDict>,
    title: Option<String>,
    indent_guides: bool,
    limits: Limits,
    overflow: Option<rich::Overflow>,
) -> PyResult<SharedChild> {
    let mut items: Vec<(String, Bound<'_, PyAny>)> = scope
        .iter()
        .map(|(key, value)| Ok((key.extract::<String>()?, value)))
        .collect::<PyResult<_>>()?;
    items.sort_by_cached_key(|(key, _)| (!key.starts_with("__"), key.to_lowercase()));
    let mut table = Table::grid().padding(0, 1, 0, 1).expand(false);
    table.add_column_with(
        CoreText::new(""),
        ColumnOptions {
            justify: Justify::Right,
            ..ColumnOptions::default()
        },
    );
    table.add_column_with(CoreText::new(""), ColumnOptions::default());
    for (key, value) in items {
        let mut key_text = CoreText::new("");
        key_text.append(
            &key,
            named(if key.starts_with("__") {
                "scope.key.special"
            } else {
                "scope.key"
            }),
        );
        key_text.append(" =", named("scope.equals"));
        let node = match value.extract::<PyRef<'_, PyNode>>() {
            Ok(node) => node.inner.clone(),
            Err(_) => traverse(&value, limits)?,
        };
        let mut layout = Layout::new(node, super::pretty::type_repr(&value)?);
        layout.indent_guides = indent_guides;
        layout.overflow = overflow;
        table.add_row_cells(vec![
            Cell::Text(key_text),
            Cell::Renderable(shared_repr(layout, &[])),
        ]);
    }
    Ok(Arc::new(NamedPanel {
        child: Arc::new(table),
        border_style: StyleType::Name("scope.border".to_string()),
        title,
        expand: false,
        width: None,
        padding: (0, 1, 0, 1),
    }))
}

impl View {
    /// Upstream's `render_scope(frame.locals, title="locals", ...)`.
    fn render_locals(&self, locals: &Bound<'_, PyDict>) -> PyResult<Option<SharedChild>> {
        if locals.is_empty() {
            return Ok(None);
        }
        let limits = Limits {
            max_length: self.locals_max_length,
            max_string: self.locals_max_string,
            max_depth: self.locals_max_depth,
        };
        render_scope(
            locals,
            Some("locals".to_string()),
            self.indent_guides,
            limits,
            self.locals_overflow,
        )
        .map(Some)
    }

    /// Upstream's `_render_stack`: the frames of one exception.
    fn render_frames(&self, py: Python<'_>, stack: &ExcStack) -> PyResult<Vec<Child>> {
        let mut parts: Vec<Child> = Vec::new();
        let frames: Vec<Bound<'_, Frame>> = stack
            .frames
            .bind(py)
            .iter()
            .map(|frame| frame.cast_into::<Frame>().map_err(PyErr::from))
            .collect::<PyResult<_>>()?;
        let count = frames.len();
        let exclude = if self.max_frames != 0 {
            let start = self.max_frames / 2;
            let end = count.saturating_sub(self.max_frames / 2);
            Some(start..end).filter(|range| range.start < range.end)
        } else {
            None
        };
        let os_path = py.import("os")?.getattr("path")?;
        let linecache = py.import("linecache")?;
        let mut excluded = false;
        for (index, frame) in frames.iter().enumerate() {
            let frame = frame.borrow();
            if let Some(range) = &exclude {
                if range.contains(&index) {
                    excluded = true;
                    continue;
                }
            }
            if excluded {
                let hidden = exclude.as_ref().map_or(0, |r| r.len());
                let mut text = styled(
                    &format!("\n... {hidden} frames hidden ..."),
                    "traceback.error",
                );
                text.set_justify(Justify::Center);
                parts.push(Box::new(text));
                excluded = false;
            }
            let first = index == 0;
            let suppressed = self
                .suppress
                .iter()
                .any(|path| frame.filename.starts_with(path.as_str()));
            let text = if os_path
                .call_method1("exists", (&frame.filename,))?
                .is_truthy()?
            {
                let mut path = styled(&frame.filename, "pygments.string");
                path_highlight(py, &mut path)?;
                let mut text = styled("", "pygments.text").append_text(&path);
                text.append(":", named("pygments.text"));
                text.append(&frame.lineno.to_string(), named("pygments.number"));
                text.append(" in ", None);
                text.append(&frame.name, named("pygments.function"));
                text
            } else {
                let mut text = styled("", "pygments.text");
                text.append("in ", None);
                text.append(&frame.name, named("pygments.function"));
                text.append(":", named("pygments.text"));
                text.append(&frame.lineno.to_string(), named("pygments.number"));
                text
            };
            if !frame.filename.starts_with('<') && !first {
                parts.push(Box::new(Blank));
            }
            parts.push(Box::new(text));
            let locals = match &frame.locals {
                Some(locals) => self.render_locals(locals.bind(py))?,
                None => None,
            };
            if frame.filename.starts_with('<') {
                if let Some(locals) = locals {
                    parts.push(Box::new(super::layout::Shared(locals)));
                }
                continue;
            }
            if suppressed {
                continue;
            }
            let prepared = (|| -> PyResult<Option<(Vec<String>, Spec)>> {
                let code_lines: Vec<String> = linecache
                    .call_method1("getlines", (&frame.filename,))?
                    .extract()?;
                let code = code_lines.concat();
                if code.is_empty() {
                    return Ok(None);
                }
                let lexer = guess_lexer(py, &frame.filename, &code)?;
                let mut spec = Spec::new(code, lexer);
                spec.theme = Some(self.theme.clone());
                spec.line_numbers = true;
                spec.line_range = Some((
                    Some(frame.lineno - self.extra_lines),
                    Some(frame.lineno + self.extra_lines),
                ));
                spec.highlight_lines = [frame.lineno].into_iter().collect();
                spec.word_wrap = self.word_wrap;
                spec.code_width = self.code_width;
                spec.indent_guides = self.indent_guides;
                Ok(Some((code_lines, spec)))
            })();
            let (code_lines, mut spec) = match prepared {
                Ok(Some(prepared)) => prepared,
                Ok(None) => continue,
                Err(error) => {
                    let message = error.value(py).str()?.to_string();
                    parts.push(Box::new(styled(&format!("\n{message}"), "traceback.error")));
                    continue;
                }
            };
            parts.push(Box::new(Blank));
            if let Some((start, end)) = frame.last_instruction {
                let chars: Vec<Vec<char>> =
                    code_lines.iter().map(|l| l.chars().collect()).collect();
                for (line1, mut column1, mut column2) in syntax_lines(start, end) {
                    let Some(line) = py_index(&chars, line1 - 1) else {
                        continue;
                    };
                    if column1 == 0 {
                        let stripped = line.iter().skip_while(|c| c.is_whitespace()).count();
                        column1 = (line.len() - stripped) as isize;
                    }
                    if column2 == -1 {
                        column2 = line.len() as isize;
                    }
                    spec.stylize_range(
                        StyleType::Name("traceback.error_range".to_string()),
                        (line1, column1),
                        (line1, column2),
                    );
                }
            }
            let syntax: SharedChild = Arc::new(SyntaxRender { spec });
            match locals {
                Some(locals) => parts.push(Box::new(FrameColumns {
                    items: vec![syntax, locals],
                })),
                None => parts.push(Box::new(super::layout::Shared(syntax))),
            }
        }
        Ok(parts)
    }

    /// Upstream's `_render_syntax_error`.
    fn render_syntax_error(&self, py: Python<'_>, error: &SyntaxErrorInfo) -> PyResult<Vec<Child>> {
        let mut parts: Vec<Child> = Vec::new();
        if error.filename != "<stdin>"
            && py
                .import("os")?
                .getattr("path")?
                .call_method1("exists", (&error.filename,))?
                .is_truthy()?
        {
            let mut text = styled("", "pygments.text");
            text.append(&format!(" {}", error.filename), named("pygments.string"));
            text.append(":", named("pygments.text"));
            text.append(&error.lineno.to_string(), named("pygments.number"));
            path_highlight(py, &mut text)?;
            parts.push(Box::new(text));
        }
        let mut text = repr_text(py, error.line.trim_end())?;
        text.set_no_wrap(Some(true));
        let length = text.plain().chars().count() as isize;
        let offset = (error.offset - 1).min(length).max(0) as usize;
        let mut arrow = markup(&format!("\n{}[traceback.offset]▲[/]", " ".repeat(offset)))?;
        arrow.set_base_style(StyleType::Name("pygments.text".to_string()));
        parts.push(Box::new(text.append_text(&arrow)));
        Ok(parts)
    }

    /// Upstream's `render_stack`.
    fn render_stack(&self, py: Python<'_>, stack: &ExcStack, last: bool) -> PyResult<Vec<Child>> {
        let mut parts: Vec<Child> = Vec::new();
        if !stack.frames.bind(py).is_empty() {
            let frames = self.render_frames(py, stack)?;
            let panel = NamedPanel {
                child: Arc::new(Stack { children: frames }),
                border_style: StyleType::Name("traceback.border".to_string()),
                title: Some("[traceback.title]Traceback [dim](most recent call last)".to_string()),
                expand: true,
                width: None,
                padding: (0, 1, 0, 1),
            };
            parts.push(Box::new(Constrained {
                child: Arc::new(panel),
                width: self.width,
            }));
        }
        let highlighted = |text: &str| repr_text(py, text);
        let mut exc_type = CoreText::new("");
        if let Some(error) = &stack.syntax_error {
            let error = error.bind(py).borrow();
            let children = self.render_syntax_error(py, &error)?;
            let panel = NamedPanel {
                child: Arc::new(Stack { children }),
                border_style: StyleType::Name("traceback.border.syntax_error".to_string()),
                title: None,
                expand: true,
                width: self.width,
                padding: (0, 1, 0, 1),
            };
            parts.push(Box::new(Constrained {
                child: Arc::new(panel),
                width: self.width,
            }));
            exc_type.append(
                &format!("{}: ", stack.exc_type),
                named("traceback.exc_type"),
            );
            parts.push(Box::new(exc_type.append_text(&highlighted(&error.msg)?)));
        } else if !stack.exc_value.is_empty() {
            exc_type.append(
                &format!("{}: ", stack.exc_type),
                named("traceback.exc_type"),
            );
            parts.push(Box::new(
                exc_type.append_text(&highlighted(&stack.exc_value)?),
            ));
        } else {
            exc_type.append(&stack.exc_type, named("traceback.exc_type"));
            parts.push(Box::new(exc_type));
        }
        for note in &stack.notes {
            let mut text = CoreText::new("");
            text.append("[NOTE] ", named("traceback.note"));
            parts.push(Box::new(text.append_text(&highlighted(note)?)));
        }
        if stack.is_group {
            for (number, exception) in stack.exceptions.bind(py).iter().enumerate() {
                let trace = exception.cast_into::<Trace>()?;
                let stacks: Vec<Bound<'_, ExcStack>> = trace
                    .borrow()
                    .stacks
                    .bind(py)
                    .iter()
                    .map(|s| s.cast_into::<ExcStack>().map_err(PyErr::from))
                    .collect::<PyResult<_>>()?;
                let mut grouped: Vec<Child> = Vec::new();
                let total = stacks.len();
                for (index, sub) in stacks.iter().enumerate() {
                    let children = self.render_stack(py, &sub.borrow(), index + 1 == total)?;
                    grouped.push(Box::new(Stack { children }));
                }
                parts.push(Box::new(Blank));
                let panel = NamedPanel {
                    child: Arc::new(Stack { children: grouped }),
                    border_style: StyleType::Name("traceback.group.border".to_string()),
                    title: Some(format!("Sub-exception #{}", number + 1)),
                    expand: true,
                    width: None,
                    padding: (0, 1, 0, 1),
                };
                parts.push(Box::new(Constrained {
                    child: Arc::new(panel),
                    width: self.width,
                }));
            }
        }
        if !last {
            let message = if stack.is_cause {
                "\n[i]The above exception was the direct cause of the following exception:\n"
            } else {
                "\n[i]During handling of the above exception, another exception occurred:\n"
            };
            parts.push(Box::new(markup(message)?));
        }
        Ok(parts)
    }
}

impl AsRenderable for Traceback {
    fn to_renderable(&self, py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        let stacks: Vec<Bound<'_, ExcStack>> = self
            .trace
            .bind(py)
            .borrow()
            .stacks
            .bind(py)
            .iter()
            .map(|s| s.cast_into::<ExcStack>().map_err(PyErr::from))
            .collect::<PyResult<_>>()?;
        let mut children: Vec<Child> = Vec::new();
        let total = stacks.len();
        for (index, stack) in stacks.iter().rev().enumerate() {
            let parts = self
                .view
                .render_stack(py, &stack.borrow(), index + 1 == total)?;
            children.push(Box::new(Stack { children: parts }));
        }
        Ok(Box::new(Stack { children }))
    }
}

/// `suppress=`: modules (their directory) or paths, normalised.
fn suppress_paths(py: Python<'_>, suppress: Option<&Bound<'_, PyAny>>) -> PyResult<Vec<String>> {
    let Some(suppress) = suppress.filter(|s| !s.is_none()) else {
        return Ok(Vec::new());
    };
    let os_path = py.import("os")?.getattr("path")?;
    let mut paths = Vec::new();
    for entity in suppress.try_iter()? {
        let entity = entity?;
        let path = if entity.is_instance_of::<PyString>() {
            entity
        } else {
            let file = entity.getattr("__file__")?;
            if file.is_none() {
                return Err(PyTypeError::new_err(format!(
                    "{} must be a module with '__file__' attribute",
                    entity.repr()?
                )));
            }
            os_path.call_method1("dirname", (file,))?
        };
        let absolute = os_path.call_method1("abspath", (path,))?;
        paths.push(os_path.call_method1("normpath", (absolute,))?.extract()?);
    }
    Ok(paths)
}

#[pymethods]
impl Traceback {
    #[new]
    #[pyo3(signature = (
        trace=None, *, width=Some(100), code_width=Some(88), extra_lines=3, theme=None,
        word_wrap=false, show_locals=false, locals_max_length=Some(LOCALS_MAX_LENGTH),
        locals_max_string=Some(LOCALS_MAX_STRING), locals_max_depth=None, locals_hide_dunder=true,
        locals_hide_sunder=false, locals_overlow=None, indent_guides=true, suppress=None,
        max_frames=100
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        py: Python<'_>,
        trace: Option<Py<Trace>>,
        width: Option<usize>,
        code_width: Option<usize>,
        extra_lines: isize,
        theme: Option<String>,
        word_wrap: bool,
        show_locals: bool,
        locals_max_length: Option<usize>,
        locals_max_string: Option<usize>,
        locals_max_depth: Option<usize>,
        locals_hide_dunder: bool,
        locals_hide_sunder: bool,
        locals_overlow: Option<String>,
        indent_guides: bool,
        suppress: Option<&Bound<'_, PyAny>>,
        max_frames: isize,
    ) -> PyResult<Self> {
        let trace = match trace {
            Some(trace) => trace,
            None => {
                let info = py.import("sys")?.call_method0("exc_info")?;
                let (exc_type, exc_value, traceback): (
                    Bound<'_, PyAny>,
                    Bound<'_, PyAny>,
                    Bound<'_, PyAny>,
                ) = info.extract()?;
                if exc_type.is_none() || exc_value.is_none() || traceback.is_none() {
                    return Err(PyValueError::new_err(
                        "Value for 'trace' required if not called in except: block",
                    ));
                }
                extract(
                    py,
                    &exc_type,
                    &exc_value,
                    &traceback,
                    LocalsOptions {
                        show: show_locals,
                        limits: Limits {
                            max_length: Some(LOCALS_MAX_LENGTH),
                            max_string: Some(LOCALS_MAX_STRING),
                            max_depth: None,
                        },
                        hide_dunder: true,
                        hide_sunder: false,
                    },
                    None,
                )?
            }
        };
        Ok(Traceback {
            trace,
            view: View {
                width,
                code_width,
                extra_lines,
                theme: theme.unwrap_or_else(|| "ansi_dark".to_string()),
                word_wrap,
                indent_guides,
                locals_max_length,
                locals_max_string,
                locals_max_depth,
                locals_overflow: locals_overlow
                    .as_deref()
                    .map(convert::overflow)
                    .transpose()?,
                suppress: suppress_paths(py, suppress)?,
                max_frames: if max_frames > 0 {
                    (max_frames as usize).max(4)
                } else {
                    0
                },
            },
            show_locals,
            locals_hide_dunder,
            locals_hide_sunder,
        })
    }

    /// A `Traceback` for an exception (`exc_type, exc_value, traceback`).
    #[classmethod]
    #[pyo3(signature = (
        exc_type, exc_value, traceback, *, width=Some(100), code_width=Some(88), extra_lines=3,
        theme=None, word_wrap=false, show_locals=false, locals_max_length=Some(LOCALS_MAX_LENGTH),
        locals_max_string=Some(LOCALS_MAX_STRING), locals_max_depth=None, locals_hide_dunder=true,
        locals_hide_sunder=false, locals_overflow=None, indent_guides=true, suppress=None,
        max_frames=100
    ))]
    #[allow(clippy::too_many_arguments)]
    fn from_exception(
        cls: &Bound<'_, PyType>,
        exc_type: &Bound<'_, PyAny>,
        exc_value: &Bound<'_, PyAny>,
        traceback: &Bound<'_, PyAny>,
        width: Option<usize>,
        code_width: Option<usize>,
        extra_lines: isize,
        theme: Option<String>,
        word_wrap: bool,
        show_locals: bool,
        locals_max_length: Option<usize>,
        locals_max_string: Option<usize>,
        locals_max_depth: Option<usize>,
        locals_hide_dunder: bool,
        locals_hide_sunder: bool,
        locals_overflow: Option<String>,
        indent_guides: bool,
        suppress: Option<&Bound<'_, PyAny>>,
        max_frames: isize,
    ) -> PyResult<Self> {
        let py = cls.py();
        let trace = extract(
            py,
            exc_type,
            exc_value,
            traceback,
            LocalsOptions {
                show: show_locals,
                limits: Limits {
                    max_length: locals_max_length,
                    max_string: locals_max_string,
                    max_depth: locals_max_depth,
                },
                hide_dunder: locals_hide_dunder,
                hide_sunder: locals_hide_sunder,
            },
            None,
        )?;
        Traceback::new(
            py,
            Some(trace),
            width,
            code_width,
            extra_lines,
            theme,
            word_wrap,
            show_locals,
            locals_max_length,
            locals_max_string,
            locals_max_depth,
            locals_hide_dunder,
            locals_hide_sunder,
            locals_overflow,
            indent_guides,
            suppress,
            max_frames,
        )
    }

    /// Extract the stacks of an exception (and its causes) into a `Trace`.
    #[classmethod]
    #[pyo3(signature = (
        exc_type, exc_value, traceback, *, show_locals=false,
        locals_max_length=Some(LOCALS_MAX_LENGTH), locals_max_string=Some(LOCALS_MAX_STRING),
        locals_max_depth=None, locals_hide_dunder=true, locals_hide_sunder=false,
        _visited_exceptions=None
    ))]
    #[allow(clippy::too_many_arguments)]
    fn extract(
        cls: &Bound<'_, PyType>,
        exc_type: &Bound<'_, PyAny>,
        exc_value: &Bound<'_, PyAny>,
        traceback: &Bound<'_, PyAny>,
        show_locals: bool,
        locals_max_length: Option<usize>,
        locals_max_string: Option<usize>,
        locals_max_depth: Option<usize>,
        locals_hide_dunder: bool,
        locals_hide_sunder: bool,
        _visited_exceptions: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Py<Trace>> {
        extract(
            cls.py(),
            exc_type,
            exc_value,
            traceback,
            LocalsOptions {
                show: show_locals,
                limits: Limits {
                    max_length: locals_max_length,
                    max_string: locals_max_string,
                    max_depth: locals_max_depth,
                },
                hide_dunder: locals_hide_dunder,
                hide_sunder: locals_hide_sunder,
            },
            _visited_exceptions,
        )
    }

    #[getter]
    fn width(&self) -> Option<usize> {
        self.view.width
    }
    #[getter]
    fn code_width(&self) -> Option<usize> {
        self.view.code_width
    }
    #[getter]
    fn extra_lines(&self) -> isize {
        self.view.extra_lines
    }
    #[getter]
    fn word_wrap(&self) -> bool {
        self.view.word_wrap
    }
    #[getter]
    fn indent_guides(&self) -> bool {
        self.view.indent_guides
    }
    #[getter]
    fn max_frames(&self) -> usize {
        self.view.max_frames
    }
    #[getter]
    fn suppress(&self) -> Vec<String> {
        self.view.suppress.clone()
    }
    /// The syntax theme's name (Rich holds a `SyntaxTheme` object).
    #[getter]
    fn theme(&self) -> &str {
        &self.view.theme
    }
}

/// `rich.traceback.install`: print uncaught exceptions with `Traceback`.
/// Returns the previous `sys.excepthook`.
#[pyfunction]
#[pyo3(signature = (
    *, console=None, width=Some(100), code_width=Some(88), extra_lines=3, theme=None,
    word_wrap=false, show_locals=false, locals_max_length=Some(LOCALS_MAX_LENGTH),
    locals_max_string=Some(LOCALS_MAX_STRING), locals_max_depth=None, locals_hide_dunder=true,
    locals_hide_sunder=None, locals_overflow=None, indent_guides=true, suppress=None,
    max_frames=100
))]
#[allow(clippy::too_many_arguments)]
fn traceback_install(
    py: Python<'_>,
    console: Option<&Bound<'_, PyAny>>,
    width: Option<usize>,
    code_width: Option<usize>,
    extra_lines: isize,
    theme: Option<String>,
    word_wrap: bool,
    show_locals: bool,
    locals_max_length: Option<usize>,
    locals_max_string: Option<usize>,
    locals_max_depth: Option<usize>,
    locals_hide_dunder: bool,
    locals_hide_sunder: Option<bool>,
    locals_overflow: Option<String>,
    indent_guides: bool,
    suppress: Option<&Bound<'_, PyAny>>,
    max_frames: isize,
) -> PyResult<Py<PyAny>> {
    let console = match console.filter(|c| !c.is_none()) {
        Some(console) => console.clone().unbind(),
        None => {
            let kwargs = PyDict::new(py);
            kwargs.set_item("stderr", true)?;
            py.get_type::<Console>().call((), Some(&kwargs))?.unbind()
        }
    };
    locals_overflow
        .as_deref()
        .map(convert::overflow)
        .transpose()?;
    let suppress = suppress.map(|s| s.clone().unbind());
    let hook = pyo3::types::PyCFunction::new_closure(
        py,
        Some(c"excepthook"),
        None,
        move |args: &Bound<'_, PyTuple>, _kwargs: Option<&Bound<'_, PyDict>>| -> PyResult<()> {
            let py = args.py();
            let (exc_type, exc_value, traceback): (
                Bound<'_, PyAny>,
                Bound<'_, PyAny>,
                Bound<'_, PyAny>,
            ) = args.extract()?;
            let class = py.get_type::<Traceback>();
            let rendered = Traceback::from_exception(
                &class,
                &exc_type,
                &exc_value,
                &traceback,
                width,
                code_width,
                extra_lines,
                theme.clone(),
                word_wrap,
                show_locals,
                locals_max_length,
                locals_max_string,
                locals_max_depth,
                locals_hide_dunder,
                locals_hide_sunder.unwrap_or(false),
                locals_overflow.clone(),
                indent_guides,
                suppress.as_ref().map(|s| s.bind(py)),
                max_frames,
            )?;
            console
                .bind(py)
                .call_method1("print", (Py::new(py, rendered)?,))?;
            Ok(())
        },
    )?;
    let sys = py.import("sys")?;
    let old = sys.getattr("excepthook")?.unbind();
    sys.setattr("excepthook", hook)?;
    Ok(old)
}

/// `Console.print_exception(*, width=100, extra_lines=3, theme=None,
/// word_wrap=False, show_locals=False, suppress=(), max_frames=100)`.
pub(crate) fn print_exception(
    console: &Bound<'_, Console>,
    args: &Bound<'_, PyTuple>,
    kwargs: Option<&Bound<'_, PyDict>>,
) -> PyResult<Py<PyAny>> {
    let py = console.py();
    if !args.is_empty() {
        return Err(PyTypeError::new_err(format!(
            "Console.print_exception() takes 1 positional argument but {} were given",
            args.len() + 1
        )));
    }
    const ALLOWED: [&str; 7] = [
        "width",
        "extra_lines",
        "theme",
        "word_wrap",
        "show_locals",
        "suppress",
        "max_frames",
    ];
    if let Some(kwargs) = kwargs {
        for key in kwargs.keys() {
            let key: String = key.extract()?;
            if !ALLOWED.contains(&key.as_str()) {
                return Err(PyTypeError::new_err(format!(
                    "Console.print_exception() got an unexpected keyword argument '{key}'"
                )));
            }
        }
    }
    let traceback = py.get_type::<Traceback>().call((), kwargs)?;
    console.call_method1("print", (traceback,))?;
    Ok(py.None())
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    let py = m.py();
    let cwd = (|| -> PyResult<String> {
        let os = py.import("os")?;
        os.getattr("path")?
            .call_method1("abspath", (os.call_method0("getcwd")?,))?
            .extract()
    })()
    .unwrap_or_default();
    let _ = IMPORT_CWD.set(cwd);
    renderable::add_renderable_class::<Traceback>(m)?;
    m.add_class::<Frame>()?;
    m.add_class::<SyntaxErrorInfo>()?;
    m.add_class::<ExcStack>()?;
    m.add_class::<Trace>()?;
    m.add_function(wrap_pyfunction!(traceback_install, m)?)?;
    Ok(())
}
