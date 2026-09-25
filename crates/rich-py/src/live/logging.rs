//! `rich.logging.RichHandler`'s rendering and `rich._log_render.LogRender`.
//!
//! `RichHandler` itself is a `logging.Handler` subclass in the glue module
//! (it has to be a Python class); everything it shows is built here: the
//! message `Text` (markup, highlighter, keywords), the level text and the
//! log row, a core `Table.grid`.

use std::sync::{Arc, Mutex};

use pyo3::prelude::*;
use pyo3::types::{PyDict, PyString};
use pyo3::{PyTraverseError, PyVisit};

use rich::protocol::Highlighter;
use rich::table::{Cell, ColumnOptions};
use rich::{Overflow, ReprHighlighter, Style as CoreStyle, Table as CoreTable, Text as CoreText};

use super::progress::cell;
use super::util;
use crate::errors::MarkupError;
use crate::renderable::PyRenderable;
use crate::style::Style;
use crate::text::Text;

/// `console.get_style(name)` as a core style.
fn console_style(console: &Bound<'_, PyAny>, name: &str) -> PyResult<CoreStyle> {
    let style = console.call_method1("get_style", (name,))?;
    Ok(style.extract::<PyRef<'_, Style>>()?.inner.clone())
}

/// `rich._log_render.LogRender`: lays out one log row.
#[pyclass(name = "_LogRender", module = "rs_rich._log_render", frozen)]
pub(crate) struct LogRender {
    show_time: bool,
    show_level: bool,
    show_path: bool,
    time_format: Py<PyAny>,
    omit_repeated_times: bool,
    level_width: Option<usize>,
    last_time: Mutex<Option<CoreText>>,
}

#[pymethods]
impl LogRender {
    #[new]
    #[pyo3(signature = (
        show_time=true, show_level=false, show_path=true, time_format=None,
        omit_repeated_times=true, level_width=Some(8)
    ))]
    fn new(
        py: Python<'_>,
        show_time: bool,
        show_level: bool,
        show_path: bool,
        time_format: Option<Py<PyAny>>,
        omit_repeated_times: bool,
        level_width: Option<usize>,
    ) -> LogRender {
        LogRender {
            show_time,
            show_level,
            show_path,
            time_format: time_format
                .unwrap_or_else(|| PyString::new(py, "[%x %X]").into_any().unbind()),
            omit_repeated_times,
            level_width,
            last_time: Mutex::new(None),
        }
    }

    #[getter]
    fn time_format(&self, py: Python<'_>) -> Py<PyAny> {
        self.time_format.clone_ref(py)
    }

    /// The row for one record: time, level, the renderables and the path.
    #[pyo3(signature = (
        console, renderables, log_time=None, time_format=None, level=None, path=None,
        line_no=None, link_path=None
    ))]
    #[allow(clippy::too_many_arguments)]
    fn __call__(
        &self,
        py: Python<'_>,
        console: &Bound<'_, PyAny>,
        renderables: &Bound<'_, PyAny>,
        log_time: Option<Bound<'_, PyAny>>,
        time_format: Option<Bound<'_, PyAny>>,
        level: Option<Bound<'_, PyAny>>,
        path: Option<String>,
        line_no: Option<i64>,
        link_path: Option<String>,
    ) -> PyResult<Py<PyAny>> {
        let mut table = CoreTable::grid().padding(0, 1, 0, 1).expand(true);
        let column = |style: CoreStyle| ColumnOptions {
            style,
            ..ColumnOptions::default()
        };
        if self.show_time {
            table.add_column_with(
                CoreText::new(""),
                column(console_style(console, "log.time")?),
            );
        }
        if self.show_level {
            table.add_column_with(
                CoreText::new(""),
                ColumnOptions {
                    width: self.level_width,
                    ..column(console_style(console, "log.level")?)
                },
            );
        }
        table.add_column_with(
            CoreText::new(""),
            ColumnOptions {
                ratio: Some(1),
                overflow: Overflow::Fold,
                ..column(console_style(console, "log.message")?)
            },
        );
        let path = path.filter(|path| self.show_path && !path.is_empty());
        if path.is_some() {
            table.add_column_with(
                CoreText::new(""),
                column(console_style(console, "log.path")?),
            );
        }

        let mut row = Vec::new();
        if self.show_time {
            let log_time = match log_time.filter(|t| !t.is_none()) {
                Some(time) => time,
                None => util::console_datetime(console)?.call0()?,
            };
            let time_format = match time_format.filter(|f| f.is_truthy().unwrap_or(false)) {
                Some(format) => format,
                None => self.time_format.bind(py).clone(),
            };
            let display = if time_format.is_callable() {
                let shown = time_format.call1((log_time,))?;
                match util::core_text(&shown) {
                    Some(text) => text,
                    None => CoreText::new(util::to_str(&shown)?),
                }
            } else {
                CoreText::new(util::to_str(
                    &log_time.call_method1("strftime", (time_format,))?,
                )?)
            };
            let mut last = self.last_time.lock().unwrap_or_else(|p| p.into_inner());
            let repeated = last.as_ref().is_some_and(|last| {
                last.plain() == display.plain() && last.spans() == display.spans()
            });
            if repeated && self.omit_repeated_times {
                row.push(Cell::Text(CoreText::new(
                    " ".repeat(display.plain().chars().count()),
                )));
            } else {
                row.push(Cell::Text(display.clone()));
                *last = Some(display);
            }
        }
        if self.show_level {
            row.push(match level.filter(|l| !l.is_none()) {
                Some(level) => cell(py, &level)?,
                None => Cell::Markup(String::new()),
            });
        }
        let children: Vec<Bound<'_, PyAny>> = renderables.try_iter()?.collect::<PyResult<_>>()?;
        row.push(match children.as_slice() {
            [only] => cell(py, only)?,
            _ => {
                let group = util::group(py, children)?;
                Cell::Renderable(PyRenderable::shared(group, None))
            }
        });
        if let Some(path) = &path {
            let link = |target: String| CoreStyle::new().with_link(target);
            let mut path_text = CoreText::new("");
            path_text.append(
                path,
                link_path
                    .as_ref()
                    .map(|link_path| link(format!("file://{link_path}")).into()),
            );
            if let Some(line_no) = line_no.filter(|line| *line != 0) {
                path_text.append(":", None);
                path_text.append(
                    &line_no.to_string(),
                    link_path
                        .as_ref()
                        .map(|link_path| link(format!("file://{link_path}#{line_no}")).into()),
                );
            }
            row.push(Cell::Text(path_text));
        }
        table.add_row_cells(row);
        util::core_renderable(py, Arc::new(table))
    }

    fn __traverse__(&self, visit: PyVisit<'_>) -> Result<(), PyTraverseError> {
        visit.call(&self.time_format)
    }
}

/// `rich.highlighter.ReprHighlighter`, as the handler's default.
#[pyclass(name = "_ReprHighlighter", module = "rs_rich.logging", frozen)]
pub(crate) struct ReprHighlight;

#[pymethods]
impl ReprHighlight {
    #[new]
    fn new() -> ReprHighlight {
        ReprHighlight
    }

    /// Highlight `text` in place.
    fn highlight(&self, text: &Bound<'_, PyAny>) -> PyResult<()> {
        let mut text = text.extract::<PyRefMut<'_, Text>>()?;
        ReprHighlighter::new().highlight(&mut text.inner);
        Ok(())
    }

    /// A highlighted copy of `text` (a `str` or `Text`).
    fn __call__(&self, py: Python<'_>, text: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        let mut inner = match util::core_text(text) {
            Some(inner) => inner,
            None if util::is_str(text) => CoreText::new(util::to_str(text)?),
            None => {
                return Err(pyo3::exceptions::PyTypeError::new_err(
                    "highlight() requires a str or Text",
                ))
            }
        };
        ReprHighlighter::new().highlight(&mut inner);
        util::new_text(py, inner)
    }
}

/// `RichHandler.get_level_text`: the level name, padded to 8, styled
/// `logging.level.<name>`.
#[pyfunction]
fn _rich_handler_level_text(py: Python<'_>, record: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
    let name = util::to_str(&record.getattr("levelname")?)?;
    util::new_text(py, rich::level_text(&name))
}

/// `RichHandler.render_message`: the message as `Text` (markup when the
/// handler or record asks), highlighted, with the keywords styled.
#[pyfunction]
fn _rich_handler_render_message(
    py: Python<'_>,
    handler: &Bound<'_, PyAny>,
    record: &Bound<'_, PyAny>,
    message: &str,
) -> PyResult<Py<PyAny>> {
    let builtins = py.import("builtins")?;
    let getattr = builtins.getattr("getattr")?;
    let use_markup = getattr
        .call1((record, "markup", handler.getattr("markup")?))?
        .is_truthy()?;
    let text = if use_markup {
        CoreText::from_markup(&rich::emoji::replace(message))
            .map_err(|e| MarkupError::new_err(e.to_string()))?
    } else {
        CoreText::new(message)
    };
    let mut message_text = util::new_text(py, text)?.into_bound(py);
    let highlighter = getattr.call1((record, "highlighter", handler.getattr("highlighter")?))?;
    if highlighter.is_truthy()? {
        message_text = highlighter.call1((message_text,))?;
    }
    if handler.getattr("keywords")?.is_none() {
        handler.setattr("keywords", handler.getattr("KEYWORDS")?)?;
    }
    let keywords = handler.getattr("keywords")?;
    if keywords.is_truthy()? {
        let words: Vec<String> = keywords
            .try_iter()?
            .map(|word| word.and_then(|w| util::to_str(&w)))
            .collect::<PyResult<_>>()?;
        let words: Vec<&str> = words.iter().map(String::as_str).collect();
        if let Ok(mut text) = message_text.extract::<PyRefMut<'_, Text>>() {
            text.inner
                .highlight_words(&words, "logging.keyword", true)
                .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?;
        } else {
            let kwargs = PyDict::new(py);
            message_text.call_method(
                "highlight_words",
                (keywords, "logging.keyword"),
                Some(&kwargs),
            )?;
        }
    }
    Ok(message_text.unbind())
}

/// `RichHandler.render`: the log row for a record.
#[pyfunction]
fn _rich_handler_render(
    py: Python<'_>,
    handler: &Bound<'_, PyAny>,
    record: &Bound<'_, PyAny>,
    traceback: &Bound<'_, PyAny>,
    message_renderable: &Bound<'_, PyAny>,
) -> PyResult<Py<PyAny>> {
    let pathname = record.getattr("pathname")?;
    let path = py
        .import("os")?
        .getattr("path")?
        .call_method1("basename", (&pathname,))?;
    let level = handler.call_method1("get_level_text", (record,))?;
    let formatter = handler.getattr("formatter")?;
    let time_format = if formatter.is_none() {
        py.None().into_bound(py)
    } else {
        formatter.getattr("datefmt")?
    };
    let log_time = py
        .import("datetime")?
        .getattr("datetime")?
        .call_method1("fromtimestamp", (record.getattr("created")?,))?;
    let renderables = if traceback.is_none() {
        pyo3::types::PyList::new(py, [message_renderable])?
    } else {
        pyo3::types::PyList::new(py, [message_renderable, traceback])?
    };
    let kwargs = PyDict::new(py);
    kwargs.set_item("log_time", log_time)?;
    kwargs.set_item("time_format", time_format)?;
    kwargs.set_item("level", level)?;
    kwargs.set_item("path", path)?;
    kwargs.set_item("line_no", record.getattr("lineno")?)?;
    let link = if handler.getattr("enable_link_path")?.is_truthy()? {
        pathname.unbind()
    } else {
        py.None()
    };
    kwargs.set_item("link_path", link)?;
    Ok(handler
        .getattr("_log_render")?
        .call((handler.getattr("console")?, renderables), Some(&kwargs))?
        .unbind())
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<LogRender>()?;
    m.add_class::<ReprHighlight>()?;
    m.add_function(pyo3::wrap_pyfunction!(_rich_handler_level_text, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(_rich_handler_render_message, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(_rich_handler_render, m)?)?;
    Ok(())
}
