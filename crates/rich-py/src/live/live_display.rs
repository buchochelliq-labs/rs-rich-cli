//! `rich.live.Live`: an auto-updating display of a renderable, its refresh
//! thread, and the redirection of `sys.stdout` / `sys.stderr` while it runs.
//!
//! # The render hook
//!
//! Upstream's `Live` pushes itself as a console render hook, so every print
//! made while it runs is written as "move the cursor over the display, the
//! print, the display again". The bindings' `Console` renders in Rust and
//! has no hooks; the display instead wraps the console's file in a
//! [`LiveFile`] while it runs, which does the same to each write: output
//! from another print arrives there already rendered, and the display is
//! rendered (captured, never recorded) after it. The display's own frames
//! pass straight through.

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
use super::util::{self, hold, Control, Item, Renderables};

// ---------------------------------------------------------------------------
// The console's live stack (Rich's `Console._live_stack`)

struct Stack {
    console: usize,
    lives: Vec<Py<Live>>,
}

static STACKS: Mutex<Vec<Stack>> = Mutex::new(Vec::new());

fn stacks() -> MutexGuard<'static, Vec<Stack>> {
    STACKS.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn key(console: &Bound<'_, PyAny>) -> usize {
    console.as_ptr() as usize
}

/// `Console.set_live`: push `live`; `True` when it is the only one.
fn set_live(console: &Bound<'_, PyAny>, live: &Bound<'_, Live>) -> bool {
    let key = key(console);
    let mut stacks = stacks();
    match stacks.iter_mut().find(|stack| stack.console == key) {
        Some(stack) => {
            stack.lives.push(live.clone().unbind());
            stack.lives.len() == 1
        }
        None => {
            stacks.push(Stack {
                console: key,
                lives: vec![live.clone().unbind()],
            });
            true
        }
    }
}

/// `Console.clear_live`: pop the top live.
fn clear_live(console: &Bound<'_, PyAny>) {
    let key = key(console);
    let removed = {
        let mut stacks = stacks();
        let mut removed = None;
        if let Some(index) = stacks.iter().position(|stack| stack.console == key) {
            removed = stacks[index].lives.pop();
            if stacks[index].lives.is_empty() {
                stacks.remove(index);
            }
        }
        removed
    };
    drop(removed);
}

fn live_stack(py: Python<'_>, console: &Bound<'_, PyAny>) -> Vec<Py<Live>> {
    let key = key(console);
    stacks()
        .iter()
        .find(|stack| stack.console == key)
        .map(|stack| stack.lives.iter().map(|live| live.clone_ref(py)).collect())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Refresh threads, stopped at interpreter exit

/// A running refresh thread: the `threading.Thread` and its `done` event.
pub(crate) struct Worker {
    pub(crate) thread: Py<PyAny>,
    pub(crate) done: Py<PyAny>,
}

static WORKERS: Mutex<Vec<(u64, Worker)>> = Mutex::new(Vec::new());
static NEXT_WORKER: AtomicU64 = AtomicU64::new(1);

fn workers() -> MutexGuard<'static, Vec<(u64, Worker)>> {
    WORKERS.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Start a daemon thread running `body` every `interval` seconds until its
/// `done` event is set. Returns its id in the exit registry.
pub(crate) fn spawn_worker<F>(py: Python<'_>, interval: f64, body: F) -> PyResult<(u64, Py<PyAny>)>
where
    F: Fn(Python<'_>, &Bound<'_, PyAny>) -> PyResult<()> + Send + Sync + 'static,
{
    let threading = py.import("threading")?;
    let done = threading.call_method0("Event")?.unbind();
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
    let kwargs = PyDict::new(py);
    kwargs.set_item("target", target)?;
    kwargs.set_item("daemon", true)?;
    let thread = threading.getattr("Thread")?.call((), Some(&kwargs))?;
    thread.call_method0("start")?;
    let id = NEXT_WORKER.fetch_add(1, Ordering::Relaxed);
    workers().push((
        id,
        Worker {
            thread: thread.unbind(),
            done: done.clone_ref(py),
        },
    ));
    Ok((id, done))
}

/// Tell a worker to stop (it finishes its current step on its own).
pub(crate) fn stop_worker(py: Python<'_>, id: u64) -> PyResult<()> {
    let worker = {
        let mut workers = workers();
        workers
            .iter()
            .position(|(worker_id, _)| *worker_id == id)
            .map(|index| workers.remove(index).1)
    };
    if let Some(worker) = worker {
        worker.done.bind(py).call_method0("set")?;
    }
    Ok(())
}

/// At interpreter exit: stop every refresh thread still running.
#[pyfunction]
fn _stop_live_threads(py: Python<'_>) -> PyResult<()> {
    let running: Vec<Worker> = workers().drain(..).map(|(_, worker)| worker).collect();
    for worker in &running {
        worker.done.bind(py).call_method0("set")?;
    }
    for worker in &running {
        let thread = worker.thread.bind(py);
        let is_current = py
            .import("threading")?
            .call_method0("current_thread")?
            .is(thread);
        if !is_current {
            thread.call_method1("join", (1.0,))?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Live

struct Hook {
    /// The `LiveFile` wrapping the console's file.
    proxy: Py<LiveFile>,
    /// What the console's `file` was: `None` when it followed `sys.stdout`.
    original: Option<Py<PyAny>>,
}

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
    /// Between upstream's `push_render_hook` and `pop_render_hook`.
    hooked: bool,
    hook: Option<Hook>,
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

    pub(crate) fn auto_refresh_now(&self) -> bool {
        self.st().auto_refresh
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

    /// `console.print(Control())` with the display's hook applied.
    fn print_frame(slf: &Bound<'_, Live>) -> PyResult<()> {
        let py = slf.py();
        let this = slf.get();
        let (console, hooked) = {
            let state = this.st();
            (state.console.clone_ref(py), state.hooked)
        };
        let items = if hooked {
            Live::process(slf, Vec::new())?
        } else {
            Vec::new()
        };
        let frame = Py::new(py, Renderables::new(items))?;
        let proxy = this.st().hook.as_ref().map(|hook| hook.proxy.clone_ref(py));
        let _through = proxy.as_ref().map(|proxy| proxy.get().pass_through());
        console.bind(py).call_method1("print", (frame,))?;
        Ok(())
    }

    fn install_hook(slf: &Bound<'_, Live>) -> PyResult<()> {
        let py = slf.py();
        slf.get().st().hooked = true;
        let console = slf.get().console_of(py);
        // Only an interactive console redraws around other prints.
        if !util::flag(&console, "is_interactive")? {
            return Ok(());
        }
        let current = console.getattr("file")?;
        // A file already redirected by a live display writes to its target.
        let target = match current.getattr_opt("rich_proxied_file")? {
            Some(file) => file,
            None => current.clone(),
        };
        let sys = py.import("sys")?;
        let default_stream =
            sys.getattr(if util::flag(&console, "stderr")? { "stderr" } else { "stdout" })?;
        let original = if current.is(&default_stream) {
            None
        } else {
            Some(current.unbind())
        };
        let proxy = Py::new(
            py,
            LiveFile {
                live: slf.clone().unbind(),
                file: target.unbind(),
                passing: Mutex::new(Vec::new()),
            },
        )?;
        console.setattr("file", proxy.clone_ref(py))?;
        slf.get().st().hook = Some(Hook { proxy, original });
        Ok(())
    }

    fn remove_hook(slf: &Bound<'_, Live>) -> PyResult<()> {
        let py = slf.py();
        let hook = {
            let mut state = slf.get().st();
            state.hooked = false;
            state.hook.take()
        };
        if let Some(hook) = hook {
            let console = slf.get().console_of(py);
            // Only undo our own wrapping: the program may have set a file since.
            if console.getattr("file")?.is(hook.proxy.bind(py)) {
                console.setattr("file", hook.original)?;
            }
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
        if let Some(stdout) = stdout {
            sys.setattr("stdout", stdout)?;
        }
        if let Some(stderr) = stderr {
            sys.setattr("stderr", stderr)?;
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
                hook: None,
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
    #[getter]
    fn renderable(slf: &Bound<'_, Self>) -> PyResult<Py<PyAny>> {
        let py = slf.py();
        let console = slf.get().console_of(py);
        let stack = live_stack(py, &console);
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
        if !set_live(&console, slf) {
            this.st().nested = true;
            return Ok(());
        }
        if this.st().screen {
            let changed = console.call_method1("set_alt_screen", (true,))?.is_truthy()?;
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
        let _held = hold(this.lock.bind(py))?;
        {
            let mut state = this.st();
            if !state.started {
                return Ok(());
            }
            state.started = false;
        }
        let console = this.console_of(py);
        clear_live(&console);
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
            stop_worker(py, id)?;
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
            let stack = live_stack(py, &console);
            if let Some(first) = stack.first() {
                first.bind(py).call_method0("refresh")?;
            }
            return Ok(());
        }
        let (started, transient) = {
            let state = this.st();
            (state.started, state.transient)
        };
        if util::flag(&console, "is_terminal")? && !util::flag(&console, "is_dumb_terminal")? {
            Live::print_frame(slf)?;
        } else if !started && !transient {
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
// The console's file while a display runs

/// Wraps the console's file while a `Live` runs: see the module docs.
#[pyclass(name = "_LiveFile", module = "rs_rich.live", frozen)]
pub(crate) struct LiveFile {
    live: Py<Live>,
    file: Py<PyAny>,
    /// Threads whose writes pass straight through (the display's own).
    passing: Mutex<Vec<std::thread::ThreadId>>,
}

/// Lets this thread's writes through until dropped.
pub(crate) struct PassThrough<'a> {
    file: &'a LiveFile,
}

impl Drop for PassThrough<'_> {
    fn drop(&mut self) {
        let me = std::thread::current().id();
        let mut passing = self
            .file
            .passing
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(index) = passing.iter().position(|id| *id == me) {
            passing.remove(index);
        }
    }
}

impl LiveFile {
    fn pass_through(&self) -> PassThrough<'_> {
        self.passing
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(std::thread::current().id());
        PassThrough { file: self }
    }

    fn passes(&self) -> bool {
        let me = std::thread::current().id();
        self.passing
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .contains(&me)
    }
}

#[pymethods]
impl LiveFile {
    fn write(&self, py: Python<'_>, text: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        let file = self.file.bind(py);
        let live = self.live.bind(py);
        let hooked = live.get().st().hooked;
        if self.passes() || !hooked {
            return Ok(file.call_method1("write", (text,))?.unbind());
        }
        let console = live.get().console_of(py);
        let items = Live::process(live, vec![Item::Segments(Vec::new())])?;
        if items.len() == 1 {
            return Ok(file.call_method1("write", (text,))?.unbind());
        }
        // Render the reset and the display the way this print would have.
        let mut before = String::new();
        let mut after = String::new();
        let mut seen_text = false;
        for item in items {
            match item {
                Item::Segments(segments) if segments.is_empty() => seen_text = true,
                Item::Segments(segments) => {
                    let codes: String = segments.iter().map(|s| s.text.as_str()).collect();
                    if seen_text {
                        after.push_str(&codes);
                    } else {
                        before.push_str(&codes);
                    }
                }
                Item::Object(object) => {
                    console.call_method0("begin_capture")?;
                    let printed = console.call_method1("print", (object,));
                    let captured: String = console.call_method0("end_capture")?.extract()?;
                    printed?;
                    after.push_str(&captured);
                }
            }
        }
        let text: String = text.extract()?;
        let output = format!("{before}{text}{after}");
        Ok(file.call_method1("write", (output,))?.unbind())
    }

    fn flush(&self, py: Python<'_>) -> PyResult<()> {
        self.file.bind(py).call_method0("flush")?;
        Ok(())
    }

    fn isatty(&self, py: Python<'_>) -> PyResult<bool> {
        self.file.bind(py).call_method0("isatty")?.is_truthy()
    }

    fn fileno(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        Ok(self.file.bind(py).call_method0("fileno")?.unbind())
    }

    #[getter]
    fn rich_proxied_file(&self, py: Python<'_>) -> Py<PyAny> {
        self.file.clone_ref(py)
    }

    fn __getattr__(&self, py: Python<'_>, name: &str) -> PyResult<Py<PyAny>> {
        Ok(self.file.bind(py).getattr(name)?.unbind())
    }

    fn __traverse__(&self, visit: PyVisit<'_>) -> Result<(), PyTraverseError> {
        visit.call(&self.live)?;
        visit.call(&self.file)
    }
}

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
    m.add_class::<LiveFile>()?;
    m.add_class::<FileProxy>()?;
    let stop = pyo3::wrap_pyfunction!(_stop_live_threads, m)?;
    m.add_function(stop.clone())?;
    py.import("atexit")?.call_method1("register", (stop,))?;
    Ok(())
}
