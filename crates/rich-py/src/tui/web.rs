//! Serving apps to a browser from Python (`rs-rich-web`): `serve`,
//! `Server` and `ServerHandle`.
//!
//! The factory is called on each session's own thread, as in Rust, so each
//! tab's app (its signals, its nodes) lives on that thread; it takes the
//! GIL to build the app and whenever the app's Python code runs. No Python
//! call waits on a session, so an exception in a session's app is reported
//! as unraisable (`sys.unraisablehook`) and ends that session.

use std::cell::RefCell;
use std::sync::Mutex;
use std::time::Duration;

use pyo3::exceptions::{PyOSError, PyRuntimeError, PyTypeError, PyValueError};
use pyo3::prelude::*;

use rich_intuituive as intuituive;

use super::app::PyApp;
use super::detached;

/// The factory as rs-rich-web takes it: a Python callable returning an
/// `App`, called on the session's thread. A factory that raises (or
/// returns something else) is reported, and the tab gets an app that says
/// so.
fn factory(app: Py<PyAny>) -> impl Fn() -> intuituive::App + Send + Sync + 'static {
    move || {
        Python::attach(|py| {
            let made = app.bind(py).call0().and_then(|value| {
                let value = value.cast::<PyApp>().map_err(|_| {
                    PyTypeError::new_err("the served app's factory must return an App")
                })?;
                let (app, shared) = value.borrow().take()?;
                shared.set_unraisable();
                Ok(app)
            });
            made.unwrap_or_else(|error| {
                error.write_unraisable(py, None);
                intuituive::App::new(|| {
                    intuituive::label(
                        "[bold red]The app failed to start.[/] The server's log says why.",
                    )
                    .on_key("q", |cx| cx.quit())
                })
            })
        })
    }
}

fn os_error(error: std::io::Error) -> PyErr {
    PyOSError::new_err(error.to_string())
}

/// `Server(address, app)`: a server listening on `address` (`"127.0.0.1:0"`
/// picks a free port) with a fresh random token, to serve the apps `app()`
/// makes, one per browser tab. Options chain: `max_sessions(n)`,
/// `token(t)`, `allow_origin(origin)`, `title(t)`. Then `run()` (serve
/// until Ctrl+C) or `spawn()` (serve in the background).
#[pyclass(name = "Server", module = "rs_rich.tui", unsendable)]
pub(crate) struct PyServer {
    server: RefCell<Option<rich_web::Server>>,
    title: RefCell<String>,
}

fn spent() -> PyErr {
    PyRuntimeError::new_err("this server has started")
}

impl PyServer {
    fn map(&self, f: impl FnOnce(rich_web::Server) -> rich_web::Server) -> PyResult<()> {
        let server = self.server.borrow_mut().take().ok_or_else(spent)?;
        *self.server.borrow_mut() = Some(f(server));
        Ok(())
    }

    fn read<R>(&self, f: impl FnOnce(&rich_web::Server) -> R) -> PyResult<R> {
        self.server.borrow().as_ref().map(f).ok_or_else(spent)
    }
}

#[pymethods]
impl PyServer {
    #[new]
    fn new(py: Python<'_>, address: &str, app: &Bound<'_, PyAny>) -> PyResult<PyServer> {
        if !app.is_callable() {
            return Err(PyTypeError::new_err(
                "app must be a function returning an App (called once per browser tab)",
            ));
        }
        let app = app.clone().unbind();
        let address = address.to_string();
        let server = detached(py, move || rich_web::Server::bind(address, factory(app)))
            .map_err(os_error)?;
        Ok(PyServer {
            server: RefCell::new(Some(server)),
            title: RefCell::new("intuiTUIve".to_string()),
        })
    }

    /// Run at most `n` sessions at once (8 by default).
    fn max_sessions(slf: Bound<'_, Self>, n: usize) -> PyResult<Bound<'_, Self>> {
        slf.borrow().map(|s| s.max_sessions(n))?;
        Ok(slf)
    }

    /// Use `token` instead of a random one: ASCII letters, digits, `-`,
    /// `_`, `.` and `~`.
    fn token<'py>(slf: Bound<'py, Self>, token: &str) -> PyResult<Bound<'py, Self>> {
        let fine = !token.is_empty()
            && token
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.~".contains(&b));
        if !fine {
            return Err(PyValueError::new_err(
                "a token is ASCII letters, digits, '-', '_', '.' and '~'",
            ));
        }
        slf.borrow().map(|s| s.token(token))?;
        Ok(slf)
    }

    /// Also accept WebSocket upgrades from pages served at `origin`.
    fn allow_origin<'py>(slf: Bound<'py, Self>, origin: &str) -> PyResult<Bound<'py, Self>> {
        slf.borrow().map(|s| s.allow_origin(origin))?;
        Ok(slf)
    }

    /// The page's title.
    fn title<'py>(slf: Bound<'py, Self>, title: &str) -> PyResult<Bound<'py, Self>> {
        slf.borrow().map(|s| s.title(title))?;
        *slf.borrow().title.borrow_mut() = title.to_string();
        Ok(slf)
    }

    /// The address it listens on, `"host:port"`.
    #[getter]
    fn local_addr(&self) -> PyResult<String> {
        self.read(|s| s.local_addr().to_string())
    }

    /// The token browsers must give.
    #[getter]
    fn access_token(&self) -> PyResult<String> {
        self.read(|s| s.access_token().to_string())
    }

    /// The address to open in a browser, with the token.
    #[getter]
    fn url(&self) -> PyResult<String> {
        self.read(rich_web::Server::url)
    }

    /// Print the URL, then serve until Ctrl+C (`KeyboardInterrupt`), which
    /// stops the server and every session.
    fn run(&self, py: Python<'_>) -> PyResult<()> {
        let server = self.server.borrow_mut().take().ok_or_else(spent)?;
        let local = server.local_addr();
        let line = format!("Serving {} at {}", self.title.borrow(), server.url());
        let print = py.import("builtins")?.getattr("print")?;
        let flush = [("flush", true)].into_py_dict(py)?;
        print.call((line,), Some(&flush))?;
        if !local.ip().is_loopback() {
            let warning = format!(
                "warning: listening on {local}, beyond this computer. Anyone who can reach it \
                 with the token can use the app: there is no other authentication and no TLS. \
                 Put it behind a reverse proxy that has both."
            );
            let stderr = py.import("sys")?.getattr("stderr")?;
            let kwargs = [("file", stderr)].into_py_dict(py)?;
            print.call((warning,), Some(&kwargs))?;
        }
        let handle = detached(py, move || server.spawn()).map_err(os_error)?;
        loop {
            detached(py, || std::thread::sleep(Duration::from_millis(100)));
            if let Err(error) = py.check_signals() {
                detached(py, move || handle.stop());
                return Err(error);
            }
        }
    }

    /// Serve on a thread of its own until the handle is stopped (or used as
    /// a context manager and left).
    fn spawn(&self, py: Python<'_>) -> PyResult<PyServerHandle> {
        let server = self.server.borrow_mut().take().ok_or_else(spent)?;
        let handle = detached(py, move || server.spawn()).map_err(os_error)?;
        Ok(PyServerHandle {
            url: handle.url().to_string(),
            local_addr: handle.local_addr().to_string(),
            handle: Mutex::new(Some(handle)),
        })
    }
}

use pyo3::types::IntoPyDict;

/// `ServerHandle`: a server running in the background: its `url` and
/// `local_addr`, `sessions()` running now, and `stop()`, which ends every
/// session. A context manager: leaving the block stops it.
#[pyclass(name = "ServerHandle", module = "rs_rich.tui", frozen)]
pub(crate) struct PyServerHandle {
    handle: Mutex<Option<rich_web::Handle>>,
    #[pyo3(get)]
    url: String,
    #[pyo3(get)]
    local_addr: String,
}

#[pymethods]
impl PyServerHandle {
    /// The sessions running now.
    fn sessions(&self) -> usize {
        self.handle
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .map_or(0, rich_web::Handle::sessions)
    }

    /// Stop accepting connections and end every session.
    fn stop(&self, py: Python<'_>) {
        let handle = self.handle.lock().unwrap_or_else(|p| p.into_inner()).take();
        if let Some(handle) = handle {
            detached(py, move || handle.stop());
        }
    }

    fn __enter__(slf: Bound<'_, Self>) -> Bound<'_, Self> {
        slf
    }

    #[pyo3(signature = (*_args))]
    fn __exit__(&self, py: Python<'_>, _args: &Bound<'_, pyo3::types::PyTuple>) -> bool {
        self.stop(py);
        false
    }

    fn __repr__(&self) -> String {
        format!("<ServerHandle {}>", self.local_addr)
    }
}

/// `serve(address, app)`: `Server(address, app).run()`: print the URL to
/// open (with its token), then serve one app per browser tab, made by
/// `app()`, until Ctrl+C.
#[pyfunction]
fn tui_serve(py: Python<'_>, address: &str, app: &Bound<'_, PyAny>) -> PyResult<()> {
    PyServer::new(py, address, app)?.run(py)
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    let py = m.py();
    m.add("TuiServer", py.get_type::<PyServer>())?;
    m.add("TuiServerHandle", py.get_type::<PyServerHandle>())?;
    m.add("TUI_XTERM_VERSION", rich_web::XTERM_VERSION)?;
    m.add("TUI_DEFAULT_MAX_SESSIONS", rich_web::DEFAULT_MAX_SESSIONS)?;
    m.add_function(wrap_pyfunction!(tui_serve, m)?)?;
    Ok(())
}
