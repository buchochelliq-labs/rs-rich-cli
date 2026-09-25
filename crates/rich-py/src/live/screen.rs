//! `rich.screen.Screen`, the context `Console.screen()` returns, and
//! `rich.pager` (`Pager`, `SystemPager`) with the context `Console.pager()`
//! returns.

use std::sync::{Mutex, MutexGuard};

use pyo3::exceptions::PyNotImplementedError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList, PyString, PyTuple};
use pyo3::{PyTraverseError, PyVisit};

use rich::segment::Segment as CoreSegment;

use super::util;
use crate::console::Console;
use crate::segment::Segment;
use crate::style::Style;

struct ScreenState {
    renderable: Py<PyAny>,
    style: Option<Py<PyAny>>,
    application_mode: bool,
}

/// `rich.screen.Screen(*renderables, style=None, application_mode=False)`:
/// fills the terminal (the console's size) and crops what does not fit.
#[pyclass(name = "Screen", module = "rs_rich.screen", frozen)]
pub(crate) struct Screen {
    state: Mutex<ScreenState>,
}

impl Screen {
    fn st(&self) -> MutexGuard<'_, ScreenState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// `Screen(renderable)`.
    pub(crate) fn wrap(py: Python<'_>, renderable: Py<PyAny>) -> PyResult<Py<Screen>> {
        Py::new(
            py,
            Screen {
                state: Mutex::new(ScreenState {
                    renderable,
                    style: None,
                    application_mode: false,
                }),
            },
        )
    }

    fn set_renderable(&self, renderable: Py<PyAny>) {
        self.st().renderable = renderable;
    }

    fn set_style_value(&self, style: Option<Py<PyAny>>) {
        self.st().style = style;
    }
}

#[pymethods]
impl Screen {
    #[new]
    #[pyo3(signature = (*renderables, style=None, application_mode=false))]
    fn new(
        py: Python<'_>,
        renderables: &Bound<'_, PyTuple>,
        style: Option<Py<PyAny>>,
        application_mode: bool,
    ) -> PyResult<Screen> {
        let children: Vec<_> = renderables.iter().collect();
        let renderable = if children.is_empty() {
            PyString::new(py, "").into_any().unbind()
        } else {
            util::group(py, children)?
        };
        Ok(Screen {
            state: Mutex::new(ScreenState {
                renderable,
                style: style.filter(|s| !s.is_none(py)),
                application_mode,
            }),
        })
    }

    #[getter]
    fn renderable(&self, py: Python<'_>) -> Py<PyAny> {
        self.st().renderable.clone_ref(py)
    }

    #[setter(renderable)]
    fn set_renderable_attr(&self, renderable: Py<PyAny>) {
        self.set_renderable(renderable);
    }

    #[getter]
    fn style(&self, py: Python<'_>) -> Py<PyAny> {
        self.st()
            .style
            .as_ref()
            .map_or_else(|| py.None(), |s| s.clone_ref(py))
    }

    #[setter]
    fn set_style(&self, py: Python<'_>, style: Option<Py<PyAny>>) {
        self.set_style_value(style.filter(|s| !s.is_none(py)));
    }

    #[getter]
    fn application_mode(&self) -> bool {
        self.st().application_mode
    }

    #[setter]
    fn set_application_mode(&self, value: bool) {
        self.st().application_mode = value;
    }

    fn __rich_console__<'py>(
        &self,
        console: &Bound<'py, PyAny>,
        options: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyList>> {
        let py = console.py();
        let (renderable, style, application_mode) = {
            let state = self.st();
            (
                state.renderable.clone_ref(py),
                state.style.as_ref().map(|s| s.clone_ref(py)),
                state.application_mode,
            )
        };
        let size = options.getattr("size")?;
        let width: usize = size.get_item(0)?.extract()?;
        let height: usize = size.get_item(1)?.extract()?;
        let style = match style {
            Some(style) if style.bind(py).is_truthy()? => {
                Some(console.call_method1("get_style", (style,))?)
            }
            _ => None,
        };
        let render_options = options.call_method1("update_dimensions", (width, height))?;
        let renderable = renderable.bind(py);
        let renderable = if renderable.is_truthy()? {
            renderable.clone()
        } else {
            PyString::new(py, "").into_any()
        };
        let kwargs = PyDict::new(py);
        kwargs.set_item("style", style.as_ref())?;
        kwargs.set_item("pad", true)?;
        let lines = console.call_method("render_lines", (renderable, render_options), Some(&kwargs))?;
        let core_style = match &style {
            Some(style) => Some(style.extract::<PyRef<'_, Style>>()?.inner.clone()),
            None => None,
        };
        // `Segment.set_shape(lines, width, height, style=style)`.
        let mut lines: Vec<Vec<CoreSegment>> = util::core_lines(&lines)?
            .into_iter()
            .take(height)
            .map(|line| CoreSegment::adjust_line_length(&line, width, core_style.clone()))
            .collect();
        while lines.len() < height {
            lines.push(vec![CoreSegment::new(" ".repeat(width), core_style.clone())]);
        }
        let new_line = if application_mode {
            CoreSegment::new("\n\r", None)
        } else {
            CoreSegment::line()
        };
        let result = PyList::empty(py);
        let last = lines.len().saturating_sub(1);
        for (index, line) in lines.iter().enumerate() {
            for segment in line {
                result.append(Segment::from_core(py, segment))?;
            }
            if index != last {
                result.append(Segment::from_core(py, &new_line))?;
            }
        }
        Ok(result)
    }

    fn __traverse__(&self, visit: PyVisit<'_>) -> Result<(), PyTraverseError> {
        if let Ok(state) = self.state.try_lock() {
            visit.call(&state.renderable)?;
            if let Some(style) = &state.style {
                visit.call(style)?;
            }
        }
        Ok(())
    }
}

/// `rich.console.ScreenContext`: what `Console.screen()` returns.
#[pyclass(name = "ScreenContext", module = "rs_rich.console", frozen)]
pub(crate) struct ScreenContext {
    console: Py<PyAny>,
    hide_cursor: bool,
    screen: Py<Screen>,
    changed: Mutex<bool>,
}

#[pymethods]
impl ScreenContext {
    #[new]
    #[pyo3(signature = (console, hide_cursor, style=None))]
    fn new(
        py: Python<'_>,
        console: Py<PyAny>,
        hide_cursor: bool,
        style: Option<Py<PyAny>>,
    ) -> PyResult<ScreenContext> {
        let style = style.unwrap_or_else(|| PyString::new(py, "").into_any().unbind());
        let screen = Screen::wrap(py, PyString::new(py, "").into_any().unbind())?;
        screen.get().set_style_value(Some(style));
        Ok(ScreenContext {
            console,
            hide_cursor,
            screen,
            changed: Mutex::new(false),
        })
    }

    #[getter]
    fn console(&self, py: Python<'_>) -> Py<PyAny> {
        self.console.clone_ref(py)
    }

    #[getter]
    fn hide_cursor(&self) -> bool {
        self.hide_cursor
    }

    #[getter]
    fn screen(&self, py: Python<'_>) -> Py<Screen> {
        self.screen.clone_ref(py)
    }

    /// Show new renderables (and / or a new style) on the screen.
    #[pyo3(signature = (*renderables, style=None))]
    fn update(
        &self,
        py: Python<'_>,
        renderables: &Bound<'_, PyTuple>,
        style: Option<Py<PyAny>>,
    ) -> PyResult<()> {
        let screen = self.screen.get();
        if !renderables.is_empty() {
            let children: Vec<_> = renderables.iter().collect();
            screen.set_renderable(util::group(py, children)?);
        }
        if let Some(style) = style.filter(|s| !s.is_none(py)) {
            screen.set_style_value(Some(style));
        }
        let kwargs = PyDict::new(py);
        kwargs.set_item("end", "")?;
        self.console
            .bind(py)
            .call_method("print", (self.screen.clone_ref(py),), Some(&kwargs))?;
        Ok(())
    }

    fn __enter__(slf: Bound<'_, Self>) -> PyResult<Bound<'_, Self>> {
        let py = slf.py();
        let this = slf.get();
        let console = this.console.bind(py);
        let changed = console.call_method1("set_alt_screen", (true,))?.is_truthy()?;
        *this.changed.lock().unwrap_or_else(|p| p.into_inner()) = changed;
        if changed && this.hide_cursor {
            console.call_method1("show_cursor", (false,))?;
        }
        Ok(slf)
    }

    #[pyo3(signature = (*_args))]
    fn __exit__(&self, py: Python<'_>, _args: &Bound<'_, PyTuple>) -> PyResult<()> {
        let changed = *self.changed.lock().unwrap_or_else(|p| p.into_inner());
        if changed {
            let console = self.console.bind(py);
            console.call_method1("set_alt_screen", (false,))?;
            if self.hide_cursor {
                console.call_method1("show_cursor", (true,))?;
            }
        }
        Ok(())
    }

    fn __traverse__(&self, visit: PyVisit<'_>) -> Result<(), PyTraverseError> {
        visit.call(&self.console)?;
        visit.call(&self.screen)
    }
}

/// `rich.pager.Pager`: the base class of a pager (implement `show`).
#[pyclass(name = "Pager", module = "rs_rich.pager", subclass)]
pub(crate) struct Pager;

#[pymethods]
impl Pager {
    #[new]
    #[pyo3(signature = (*_args, **_kwargs))]
    fn new(_args: &Bound<'_, PyTuple>, _kwargs: Option<&Bound<'_, PyDict>>) -> Pager {
        Pager
    }

    /// Show `content` in the pager.
    fn show(&self, _content: &str) -> PyResult<()> {
        Err(PyNotImplementedError::new_err(
            "Pager.show is abstract: subclass Pager and implement show(content)",
        ))
    }
}

/// `rich.pager.SystemPager`: the pager `pydoc` uses.
#[pyclass(name = "SystemPager", module = "rs_rich.pager", extends = Pager, subclass)]
pub(crate) struct SystemPager;

#[pymethods]
impl SystemPager {
    #[new]
    #[pyo3(signature = (*_args, **_kwargs))]
    fn new(
        _args: &Bound<'_, PyTuple>,
        _kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyClassInitializer<SystemPager> {
        PyClassInitializer::from(Pager).add_subclass(SystemPager)
    }

    fn _pager(&self, py: Python<'_>, content: &str) -> PyResult<Py<PyAny>> {
        Ok(py.import("pydoc")?.call_method1("pager", (content,))?.unbind())
    }

    fn show(slf: &Bound<'_, Self>, content: &str) -> PyResult<()> {
        slf.call_method1("_pager", (content,))?;
        Ok(())
    }
}

/// Remove the escape sequences a print of styled segments writes: SGR
/// (`ESC [ ... m`) and, with `links`, only OSC 8 hyperlinks.
fn strip_codes(content: &str, styles: bool, links: bool) -> String {
    let mut out = String::with_capacity(content.len());
    let mut rest = content;
    while let Some(index) = rest.find('\x1b') {
        out.push_str(&rest[..index]);
        let tail = &rest[index..];
        if !styles && tail.starts_with("\x1b[") {
            // A CSI sequence: parameters then a final byte.
            let body = &tail[2..];
            let end = body
                .char_indices()
                .find(|(_, c)| ('\x40'..='\x7e').contains(c))
                .map(|(i, c)| (i, c));
            if let Some((i, 'm')) = end {
                rest = &body[i + 1..];
                continue;
            }
        }
        if (!styles || !links) && tail.starts_with("\x1b]8;") {
            if let Some(end) = tail.find("\x1b\\") {
                rest = &tail[end + 2..];
                continue;
            }
        }
        out.push('\x1b');
        rest = &tail[1..];
    }
    out.push_str(rest);
    out
}

/// `rich.console.PagerContext`: what `Console.pager()` returns. Output
/// printed inside it is shown in the pager when the block ends.
#[pyclass(name = "PagerContext", module = "rs_rich.console", frozen)]
pub(crate) struct PagerContext {
    console: Py<PyAny>,
    pager: Py<PyAny>,
    styles: bool,
    links: bool,
}

#[pymethods]
impl PagerContext {
    #[new]
    #[pyo3(signature = (console, pager=None, styles=false, links=false))]
    fn new(
        py: Python<'_>,
        console: Py<PyAny>,
        pager: Option<Py<PyAny>>,
        styles: bool,
        links: bool,
    ) -> PyResult<PagerContext> {
        let pager = match pager.filter(|p| !p.is_none(py)) {
            Some(pager) => pager,
            None => py.get_type::<SystemPager>().call0()?.unbind(),
        };
        Ok(PagerContext {
            console,
            pager,
            styles,
            links,
        })
    }

    #[getter]
    fn pager(&self, py: Python<'_>) -> Py<PyAny> {
        self.pager.clone_ref(py)
    }

    #[getter]
    fn styles(&self) -> bool {
        self.styles
    }

    #[getter]
    fn links(&self) -> bool {
        self.links
    }

    fn __enter__(slf: Bound<'_, Self>) -> PyResult<Bound<'_, Self>> {
        slf.get()
            .console
            .bind(slf.py())
            .call_method0("begin_capture")?;
        Ok(slf)
    }

    #[pyo3(signature = (exc_type=None, _exc_value=None, _traceback=None))]
    fn __exit__(
        &self,
        py: Python<'_>,
        exc_type: Option<&Bound<'_, PyAny>>,
        _exc_value: Option<&Bound<'_, PyAny>>,
        _traceback: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<()> {
        let content: String = self
            .console
            .bind(py)
            .call_method0("end_capture")?
            .extract()?;
        if exc_type.is_none_or(|t| t.is_none()) {
            let content = strip_codes(&content, self.styles, self.links);
            self.pager.bind(py).call_method1("show", (content,))?;
        }
        Ok(())
    }

    fn __traverse__(&self, visit: PyVisit<'_>) -> Result<(), PyTraverseError> {
        visit.call(&self.console)?;
        visit.call(&self.pager)
    }
}

/// `Console.screen(hide_cursor=True, style=None)`.
pub(crate) fn console_screen(
    console: &Bound<'_, Console>,
    args: &Bound<'_, PyTuple>,
    kwargs: Option<&Bound<'_, PyDict>>,
) -> PyResult<Py<PyAny>> {
    let py = console.py();
    let call_kwargs = PyDict::new(py);
    let names = ["hide_cursor", "style"];
    if args.len() > names.len() {
        return Err(pyo3::exceptions::PyTypeError::new_err(format!(
            "screen() takes at most 2 arguments ({} given)",
            args.len()
        )));
    }
    for (name, value) in names.iter().zip(args.iter()) {
        call_kwargs.set_item(name, value)?;
    }
    if let Some(kwargs) = kwargs {
        call_kwargs.update(kwargs.as_mapping())?;
    }
    let hide_cursor = match call_kwargs.get_item("hide_cursor")? {
        Some(value) => value.is_truthy()?,
        None => true,
    };
    let style = call_kwargs
        .get_item("style")?
        .filter(|s| !s.is_none())
        .map(|s| s.unbind());
    let context = ScreenContext::new(py, console.clone().into_any().unbind(), hide_cursor, style)?;
    Ok(Py::new(py, context)?.into_any())
}

/// `Console.pager(pager=None, styles=False, links=False)`.
pub(crate) fn console_pager(
    console: &Bound<'_, Console>,
    args: &Bound<'_, PyTuple>,
    kwargs: Option<&Bound<'_, PyDict>>,
) -> PyResult<Py<PyAny>> {
    let py = console.py();
    let mut all = vec![console.clone().into_any()];
    all.extend(args.iter());
    let args = PyTuple::new(py, all)?;
    Ok(py.get_type::<PagerContext>().call(args, kwargs)?.unbind())
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Screen>()?;
    m.add_class::<ScreenContext>()?;
    m.add_class::<Pager>()?;
    m.add_class::<SystemPager>()?;
    m.add_class::<PagerContext>()?;
    Ok(())
}
