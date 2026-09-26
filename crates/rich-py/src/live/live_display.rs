//! `rich.live.Live`: an auto-updating display of a renderable, its refresh
//! thread, and the redirection of `sys.stdout` / `sys.stderr` while it runs.
//!
//! # The render hook
//!
//! As upstream's, a `Live` pushes itself as a console render hook
//! (`Console.push_render_hook`), so every print made while it runs prints
//! "move the cursor over the display, the print, the display again"
//! (`process_renderables`), and is recorded like any print. The display's
//! own frames are a print of an empty `Control` through the same hook.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};

use pyo3::exceptions::{PyAssertionError, PyTypeError};
use pyo3::prelude::*;
use pyo3::types::{PyCFunction, PyDict, PyList, PyString, PyTuple};
use pyo3::{PyTraverseError, PyVisit};

use rich::control::Control as CoreControl;
use rich::segment::Segment as CoreSegment;
use rich::{AnsiDecoder, Text as CoreText};

use super::live_render::LiveRender;
use super::screen::Screen;
use super::util::{self, hold, Control, Item};

// ---------------------------------------------------------------------------
// The console's live stack (Rich's `Console._live_stack`)

/// The console's running displays, outermost first.
fn live_stack(console: &Bound<'_, PyAny>) -> PyResult<Vec<Py<PyAny>>> {
    console
        .getattr("_live_stack")?
        .try_iter()?
        .map(|live| live.map(Bound::unbind))
        .collect()
}

// ---------------------------------------------------------------------------
// Refresh threads, stopped at interpreter exit

/// A running refresh thread: the `threading.Thread` and its `done` event.
pub(crate) struct Worker {
    id: u64,
    thread: Py<PyAny>,
    done: Py<PyAny>,
}

static WORKERS: Mutex<Vec<Worker>> = Mutex::new(Vec::new());
static NEXT_WORKER: AtomicU64 = AtomicU64::new(1);

fn workers() -> MutexGuard<'static, Vec<Worker>> {
    WORKERS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Start `target` on a daemon thread stopped by setting `done`, and record
/// it in the exit registry. Returns its id there.
pub(crate) fn start_worker(
    py: Python<'_>,
    target: Bound<'_, PyCFunction>,
    done: &Py<PyAny>,
) -> PyResult<u64> {
    let kwargs = PyDict::new(py);
    kwargs.set_item("target", target)?;
    kwargs.set_item("daemon", true)?;
    let thread = py
        .import("threading")?
        .getattr("Thread")?
        .call((), Some(&kwargs))?;
    thread.call_method0("start")?;
    let id = NEXT_WORKER.fetch_add(1, Ordering::Relaxed);
    workers().push(Worker {
        id,
        thread: thread.unbind(),
        done: done.clone_ref(py),
    });
    Ok(id)
}

/// Start a daemon thread running `body` every `interval` seconds until its
/// `done` event is set. Returns its id in the exit registry.
pub(crate) fn spawn_worker<F>(py: Python<'_>, interval: f64, body: F) -> PyResult<(u64, Py<PyAny>)>
where
    F: Fn(Python<'_>, &Bound<'_, PyAny>) -> PyResult<()> + Send + Sync + 'static,
{
    let done = py.import("threading")?.call_method0("Event")?.unbind();
    let event = done.clone_ref(py);
    let target = PyCFunction::new_closure(
        py,
        None,
        None,
        move |args: &Bound<'_, PyTuple>, _kwargs: Option<&Bound<'_, PyDict>>| -> PyResult<()> {
            let py = args.py();
            let event = event.bind(py);
            loop {
                if event.call_method1("wait", (interval,))?.is_truthy()? {
                    return Ok(());
                }
                body(py, event)?;
            }
        },
    )?;
    let id = start_worker(py, target, &done)?;
    Ok((id, done))
}

/// Tell a worker to stop, taking it out of the exit registry.
pub(crate) fn signal_worker(py: Python<'_>, id: u64) -> PyResult<Option<Worker>> {
    let worker = {
        let mut workers = workers();
        workers
            .iter()
            .position(|worker| worker.id == id)
            .map(|index| workers.remove(index))
    };
    if let Some(worker) = &worker {
        worker.done.bind(py).call_method0("set")?;
    }
    Ok(worker)
}

/// Wait for a signalled worker to finish. Unlike upstream's threads, ours
/// run Rust frames, which must not still be running when the interpreter
/// finalizes (it unwinds them and aborts). A worker cannot join itself; it
/// goes back to the registry for the exit hook instead.
pub(crate) fn join_worker(py: Python<'_>, worker: Worker) -> PyResult<()> {
    let thread = worker.thread.bind(py);
    let current = py.import("threading")?.call_method0("current_thread")?;
    if current.is(thread) {
        workers().push(worker);
        return Ok(());
    }
    // `Thread.join` releases the GIL while it waits.
    thread.call_method0("join")?;
    Ok(())
}

/// Tell a worker to stop, and wait for it.
pub(crate) fn stop_worker(py: Python<'_>, id: u64) -> PyResult<()> {
    match signal_worker(py, id)? {
        Some(worker) => join_worker(py, worker),
        None => Ok(()),
    }
}

/// Keep a signalled worker that cannot be joined now for the exit hook.
fn defer_worker(worker: Worker) {
    workers().push(worker);
}

/// At interpreter exit: stop every refresh thread still running.
#[pyfunction]
fn _stop_live_threads(py: Python<'_>) -> PyResult<()> {
    let running: Vec<Worker> = workers().drain(..).collect();
    for worker in &running {
        worker.done.bind(py).call_method0("set")?;
    }
    let current = py.import("threading")?.call_method0("current_thread")?;
    for worker in &running {
        let thread = worker.thread.bind(py);
        if !current.is(thread) {
            thread.call_method1("join", (1.0,))?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Live

struct LiveState {
    renderable: Option<Py<PyAny>>,
    console: Py<PyAny>,
    screen: bool,
    alt_screen: bool,
    redirect_stdout: bool,
    redirect_stderr: bool,
    restore_stdout: Option<Py<PyAny>>,
    restore_stderr: Option<Py<PyAny>>,
    auto_refresh: bool,
    started: bool,
    transient: bool,
    refresh_thread: Option<(u64, Py<PyAny>)>,
    refresh_per_second: f64,
    vertical_overflow: String,
    get_renderable: Option<Py<PyAny>>,
    nested: bool,
    /// Between `push_render_hook` and `pop_render_hook`.
    hooked: bool,
    ipy_widget: Option<Py<PyAny>>,
}

/// `rich.live.Live`.
#[pyclass(name = "Live", module = "rs_rich.live", frozen, subclass)]
pub(crate) struct Live {
    state: Mutex<LiveState>,
    lock: Py<PyAny>,
    live_render: Py<LiveRender>,
}

impl Live {
    fn st(&self) -> MutexGuard<'_, LiveState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub(crate) fn console_of<'py>(&self, py: Python<'py>) -> Bound<'py, PyAny> {
        self.st().console.bind(py).clone()
    }

    pub(crate) fn is_started_now(&self) -> bool {
        self.st().started
    }

    /// The renderable, or `""` (Rich's `renderable or ""`).
    fn own_renderable<'py>(slf: &Bound<'py, Live>) -> PyResult<Bound<'py, PyAny>> {
        let py = slf.py();
        let (getter, renderable) = {
            let state = slf.get().st();
            (
                state.get_renderable.as_ref().map(|g| g.clone_ref(py)),
                state.renderable.as_ref().map(|r| r.clone_ref(py)),
            )
        };
        let value = match getter {
            Some(getter) => getter.bind(py).call0()?,
            None => match renderable {
                Some(renderable) => renderable.bind(py).clone(),
                None => py.None().into_bound(py),
            },
        };
        if value.is_none() || !value.is_truthy()? {
            return Ok(PyString::new(py, "").into_any());
        }
        Ok(value)
    }

    /// Rich's `process_renderables`: what a print made while the display is
    /// hooked in prints.
    fn process(slf: &Bound<'_, Live>, mut items: Vec<Item>) -> PyResult<Vec<Item>> {
        let py = slf.py();
        let this = slf.get();
        let (console, alt_screen, started, transient, overflow) = {
            let state = this.st();
            (
                state.console.clone_ref(py),
                state.alt_screen,
                state.started,
                state.transient,
                state.vertical_overflow.clone(),
            )
        };
        let live_render = this.live_render.bind(py);
        live_render.get().set_overflow(&overflow);
        let render = Item::Object(live_render.clone().into_any().unbind());
        if util::flag(console.bind(py), "is_interactive")? {
            let reset = if alt_screen {
                CoreControl::home().as_str().to_string()
            } else {
                live_render.get().position_codes()
            };
            items.insert(0, Item::Segments(vec![CoreSegment::control(reset)]));
            items.push(render);
        } else if !started && !transient {
            items.push(render);
        }
        Ok(items)
    }

    /// `with console: console.print(Control())`: the hook adds the display.
    fn print_frame(slf: &Bound<'_, Live>) -> PyResult<()> {
        let py = slf.py();
        let console = slf.get().console_of(py);
        let control = Py::new(
            py,
            Control {
                codes: String::new(),
            },
        )?;
        console.call_method0("__enter__")?;
        let printed = console.call_method1("print", (control,));
        let exited = console.call_method1("__exit__", (py.None(), py.None(), py.None()));
        printed?;
        exited?;
        Ok(())
    }

    fn install_hook(slf: &Bound<'_, Live>) -> PyResult<()> {
        let py = slf.py();
        slf.get().st().hooked = true;
        let console = slf.get().console_of(py);
        console.call_method1("push_render_hook", (slf,))?;
        Ok(())
    }

    fn remove_hook(slf: &Bound<'_, Live>) -> PyResult<()> {
        let py = slf.py();
        let hooked = std::mem::replace(&mut slf.get().st().hooked, false);
        if hooked {
            slf.get().console_of(py).call_method0("pop_render_hook")?;
        }
        Ok(())
    }

    fn enable_redirect_io(slf: &Bound<'_, Live>) -> PyResult<()> {
        let py = slf.py();
        let console = slf.get().console_of(py);
        if !util::flag(&console, "is_terminal")? {
            return Ok(());
        }
        let sys = py.import("sys")?;
        let (redirect_stdout, redirect_stderr) = {
            let state = slf.get().st();
            (state.redirect_stdout, state.redirect_stderr)
        };
        for (name, redirect) in [("stdout", redirect_stdout), ("stderr", redirect_stderr)] {
            let stream = sys.getattr(name)?;
            if !redirect || stream.is_instance_of::<FileProxy>() {
                continue;
            }
            let proxy = FileProxy::create(py, console.clone().unbind(), stream.clone().unbind())?;
            sys.setattr(name, proxy)?;
            let mut state = slf.get().st();
            if name == "stdout" {
                state.restore_stdout = Some(stream.unbind());
            } else {
                state.restore_stderr = Some(stream.unbind());
            }
        }
        Ok(())
    }

    fn disable_redirect_io(slf: &Bound<'_, Live>) -> PyResult<()> {
        let py = slf.py();
        let sys = py.import("sys")?;
        let (stdout, stderr) = {
            let mut state = slf.get().st();
            (state.restore_stdout.take(), state.restore_stderr.take())
        };
        // Upstream's proxy is an `io.TextIOBase`: dropping it closes it,
        // which flushes a partial line through the console.
        for (name, restore) in [("stdout", stdout), ("stderr", stderr)] {
            if let Some(restore) = restore {
                let proxy = sys.getattr(name)?;
                sys.setattr(name, restore)?;
                if proxy.is_instance_of::<FileProxy>() {
                    proxy.call_method0("flush")?;
                }
            }
        }
        Ok(())
    }

    fn start_thread(slf: &Bound<'_, Live>) -> PyResult<()> {
        let py = slf.py();
        let rate = slf.get().st().refresh_per_second;
        let live = slf.clone().unbind();
        let (id, done) = spawn_worker(py, 1.0 / rate, move |py, done| {
            let live = live.bind(py);
            let _held = hold(live.get().lock.bind(py))?;
            if !done.call_method0("is_set")?.is_truthy()? {
                live.call_method0("refresh")?;
            }
            Ok(())
        })?;
        slf.get().st().refresh_thread = Some((id, done));
        Ok(())
    }

    /// `stop` under the display's lock; the signalled refresh thread is left
    /// in `worker` for the caller to join.
    fn stop_held(slf: &Bound<'_, Live>, worker: &mut Option<Worker>) -> PyResult<()> {
        let py = slf.py();
        let this = slf.get();
        {
            let mut state = this.st();
            if !state.started {
                return Ok(());
            }
            state.started = false;
        }
        let console = this.console_of(py);
        console.call_method0("clear_live")?;
        let (nested, transient) = {
            let state = this.st();
            (state.nested, state.transient)
        };
        if nested {
            if !transient {
                let renderable = slf.getattr("renderable")?;
                console.call_method1("print", (renderable,))?;
            }
            return Ok(());
        }
        let thread = this.st().refresh_thread.take();
        if let Some((id, _)) = thread {
            *worker = signal_worker(py, id)?;
        }
        let alt_screen = {
            let mut state = this.st();
            state.vertical_overflow = "visible".to_string();
            state.alt_screen
        };
        let refreshed = if alt_screen {
            Ok(())
        } else {
            slf.call_method0("refresh").map(|_| ())
        };
        // Upstream's `finally:` block.
        Live::disable_redirect_io(slf)?;
        Live::remove_hook(slf)?;
        let live_render = this.live_render.bind(py).get();
        if !alt_screen && util::flag(&console, "is_terminal")? && live_render.height() > 0 {
            console.call_method0("line")?;
        }
        console.call_method1("show_cursor", (true,))?;
        if alt_screen {
            console.call_method1("set_alt_screen", (false,))?;
        }
        if transient && !alt_screen {
            util::control(&console, &live_render.restore_codes())?;
        }
        refreshed
    }
}

#[pymethods]
impl Live {
    #[new]
    #[pyo3(signature = (
        renderable=None, *, console=None, screen=false, auto_refresh=true,
        refresh_per_second=4.0, transient=false, redirect_stdout=true, redirect_stderr=true,
        vertical_overflow="ellipsis", get_renderable=None
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        py: Python<'_>,
        renderable: Option<Py<PyAny>>,
        console: Option<Bound<'_, PyAny>>,
        screen: bool,
        auto_refresh: bool,
        refresh_per_second: f64,
        transient: bool,
        redirect_stdout: bool,
        redirect_stderr: bool,
        vertical_overflow: &str,
        get_renderable: Option<Py<PyAny>>,
    ) -> PyResult<Live> {
        if refresh_per_second <= 0.0 {
            return Err(PyAssertionError::new_err("refresh_per_second must be > 0"));
        }
        let console = util::console_or_global(py, console)?;
        let renderable = renderable.filter(|r| !r.is_none(py));
        let get_renderable = get_renderable.filter(|g| !g.is_none(py));
        // Rich renders `self.get_renderable()` into the LiveRender up front.
        let first = match &get_renderable {
            Some(getter) => getter.bind(py).call0()?,
            None => match &renderable {
                Some(renderable) => renderable.bind(py).clone(),
                None => py.None().into_bound(py),
            },
        };
        let first = if first.is_none() || !first.is_truthy()? {
            PyString::new(py, "").into_any()
        } else {
            first
        };
        Ok(Live {
            state: Mutex::new(LiveState {
                renderable,
                console: console.unbind(),
                screen,
                alt_screen: false,
                redirect_stdout,
                redirect_stderr,
                restore_stdout: None,
                restore_stderr: None,
                auto_refresh,
                started: false,
                transient: screen || transient,
                refresh_thread: None,
                refresh_per_second,
                vertical_overflow: vertical_overflow.to_string(),
                get_renderable,
                nested: false,
                hooked: false,
                ipy_widget: None,
            }),
            lock: util::rlock(py)?,
            live_render: LiveRender::create(py, first.unbind(), vertical_overflow)?,
        })
    }

    #[getter]
    fn console(&self, py: Python<'_>) -> Py<PyAny> {
        self.st().console.clone_ref(py)
    }

    #[setter]
    fn set_console(&self, console: Py<PyAny>) {
        self.st().console = console;
    }

    #[getter]
    fn is_started(&self) -> bool {
        self.st().started
    }

    #[getter]
    fn auto_refresh(&self) -> bool {
        self.st().auto_refresh
    }

    #[setter]
    fn set_auto_refresh(&self, value: bool) {
        self.st().auto_refresh = value;
    }

    #[getter]
    fn transient(&self) -> bool {
        self.st().transient
    }

    #[setter]
    fn set_transient(&self, value: bool) {
        self.st().transient = value;
    }

    #[getter]
    fn refresh_per_second(&self) -> f64 {
        self.st().refresh_per_second
    }

    #[setter]
    fn set_refresh_per_second(&self, value: f64) {
        self.st().refresh_per_second = value;
    }

    #[getter]
    fn vertical_overflow(&self) -> String {
        self.st().vertical_overflow.clone()
    }

    #[setter]
    fn set_vertical_overflow(&self, value: String) {
        self.st().vertical_overflow = value;
    }

    #[getter]
    fn ipy_widget(&self, py: Python<'_>) -> Py<PyAny> {
        self.st()
            .ipy_widget
            .as_ref()
            .map_or_else(|| py.None(), |widget| widget.clone_ref(py))
    }

    #[getter]
    fn _live_render(&self, py: Python<'_>) -> Py<LiveRender> {
        self.live_render.clone_ref(py)
    }

    #[getter]
    fn _lock(&self, py: Python<'_>) -> Py<PyAny> {
        self.lock.clone_ref(py)
    }

    /// The renderable to show: the one given, or what `get_renderable`
    /// returns, or `""`.
    fn get_renderable<'py>(slf: &Bound<'py, Self>) -> PyResult<Bound<'py, PyAny>> {
        Live::own_renderable(slf)
    }

    /// What the display shows: its renderable, all of the stacked displays'
    /// when it is the outermost, in a `Screen` on the alternate screen.
    #[getter(renderable)]
    fn shown_renderable(slf: &Bound<'_, Self>) -> PyResult<Py<PyAny>> {
        let py = slf.py();
        let console = slf.get().console_of(py);
        let stack = live_stack(&console)?;
        let renderable = if stack.first().is_some_and(|first| first.bind(py).is(slf)) {
            let mut children = Vec::new();
            for live in &stack {
                children.push(live.bind(py).call_method0("get_renderable")?);
            }
            util::group(py, children)?
        } else {
            slf.call_method0("get_renderable")?.unbind()
        };
        if slf.get().st().alt_screen {
            return Ok(Screen::wrap(py, renderable)?.into_any());
        }
        Ok(renderable)
    }

    /// Start the display (drawing it now with `refresh=True`).
    #[pyo3(signature = (refresh=false))]
    fn start(slf: &Bound<'_, Self>, refresh: bool) -> PyResult<()> {
        let py = slf.py();
        let this = slf.get();
        let _held = hold(this.lock.bind(py))?;
        {
            let mut state = this.st();
            if state.started {
                return Ok(());
            }
            state.started = true;
        }
        let console = this.console_of(py);
        if !console.call_method1("set_live", (slf,))?.is_truthy()? {
            this.st().nested = true;
            return Ok(());
        }
        if this.st().screen {
            let changed = console
                .call_method1("set_alt_screen", (true,))?
                .is_truthy()?;
            this.st().alt_screen = changed;
        }
        console.call_method1("show_cursor", (false,))?;
        Live::install_hook(slf)?;
        Live::enable_redirect_io(slf)?;
        if refresh {
            if let Err(error) = slf.call_method0("refresh") {
                slf.call_method0("stop")?;
                return Err(error);
            }
        }
        if this.st().auto_refresh {
            Live::start_thread(slf)?;
        }
        Ok(())
    }

    /// Stop the display, leaving its last frame (or nothing, if transient).
    fn stop(slf: &Bound<'_, Self>) -> PyResult<()> {
        let py = slf.py();
        let this = slf.get();
        let mut worker = None;
        let result = {
            let _held = hold(this.lock.bind(py))?;
            Live::stop_held(slf, &mut worker)
        };
        // Wait for the refresh thread outside the lock, which it takes to
        // refresh; a caller still holding it must leave that to the exit hook.
        if let Some(worker) = worker {
            let lock = this.lock.bind(py);
            if lock.call_method0("_is_owned")?.is_truthy()? {
                defer_worker(worker);
            } else {
                join_worker(py, worker)?;
            }
        }
        result
    }

    fn __enter__(slf: Bound<'_, Self>) -> PyResult<Bound<'_, Self>> {
        let refresh = slf.get().st().renderable.is_some();
        slf.call_method1("start", (refresh,))?;
        Ok(slf)
    }

    #[pyo3(signature = (*_args))]
    fn __exit__(slf: &Bound<'_, Self>, _args: &Bound<'_, PyTuple>) -> PyResult<()> {
        slf.call_method0("stop")?;
        Ok(())
    }

    /// Show a new renderable (a `str` is rendered as console markup now).
    #[pyo3(signature = (renderable, *, refresh=false))]
    fn update(slf: &Bound<'_, Self>, renderable: Bound<'_, PyAny>, refresh: bool) -> PyResult<()> {
        let py = slf.py();
        let this = slf.get();
        let renderable = if util::is_str(&renderable) {
            this.console_of(py)
                .call_method1("render_str", (renderable,))?
        } else {
            renderable
        };
        let _held = hold(this.lock.bind(py))?;
        this.st().renderable = Some(renderable.unbind());
        if refresh {
            slf.call_method0("refresh")?;
        }
        Ok(())
    }

    /// Draw the display now.
    fn refresh(slf: &Bound<'_, Self>) -> PyResult<()> {
        let py = slf.py();
        let this = slf.get();
        let _held = hold(this.lock.bind(py))?;
        let renderable = slf.getattr("renderable")?;
        this.live_render.bind(py).get().set(renderable.unbind());
        let console = this.console_of(py);
        if this.st().nested {
            let stack = live_stack(&console)?;
            if let Some(first) = stack.first() {
                first.bind(py).call_method0("refresh")?;
            }
            return Ok(());
        }
        let (started, transient) = {
            let state = this.st();
            (state.started, state.transient)
        };
        let terminal =
            util::flag(&console, "is_terminal")? && !util::flag(&console, "is_dumb_terminal")?;
        // A file or dumb terminal sees only the final frame.
        if terminal || (!started && !transient) {
            Live::print_frame(slf)?;
        }
        Ok(())
    }

    /// Rich's render hook: the renderables a print made during the display
    /// prints (a cursor reset first and the display last, on a terminal).
    fn process_renderables<'py>(
        slf: &Bound<'py, Self>,
        renderables: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyList>> {
        let py = slf.py();
        let _held = hold(slf.get().lock.bind(py))?;
        let items: Vec<Item> = renderables
            .try_iter()?
            .map(|item| item.map(|item| Item::Object(item.unbind())))
            .collect::<PyResult<_>>()?;
        let result = PyList::empty(py);
        for item in Live::process(slf, items)? {
            match item {
                Item::Segments(segments) => {
                    let codes: String = segments.iter().map(|s| s.text.as_str()).collect();
                    result.append(Control { codes })?;
                }
                Item::Object(object) => result.append(object)?,
            }
        }
        Ok(result)
    }

    fn __traverse__(&self, visit: PyVisit<'_>) -> Result<(), PyTraverseError> {
        visit.call(&self.lock)?;
        visit.call(&self.live_render)?;
        if let Ok(state) = self.state.try_lock() {
            visit.call(&state.console)?;
            for object in [&state.renderable, &state.get_renderable]
                .into_iter()
                .flatten()
            {
                visit.call(object)?;
            }
        }
        Ok(())
    }

    fn __clear__(&self) {
        if let Ok(mut state) = self.state.try_lock() {
            state.renderable = None;
            state.get_renderable = None;
        }
    }
}

// ---------------------------------------------------------------------------
// Standard streams while a display runs

/// `rich.file_proxy.FileProxy`: `sys.stdout` / `sys.stderr` while a display
/// runs; each complete line is printed through the console, above it.
#[pyclass(name = "_FileProxy", module = "rs_rich.live", frozen)]
pub(crate) struct FileProxy {
    console: Py<PyAny>,
    file: Py<PyAny>,
    buffer: Mutex<String>,
    decoder: Mutex<AnsiDecoder>,
}

impl FileProxy {
    fn create(py: Python<'_>, console: Py<PyAny>, file: Py<PyAny>) -> PyResult<Py<FileProxy>> {
        Py::new(
            py,
            FileProxy {
                console,
                file,
                buffer: Mutex::new(String::new()),
                decoder: Mutex::new(AnsiDecoder::new()),
            },
        )
    }

    fn buffer(&self) -> MutexGuard<'_, String> {
        self.buffer
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[pymethods]
impl FileProxy {
    fn write(&self, py: Python<'_>, text: &Bound<'_, PyAny>) -> PyResult<usize> {
        let Ok(text) = text.cast::<PyString>() else {
            return Err(PyTypeError::new_err(format!(
                "write() argument must be str, not {}",
                text.get_type().name()?
            )));
        };
        let mut text = text.to_cow()?.into_owned();
        let mut lines = Vec::new();
        {
            let mut buffer = self.buffer();
            while !text.is_empty() {
                match text.find('\n') {
                    Some(index) => {
                        let line = format!("{}{}", buffer, &text[..index]);
                        buffer.clear();
                        lines.push(line);
                        text = text[index + 1..].to_string();
                    }
                    None => {
                        buffer.push_str(&text);
                        text.clear();
                    }
                }
            }
        }
        if !lines.is_empty() {
            let decoded: Vec<CoreText> = {
                let mut decoder = self
                    .decoder
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                lines.iter().map(|line| decoder.decode_line(line)).collect()
            };
            let output = CoreText::new("\n").join(&decoded);
            let console = self.console.bind(py);
            console.call_method0("__enter__")?;
            let printed = console.call_method1("print", (util::new_text(py, output)?,));
            console.call_method1("__exit__", (py.None(), py.None(), py.None()))?;
            printed?;
        }
        // Rich returns the length of what was left unwritten (0).
        Ok(text.chars().count())
    }

    fn flush(&self, py: Python<'_>) -> PyResult<()> {
        let output = std::mem::take(&mut *self.buffer());
        if !output.is_empty() {
            self.console.bind(py).call_method1("print", (output,))?;
        }
        Ok(())
    }

    fn fileno(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        Ok(self.file.bind(py).call_method0("fileno")?.unbind())
    }

    fn isatty(&self, py: Python<'_>) -> PyResult<bool> {
        self.file.bind(py).call_method0("isatty")?.is_truthy()
    }

    #[getter]
    fn rich_proxied_file(&self, py: Python<'_>) -> Py<PyAny> {
        self.file.clone_ref(py)
    }

    fn __getattr__(&self, py: Python<'_>, name: &str) -> PyResult<Py<PyAny>> {
        Ok(self.file.bind(py).getattr(name)?.unbind())
    }

    fn __traverse__(&self, visit: PyVisit<'_>) -> Result<(), PyTraverseError> {
        visit.call(&self.console)?;
        visit.call(&self.file)
    }
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    let py = m.py();
    m.add_class::<Live>()?;
    m.add_class::<FileProxy>()?;
    let stop = pyo3::wrap_pyfunction!(_stop_live_threads, m)?;
    m.add_function(stop.clone())?;
    py.import("atexit")?.call_method1("register", (stop,))?;
    Ok(())
}
