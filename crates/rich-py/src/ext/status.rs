//! `rs_rich.ext.transfer` (download and upload progress with rate, ETA and
//! retries, and file wrappers that count bytes), `.countdown` (retry
//! backoff, retry and rate-limit status, countdown bars and waits) and
//! `.notify` (transient notifications).
//!
//! Times are seconds. A transfer's `now` defaults to its clock: a
//! `ManualClock` when given, else seconds since the transfer was created.

use std::io::{Read, Write};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use pyo3::exceptions::{PyIOError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyString};

use rich::protocol::Renderable;
use rich_ext::a11y::policy::SymbolSet;
use rich_ext::countdown::{
    remaining_label, Backoff as CoreBackoff, CountdownBar as CoreBar, CountdownWait,
    RateLimit as CoreRateLimit, RetryStatus as CoreRetry, WaitOutcome,
};
use rich_ext::notify::{
    Notification as CoreNotification, NotificationId, Notifications as CoreNotifications,
    ToastStyle,
};
use rich_ext::transfer::{
    is_cancelled, Direction, Transfer as CoreTransfer, TransferReader as CoreReader, TransferState,
    TransferWriter as CoreWriter, Transfers as CoreTransfers,
};
use rich_ext::workflow::Clock;

use super::common::{self, names};
use super::terminal::{self, symbol_set};
use super::workflow::{token_arg, ManualClock};
use crate::renderable::{self, AsRenderable};

names!(direction, direction_name, Direction, "direction", {
    "download" => Direction::Download,
    "upload" => Direction::Upload,
});

names!(transfer_state_name_of, transfer_state_name, TransferState, "transfer state", {
    "active" => TransferState::Active,
    "paused" => TransferState::Paused,
    "retrying" => TransferState::Retrying,
    "done" => TransferState::Done,
    "failed" => TransferState::Failed,
    "cancelled" => TransferState::Cancelled,
});

names!(toast_style, toast_style_name, ToastStyle, "toast style", {
    "line" => ToastStyle::Line,
    "panel" => ToastStyle::Panel,
});

/// Where a transfer's `now` comes from when not given.
#[derive(Clone)]
enum TimeSource {
    Since(Instant),
    Manual(rich_ext::workflow::ManualClock),
}

impl TimeSource {
    fn from_arg(clock: Option<PyRef<'_, ManualClock>>) -> TimeSource {
        match clock {
            Some(clock) => TimeSource::Manual(clock.inner.clone()),
            None => TimeSource::Since(Instant::now()),
        }
    }

    fn now(&self) -> Duration {
        match self {
            TimeSource::Since(origin) => origin.elapsed(),
            TimeSource::Manual(clock) => clock.now(),
        }
    }

    fn closure(&self) -> rich_ext::transfer::Clock {
        let source = self.clone();
        Arc::new(move || source.now())
    }
}

// ---------------------------------------------------------------------------
// Transfers

/// `Transfer(name, *, direction="download", total=None, max_attempts=None,
/// rate_window=5, bar_width=24, symbols="unicode", clock=None)`: one
/// download or upload: size, rate, ETA, retries, and a final state.
#[pyclass(name = "Transfer", module = "rs_rich.ext.transfer", frozen)]
pub(crate) struct Transfer {
    shared: Arc<Mutex<CoreTransfer>>,
    time: TimeSource,
}

impl Transfer {
    fn lock(&self) -> MutexGuard<'_, CoreTransfer> {
        self.shared.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn now(&self, now: Option<&Bound<'_, PyAny>>) -> PyResult<Duration> {
        Ok(match common::opt_seconds(now)? {
            Some(now) => now,
            None => self.time.now(),
        })
    }
}

impl AsRenderable for Transfer {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(self.lock().clone()))
    }
}

#[pymethods]
impl Transfer {
    #[new]
    #[pyo3(signature = (
        name, *, direction="download", total=None, max_attempts=None, rate_window=None,
        bar_width=24, symbols="unicode", clock=None
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        name: String,
        direction: &str,
        total: Option<u64>,
        max_attempts: Option<u32>,
        rate_window: Option<&Bound<'_, PyAny>>,
        bar_width: usize,
        symbols: &str,
        clock: Option<PyRef<'_, ManualClock>>,
    ) -> PyResult<Self> {
        let mut inner = CoreTransfer::new(name, self::direction(direction)?)
            .bar_width(bar_width)
            .symbols(symbol_set(symbols)?);
        if let Some(total) = total {
            inner = inner.total(total);
        }
        if let Some(max) = max_attempts {
            inner = inner.max_attempts(max);
        }
        if let Some(window) = common::opt_seconds(rate_window)? {
            inner = inner.rate_window(window);
        }
        Ok(Transfer {
            shared: Arc::new(Mutex::new(inner)),
            time: TimeSource::from_arg(clock),
        })
    }

    #[getter]
    fn name(&self) -> String {
        self.lock().name().to_string()
    }
    #[getter]
    fn direction(&self) -> &'static str {
        direction_name(self.lock().direction())
    }
    #[getter]
    fn get_total(&self) -> Option<u64> {
        self.lock().total_bytes()
    }
    #[setter]
    fn set_total(&self, total: Option<u64>) {
        self.lock().set_total(total);
    }
    #[getter]
    fn completed(&self) -> u64 {
        self.lock().completed()
    }
    #[getter]
    fn attempt(&self) -> u32 {
        self.lock().attempt()
    }
    /// `active`, `paused`, `retrying`, `done`, `failed` or `cancelled`.
    #[getter]
    fn state(&self) -> &'static str {
        transfer_state_name(self.lock().state())
    }
    #[getter]
    fn finished(&self) -> bool {
        self.lock().state().is_finished()
    }
    #[getter]
    fn error(&self) -> Option<String> {
        self.lock().error().map(str::to_string)
    }
    /// Completed over total (`None` without a total).
    #[getter]
    fn fraction(&self) -> Option<f64> {
        self.lock().fraction()
    }
    /// Bytes per second over the rate window, while active.
    #[getter]
    fn rate(&self) -> Option<f64> {
        self.lock().rate()
    }
    /// Seconds left at the current rate.
    #[getter]
    fn eta(&self) -> Option<f64> {
        self.lock().eta().map(|d| d.as_secs_f64())
    }

    /// Count `nbytes` more.
    #[pyo3(signature = (nbytes, *, now=None))]
    fn advance(&self, nbytes: u64, now: Option<&Bound<'_, PyAny>>) -> PyResult<()> {
        let now = self.now(now)?;
        self.lock().advance(nbytes, now);
        Ok(())
    }

    /// Set the completed byte count.
    #[pyo3(signature = (nbytes, *, now=None))]
    fn update(&self, nbytes: u64, now: Option<&Bound<'_, PyAny>>) -> PyResult<()> {
        let now = self.now(now)?;
        self.lock().set_completed(nbytes, now);
        Ok(())
    }

    #[pyo3(signature = (*, now=None))]
    fn start(&self, now: Option<&Bound<'_, PyAny>>) -> PyResult<()> {
        let now = self.now(now)?;
        self.lock().start(now);
        Ok(())
    }

    #[pyo3(signature = (*, now=None))]
    fn pause(&self, now: Option<&Bound<'_, PyAny>>) -> PyResult<()> {
        let now = self.now(now)?;
        self.lock().pause(now);
        Ok(())
    }

    /// Record a failed attempt; `False` when there are no attempts left
    /// (the transfer failed).
    fn retry(&self, reason: String) -> bool {
        self.lock().retry(reason)
    }

    #[pyo3(signature = (*, now=None))]
    fn finish(&self, now: Option<&Bound<'_, PyAny>>) -> PyResult<()> {
        let now = self.now(now)?;
        self.lock().finish(now);
        Ok(())
    }

    fn fail(&self, reason: String) {
        self.lock().fail(reason);
    }

    fn cancel(&self) {
        self.lock().cancel();
    }

    /// The fields to update a Rich `Progress` task with, as a dict
    /// (`total`, `completed`, `description`): use as
    /// `progress.update(task_id, **transfer.task_fields())`.
    fn task_fields<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, pyo3::types::PyDict>> {
        let update = self.lock().task_update();
        let fields = pyo3::types::PyDict::new(py);
        if let Some(total) = update.total {
            fields.set_item("total", total)?;
        }
        if let Some(completed) = update.completed {
            fields.set_item("completed", completed)?;
        }
        if let Some(description) = update.description {
            fields.set_item("description", description)?;
        }
        Ok(fields)
    }

    /// Wrap a binary file object so reads count toward this transfer (and
    /// finish it at end of file).
    #[pyo3(signature = (file, *, cancel=None))]
    fn wrap_reader(
        &self,
        file: Py<PyAny>,
        cancel: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<TransferReader> {
        TransferReader::build(file, self, cancel)
    }

    /// Wrap a binary file object so writes count toward this transfer.
    #[pyo3(signature = (file, *, cancel=None))]
    fn wrap_writer(
        &self,
        file: Py<PyAny>,
        cancel: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<TransferWriter> {
        TransferWriter::build(file, self, cancel)
    }
}

/// `Transfers(transfers=(), *, summary=False, symbols="unicode")`: several
/// transfers with aligned columns and an optional summary line. Holds the
/// `Transfer` objects, so it shows their latest state.
#[pyclass(name = "Transfers", module = "rs_rich.ext.transfer")]
pub(crate) struct Transfers {
    items: Vec<Py<Transfer>>,
    #[pyo3(get, set)]
    summary: bool,
    symbols: SymbolSet,
}

impl Transfers {
    fn build(&self, py: Python<'_>) -> CoreTransfers {
        let mut transfers = CoreTransfers::new()
            .summary(self.summary)
            .symbols(self.symbols);
        for item in &self.items {
            transfers.push(item.bind(py).get().lock().clone());
        }
        transfers
    }
}

impl AsRenderable for Transfers {
    fn to_renderable(&self, py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(self.build(py)))
    }
}

#[pymethods]
impl Transfers {
    #[new]
    #[pyo3(signature = (transfers=None, *, summary=false, symbols="unicode"))]
    fn new(transfers: Option<&Bound<'_, PyAny>>, summary: bool, symbols: &str) -> PyResult<Self> {
        let mut items = Vec::new();
        if let Some(transfers) = transfers {
            for item in transfers.try_iter()? {
                items.push(item?.extract::<Py<Transfer>>()?);
            }
        }
        Ok(Transfers {
            items,
            summary,
            symbols: symbol_set(symbols)?,
        })
    }

    /// Add a transfer; returns its index.
    fn push(&mut self, transfer: Py<Transfer>) -> usize {
        self.items.push(transfer);
        self.items.len() - 1
    }

    fn __len__(&self) -> usize {
        self.items.len()
    }

    fn __getitem__(&self, py: Python<'_>, index: usize) -> PyResult<Py<Transfer>> {
        self.items
            .get(index)
            .map(|t| t.clone_ref(py))
            .ok_or_else(|| pyo3::exceptions::PyIndexError::new_err("transfer index out of range"))
    }

    /// Whether every transfer is done, failed or cancelled.
    fn finished(&self, py: Python<'_>) -> bool {
        self.build(py).finished()
    }

    /// The combined rate of the active transfers.
    fn rate(&self, py: Python<'_>) -> Option<f64> {
        self.build(py).rate()
    }

    fn __traverse__(&self, visit: pyo3::PyVisit<'_>) -> Result<(), pyo3::PyTraverseError> {
        for item in &self.items {
            visit.call(item)?;
        }
        Ok(())
    }

    fn __clear__(&mut self) {
        self.items.clear();
    }
}

/// A Python binary file as a Rust reader or writer.
struct PyFile(Py<PyAny>);

fn io_error(error: PyErr) -> std::io::Error {
    std::io::Error::other(error.to_string())
}

impl Read for PyFile {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        Python::attach(|py| {
            let data = self
                .0
                .bind(py)
                .call_method1("read", (buf.len(),))
                .map_err(io_error)?;
            let bytes: &[u8] = if let Ok(b) = data.cast::<PyBytes>() {
                b.as_bytes()
            } else {
                return Err(std::io::Error::other("read() must return bytes"));
            };
            let n = bytes.len().min(buf.len());
            buf[..n].copy_from_slice(&bytes[..n]);
            Ok(n)
        })
    }
}

impl Write for PyFile {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        Python::attach(|py| {
            let written = self
                .0
                .bind(py)
                .call_method1("write", (PyBytes::new(py, buf),))
                .map_err(io_error)?;
            Ok(written.extract::<usize>().unwrap_or(buf.len()))
        })
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Python::attach(|py| {
            if let Some(flush) = self.0.bind(py).getattr_opt("flush").map_err(io_error)? {
                flush.call0().map_err(io_error)?;
            }
            Ok(())
        })
    }
}

fn transfer_io_error(error: std::io::Error) -> PyErr {
    if is_cancelled(&error) {
        return super::common::TransferCancelled::new_err("transfer cancelled");
    }
    PyIOError::new_err(error.to_string())
}

/// A file wrapper counting what is read: `Transfer.wrap_reader(file)`.
#[pyclass(name = "TransferReader", module = "rs_rich.ext.transfer")]
pub(crate) struct TransferReader {
    inner: CoreReader<PyFile>,
}

impl TransferReader {
    fn build(
        file: Py<PyAny>,
        transfer: &Transfer,
        cancel: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        let mut inner =
            CoreReader::new(PyFile(file), transfer.shared.clone()).clock(transfer.time.closure());
        if let Some(token) = token_arg(cancel)? {
            inner = inner.cancel(token);
        }
        Ok(TransferReader { inner })
    }
}

#[pymethods]
impl TransferReader {
    /// Read up to `size` bytes (all that is left with `-1`).
    #[pyo3(signature = (size=-1))]
    fn read<'py>(&mut self, py: Python<'py>, size: isize) -> PyResult<Bound<'py, PyBytes>> {
        let mut out = Vec::new();
        if size < 0 {
            self.inner
                .read_to_end(&mut out)
                .map_err(transfer_io_error)?;
        } else {
            out.resize(size as usize, 0);
            let n = self.inner.read(&mut out).map_err(transfer_io_error)?;
            out.truncate(n);
        }
        Ok(PyBytes::new(py, &out))
    }

    fn __enter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    #[pyo3(signature = (*_args))]
    fn __exit__(&mut self, _args: &Bound<'_, pyo3::types::PyTuple>) -> bool {
        false
    }
}

/// A file wrapper counting what is written: `Transfer.wrap_writer(file)`.
#[pyclass(name = "TransferWriter", module = "rs_rich.ext.transfer")]
pub(crate) struct TransferWriter {
    inner: CoreWriter<PyFile>,
}

impl TransferWriter {
    fn build(
        file: Py<PyAny>,
        transfer: &Transfer,
        cancel: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        let mut inner =
            CoreWriter::new(PyFile(file), transfer.shared.clone()).clock(transfer.time.closure());
        if let Some(token) = token_arg(cancel)? {
            inner = inner.cancel(token);
        }
        Ok(TransferWriter { inner })
    }
}

#[pymethods]
impl TransferWriter {
    /// Write `data` (`bytes`, or `str` as UTF-8); returns the bytes written.
    fn write(&mut self, data: &Bound<'_, PyAny>) -> PyResult<usize> {
        let bytes: Vec<u8> = if let Ok(text) = data.cast::<PyString>() {
            text.to_cow()?.as_bytes().to_vec()
        } else {
            data.extract::<Vec<u8>>()?
        };
        self.inner.write_all(&bytes).map_err(transfer_io_error)?;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> PyResult<()> {
        self.inner.flush().map_err(transfer_io_error)
    }

    fn __enter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    #[pyo3(signature = (*_args))]
    fn __exit__(&mut self, _args: &Bound<'_, pyo3::types::PyTuple>) -> PyResult<bool> {
        self.flush()?;
        Ok(false)
    }
}

// ---------------------------------------------------------------------------
// Countdowns

/// `remaining_label(seconds)`: `"4s"`, `"1m 05s"` (rounding up).
#[pyfunction]
#[pyo3(name = "remaining_label")]
fn py_remaining_label(seconds: &Bound<'_, PyAny>) -> PyResult<String> {
    Ok(remaining_label(common::seconds(seconds)?))
}

/// `Backoff(initial, *, factor=2.0, max=60, attempts=None, jitter=0.0,
/// seed=0)`: exponential retry delays, capped, with seeded jitter.
#[pyclass(name = "Backoff", module = "rs_rich.ext.countdown", frozen)]
pub(crate) struct Backoff {
    inner: CoreBackoff,
}

#[pymethods]
impl Backoff {
    #[new]
    #[pyo3(signature = (initial, *, factor=2.0, max=None, attempts=None, jitter=0.0, seed=0))]
    fn new(
        initial: &Bound<'_, PyAny>,
        factor: f64,
        max: Option<&Bound<'_, PyAny>>,
        attempts: Option<u32>,
        jitter: f64,
        seed: u64,
    ) -> PyResult<Self> {
        let mut inner = CoreBackoff::new(common::seconds(initial)?)
            .factor(factor)
            .jitter(jitter, seed);
        if let Some(max) = common::opt_seconds(max)? {
            inner = inner.max(max);
        }
        if let Some(attempts) = attempts {
            inner = inner.attempts(attempts);
        }
        Ok(Backoff { inner })
    }

    #[getter]
    fn max_attempts(&self) -> Option<u32> {
        self.inner.max_attempts()
    }

    /// The delay (seconds) before retry `attempt` (1-based), or `None` when
    /// out of attempts.
    fn delay(&self, attempt: u32) -> Option<f64> {
        self.inner.delay(attempt).map(|d| d.as_secs_f64())
    }

    /// Every delay, up to `limit` of them (all for a bounded backoff).
    #[pyo3(signature = (limit=None))]
    fn delays(&self, limit: Option<usize>) -> PyResult<Vec<f64>> {
        let limit = match (limit, self.inner.max_attempts()) {
            (Some(limit), _) => limit,
            (None, Some(max)) => max as usize,
            (None, None) => {
                return Err(PyValueError::new_err(
                    "an unbounded backoff needs delays(limit)",
                ))
            }
        };
        Ok(self
            .inner
            .delays()
            .take(limit)
            .map(|d| d.as_secs_f64())
            .collect())
    }

    /// The `RetryStatus` for retry `attempt`, or `None` for attempt 0.
    fn status(&self, attempt: u32, reason: String) -> Option<RetryStatus> {
        self.inner
            .status(attempt, reason)
            .map(|inner| RetryStatus { inner })
    }
}

/// `CountdownBar(total, remaining, *, width=20)`: a bar that empties as
/// time runs out.
#[pyclass(name = "CountdownBar", module = "rs_rich.ext.countdown", frozen)]
pub(crate) struct CountdownBar {
    inner: CoreBar,
}

impl AsRenderable for CountdownBar {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(self.inner.clone()))
    }
}

#[pymethods]
impl CountdownBar {
    #[new]
    #[pyo3(signature = (total, remaining, *, width=20))]
    fn new(total: &Bound<'_, PyAny>, remaining: &Bound<'_, PyAny>, width: usize) -> PyResult<Self> {
        Ok(CountdownBar {
            inner: CoreBar::new(common::seconds(total)?, common::seconds(remaining)?).width(width),
        })
    }
}

/// `RetryStatus(attempt, *, max_attempts=None, reason=None,
/// retrying_in=None, bar=None, bar_width=20, symbols="unicode")`: `retry
/// 2/5: timeout — retrying in 4s`, or giving up when `retrying_in` is None.
#[pyclass(name = "RetryStatus", module = "rs_rich.ext.countdown", frozen)]
pub(crate) struct RetryStatus {
    inner: CoreRetry,
}

impl AsRenderable for RetryStatus {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(self.inner.clone()))
    }
}

#[pymethods]
impl RetryStatus {
    #[new]
    #[pyo3(signature = (attempt, *, max_attempts=None, reason=None, retrying_in=None, bar=None, bar_width=20, symbols="unicode"))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        attempt: u32,
        max_attempts: Option<u32>,
        reason: Option<String>,
        retrying_in: Option<&Bound<'_, PyAny>>,
        bar: Option<&Bound<'_, PyAny>>,
        bar_width: usize,
        symbols: &str,
    ) -> PyResult<Self> {
        let mut inner = CoreRetry::new(attempt)
            .bar_width(bar_width)
            .symbols(symbol_set(symbols)?);
        if let Some(max) = max_attempts {
            inner = inner.max_attempts(max);
        }
        if let Some(reason) = reason {
            inner = inner.reason(reason);
        }
        if let Some(left) = common::opt_seconds(retrying_in)? {
            inner = inner.retrying_in(left);
        }
        if let Some(delay) = common::opt_seconds(bar)? {
            inner = inner.bar(delay);
        }
        Ok(RetryStatus { inner })
    }

    #[getter]
    fn giving_up(&self) -> bool {
        self.inner.giving_up()
    }

    /// `warning` while retrying, `error` when giving up.
    #[getter]
    fn status(&self) -> &'static str {
        terminal::status_name(self.inner.status())
    }

    /// The same status with `seconds` left.
    fn at(&self, seconds: &Bound<'_, PyAny>) -> PyResult<RetryStatus> {
        Ok(RetryStatus {
            inner: self.inner.at(common::seconds(seconds)?),
        })
    }
}

/// `RateLimit(resets_in, *, limit=None, remaining=None, scope=None,
/// bar=None, bar_width=20, symbols="unicode")`: `rate limited (api): 0/100
/// left — resets in 42s`.
#[pyclass(name = "RateLimit", module = "rs_rich.ext.countdown", frozen)]
pub(crate) struct RateLimit {
    inner: CoreRateLimit,
}

impl AsRenderable for RateLimit {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(self.inner.clone()))
    }
}

#[pymethods]
impl RateLimit {
    #[new]
    #[pyo3(signature = (resets_in, *, limit=None, remaining=None, scope=None, bar=None, bar_width=20, symbols="unicode"))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        resets_in: &Bound<'_, PyAny>,
        limit: Option<u64>,
        remaining: Option<u64>,
        scope: Option<String>,
        bar: Option<&Bound<'_, PyAny>>,
        bar_width: usize,
        symbols: &str,
    ) -> PyResult<Self> {
        let mut inner = CoreRateLimit::new(common::seconds(resets_in)?)
            .bar_width(bar_width)
            .symbols(symbol_set(symbols)?);
        if let Some(limit) = limit {
            inner = inner.limit(limit);
        }
        if let Some(remaining) = remaining {
            inner = inner.remaining(remaining);
        }
        if let Some(scope) = scope {
            inner = inner.scope(scope);
        }
        if let Some(window) = common::opt_seconds(bar)? {
            inner = inner.bar(window);
        }
        Ok(RateLimit { inner })
    }

    /// The same limit with `seconds` until it resets.
    fn at(&self, seconds: &Bound<'_, PyAny>) -> PyResult<RateLimit> {
        Ok(RateLimit {
            inner: self.inner.at(common::seconds(seconds)?),
        })
    }
}

/// `countdown_wait(total, *, tick=0.25, cancel=None, on_tick=None,
/// sleep=None)`: wait `total` seconds in `tick` steps, calling
/// `on_tick(seconds_left)` before each; `"elapsed"` or `"cancelled"`.
/// `sleep(seconds)` replaces the real sleep (for tests).
#[pyfunction]
#[pyo3(signature = (total, *, tick=None, cancel=None, on_tick=None, sleep=None))]
pub(crate) fn countdown_wait(
    py: Python<'_>,
    total: &Bound<'_, PyAny>,
    tick: Option<&Bound<'_, PyAny>>,
    cancel: Option<&Bound<'_, PyAny>>,
    on_tick: Option<Py<PyAny>>,
    sleep: Option<Py<PyAny>>,
) -> PyResult<&'static str> {
    let mut wait = CountdownWait::new(common::seconds(total)?);
    if let Some(tick) = common::opt_seconds(tick)? {
        wait = wait.tick(tick);
    }
    if let Some(token) = token_arg(cancel)? {
        wait = wait.cancel(token);
    }
    let error: std::cell::RefCell<Option<PyErr>> = std::cell::RefCell::new(None);
    let wait = wait.sleeper(|step: Duration| {
        if error.borrow().is_some() {
            return;
        }
        let result = match &sleep {
            Some(sleep) => sleep.bind(py).call1((step.as_secs_f64(),)).map(|_| ()),
            None => {
                py.detach(|| std::thread::sleep(step));
                py.check_signals()
            }
        };
        if let Err(e) = result {
            *error.borrow_mut() = Some(e);
        }
    });
    let outcome = wait.run(|left| {
        if error.borrow().is_some() {
            return;
        }
        if let Some(callback) = &on_tick {
            if let Err(e) = callback.bind(py).call1((left.as_secs_f64(),)) {
                *error.borrow_mut() = Some(e);
            }
        }
    });
    if let Some(error) = error.into_inner() {
        return Err(error);
    }
    Ok(match outcome {
        WaitOutcome::Elapsed => "elapsed",
        WaitOutcome::Cancelled => "cancelled",
    })
}

// ---------------------------------------------------------------------------
// Notifications

/// `Notification(message, *, status="info", title=None, ttl=None,
/// symbols="unicode", toast="line")`: a one-line toast (or a small panel).
#[pyclass(
    name = "Notification",
    module = "rs_rich.ext.notify",
    frozen,
    skip_from_py_object
)]
#[derive(Clone)]
pub(crate) struct Notification {
    inner: CoreNotification,
}

impl AsRenderable for Notification {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(self.inner.clone()))
    }
}

#[pymethods]
impl Notification {
    #[new]
    #[pyo3(signature = (message, *, status="info", title=None, ttl=None, symbols="unicode", toast="line"))]
    fn new(
        message: String,
        status: &str,
        title: Option<String>,
        ttl: Option<&Bound<'_, PyAny>>,
        symbols: &str,
        toast: &str,
    ) -> PyResult<Self> {
        let mut inner = CoreNotification::new(terminal::status(status)?, message)
            .symbols(symbol_set(symbols)?)
            .toast_style(toast_style(toast)?);
        if let Some(title) = title {
            inner = inner.title(title);
        }
        if let Some(ttl) = common::opt_seconds(ttl)? {
            inner = inner.ttl(ttl);
        }
        Ok(Notification { inner })
    }

    #[getter]
    fn status(&self) -> &'static str {
        terminal::status_name(self.inner.status())
    }
    #[getter]
    fn title(&self) -> Option<String> {
        self.inner.get_title().map(str::to_string)
    }
    #[getter]
    fn message(&self) -> String {
        self.inner.message().to_string()
    }
    #[getter]
    fn ttl(&self) -> Option<f64> {
        self.inner.get_ttl().map(|d| d.as_secs_f64())
    }
}

/// `Notifications(*, default_ttl=5, max_visible=3, transient=True,
/// symbols=None, toast=None)`: a stack of toasts that expire. Not
/// transient (a plain stream), they are logged to print once instead.
#[pyclass(name = "Notifications", module = "rs_rich.ext.notify")]
pub(crate) struct Notifications {
    pub(crate) inner: CoreNotifications,
    ids: Vec<NotificationId>,
    pub(crate) region: Option<rich_ext::live::RegionId>,
}

impl AsRenderable for Notifications {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(self.inner.clone()))
    }
}

#[pymethods]
impl Notifications {
    #[new]
    #[pyo3(signature = (*, default_ttl=Some(5.0), max_visible=3, transient=true, symbols=None, toast=None))]
    fn new(
        default_ttl: Option<f64>,
        max_visible: usize,
        transient: bool,
        symbols: Option<&str>,
        toast: Option<&str>,
    ) -> PyResult<Self> {
        let ttl = default_ttl
            .map(|secs| {
                Duration::try_from_secs_f64(secs).map_err(|e| PyValueError::new_err(e.to_string()))
            })
            .transpose()?;
        let mut inner = CoreNotifications::new()
            .default_ttl(ttl)
            .max_visible(max_visible)
            .transient(transient);
        if let Some(symbols) = symbols {
            inner = inner.symbols(symbol_set(symbols)?);
        }
        if let Some(toast) = toast {
            inner = inner.toast_style(toast_style(toast)?);
        }
        Ok(Notifications {
            inner,
            ids: Vec::new(),
            region: None,
        })
    }

    /// Show a notification from `now` (seconds); returns its id.
    fn push(
        &mut self,
        notification: PyRef<'_, Notification>,
        now: &Bound<'_, PyAny>,
    ) -> PyResult<usize> {
        let id = self
            .inner
            .push(notification.inner.clone(), common::seconds(now)?);
        self.ids.push(id);
        Ok(self.ids.len() - 1)
    }

    /// Remove a notification early; `False` if it was already gone.
    fn dismiss(&mut self, id: usize) -> bool {
        match self.ids.get(id) {
            Some(id) => self.inner.dismiss(*id),
            None => false,
        }
    }

    /// Drop the notifications expired at `now`; returns how many.
    fn expire(&mut self, now: &Bound<'_, PyAny>) -> PyResult<usize> {
        Ok(self.inner.expire(common::seconds(now)?))
    }

    /// When the next notification expires (seconds), if any.
    fn next_expiry(&self) -> Option<f64> {
        self.inner.next_expiry().map(|d| d.as_secs_f64())
    }

    /// The visible notifications' messages.
    fn messages(&self) -> Vec<String> {
        self.inner.iter().map(|n| n.message().to_string()).collect()
    }

    /// Take the notifications logged while not transient.
    fn take_log(&mut self) -> Vec<Notification> {
        self.inner
            .take_log()
            .into_iter()
            .map(|inner| Notification { inner })
            .collect()
    }

    fn __len__(&self) -> usize {
        self.inner.len()
    }
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    renderable::add_renderable_class::<Transfer>(m)?;
    renderable::add_renderable_class::<Transfers>(m)?;
    m.add_class::<TransferReader>()?;
    m.add_class::<TransferWriter>()?;
    m.add_function(pyo3::wrap_pyfunction!(py_remaining_label, m)?)?;
    m.add_class::<Backoff>()?;
    renderable::add_renderable_class::<CountdownBar>(m)?;
    renderable::add_renderable_class::<RetryStatus>(m)?;
    renderable::add_renderable_class::<RateLimit>(m)?;
    m.add_function(pyo3::wrap_pyfunction!(countdown_wait, m)?)?;
    renderable::add_renderable_class::<Notification>(m)?;
    renderable::add_renderable_class::<Notifications>(m)?;
    m.add("TRANSFER_STYLES", rich_ext::transfer::STYLES.to_vec())?;
    Ok(())
}
