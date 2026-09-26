//! `rs_rich.ext.workflow` (command records and runner, task trees,
//! completion summaries, clocks) and `rs_rich.ext.cancel` (cancel tokens).
//!
//! Task ids are the `int`s `TaskTree.add` returns. Durations are seconds.

use std::collections::BTreeMap;
use std::time::Duration;

use pyo3::exceptions::{PyIndexError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyType;

use rich::console::{Console as CoreConsole, ConsoleOptions as CoreOptions};
use rich::protocol::Renderable;
use rich::segment::Segment as CoreSegment;
use rich_ext::a11y::policy::{AccessibilityPolicy as CorePolicy, SymbolSet};
use rich_ext::cancel::CancelToken as CoreToken;
use rich_ext::workflow::{
    CommandRecord as CoreRecord, CommandRunner, CommandStatus, CompletionSummary as CoreSummary,
    State, Stream, SummaryItem as CoreItem, TaskId, TaskTree as CoreTree,
};
use rich_ext::workflow::{ManualClock as CoreManualClock, SystemClock};

use super::common::{self, names};
use super::diagnostic::Diagnostic;
use super::terminal::{policy_arg, symbol_set};
use crate::renderable::{self, AsRenderable};

names!(state, state_name, State, "state", {
    "failed" => State::Failed,
    "cancelled" => State::Cancelled,
    "warning" => State::Warning,
    "running" => State::Running,
    "pending" => State::Pending,
    "succeeded" => State::Succeeded,
    "skipped" => State::Skipped,
});

names!(stream, stream_name, Stream, "stream", {
    "stdout" => Stream::Stdout,
    "stderr" => Stream::Stderr,
});

fn counts_dict(counts: BTreeMap<State, usize>) -> BTreeMap<&'static str, usize> {
    counts
        .into_iter()
        .map(|(state, count)| (state_name(state), count))
        .collect()
}

/// `state_marker(state, symbols="unicode")`: how a state is marked
/// (`"✔ ok"`, `"[RUN]"`, `"failed:"`).
#[pyfunction]
#[pyo3(signature = (state, symbols="unicode"))]
fn state_marker(state: &str, symbols: &str) -> PyResult<&'static str> {
    Ok(self::state(state)?.marker(symbol_set(symbols)?))
}

/// `state_label(state, count)`: `"2 failed"`, `"1 warning"`, `"3 warnings"`.
#[pyfunction]
fn state_label(state: &str, count: usize) -> PyResult<String> {
    Ok(self::state(state)?.count_label(count))
}

// ---------------------------------------------------------------------------
// Cancellation and clocks

/// `CancelToken()`: one cancellation flag shared by clones; a `child()` is
/// cancelled with its parent but can be cancelled alone.
#[pyclass(
    name = "CancelToken",
    module = "rs_rich.ext.cancel",
    frozen,
    skip_from_py_object
)]
#[derive(Clone)]
pub(crate) struct CancelToken {
    pub(crate) inner: CoreToken,
}

pub(crate) fn token_arg(value: Option<&Bound<'_, PyAny>>) -> PyResult<Option<CoreToken>> {
    value
        .filter(|v| !v.is_none())
        .map(|v| Ok(v.extract::<PyRef<'_, CancelToken>>()?.inner.clone()))
        .transpose()
}

#[pymethods]
impl CancelToken {
    #[new]
    fn new() -> Self {
        CancelToken {
            inner: CoreToken::new(),
        }
    }

    /// A token cancelled when this one is, which can be cancelled alone.
    fn child(&self) -> CancelToken {
        CancelToken {
            inner: self.inner.child(),
        }
    }

    /// Cancel this token, its clones and its children.
    fn cancel(&self) {
        self.inner.cancel();
    }

    /// Whether this token or an ancestor was cancelled.
    #[getter]
    fn is_cancelled(&self) -> bool {
        self.inner.is_cancelled()
    }

    fn __bool__(&self) -> bool {
        self.inner.is_cancelled()
    }

    fn __repr__(&self) -> String {
        format!("<CancelToken cancelled={}>", self.inner.is_cancelled())
    }
}

/// `ManualClock()`: a clock that moves only when told, for deterministic
/// durations and spinners. Clones share the time.
#[pyclass(name = "ManualClock", module = "rs_rich.ext.workflow", frozen)]
pub(crate) struct ManualClock {
    pub(crate) inner: CoreManualClock,
}

#[pymethods]
impl ManualClock {
    #[new]
    #[pyo3(signature = (start=None))]
    fn new(start: Option<&Bound<'_, PyAny>>) -> PyResult<Self> {
        let inner = CoreManualClock::new();
        if let Some(start) = common::opt_seconds(start)? {
            inner.set(start);
        }
        Ok(ManualClock { inner })
    }

    /// Move the clock forward by `seconds`.
    fn advance(&self, seconds: &Bound<'_, PyAny>) -> PyResult<()> {
        self.inner.advance(common::seconds(seconds)?);
        Ok(())
    }

    /// Set the clock to `seconds`.
    fn set(&self, seconds: &Bound<'_, PyAny>) -> PyResult<()> {
        self.inner.set(common::seconds(seconds)?);
        Ok(())
    }

    /// The time, in seconds.
    fn now(&self) -> f64 {
        rich_ext::workflow::Clock::now(&self.inner).as_secs_f64()
    }
}

// ---------------------------------------------------------------------------
// Task trees

/// `TaskTree(title=None, *, clock=None)`: nested tasks with timing,
/// aggregate status and cancellation. `add(label, parent=None)` returns a
/// task id (`int`).
#[pyclass(name = "TaskTree", module = "rs_rich.ext.workflow")]
pub(crate) struct TaskTree {
    inner: CoreTree,
    ids: Vec<TaskId>,
    collapse_finished: bool,
    show_durations: bool,
    symbols: SymbolSet,
    animate: bool,
    policy: Option<CorePolicy>,
}

impl TaskTree {
    fn id(&self, id: usize) -> PyResult<TaskId> {
        self.ids
            .get(id)
            .copied()
            .ok_or_else(|| PyIndexError::new_err(format!("no task {id} in this tree")))
    }

    fn index(&self, id: TaskId) -> usize {
        self.ids.iter().position(|i| *i == id).unwrap_or(usize::MAX)
    }
}

struct TreeView {
    tree: CoreTree,
    collapse_finished: bool,
    show_durations: bool,
    symbols: SymbolSet,
    animate: bool,
    policy: Option<CorePolicy>,
}

impl Renderable for TreeView {
    fn rich_render(&self, console: &CoreConsole, options: &CoreOptions) -> Vec<CoreSegment> {
        let mut view = self
            .tree
            .view()
            .collapse_finished(self.collapse_finished)
            .show_durations(self.show_durations)
            .symbols(self.symbols)
            .animate(self.animate);
        if let Some(policy) = &self.policy {
            view = view.policy(policy);
        }
        view.rich_render(console, options)
    }
}

/// A snapshot of a `TaskTree` with view options: `TaskTree.view(...)`.
#[pyclass(name = "TaskTreeView", module = "rs_rich.ext.workflow", frozen)]
pub(crate) struct TaskTreeView {
    view: std::sync::Mutex<Option<TreeView>>,
}

impl AsRenderable for TaskTreeView {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        let view = self.view.lock().unwrap_or_else(|e| e.into_inner());
        let view = view.as_ref().expect("a view holds its tree");
        Ok(Box::new(TreeView {
            tree: view.tree.clone(),
            collapse_finished: view.collapse_finished,
            show_durations: view.show_durations,
            symbols: view.symbols,
            animate: view.animate,
            policy: view.policy.clone(),
        }))
    }
}

impl AsRenderable for TaskTree {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(TreeView {
            tree: self.inner.clone(),
            collapse_finished: self.collapse_finished,
            show_durations: self.show_durations,
            symbols: self.symbols,
            animate: self.animate,
            policy: self.policy.clone(),
        }))
    }
}

#[pymethods]
impl TaskTree {
    #[new]
    #[pyo3(signature = (
        title=None, *, clock=None, collapse_finished=false, show_durations=true,
        symbols="unicode", animate=true, policy=None
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        title: Option<String>,
        clock: Option<PyRef<'_, ManualClock>>,
        collapse_finished: bool,
        show_durations: bool,
        symbols: &str,
        animate: bool,
        policy: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        let mut inner = match clock {
            Some(clock) => CoreTree::with_clock(clock.inner.clone()),
            None => CoreTree::with_clock(SystemClock::new()),
        };
        if let Some(title) = title {
            inner = inner.title(title);
        }
        Ok(TaskTree {
            inner,
            ids: Vec::new(),
            collapse_finished,
            show_durations,
            symbols: symbol_set(symbols)?,
            animate,
            policy: policy_arg(policy)?,
        })
    }

    #[getter]
    fn title(&self) -> Option<String> {
        self.inner.get_title().map(str::to_string)
    }

    /// The tree as it is now, with view options (defaults: the tree's).
    #[pyo3(signature = (*, collapse_finished=None, show_durations=None, symbols=None, animate=None, policy=None))]
    fn view(
        &self,
        collapse_finished: Option<bool>,
        show_durations: Option<bool>,
        symbols: Option<&str>,
        animate: Option<bool>,
        policy: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<TaskTreeView> {
        let view = TreeView {
            tree: self.inner.clone(),
            collapse_finished: collapse_finished.unwrap_or(self.collapse_finished),
            show_durations: show_durations.unwrap_or(self.show_durations),
            symbols: match symbols {
                Some(symbols) => symbol_set(symbols)?,
                None => self.symbols,
            },
            animate: animate.unwrap_or(self.animate),
            policy: match policy_arg(policy)? {
                Some(policy) => Some(policy),
                None => self.policy.clone(),
            },
        };
        Ok(TaskTreeView {
            view: std::sync::Mutex::new(Some(view)),
        })
    }

    /// Add a pending task under `parent` (a task id) or at the top level.
    #[pyo3(signature = (label, parent=None))]
    fn add(&mut self, label: String, parent: Option<usize>) -> PyResult<usize> {
        let parent = parent.map(|p| self.id(p)).transpose()?;
        let id = self.inner.add(parent, label);
        self.ids.push(id);
        Ok(self.ids.len() - 1)
    }

    fn start(&mut self, task: usize) -> PyResult<()> {
        let id = self.id(task)?;
        self.inner.start(id);
        Ok(())
    }

    fn succeed(&mut self, task: usize) -> PyResult<()> {
        let id = self.id(task)?;
        self.inner.succeed(id);
        Ok(())
    }

    fn warn(&mut self, task: usize, message: String) -> PyResult<()> {
        let id = self.id(task)?;
        self.inner.warn(id, message);
        Ok(())
    }

    fn fail(&mut self, task: usize, message: String) -> PyResult<()> {
        let id = self.id(task)?;
        self.inner.fail(id, message);
        Ok(())
    }

    #[pyo3(signature = (task, reason=None))]
    fn skip(&mut self, task: usize, reason: Option<&str>) -> PyResult<()> {
        let id = self.id(task)?;
        self.inner.skip(id, reason);
        Ok(())
    }

    fn note(&mut self, task: usize, note: String) -> PyResult<()> {
        let id = self.id(task)?;
        self.inner.note(id, note);
        Ok(())
    }

    #[pyo3(signature = (task, completed, total=None))]
    fn progress(&mut self, task: usize, completed: u64, total: Option<u64>) -> PyResult<()> {
        let id = self.id(task)?;
        self.inner.progress(id, completed, total);
        Ok(())
    }

    /// Cancel a task (and the tasks under it).
    fn cancel(&mut self, task: usize) -> PyResult<()> {
        let id = self.id(task)?;
        self.inner.cancel(id);
        Ok(())
    }

    fn cancel_all(&mut self) {
        self.inner.cancel_all();
    }

    /// Mark tasks whose tokens were cancelled; returns how many changed.
    fn sync_cancelled(&mut self) -> usize {
        self.inner.sync_cancelled()
    }

    /// The task's cancel token (cancelled with its parent's).
    fn token(&self, task: usize) -> PyResult<CancelToken> {
        Ok(CancelToken {
            inner: self.inner.token(self.id(task)?),
        })
    }

    fn root_token(&self) -> CancelToken {
        CancelToken {
            inner: self.inner.root_token(),
        }
    }

    fn label(&self, task: usize) -> PyResult<String> {
        Ok(self.inner.label(self.id(task)?).to_string())
    }

    fn get_note(&self, task: usize) -> PyResult<Option<String>> {
        Ok(self.inner.get_note(self.id(task)?).map(str::to_string))
    }

    fn parent(&self, task: usize) -> PyResult<Option<usize>> {
        Ok(self.inner.parent(self.id(task)?).map(|p| self.index(p)))
    }

    fn children(&self, task: usize) -> PyResult<Vec<usize>> {
        Ok(self
            .inner
            .children(self.id(task)?)
            .iter()
            .map(|c| self.index(*c))
            .collect())
    }

    fn roots(&self) -> Vec<usize> {
        self.inner.roots().iter().map(|c| self.index(*c)).collect()
    }

    /// Every task, depth first.
    fn tasks(&self) -> Vec<usize> {
        self.inner.iter().map(|c| self.index(c)).collect()
    }

    fn leaves(&self) -> Vec<usize> {
        self.inner.leaves().map(|c| self.index(c)).collect()
    }

    fn get_progress(&self, task: usize) -> PyResult<Option<(u64, Option<u64>)>> {
        Ok(self.inner.get_progress(self.id(task)?))
    }

    /// A task's state (a parent's aggregates its children's).
    fn state(&self, task: usize) -> PyResult<&'static str> {
        Ok(state_name(self.inner.state(self.id(task)?)))
    }

    fn overall(&self) -> &'static str {
        state_name(self.inner.overall())
    }

    #[getter]
    fn is_finished(&self) -> bool {
        self.inner.is_finished()
    }

    /// Seconds from start to finish (or now), if started.
    fn elapsed(&self, task: usize) -> PyResult<Option<f64>> {
        Ok(self.inner.elapsed(self.id(task)?).map(|d| d.as_secs_f64()))
    }

    fn total_elapsed(&self) -> Option<f64> {
        self.inner.total_elapsed().map(|d| d.as_secs_f64())
    }

    /// Leaf tasks per state.
    fn counts(&self) -> BTreeMap<&'static str, usize> {
        counts_dict(self.inner.counts())
    }

    /// A `CompletionSummary` of the tree (its title, leaf counts, the
    /// tasks that did not succeed), with optional next steps.
    #[pyo3(signature = (*, next_steps=None, show_all_items=false, symbols=None, policy=None))]
    fn summary(
        &self,
        next_steps: Option<&Bound<'_, PyAny>>,
        show_all_items: bool,
        symbols: Option<&str>,
        policy: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<CompletionSummary> {
        let mut inner = CoreSummary::from(&self.inner).show_all_items(show_all_items);
        if let Some(steps) = next_steps {
            for step in common::strings(steps)? {
                inner = inner.next_step(step);
            }
        }
        if let Some(symbols) = symbols {
            inner = inner.symbols(symbol_set(symbols)?);
        }
        if let Some(policy) = policy_arg(policy)? {
            inner = inner.policy(&policy);
        }
        Ok(CompletionSummary { inner })
    }

    fn __len__(&self) -> usize {
        self.ids.len()
    }
}

// ---------------------------------------------------------------------------
// Completion summaries

/// `SummaryItem(state, label, *, duration=None, detail=None)`.
#[pyclass(
    name = "SummaryItem",
    module = "rs_rich.ext.workflow",
    frozen,
    skip_from_py_object
)]
#[derive(Clone)]
pub(crate) struct SummaryItem {
    inner: CoreItem,
}

#[pymethods]
impl SummaryItem {
    #[new]
    #[pyo3(signature = (state, label, *, duration=None, detail=None))]
    fn new(
        state: &str,
        label: String,
        duration: Option<&Bound<'_, PyAny>>,
        detail: Option<String>,
    ) -> PyResult<Self> {
        let mut inner = CoreItem::new(self::state(state)?, label);
        if let Some(duration) = common::opt_seconds(duration)? {
            inner = inner.duration(duration);
        }
        if let Some(detail) = detail {
            inner = inner.detail(detail);
        }
        Ok(SummaryItem { inner })
    }

    /// The item for a finished command.
    #[classmethod]
    fn from_record(_cls: &Bound<'_, PyType>, record: PyRef<'_, CommandRecord>) -> Self {
        SummaryItem {
            inner: CoreItem::from(&record.inner),
        }
    }

    #[getter]
    fn state(&self) -> &'static str {
        state_name(self.inner.state)
    }
    #[getter]
    fn label(&self) -> String {
        self.inner.label.clone()
    }
    #[getter]
    fn duration(&self) -> Option<f64> {
        self.inner.duration.map(|d| d.as_secs_f64())
    }
    #[getter]
    fn detail(&self) -> Option<String> {
        self.inner.detail.clone()
    }
}

fn item_arg(value: &Bound<'_, PyAny>) -> PyResult<CoreItem> {
    if let Ok(item) = value.extract::<PyRef<'_, SummaryItem>>() {
        return Ok(item.inner.clone());
    }
    if let Ok(record) = value.extract::<PyRef<'_, CommandRecord>>() {
        return Ok(CoreItem::from(&record.inner));
    }
    let (state, label): (String, String) = value.extract()?;
    Ok(CoreItem::new(self::state(&state)?, label))
}

/// `CompletionSummary(title, *, status=None, counts=None, duration=None,
/// items=(), next_steps=(), show_all_items=False, symbols="unicode",
/// policy=None)`: the end-of-run summary: overall status, counts, the
/// items that did not succeed, and next steps.
#[pyclass(name = "CompletionSummary", module = "rs_rich.ext.workflow", frozen)]
pub(crate) struct CompletionSummary {
    inner: CoreSummary,
}

impl AsRenderable for CompletionSummary {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(self.inner.clone()))
    }
}

#[pymethods]
impl CompletionSummary {
    #[new]
    #[pyo3(signature = (
        title, *, status=None, counts=None, duration=None, items=None, next_steps=None,
        show_all_items=false, symbols="unicode", policy=None
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        title: String,
        status: Option<&str>,
        counts: Option<&Bound<'_, PyAny>>,
        duration: Option<&Bound<'_, PyAny>>,
        items: Option<&Bound<'_, PyAny>>,
        next_steps: Option<&Bound<'_, PyAny>>,
        show_all_items: bool,
        symbols: &str,
        policy: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        let mut inner = CoreSummary::new(title)
            .show_all_items(show_all_items)
            .symbols(symbol_set(symbols)?);
        if let Some(status) = status {
            inner = inner.status(state(status)?);
        }
        if let Some(counts) = counts {
            for (name, count) in common::pairs(counts)? {
                inner = inner.count(state(&name)?, count.extract()?);
            }
        }
        if let Some(duration) = common::opt_seconds(duration)? {
            inner = inner.duration(duration);
        }
        if let Some(items) = items {
            for item in items.try_iter()? {
                inner = inner.push(item_arg(&item?)?);
            }
        }
        if let Some(steps) = next_steps {
            for step in common::strings(steps)? {
                inner = inner.next_step(step);
            }
        }
        if let Some(policy) = policy_arg(policy)? {
            inner = inner.policy(&policy);
        }
        Ok(CompletionSummary { inner })
    }

    /// Items per state (the given counts, else counted from the items).
    fn counts(&self) -> BTreeMap<&'static str, usize> {
        counts_dict(self.inner.counts())
    }

    /// The overall state: the given status, else the worst item's.
    fn overall(&self) -> &'static str {
        state_name(self.inner.overall())
    }
}

// ---------------------------------------------------------------------------
// Commands

/// `CommandRecord(program, args=(), *, cwd=None, stdout=None, stderr=None,
/// status="running", duration=0)`: a process's output, exit status and
/// duration. `status` is an exit code (`int`), `"running"`,
/// `"cancelled"`, `("signal", n)` or `("failed_to_start", reason)`.
#[pyclass(
    name = "CommandRecord",
    module = "rs_rich.ext.workflow",
    skip_from_py_object
)]
#[derive(Clone)]
pub(crate) struct CommandRecord {
    pub(crate) inner: CoreRecord,
}

fn command_status(value: &Bound<'_, PyAny>) -> PyResult<CommandStatus> {
    if let Ok(code) = value.extract::<i32>() {
        return Ok(CommandStatus::Exited(code));
    }
    if let Ok(name) = value.extract::<String>() {
        return match name.as_str() {
            "running" => Ok(CommandStatus::Running),
            "cancelled" => Ok(CommandStatus::Cancelled),
            other => Err(PyValueError::new_err(format!(
                "invalid status {other:?}; expected an exit code, running, cancelled, \
                 (\"signal\", n) or (\"failed_to_start\", reason)"
            ))),
        };
    }
    let (kind, detail): (String, Bound<'_, PyAny>) = value.extract()?;
    match kind.as_str() {
        "exited" => Ok(CommandStatus::Exited(detail.extract()?)),
        "signal" => Ok(CommandStatus::Signalled(detail.extract()?)),
        "failed_to_start" => Ok(CommandStatus::FailedToStart(detail.extract()?)),
        other => Err(PyValueError::new_err(format!(
            "invalid status kind {other:?}"
        ))),
    }
}

fn status_value(py: Python<'_>, status: &CommandStatus) -> PyResult<Py<PyAny>> {
    Ok(match status {
        CommandStatus::Running => "running".into_pyobject(py)?.into_any().unbind(),
        CommandStatus::Cancelled => "cancelled".into_pyobject(py)?.into_any().unbind(),
        CommandStatus::Exited(code) => code.into_pyobject(py)?.into_any().unbind(),
        CommandStatus::Signalled(signal) => {
            ("signal", *signal).into_pyobject(py)?.into_any().unbind()
        }
        CommandStatus::FailedToStart(reason) => ("failed_to_start", reason.clone())
            .into_pyobject(py)?
            .into_any()
            .unbind(),
    })
}

impl AsRenderable for CommandRecord {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(self.inner.clone()))
    }
}

#[pymethods]
impl CommandRecord {
    #[new]
    #[pyo3(signature = (program, args=None, *, cwd=None, stdout=None, stderr=None, status=None, duration=None))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        program: String,
        args: Option<&Bound<'_, PyAny>>,
        cwd: Option<std::path::PathBuf>,
        stdout: Option<&str>,
        stderr: Option<&str>,
        status: Option<&Bound<'_, PyAny>>,
        duration: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        let args = match args {
            Some(args) => common::strings(args)?,
            None => Vec::new(),
        };
        let mut inner = CoreRecord::new(program, args);
        if let Some(cwd) = cwd {
            inner = inner.cwd(cwd);
        }
        if let Some(text) = stdout {
            inner = inner.stdout(text);
        }
        if let Some(text) = stderr {
            inner = inner.stderr(text);
        }
        if let Some(status) = status {
            inner = inner.status(command_status(status)?);
        }
        if let Some(duration) = common::opt_seconds(duration)? {
            inner = inner.duration(duration);
        }
        Ok(CommandRecord { inner })
    }

    #[getter]
    fn program(&self) -> String {
        self.inner.program.clone()
    }
    #[getter]
    fn args(&self) -> Vec<String> {
        self.inner.args.clone()
    }
    #[getter]
    fn cwd(&self) -> Option<std::path::PathBuf> {
        self.inner.cwd.clone()
    }
    /// `(stream, text)` for each output line, in order.
    #[getter]
    fn lines(&self) -> Vec<(&'static str, String)> {
        self.inner
            .lines
            .iter()
            .map(|l| (stream_name(l.stream), l.text.clone()))
            .collect()
    }
    #[getter]
    fn get_status(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        status_value(py, &self.inner.status)
    }
    #[setter]
    fn set_status(&mut self, value: &Bound<'_, PyAny>) -> PyResult<()> {
        self.inner.status = command_status(value)?;
        Ok(())
    }
    /// The exit code, when the process exited.
    #[getter]
    fn returncode(&self) -> Option<i32> {
        match self.inner.status {
            CommandStatus::Exited(code) => Some(code),
            _ => None,
        }
    }
    #[getter]
    fn get_duration(&self) -> f64 {
        self.inner.duration.as_secs_f64()
    }
    #[setter]
    fn set_duration(&mut self, value: &Bound<'_, PyAny>) -> PyResult<()> {
        self.inner.duration = common::seconds(value)?;
        Ok(())
    }
    #[getter]
    fn state(&self) -> &'static str {
        state_name(self.inner.state())
    }
    #[getter]
    fn success(&self) -> bool {
        self.inner.status.success()
    }
    /// `exit 2`, `signal 9`, `failed to start` or `""`.
    #[getter]
    fn detail(&self) -> String {
        self.inner.status.detail()
    }
    /// The command line, quoted for a POSIX shell.
    #[getter]
    fn command_line(&self) -> String {
        self.inner.command_line()
    }

    /// One stream's output (`"stdout"` or `"stderr"`).
    #[pyo3(signature = (stream="stdout"))]
    fn output(&self, stream: &str) -> PyResult<String> {
        Ok(self.inner.output(self::stream(stream)?))
    }

    /// Add output (split into lines) to a stream.
    fn push(&mut self, stream: &str, text: &str) -> PyResult<()> {
        self.inner.push(self::stream(stream)?, text);
        Ok(())
    }

    /// The failure as an error `Diagnostic`, or `None` for a success.
    fn diagnostic(&self) -> Option<Diagnostic> {
        self.inner
            .diagnostic()
            .map(|d| Diagnostic::from_core(d, rich_ext::event::EventView::Compact))
    }

    /// A `CommandView` of the record.
    #[pyo3(signature = (
        *, tail=Some(10), full_on_failure=true, show_cwd=true, show_diagnostic=true, help=None,
        symbols="unicode", animate=true, policy=None
    ))]
    #[allow(clippy::too_many_arguments)]
    fn view(
        &self,
        tail: Option<usize>,
        full_on_failure: bool,
        show_cwd: bool,
        show_diagnostic: bool,
        help: Option<&Bound<'_, PyAny>>,
        symbols: &str,
        animate: bool,
        policy: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<CommandView> {
        Ok(CommandView {
            record: self.inner.clone(),
            tail,
            full_on_failure,
            show_cwd,
            show_diagnostic,
            help: help.map(common::strings).transpose()?.unwrap_or_default(),
            symbols: symbol_set(symbols)?,
            animate,
            policy: policy_arg(policy)?,
        })
    }
}

/// A command's header, status and output (its last `tail` lines, or all of
/// them on failure), with its diagnostic. `CommandRecord.view(...)`.
#[pyclass(name = "CommandView", module = "rs_rich.ext.workflow", frozen)]
pub(crate) struct CommandView {
    record: CoreRecord,
    tail: Option<usize>,
    full_on_failure: bool,
    show_cwd: bool,
    show_diagnostic: bool,
    help: Vec<String>,
    symbols: SymbolSet,
    animate: bool,
    policy: Option<CorePolicy>,
}

struct OwnedCommandView {
    record: CoreRecord,
    tail: Option<usize>,
    full_on_failure: bool,
    show_cwd: bool,
    show_diagnostic: bool,
    help: Vec<String>,
    symbols: SymbolSet,
    animate: bool,
    policy: Option<CorePolicy>,
}

impl Renderable for OwnedCommandView {
    fn rich_render(&self, console: &CoreConsole, options: &CoreOptions) -> Vec<CoreSegment> {
        let mut view = self
            .record
            .view()
            .full_on_failure(self.full_on_failure)
            .show_cwd(self.show_cwd)
            .show_diagnostic(self.show_diagnostic)
            .symbols(self.symbols)
            .animate(self.animate);
        view = match self.tail {
            Some(lines) => view.tail(lines),
            None => view.show_all(),
        };
        for help in &self.help {
            view = view.help(help.clone());
        }
        if let Some(policy) = &self.policy {
            view = view.policy(policy);
        }
        view.rich_render(console, options)
    }
}

impl AsRenderable for CommandView {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(OwnedCommandView {
            record: self.record.clone(),
            tail: self.tail,
            full_on_failure: self.full_on_failure,
            show_cwd: self.show_cwd,
            show_diagnostic: self.show_diagnostic,
            help: self.help.clone(),
            symbols: self.symbols,
            animate: self.animate,
            policy: self.policy.clone(),
        }))
    }
}

/// `run_command(args, *, cwd=None, env=None, cancel=None, tick=0.1,
/// on_update=None)`: run a process to completion, capturing its output
/// line by line, and return its `CommandRecord`. `on_update(record)` is
/// called with each new line (and on each tick); a cancelled `cancel`
/// token stops the process.
#[pyfunction]
#[pyo3(signature = (args, *, cwd=None, env=None, cancel=None, tick=None, on_update=None))]
fn run_command(
    py: Python<'_>,
    args: &Bound<'_, PyAny>,
    cwd: Option<std::path::PathBuf>,
    env: Option<&Bound<'_, PyAny>>,
    cancel: Option<&Bound<'_, PyAny>>,
    tick: Option<&Bound<'_, PyAny>>,
    on_update: Option<Py<PyAny>>,
) -> PyResult<CommandRecord> {
    let args = common::strings(args)?;
    let Some((program, rest)) = args.split_first() else {
        return Err(PyValueError::new_err("run_command needs a program"));
    };
    let mut command = std::process::Command::new(program);
    command.args(rest);
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    if let Some(env) = env {
        command.env_clear();
        for (key, value) in common::pairs(env)? {
            command.env(key, value.str()?.to_string());
        }
    }
    let token = common_token(cancel)?;
    let tick = common::opt_seconds(tick)?.unwrap_or(Duration::from_millis(100));
    let mut error: Option<PyErr> = None;
    let record = py.detach(|| {
        let mut runner = CommandRunner::new().tick(tick);
        if let Some(token) = token {
            runner = runner.cancel(token);
        }
        if let Some(callback) = &on_update {
            runner = runner.on_update(|record: &CoreRecord| {
                if error.is_some() {
                    return;
                }
                Python::attach(|py| {
                    let result = Py::new(
                        py,
                        CommandRecord {
                            inner: record.clone(),
                        },
                    )
                    .and_then(|r| callback.bind(py).call1((r,)).map(|_| ()));
                    if let Err(e) = result {
                        error = Some(e);
                    }
                });
            });
        }
        runner.run(&mut command)
    });
    if let Some(error) = error {
        return Err(error);
    }
    Ok(CommandRecord { inner: record })
}

fn common_token(value: Option<&Bound<'_, PyAny>>) -> PyResult<Option<CoreToken>> {
    token_arg(value)
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(pyo3::wrap_pyfunction!(state_marker, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(state_label, m)?)?;
    m.add_class::<CancelToken>()?;
    m.add_class::<ManualClock>()?;
    renderable::add_renderable_class::<TaskTree>(m)?;
    renderable::add_renderable_class::<TaskTreeView>(m)?;
    m.add_class::<SummaryItem>()?;
    renderable::add_renderable_class::<CompletionSummary>(m)?;
    renderable::add_renderable_class::<CommandRecord>(m)?;
    renderable::add_renderable_class::<CommandView>(m)?;
    m.add_function(pyo3::wrap_pyfunction!(run_command, m)?)?;
    m.add(
        "WORKFLOW_STATES",
        State::ALL
            .iter()
            .map(|s| state_name(*s))
            .collect::<Vec<_>>(),
    )?;
    m.add("WORKFLOW_STYLES", rich_ext::workflow::STYLES.to_vec())?;
    Ok(())
}
