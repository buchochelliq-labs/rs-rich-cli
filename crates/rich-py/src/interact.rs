//! `rs_rich.interact`: the `rs-rich-interact` crate (0.0.13 workstream 5):
//! interactive components, the drivers that run them and the fuzzy matcher.
//!
//! Owner: the interact area. Rich has no interactive components, so this
//! mirrors the Rust crate, not Rich. A component class here stores what it
//! was given (the Python values of its items, a validator, renderables) and
//! builds a fresh Rust component for every run, so a component can be run
//! again. Its items' values stay Python objects: the Rust component chooses
//! among item indices, and the index picks the object back out.
//!
//! Three drivers, one per [`Mode`]:
//!
//! - **terminal** (`run`, `ask`): `rich_interact::run`, with the GIL released
//!   while it waits for keys; without a terminal it follows the fallback;
//! - **headless** (`headless`): scripted keys in, painted frames out, for
//!   tests, with virtual time;
//! - **line prompts** (`degrade`): the fallback path with scripted answers.
//!
//! Every run is inside a render scope (`ext::common::scoped`), so Python
//! renderables in previews, confirmation bodies and pagers render, and an
//! exception raised by Python code during the run comes out of the call.

use std::time::Duration;

use pyo3::create_exception;
use pyo3::exceptions::{PyException, PyKeyboardInterrupt, PyOSError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyList, PyString};
use pyo3::{PyTraverseError, PyVisit};

use rich_interact::headless::{self, Script as CoreScript};
use rich_interact::policy::ScriptedLineIo;
use rich_interact::{
    Component, Error as RunError, Event, Fallback, Key, LoopOptions, NotInteractive as CoreNot,
    Outcome as CoreOutcome, Output, Policy, Reason, RunOptions, SessionOptions,
};

mod components;

use crate::ext::common::scoped;

create_exception!(_native, InteractError, PyException);
create_exception!(_native, Cancelled, InteractError);
create_exception!(_native, NotInteractive, InteractError);

// ---------------------------------------------------------------------------
// Names for Rust enums

fn fallback(name: &str) -> PyResult<Fallback> {
    Ok(match name {
        "prompt" => Fallback::Prompt,
        "default" => Fallback::Default,
        "error" => Fallback::Error,
        other => {
            return Err(PyValueError::new_err(format!(
                "invalid fallback {other:?}; expected prompt, default or error"
            )))
        }
    })
}

fn output(name: &str) -> PyResult<Output> {
    Ok(match name {
        "stdout" => Output::Stdout,
        "stderr" => Output::Stderr,
        other => {
            return Err(PyValueError::new_err(format!(
                "invalid output {other:?}; expected stdout or stderr"
            )))
        }
    })
}

const REASONS: &[(&str, Reason)] = &[
    ("stdin_not_terminal", Reason::StdinNotTerminal),
    ("stdout_not_terminal", Reason::StdoutNotTerminal),
    ("stderr_not_terminal", Reason::StderrNotTerminal),
    ("no_terminal", Reason::NoTerminal),
    ("ci", Reason::Ci),
    ("dumb_terminal", Reason::DumbTerminal),
    ("requested", Reason::Requested),
];

fn reason(name: &str) -> PyResult<Reason> {
    REASONS
        .iter()
        .find(|(text, _)| *text == name)
        .map(|(_, reason)| *reason)
        .ok_or_else(|| {
            let names: Vec<&str> = REASONS.iter().map(|(text, _)| *text).collect();
            PyValueError::new_err(format!(
                "invalid reason {name:?}; expected {}",
                names.join(", ")
            ))
        })
}

fn reason_name(reason: Reason) -> &'static str {
    REASONS
        .iter()
        .find(|(_, r)| *r == reason)
        .map_or("unknown", |(text, _)| text)
}

// ---------------------------------------------------------------------------
// Scripts

/// `Script(keys=None)`: scripted events for `headless`, built by chaining:
/// `Script().keys("down down").text("abc").keys("enter")`.
#[pyclass(name = "Script", module = "rs_rich.interact", skip_from_py_object)]
#[derive(Clone, Default)]
pub(crate) struct Script {
    inner: CoreScript,
}

fn parse_keys(script: CoreScript, names: &str) -> PyResult<CoreScript> {
    let mut script = script;
    for name in names.split_whitespace() {
        let key = Key::parse(name)
            .ok_or_else(|| PyValueError::new_err(format!("unknown key name {name:?}")))?;
        script = script.event(Event::Key(key));
    }
    Ok(script)
}

#[pymethods]
impl Script {
    #[new]
    #[pyo3(signature = (keys=None))]
    fn new(keys: Option<&str>) -> PyResult<Self> {
        let inner = match keys {
            Some(keys) => parse_keys(CoreScript::new(), keys)?,
            None => CoreScript::new(),
        };
        Ok(Script { inner })
    }

    /// Key names separated by spaces: `enter`, `down`, `tab`, `shift+tab`,
    /// `ctrl+c`, `esc`, `space`, `pagedown`, `f5` or one character.
    fn keys<'py>(slf: Bound<'py, Self>, names: &str) -> PyResult<Bound<'py, Self>> {
        {
            let mut this = slf.borrow_mut();
            let script = std::mem::take(&mut this.inner);
            this.inner = parse_keys(script, names)?;
        }
        Ok(slf)
    }

    /// Each character of `text` as a key press.
    fn text<'py>(slf: Bound<'py, Self>, text: &str) -> Bound<'py, Self> {
        {
            let mut this = slf.borrow_mut();
            this.inner = std::mem::take(&mut this.inner).text(text);
        }
        slf
    }

    /// `text` pasted at once (a bracketed paste).
    fn paste<'py>(slf: Bound<'py, Self>, text: &str) -> Bound<'py, Self> {
        {
            let mut this = slf.borrow_mut();
            this.inner = std::mem::take(&mut this.inner).event(Event::Paste(text.to_string()));
        }
        slf
    }

    /// The terminal resized to `columns` × `rows`.
    fn resize<'py>(slf: Bound<'py, Self>, columns: u16, rows: u16) -> Bound<'py, Self> {
        {
            let mut this = slf.borrow_mut();
            this.inner = std::mem::take(&mut this.inner).resize(columns, rows);
        }
        slf
    }

    /// Let `seconds` of virtual time pass (ticks and timers fire).
    fn wait<'py>(slf: Bound<'py, Self>, seconds: f64) -> PyResult<Bound<'py, Self>> {
        let duration = Duration::try_from_secs_f64(seconds)
            .map_err(|error| PyValueError::new_err(error.to_string()))?;
        {
            let mut this = slf.borrow_mut();
            this.inner = std::mem::take(&mut this.inner).wait(duration);
        }
        Ok(slf)
    }

    fn __len__(&self) -> usize {
        self.inner.0.len()
    }

    fn __repr__(&self) -> String {
        format!("<Script steps={}>", self.inner.0.len())
    }
}

fn script_arg(value: Option<&Bound<'_, PyAny>>) -> PyResult<CoreScript> {
    let Some(value) = value.filter(|value| !value.is_none()) else {
        return Ok(CoreScript::new());
    };
    if let Ok(names) = value.cast::<PyString>() {
        return parse_keys(CoreScript::new(), &names.to_cow()?);
    }
    if let Ok(script) = value.cast::<Script>() {
        return Ok(script.borrow().inner.clone());
    }
    Err(PyTypeError::new_err(format!(
        "script must be a str of key names or a Script, got {}",
        value.get_type().name()?
    )))
}

// ---------------------------------------------------------------------------
// Outcomes and records

/// How a component ended: `kind` is `"done"`, `"cancelled"` (Escape, `q`)
/// or `"interrupted"` (Ctrl+C); `value` is the answer when done.
#[pyclass(name = "Outcome", module = "rs_rich.interact", frozen)]
pub(crate) struct Outcome {
    kind: &'static str,
    value: Option<Py<PyAny>>,
    action: Option<String>,
}

#[pymethods]
impl Outcome {
    fn __traverse__(&self, visit: PyVisit<'_>) -> Result<(), PyTraverseError> {
        if let Some(value) = &self.value {
            visit.call(value)?;
        }
        Ok(())
    }

    #[getter]
    fn kind(&self) -> &'static str {
        self.kind
    }

    /// The answer, or `None` when the component did not finish with one.
    #[getter]
    fn value(&self, py: Python<'_>) -> Py<PyAny> {
        self.value
            .as_ref()
            .map_or_else(|| py.None(), |value| value.clone_ref(py))
    }

    /// The id of the item `Action` whose key picked the item, if one did.
    #[getter]
    fn action(&self) -> Option<&str> {
        self.action.as_deref()
    }

    #[getter]
    fn done(&self) -> bool {
        self.kind == "done"
    }

    #[getter]
    fn cancelled(&self) -> bool {
        self.kind == "cancelled"
    }

    #[getter]
    fn interrupted(&self) -> bool {
        self.kind == "interrupted"
    }

    /// The value; `Cancelled` when cancelled, `KeyboardInterrupt` when
    /// interrupted.
    fn unwrap(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        match self.kind {
            "cancelled" => Err(Cancelled::new_err("cancelled")),
            "interrupted" => Err(PyKeyboardInterrupt::new_err(())),
            _ => Ok(self.value(py)),
        }
    }

    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        Ok(match &self.value {
            Some(value) => format!("Outcome(done, {})", value.bind(py).repr()?),
            None => format!("Outcome({})", self.kind),
        })
    }
}

/// What a `headless` or `degrade` run did: the `outcome` (`None` when a
/// headless script ran out first), every painted frame as plain text, the
/// bytes written, and for `degrade` the line prompts written.
#[pyclass(name = "Record", module = "rs_rich.interact", frozen)]
pub(crate) struct Record {
    outcome: Option<Py<Outcome>>,
    frames: Vec<String>,
    writes: Vec<String>,
    handoffs: Vec<String>,
    prompts: String,
    secrets: usize,
}

#[pymethods]
impl Record {
    fn __traverse__(&self, visit: PyVisit<'_>) -> Result<(), PyTraverseError> {
        if let Some(outcome) = &self.outcome {
            visit.call(outcome)?;
        }
        Ok(())
    }

    #[getter]
    fn outcome(&self, py: Python<'_>) -> Option<Py<Outcome>> {
        self.outcome.as_ref().map(|outcome| outcome.clone_ref(py))
    }

    /// Whether the component finished (the script did not run out first).
    #[getter]
    fn finished(&self) -> bool {
        self.outcome.is_some()
    }

    /// The outcome's value (`None` when unfinished or not done).
    #[getter]
    fn value(&self, py: Python<'_>) -> Py<PyAny> {
        match &self.outcome {
            Some(outcome) => outcome.get().value(py),
            None => py.None(),
        }
    }

    /// The plain text of every paint that changed something.
    #[getter]
    fn frames(&self) -> Vec<String> {
        self.frames.clone()
    }

    /// The last frame painted (`""` if none).
    #[getter]
    fn last_frame(&self) -> &str {
        self.frames.last().map_or("", String::as_str)
    }

    /// Every write, in order: paints (with their escape sequences), finishing.
    #[getter]
    fn writes(&self) -> Vec<String> {
        self.writes.clone()
    }

    /// Everything written, as one string.
    #[getter]
    fn output(&self) -> String {
        self.writes.concat()
    }

    /// Programs handed the terminal.
    #[getter]
    fn handoffs(&self) -> Vec<String> {
        self.handoffs.clone()
    }

    /// `degrade`: the line prompts written.
    #[getter]
    fn prompts(&self) -> &str {
        &self.prompts
    }

    /// `degrade`: how many answers were read as secrets (without echo).
    #[getter]
    fn secrets(&self) -> usize {
        self.secrets
    }

    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        let outcome = match &self.outcome {
            Some(outcome) => outcome.get().__repr__(py)?,
            None => "unfinished".to_string(),
        };
        Ok(format!("<Record {outcome} frames={}>", self.frames.len()))
    }
}

// ---------------------------------------------------------------------------
// Driving a component

/// How to run a component.
pub(crate) enum Mode {
    Terminal(RunOptions),
    Headless {
        script: CoreScript,
        columns: u16,
        rows: u16,
    },
    Degrade {
        fallback: Fallback,
        reason: Reason,
        answers: Vec<String>,
    },
}

impl Mode {
    fn size(&self) -> (usize, usize) {
        match self {
            Mode::Headless { columns, rows, .. } => (usize::from(*columns), usize::from(*rows)),
            _ => {
                let console = rich::Console::new();
                (console.width(), console.height())
            }
        }
    }
}

/// A component's configuration, with no Python references it cannot move:
/// built into a Rust component with the GIL released.
pub(crate) trait Build: Send {
    type C: Component;
    fn build(self) -> Self::C;
    /// The item action that picked the answer, for pickers.
    fn action(_component: &Self::C) -> Option<String> {
        None
    }
}

/// What one run produced, before its value is a Python object.
pub(crate) struct Ran<T> {
    /// `Ok(None)`: a headless script ran out first.
    outcome: Result<Option<CoreOutcome<T>>, RunError>,
    record: headless::Record,
    prompts: String,
    secrets: usize,
    action: Option<String>,
}

/// Run a built component in `mode`, with the GIL released.
pub(crate) fn execute<B>(
    py: Python<'_>,
    build: B,
    mode: Mode,
) -> PyResult<Ran<<B::C as Component>::Output>>
where
    B: Build,
    <B::C as Component>::Output: Send,
{
    let (width, height) = mode.size();
    // Renderables see a terminal only when the run gets one: a run that
    // takes its fallback (a pipe, `interactive=False`) renders as for a file.
    let interactive = match &mode {
        Mode::Terminal(options) => options.policy.detect_for(options.session.output).is_ok(),
        _ => false,
    };
    scoped(py, width, height, interactive, || {
        Ok(py.detach(move || {
            let mut component = build.build();
            let mut ran = Ran {
                outcome: Ok(None),
                record: headless::Record::default(),
                prompts: String::new(),
                secrets: 0,
                action: None,
            };
            match mode {
                Mode::Terminal(options) => {
                    ran.outcome = rich_interact::run(&mut component, &options).map(Some);
                }
                Mode::Headless {
                    script,
                    columns,
                    rows,
                } => {
                    let (outcome, record) = headless::run(&mut component, script, columns, rows);
                    ran.outcome = match outcome {
                        Ok(outcome) => Ok(Some(outcome)),
                        Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => Ok(None),
                        Err(error) => Err(RunError::Io(error)),
                    };
                    ran.record = record;
                }
                Mode::Degrade {
                    fallback,
                    reason,
                    answers,
                } => {
                    let mut io = ScriptedLineIo::new(answers);
                    ran.outcome =
                        rich_interact::degrade(&mut component, fallback, reason, &mut io).map(Some);
                    ran.prompts = io.written;
                    ran.secrets = io.secrets;
                }
            }
            ran.action = B::action(&component);
            ran
        }))
    })
}

fn run_error(error: RunError) -> PyErr {
    match error {
        RunError::Io(error) => PyOSError::new_err(error.to_string()),
        RunError::NotInteractive(error) => {
            let reason = match &error {
                CoreNot::Terminal(reason) | CoreNot::NoDefault(reason) => {
                    Some(reason_name(*reason))
                }
                _ => None,
            };
            let err = NotInteractive::new_err(error.to_string());
            Python::attach(|py| {
                let _ = err.value(py).setattr("reason", reason);
            });
            err
        }
    }
}

/// A finished run as a `Record`, its value converted by `convert`.
pub(crate) fn record<T>(
    py: Python<'_>,
    ran: Ran<T>,
    convert: impl FnOnce(T) -> PyResult<Py<PyAny>>,
) -> PyResult<Record> {
    let outcome = ran.outcome.map_err(run_error)?;
    let outcome = match outcome {
        None => None,
        Some(outcome) => {
            let (kind, value) = match outcome {
                CoreOutcome::Done(value) => ("done", Some(convert(value)?)),
                CoreOutcome::Cancelled => ("cancelled", None),
                CoreOutcome::Interrupted => ("interrupted", None),
            };
            Some(Py::new(
                py,
                Outcome {
                    kind,
                    value,
                    action: ran.action,
                },
            )?)
        }
    };
    Ok(Record {
        outcome,
        frames: ran.record.frames,
        writes: ran.record.writes,
        handoffs: ran.record.handoffs,
        prompts: ran.prompts,
        secrets: ran.secrets,
    })
}

// ---------------------------------------------------------------------------
// The module functions

/// `run(component, *, fallback="prompt", interactive=None, output="stdout",
/// no_color=None, height=None, transient=False, tty_keys=False,
/// alternate_screen=False, mouse=False)`: run on the terminal until the
/// component finishes, and return its `Outcome`. Without a terminal
/// (`interactive=False`, a pipe, CI, `TERM=dumb`) the fallback decides:
/// ask line by line, return the default, or raise `NotInteractive`.
#[pyfunction]
#[pyo3(signature = (
    component, *, fallback="prompt", interactive=None, output="stdout", no_color=None,
    height=None, transient=false, tty_keys=false, alternate_screen=false, mouse=false
))]
#[allow(clippy::too_many_arguments)]
fn interact_run(
    py: Python<'_>,
    component: &Bound<'_, PyAny>,
    fallback: &str,
    interactive: Option<bool>,
    output: &str,
    no_color: Option<bool>,
    height: Option<usize>,
    transient: bool,
    tty_keys: bool,
    alternate_screen: bool,
    mouse: bool,
) -> PyResult<Py<Outcome>> {
    let mut paint = LoopOptions {
        height,
        transient,
        ..LoopOptions::default()
    };
    if let Some(no_color) = no_color {
        paint.no_color = no_color;
    }
    let options = RunOptions {
        policy: Policy {
            interactive,
            fallback: self::fallback(fallback)?,
            tty_keys,
        },
        session: SessionOptions {
            alternate_screen,
            mouse,
            bracketed_paste: true,
            output: self::output(output)?,
        },
        paint,
    };
    let record = components::drive(py, component, Mode::Terminal(options))?;
    record
        .outcome
        .ok_or_else(|| InteractError::new_err("the component did not finish"))
}

/// `ask(component, **options)`: `run`, then the answer: the value when
/// done, `Cancelled` when cancelled, `KeyboardInterrupt` on Ctrl+C.
#[pyfunction]
#[pyo3(signature = (component, **options))]
fn interact_ask(
    py: Python<'_>,
    component: &Bound<'_, PyAny>,
    options: Option<&Bound<'_, pyo3::types::PyDict>>,
) -> PyResult<Py<PyAny>> {
    let run = wrap_pyfunction!(interact_run, py)?;
    let outcome = run.call((component,), options)?;
    outcome.cast::<Outcome>()?.get().unwrap(py)
}

/// `headless(component, script=None, *, width=80, height=24)`: run with
/// scripted keys (a `str` of key names or a `Script`) at `width` × `height`,
/// and return the `Record`.
#[pyfunction]
#[pyo3(signature = (component, script=None, *, width=80, height=24))]
fn interact_headless(
    py: Python<'_>,
    component: &Bound<'_, PyAny>,
    script: Option<&Bound<'_, PyAny>>,
    width: u16,
    height: u16,
) -> PyResult<Record> {
    let mode = Mode::Headless {
        script: script_arg(script)?,
        columns: width.max(1),
        rows: height.max(1),
    };
    components::drive(py, component, mode)
}

/// `degrade(component, answers=(), *, fallback="prompt",
/// reason="requested")`: what `run` does without a terminal, with the
/// line prompts answered from `answers`.
#[pyfunction]
#[pyo3(signature = (component, answers=Vec::new(), *, fallback="prompt", reason="requested"))]
fn interact_degrade(
    py: Python<'_>,
    component: &Bound<'_, PyAny>,
    answers: Vec<String>,
    fallback: &str,
    reason: &str,
) -> PyResult<Record> {
    let mode = Mode::Degrade {
        fallback: self::fallback(fallback)?,
        reason: self::reason(reason)?,
        answers,
    };
    components::drive(py, component, mode)
}

// ---------------------------------------------------------------------------
// Fuzzy matching

/// A fuzzy match: its `score` and the matched character `positions`.
#[pyclass(name = "Match", module = "rs_rich.interact", frozen)]
pub(crate) struct Match {
    #[pyo3(get)]
    score: i64,
    #[pyo3(get)]
    positions: Vec<usize>,
}

#[pymethods]
impl Match {
    fn __repr__(&self) -> String {
        format!(
            "Match(score={}, positions={:?})",
            self.score, self.positions
        )
    }

    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        other.cast::<Match>().is_ok_and(|other| {
            other.get().score == self.score && other.get().positions == self.positions
        })
    }
}

impl From<rich_interact::fuzzy::Match> for Match {
    fn from(found: rich_interact::fuzzy::Match) -> Match {
        Match {
            score: found.score,
            positions: found.positions,
        }
    }
}

/// `fuzzy(pattern, candidate)`: the `Match` of `pattern` in `candidate`, or
/// `None`. Every space-separated term must match; case is ignored unless
/// the pattern has an upper-case letter.
#[pyfunction]
fn fuzzy_match(pattern: &str, candidate: &str) -> Option<Match> {
    rich_interact::fuzzy::fuzzy(pattern, candidate).map(Match::from)
}

/// `rank(pattern, candidates)`: the matching candidates as `(index, Match)`,
/// best first; ties go to the shorter, then the earlier candidate.
#[pyfunction]
fn fuzzy_rank<'py>(
    py: Python<'py>,
    pattern: &str,
    candidates: &Bound<'py, PyAny>,
) -> PyResult<Bound<'py, PyList>> {
    let candidates = crate::ext::common::strings(candidates)?;
    let ranked = rich_interact::fuzzy::rank(pattern, candidates.iter().map(String::as_str));
    let items = ranked
        .into_iter()
        .map(|(index, found)| Ok((index, Py::new(py, Match::from(found))?)))
        .collect::<PyResult<Vec<_>>>()?;
    PyList::new(py, items)
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    let py = m.py();
    m.add("InteractError", py.get_type::<InteractError>())?;
    m.add("Cancelled", py.get_type::<Cancelled>())?;
    m.add("NotInteractive", py.get_type::<NotInteractive>())?;
    // Short names that other areas already use in the flat native module
    // get an `Interact` prefix there; `rs_rich.interact` has the short ones.
    m.add("InteractScript", py.get_type::<Script>())?;
    m.add("InteractOutcome", py.get_type::<Outcome>())?;
    m.add("InteractRecord", py.get_type::<Record>())?;
    m.add("FuzzyMatch", py.get_type::<Match>())?;
    m.add_function(wrap_pyfunction!(interact_run, m)?)?;
    m.add_function(wrap_pyfunction!(interact_ask, m)?)?;
    m.add_function(wrap_pyfunction!(interact_headless, m)?)?;
    m.add_function(wrap_pyfunction!(interact_degrade, m)?)?;
    m.add_function(wrap_pyfunction!(fuzzy_match, m)?)?;
    m.add_function(wrap_pyfunction!(fuzzy_rank, m)?)?;
    components::register(m)
}
