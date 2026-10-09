//! Other programs and web pages in an app, from Python (`rs-rich-embed`):
//! `terminal` (a terminal pane), `web_view` (a page shown by a terminal
//! browser), and the replay host tests use.
//!
//! Bound: the local PTY and the replay host behind a pane, and the program
//! engine (a terminal browser such as w3m, Carbonyl, Browsh or Chawan)
//! behind a web view. Not bound: a `PtyHost` or `WebEngine` written in
//! Python, and the Chrome and Browsh engines (their crate features are off
//! in this wheel).

use std::cell::RefCell;

use pyo3::exceptions::{PyRuntimeError, PyTypeError};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyString, PyTuple};

use rich_embed::{
    Command, ExitStatus, ProgramEngine, PtyHost, ReplayHandle, ReplayHost, TerminalPane, WebHandle,
    WebView,
};
use rich_intuituive::Node;

use super::node::PyNode;
use super::reactive::{call_with_ctx, PySignal, PyValue, Slot};
use super::{require, Callback};

/// `ExitStatus`: how a program ended: its `code` (1 when a signal ended
/// it), the `signal`'s name if one did, and whether it was a `success`.
#[pyclass(name = "ExitStatus", module = "rs_rich.tui", frozen)]
pub(crate) struct PyExitStatus(pub(crate) ExitStatus);

#[pymethods]
impl PyExitStatus {
    #[getter]
    fn code(&self) -> u32 {
        self.0.code()
    }

    #[getter]
    fn signal(&self) -> Option<&str> {
        self.0.signal()
    }

    #[getter]
    fn success(&self) -> bool {
        self.0.success()
    }

    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        other
            .cast::<PyExitStatus>()
            .is_ok_and(|other| other.get().0 == self.0)
    }

    fn __repr__(&self) -> String {
        format!("<ExitStatus {}>", self.0)
    }
}

fn status_arg(value: &Bound<'_, PyAny>) -> PyResult<ExitStatus> {
    if let Ok(status) = value.cast::<PyExitStatus>() {
        return Ok(status.get().0.clone());
    }
    if let Ok(signal) = value.cast::<PyString>() {
        return Ok(ExitStatus::with_signal(signal.to_cow()?.into_owned()));
    }
    let code: u32 = value
        .extract()
        .map_err(|_| PyTypeError::new_err("an exit status is a code (int) or a signal's name"))?;
    Ok(ExitStatus::with_code(code))
}

/// A command: a program name, or a list of the program and its arguments.
fn command_arg(value: &Bound<'_, PyAny>) -> PyResult<Command> {
    if let Ok(program) = value.cast::<PyString>() {
        return Ok(Command::new(program.to_cow()?.into_owned()));
    }
    let words: Vec<String> = value
        .extract()
        .map_err(|_| PyTypeError::new_err("a command is a str or a list of str"))?;
    let (program, args) = words
        .split_first()
        .ok_or_else(|| PyTypeError::new_err("a command needs a program"))?;
    Ok(Command::new(program.clone()).args(args.iter().cloned()))
}

// ---------------------------------------------------------------------------
// Replay hosts

/// `ReplayHost(output=b"", exit=None)`: a pretend program for tests: it
/// plays `output` once it starts, then exits with `exit` (a code or a
/// signal's name) if given. `handle()` feeds it more and reads what the
/// pane sent.
#[pyclass(name = "ReplayHost", module = "rs_rich.tui", unsendable)]
pub(crate) struct PyReplayHost(RefCell<Option<ReplayHost>>);

#[pymethods]
impl PyReplayHost {
    #[new]
    #[pyo3(signature = (output=None, exit=None))]
    fn new(output: Option<&Bound<'_, PyAny>>, exit: Option<&Bound<'_, PyAny>>) -> PyResult<Self> {
        let mut host = ReplayHost::new();
        if let Some(output) = output.filter(|o| !o.is_none()) {
            host = host.output(bytes_arg(output)?);
        }
        if let Some(exit) = exit.filter(|e| !e.is_none()) {
            host = host.exit(status_arg(exit)?);
        }
        Ok(PyReplayHost(RefCell::new(Some(host))))
    }

    fn handle(&self) -> PyResult<PyReplayHandle> {
        self.0
            .borrow()
            .as_ref()
            .map(|host| PyReplayHandle(host.handle()))
            .ok_or_else(|| PyRuntimeError::new_err("this host is in a pane"))
    }
}

fn bytes_arg(value: &Bound<'_, PyAny>) -> PyResult<Vec<u8>> {
    if let Ok(text) = value.cast::<PyString>() {
        return Ok(text.to_cow()?.as_bytes().to_vec());
    }
    value
        .extract::<Vec<u8>>()
        .map_err(|_| PyTypeError::new_err("output is bytes or a str"))
}

/// `ReplayHandle`: feeds a `ReplayHost` (from any thread) and reads back
/// what the pane sent it.
#[pyclass(name = "ReplayHandle", module = "rs_rich.tui", frozen)]
pub(crate) struct PyReplayHandle(ReplayHandle);

#[pymethods]
impl PyReplayHandle {
    /// More output from the program.
    fn feed(&self, output: &Bound<'_, PyAny>) -> PyResult<()> {
        self.0.feed(bytes_arg(output)?);
        Ok(())
    }

    /// The program exits with `status` (a code or a signal's name).
    fn exit(&self, status: &Bound<'_, PyAny>) -> PyResult<()> {
        self.0.exit(status_arg(status)?);
        Ok(())
    }

    /// Every byte the pane sent: keys, the mouse, pastes.
    fn written<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        PyBytes::new(py, &self.0.written())
    }

    /// The size it started at, once it has.
    fn started(&self) -> Option<(u16, u16)> {
        self.0.started()
    }

    /// Every resize after the start.
    fn sizes(&self) -> Vec<(u16, u16)> {
        self.0.sizes()
    }

    fn killed(&self) -> bool {
        self.0.killed()
    }
}

// ---------------------------------------------------------------------------
// Terminal panes

/// `TerminalPane`: a program running in a pane. `on_exit(handler)` runs
/// `handler(status, cx)` when it exits; `status` is a signal of its
/// `ExitStatus` (`None` while it runs); `scrollback(rows)` and
/// `release_keys(keys)` (keys that go to the app, not the program) set it
/// up; `node()` (or using it as a node) puts it in the tree.
#[pyclass(name = "TerminalPane", module = "rs_rich.tui", unsendable)]
pub(crate) struct PyTerminalPane(RefCell<Option<TerminalPane>>);

fn pane_used() -> PyErr {
    PyRuntimeError::new_err("this pane is already in a tree")
}

impl PyTerminalPane {
    fn map(&self, f: impl FnOnce(TerminalPane) -> TerminalPane) -> PyResult<()> {
        let pane = self.0.borrow_mut().take().ok_or_else(pane_used)?;
        *self.0.borrow_mut() = Some(f(pane));
        Ok(())
    }
}

#[pymethods]
impl PyTerminalPane {
    fn on_exit<'py>(
        slf: Bound<'py, Self>,
        handler: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, Self>> {
        let handler = Callback::checked(handler, "on_exit's handler")?;
        slf.borrow().map(|pane| {
            pane.on_exit(move |status, cx| {
                let status = Python::attach(|py| {
                    Py::new(py, PyExitStatus(status)).map(|s| PyValue(s.into_any()))
                });
                match status {
                    Ok(status) => call_with_ctx(&handler, Some(status), cx),
                    Err(error) => {
                        Python::attach(|py| super::fail(py, handler.app().as_ref(), error))
                    }
                }
            })
        })?;
        Ok(slf)
    }

    /// How the program ended, as a signal: `None` while it runs.
    #[getter]
    fn status(&self) -> PyResult<PySignal> {
        let pane = self.0.borrow();
        let pane = pane.as_ref().ok_or_else(pane_used)?;
        Ok(PySignal::wrap(Slot::Exit(pane.status())))
    }

    fn scrollback(slf: Bound<'_, Self>, rows: usize) -> PyResult<Bound<'_, Self>> {
        slf.borrow().map(|pane| pane.scrollback(rows))?;
        Ok(slf)
    }

    fn release_keys<'py>(slf: Bound<'py, Self>, keys: &str) -> PyResult<Bound<'py, Self>> {
        super::keys(keys)?;
        slf.borrow().map(|pane| pane.release_keys(keys))?;
        Ok(slf)
    }

    /// The pane as a node.
    fn node(&self) -> PyResult<PyNode> {
        let pane = self.0.borrow_mut().take().ok_or_else(pane_used)?;
        Ok(PyNode::new(pane.node()))
    }
}

/// `terminal(command)`: a pane running `command` (a program, or a list of
/// the program and its arguments) on this machine, in a PTY (ConPTY on
/// Windows).
#[pyfunction]
fn tui_terminal(command: &Bound<'_, PyAny>) -> PyResult<PyTerminalPane> {
    require("terminal()")?;
    let command = command_arg(command)?;
    Ok(PyTerminalPane(RefCell::new(Some(rich_embed::terminal(
        command,
    )))))
}

/// `terminal_with(host)`: a pane over `host`, a `ReplayHost` (the host is
/// used up).
#[pyfunction]
fn tui_terminal_with(host: &Bound<'_, PyReplayHost>) -> PyResult<PyTerminalPane> {
    require("terminal_with()")?;
    let host = host
        .borrow()
        .0
        .borrow_mut()
        .take()
        .ok_or_else(|| PyRuntimeError::new_err("this host is already in a pane"))?;
    Ok(PyTerminalPane(RefCell::new(Some(
        rich_embed::terminal_with(host),
    ))))
}

// ---------------------------------------------------------------------------
// Web views

/// `ProgramEngine(program, args=())`: a terminal browser behind a web view,
/// started with the address as its last argument. `ProgramEngine.detect()`
/// takes the one `RICH_BROWSER` names, else the first known one on `PATH`;
/// `ProgramEngine.with_hosts(make)` runs each page in the `ReplayHost`
/// `make(url)` returns (for tests).
#[pyclass(name = "ProgramEngine", module = "rs_rich.tui", unsendable)]
pub(crate) struct PyProgramEngine(RefCell<Option<ProgramEngine>>);

#[pymethods]
impl PyProgramEngine {
    #[new]
    #[pyo3(signature = (program, args=None))]
    fn new(program: &str, args: Option<Vec<String>>) -> PyProgramEngine {
        let engine = ProgramEngine::new(program).args(args.unwrap_or_default());
        PyProgramEngine(RefCell::new(Some(engine)))
    }

    #[staticmethod]
    fn detect() -> PyProgramEngine {
        PyProgramEngine(RefCell::new(Some(ProgramEngine::detect())))
    }

    #[staticmethod]
    fn with_hosts(make: &Bound<'_, PyAny>) -> PyResult<PyProgramEngine> {
        let make = Callback::checked(make, "with_hosts' function")?;
        let engine = ProgramEngine::with_hosts(move |url: &str| -> Box<dyn PtyHost> {
            let url = url.to_string();
            let host = make.call(
                |py| PyTuple::new(py, [url]),
                |value| {
                    let host = value.cast::<PyReplayHost>().map_err(|_| {
                        PyTypeError::new_err("with_hosts' function returns a ReplayHost")
                    })?;
                    let taken = host.borrow().0.borrow_mut().take();
                    taken.ok_or_else(|| PyRuntimeError::new_err("this host is already in a pane"))
                },
            );
            Box::new(host.unwrap_or_default())
        });
        Ok(PyProgramEngine(RefCell::new(Some(engine))))
    }
}

/// `WebView`: a web page in a pane. `handle()` has its signals and
/// `open`, `back`, `forward`, `reload`; `address_bar(on)` and
/// `release_keys(keys)` set it up; `node()` (or using it as a node) puts it
/// in the tree.
#[pyclass(name = "WebView", module = "rs_rich.tui", unsendable)]
pub(crate) struct PyWebView(RefCell<Option<WebView>>);

impl PyWebView {
    fn map(&self, f: impl FnOnce(WebView) -> WebView) -> PyResult<()> {
        let view = self.0.borrow_mut().take().ok_or_else(pane_used)?;
        *self.0.borrow_mut() = Some(f(view));
        Ok(())
    }
}

#[pymethods]
impl PyWebView {
    fn handle(&self) -> PyResult<PyWebHandle> {
        let view = self.0.borrow();
        let view = view.as_ref().ok_or_else(pane_used)?;
        Ok(PyWebHandle(view.handle(), super::Home::here()))
    }

    /// Show the address bar (default) or not.
    #[pyo3(signature = (on=true))]
    fn address_bar(slf: Bound<'_, Self>, on: bool) -> PyResult<Bound<'_, Self>> {
        slf.borrow().map(|view| view.address_bar(on))?;
        Ok(slf)
    }

    fn release_keys<'py>(slf: Bound<'py, Self>, keys: &str) -> PyResult<Bound<'py, Self>> {
        super::keys(keys)?;
        slf.borrow().map(|view| view.release_keys(keys))?;
        Ok(slf)
    }

    fn node(&self) -> PyResult<PyNode> {
        let view = self.0.borrow_mut().take().ok_or_else(pane_used)?;
        Ok(PyNode::new(view.node()))
    }
}

/// `WebHandle`: a web view's state as signals (`address`, `title`,
/// `loading`, `can_go_back`, `can_go_forward`, `error`; setting `address`
/// opens it), and `open(url)`, `back()`, `forward()`, `reload()`.
#[pyclass(name = "WebHandle", module = "rs_rich.tui", frozen)]
pub(crate) struct PyWebHandle(WebHandle, super::Home);

impl PyWebHandle {
    fn handle(&self) -> PyResult<WebHandle> {
        self.1.check("web view's handle")?;
        Ok(self.0)
    }
}

#[pymethods]
impl PyWebHandle {
    #[getter]
    fn address(&self) -> PyResult<PySignal> {
        Ok(PySignal::wrap(Slot::Str(self.handle()?.address())))
    }

    #[getter]
    fn title(&self) -> PyResult<PySignal> {
        Ok(PySignal::wrap(Slot::Str(self.handle()?.title())))
    }

    #[getter]
    fn loading(&self) -> PyResult<PySignal> {
        Ok(PySignal::wrap(Slot::Bool(self.handle()?.loading())))
    }

    #[getter]
    fn can_go_back(&self) -> PyResult<PySignal> {
        Ok(PySignal::wrap(Slot::Bool(self.handle()?.can_go_back())))
    }

    #[getter]
    fn can_go_forward(&self) -> PyResult<PySignal> {
        Ok(PySignal::wrap(Slot::Bool(self.handle()?.can_go_forward())))
    }

    #[getter]
    fn error(&self) -> PyResult<PySignal> {
        Ok(PySignal::wrap(Slot::Key(self.handle()?.error())))
    }

    fn open(&self, url: &str) -> PyResult<()> {
        self.handle()?.open(url);
        Ok(())
    }

    fn back(&self) -> PyResult<()> {
        self.handle()?.back();
        Ok(())
    }

    fn forward(&self) -> PyResult<()> {
        self.handle()?.forward();
        Ok(())
    }

    fn reload(&self) -> PyResult<()> {
        self.handle()?.reload();
        Ok(())
    }
}

/// `web_view(url, engine=None)`: the page at `url` shown by `engine` (a
/// `ProgramEngine`, used up), else by the terminal browser
/// `ProgramEngine.detect()` finds.
#[pyfunction]
#[pyo3(signature = (url, engine=None))]
fn tui_web_view(url: &str, engine: Option<&Bound<'_, PyProgramEngine>>) -> PyResult<PyWebView> {
    require("web_view()")?;
    let view =
        match engine {
            Some(engine) => {
                let engine =
                    engine.borrow().0.borrow_mut().take().ok_or_else(|| {
                        PyRuntimeError::new_err("this engine is already in a view")
                    })?;
                rich_embed::web_view_with(engine, url)
            }
            None => rich_embed::web_view(url),
        };
    Ok(PyWebView(RefCell::new(Some(view))))
}

/// A pane or view given where a node goes: its node.
pub(crate) fn take_embedded(value: &Bound<'_, PyAny>) -> PyResult<Option<Node>> {
    if let Ok(pane) = value.cast::<PyTerminalPane>() {
        let pane = pane.borrow().0.borrow_mut().take().ok_or_else(pane_used)?;
        return Ok(Some(pane.node()));
    }
    if let Ok(view) = value.cast::<PyWebView>() {
        let view = view.borrow().0.borrow_mut().take().ok_or_else(pane_used)?;
        return Ok(Some(view.node()));
    }
    Ok(None)
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    let py = m.py();
    for (name, class) in [
        ("TuiExitStatus", py.get_type::<PyExitStatus>()),
        ("TuiReplayHost", py.get_type::<PyReplayHost>()),
        ("TuiReplayHandle", py.get_type::<PyReplayHandle>()),
        ("TuiTerminalPane", py.get_type::<PyTerminalPane>()),
        ("TuiProgramEngine", py.get_type::<PyProgramEngine>()),
        ("TuiWebView", py.get_type::<PyWebView>()),
        ("TuiWebHandle", py.get_type::<PyWebHandle>()),
    ] {
        m.add(name, class)?;
    }
    m.add("TUI_DEFAULT_SCROLLBACK", rich_embed::DEFAULT_SCROLLBACK)?;
    m.add_function(wrap_pyfunction!(tui_terminal, m)?)?;
    m.add_function(wrap_pyfunction!(tui_terminal_with, m)?)?;
    m.add_function(wrap_pyfunction!(tui_web_view, m)?)?;
    Ok(())
}
