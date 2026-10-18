//! `Reorder` (#720) and `DatePicker`, `TimePicker` and `DateTimePicker`
//! (#719) for `rs_rich.interact`: configurations built into a fresh
//! `rich_interact` component per run, like the other component classes.
//! Dates and times are Python's `datetime.date`, and naive `datetime.time`
//! and `datetime.datetime`: one with a time zone is refused, and
//! microseconds are dropped (the pickers work to the second). The pickers
//! keep to the years 1 to 9999, `datetime.date.min` to `max`.

use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList, PyString};
use pyo3::{PyTraverseError, PyVisit};

use rich_interact::datetime::{Date, DateTime, Time};
use rich_interact::{
    DatePicker as CoreDate, DateTimePicker as CoreDateTime, Item as CoreItem, Reorder as CoreOrder,
    TimePicker as CoreTime,
};

use super::components::{core_items, item_values, items, Item};
use super::compose::{self, Node};
use super::{execute, record, Build, Mode, Record};
use crate::renderable;

// ---------------------------------------------------------------------------
// Python's dates and times

fn part(value: &Bound<'_, PyAny>, name: &str, what: &str) -> PyResult<i64> {
    value
        .getattr(name)
        .and_then(|part| part.extract())
        .map_err(|_| PyTypeError::new_err(format!("expected {what}")))
}

/// A `datetime.date` (or a `datetime.datetime`'s day).
pub(crate) fn date_from(value: &Bound<'_, PyAny>) -> PyResult<Date> {
    let what = "a datetime.date";
    let (year, month, day) = (
        part(value, "year", what)?,
        part(value, "month", what)?,
        part(value, "day", what)?,
    );
    let date = match (
        i32::try_from(year),
        u32::try_from(month),
        u32::try_from(day),
    ) {
        (Ok(year), Ok(month), Ok(day)) => Date::new(year, month, day),
        _ => return Err(PyValueError::new_err("the date is out of range")),
    };
    if !date.is_valid() {
        return Err(PyValueError::new_err(
            "the day is out of range for its month",
        ));
    }
    Ok(date)
}

/// Refuse `value` when it has a time zone: the pickers take naive values,
/// and keeping the clock while dropping the offset would change the moment.
fn naive(value: &Bound<'_, PyAny>, what: &str) -> PyResult<()> {
    if value.hasattr("utcoffset")? && !value.call_method0("utcoffset")?.is_none() {
        return Err(PyTypeError::new_err(format!(
            "{what} with a time zone: the pickers take naive values"
        )));
    }
    Ok(())
}

/// A naive `datetime.time` (or a `datetime.datetime`'s time), to the
/// second: microseconds are dropped.
pub(crate) fn time_from(value: &Bound<'_, PyAny>) -> PyResult<Time> {
    let what = "a datetime.time";
    naive(value, what)?;
    let (hour, minute, second) = (
        part(value, "hour", what)?,
        part(value, "minute", what)?,
        part(value, "second", what)?,
    );
    let time = match (
        u32::try_from(hour),
        u32::try_from(minute),
        u32::try_from(second),
    ) {
        (Ok(hour), Ok(minute), Ok(second)) => Time::new(hour, minute, second),
        _ => return Err(PyValueError::new_err("the time is out of range")),
    };
    if !time.is_valid() {
        return Err(PyValueError::new_err("the time is out of range"));
    }
    Ok(time)
}

/// A naive `datetime.datetime`, to the second.
pub(crate) fn datetime_from(value: &Bound<'_, PyAny>) -> PyResult<DateTime> {
    if !value.hasattr("hour")? || !value.hasattr("year")? {
        return Err(PyTypeError::new_err("expected a datetime.datetime"));
    }
    naive(value, "a datetime.datetime")?;
    Ok(DateTime::new(date_from(value)?, time_from(value)?))
}

fn datetime_module(py: Python<'_>) -> PyResult<Bound<'_, PyModule>> {
    py.import("datetime")
}

pub(crate) fn date_to(py: Python<'_>, date: Date) -> PyResult<Py<PyAny>> {
    Ok(datetime_module(py)?
        .getattr("date")?
        .call1((date.year, date.month, date.day))?
        .unbind())
}

pub(crate) fn time_to(py: Python<'_>, time: Time) -> PyResult<Py<PyAny>> {
    Ok(datetime_module(py)?
        .getattr("time")?
        .call1((time.hour, time.minute, time.second))?
        .unbind())
}

pub(crate) fn datetime_to(py: Python<'_>, at: DateTime) -> PyResult<Py<PyAny>> {
    let (date, time) = (at.date, at.time);
    Ok(datetime_module(py)?
        .getattr("datetime")?
        .call1((
            date.year,
            date.month,
            date.day,
            time.hour,
            time.minute,
            time.second,
        ))?
        .unbind())
}

/// An optional argument converted by `from`, `None` passing through.
fn optional<T>(
    value: Option<&Bound<'_, PyAny>>,
    from: fn(&Bound<'_, PyAny>) -> PyResult<T>,
    name: &str,
) -> PyResult<Option<T>> {
    value
        .filter(|value| !value.is_none())
        .map(|value| from(value).map_err(|error| PyTypeError::new_err(format!("{name}: {error}"))))
        .transpose()
}

/// A validator's verdict on `value`: `None` or `True` passes, `False`
/// fails, a `str` is the message, and a raised `ValueError` is its
/// message. Any other exception is raised from the run.
fn verdict(
    validator: &Py<PyAny>,
    value: impl FnOnce(Python<'_>) -> PyResult<Py<PyAny>>,
) -> Result<(), String> {
    Python::attach(|py| {
        let called = value(py).and_then(|value| validator.bind(py).call1((value,)));
        match called {
            Ok(verdict) => {
                if verdict.is_none() {
                    return Ok(());
                }
                if let Ok(message) = verdict.cast::<PyString>() {
                    return Err(message.to_string());
                }
                match verdict.is_truthy() {
                    Ok(true) => Ok(()),
                    Ok(false) => Err("invalid value".to_string()),
                    Err(error) => {
                        let message = error.to_string();
                        renderable::report_error(py, error);
                        Err(message)
                    }
                }
            }
            Err(error) if error.is_instance_of::<PyValueError>(py) => Err(error
                .value(py)
                .str()
                .map_or_else(|_| "invalid value".to_string(), |s| s.to_string())),
            Err(error) => {
                let message = error.to_string();
                renderable::report_error(py, error);
                Err(message)
            }
        }
    })
}

// ---------------------------------------------------------------------------
// Reorder

/// `Reorder(prompt, items, *, height=10, mouse=False)`: put the items in
/// order. Alt+↑/↓, Ctrl+↑/↓ or `K`/`J` move the focused item; Space grabs
/// it for the arrows to carry; with `mouse`, a drag carries a row. Enter
/// returns the items' values in the new order. Without a terminal it reads
/// the new order as numbers (`3 1 2`; the rest keep theirs).
#[pyclass(name = "Reorder", module = "rs_rich.interact", frozen)]
pub(crate) struct Reorder {
    #[pyo3(get)]
    prompt: String,
    items: Vec<Py<Item>>,
    #[pyo3(get)]
    height: usize,
    #[pyo3(get)]
    mouse: bool,
}

struct ReorderBuild {
    prompt: String,
    items: Vec<CoreItem<usize>>,
    height: usize,
    mouse: bool,
}

impl Build for ReorderBuild {
    type C = CoreOrder<usize>;

    fn build(self) -> CoreOrder<usize> {
        CoreOrder::new(self.prompt, self.items)
            .height(self.height)
            .with_mouse(self.mouse)
    }
}

#[pymethods]
impl Reorder {
    fn __traverse__(&self, visit: PyVisit<'_>) -> Result<(), PyTraverseError> {
        for item in &self.items {
            visit.call(item)?;
        }
        Ok(())
    }

    #[new]
    #[pyo3(signature = (prompt, items, *, height=10, mouse=false))]
    fn new(
        py: Python<'_>,
        prompt: String,
        items: &Bound<'_, PyAny>,
        height: usize,
        mouse: bool,
    ) -> PyResult<Self> {
        if height == 0 {
            return Err(PyValueError::new_err("height must be at least 1"));
        }
        Ok(Reorder {
            prompt,
            items: self::items(py, items)?,
            height,
            mouse,
        })
    }

    #[getter]
    fn items(&self, py: Python<'_>) -> Vec<Py<Item>> {
        self.items.iter().map(|item| item.clone_ref(py)).collect()
    }

    /// The items' values, in their first order.
    #[getter]
    fn values<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyList>> {
        item_values(py, &self.items)
    }

    #[pyo3(signature = (**options))]
    fn ask(slf: &Bound<'_, Self>, options: Option<&Bound<'_, PyDict>>) -> PyResult<Py<PyAny>> {
        super::interact_ask(slf.py(), slf.as_any(), options)
    }

    #[pyo3(signature = (script=None, *, width=80, height=24))]
    fn headless(
        slf: &Bound<'_, Self>,
        script: Option<&Bound<'_, PyAny>>,
        width: u16,
        height: u16,
    ) -> PyResult<Record> {
        super::interact_headless(slf.py(), slf.as_any(), script, width, height)
    }
}

impl Reorder {
    fn prepare(&self, py: Python<'_>) -> PyResult<ReorderBuild> {
        Ok(ReorderBuild {
            prompt: self.prompt.clone(),
            items: core_items(py, &self.items)?,
            height: self.height,
            mouse: self.mouse,
        })
    }

    /// The values at `order`, as a Python list.
    fn answer(&self, py: Python<'_>, order: Vec<usize>) -> PyResult<Py<PyAny>> {
        let values = order
            .into_iter()
            .map(|index| self.items[index].get().value.clone_ref(py));
        Ok(PyList::new(py, values)?.into_any().unbind())
    }
}

// ---------------------------------------------------------------------------
// The date and time pickers

/// What every picker holds; the kind decides which parts it uses.
struct DateConfig {
    prompt: String,
    value: Option<DateTime>,
    default: Option<DateTime>,
    min: Option<DateTime>,
    max: Option<DateTime>,
    today: Option<Date>,
    seconds: bool,
    twelve_hour: bool,
    mouse: bool,
    validator: Option<Py<PyAny>>,
}

impl DateConfig {
    /// Check the range, as the CLI does: `min` not after `max`, and
    /// `default` within them as the picker compares it (to the minute when
    /// a picker with a time has no seconds). The picker would refuse a
    /// default outside them, so it is no default.
    fn check(&self, to_the_minute: bool) -> PyResult<()> {
        if let (Some(min), Some(max)) = (self.min, self.max) {
            if min > max {
                return Err(PyValueError::new_err("min is after max"));
            }
        }
        if let Some(mut default) = self.default {
            if to_the_minute {
                default.time.second = 0;
            }
            let early = self.min.is_some_and(|min| default < min);
            let late = self.max.is_some_and(|max| default > max);
            if early || late {
                return Err(PyValueError::new_err("default is outside min and max"));
            }
        }
        Ok(())
    }

    fn clone_ref(&self, py: Python<'_>) -> DateConfig {
        DateConfig {
            prompt: self.prompt.clone(),
            value: self.value,
            default: self.default,
            min: self.min,
            max: self.max,
            today: self.today,
            seconds: self.seconds,
            twelve_hour: self.twelve_hour,
            mouse: self.mouse,
            validator: self.validator.as_ref().map(|v| v.clone_ref(py)),
        }
    }
}

struct DateBuild(DateConfig);

impl Build for DateBuild {
    type C = CoreDate;

    fn build(self) -> CoreDate {
        let config = self.0;
        let mut picker = CoreDate::new(config.prompt).with_mouse(config.mouse);
        if let Some(today) = config.today {
            picker = picker.today(today);
        }
        if let Some(value) = config.value {
            picker = picker.value(value.date);
        }
        if let Some(default) = config.default {
            picker = picker.default(default.date);
        }
        if let Some(min) = config.min {
            picker = picker.min(min.date);
        }
        if let Some(max) = config.max {
            picker = picker.max(max.date);
        }
        if let Some(validator) = config.validator {
            picker = picker.validate(move |date| verdict(&validator, |py| date_to(py, date)));
        }
        picker
    }
}

struct TimeBuild(DateConfig);

impl Build for TimeBuild {
    type C = CoreTime;

    fn build(self) -> CoreTime {
        let config = self.0;
        let mut picker = CoreTime::new(config.prompt)
            .seconds(config.seconds)
            .twelve_hour(config.twelve_hour)
            .with_mouse(config.mouse);
        if let Some(value) = config.value {
            picker = picker.value(value.time);
        }
        if let Some(default) = config.default {
            picker = picker.default(default.time);
        }
        if let Some(min) = config.min {
            picker = picker.min(min.time);
        }
        if let Some(max) = config.max {
            picker = picker.max(max.time);
        }
        if let Some(validator) = config.validator {
            picker = picker.validate(move |time| verdict(&validator, |py| time_to(py, time)));
        }
        picker
    }
}

struct DateTimeBuild(DateConfig);

impl Build for DateTimeBuild {
    type C = CoreDateTime;

    fn build(self) -> CoreDateTime {
        let config = self.0;
        let mut picker = CoreDateTime::new(config.prompt)
            .seconds(config.seconds)
            .twelve_hour(config.twelve_hour)
            .with_mouse(config.mouse);
        if let Some(today) = config.today {
            picker = picker.today(today);
        }
        if let Some(value) = config.value {
            picker = picker.value(value);
        }
        if let Some(default) = config.default {
            picker = picker.default(default);
        }
        if let Some(min) = config.min {
            picker = picker.min(min);
        }
        if let Some(max) = config.max {
            picker = picker.max(max);
        }
        if let Some(validator) = config.validator {
            picker = picker.validate(move |at| verdict(&validator, |py| datetime_to(py, at)));
        }
        picker
    }
}

/// A day at midnight: how a date-only value is held.
fn day(date: Date) -> DateTime {
    DateTime::from(date)
}

/// A time on an unused day: how a time-only value is held.
fn clock(time: Time) -> DateTime {
    DateTime::new(Date::new(2000, 1, 1), time)
}

/// The arguments every picker takes, converted by `from`; each picker
/// [checks](DateConfig::check) the range once it has them all.
#[allow(clippy::too_many_arguments)]
fn config(
    prompt: String,
    value: Option<&Bound<'_, PyAny>>,
    default: Option<&Bound<'_, PyAny>>,
    min: Option<&Bound<'_, PyAny>>,
    max: Option<&Bound<'_, PyAny>>,
    from: fn(&Bound<'_, PyAny>) -> PyResult<DateTime>,
    validate: Option<&Bound<'_, PyAny>>,
    mouse: bool,
) -> PyResult<DateConfig> {
    let validator = validate
        .filter(|v| !v.is_none())
        .map(|v| {
            if v.is_callable() {
                Ok(v.clone().unbind())
            } else {
                Err(PyTypeError::new_err("validate must be callable"))
            }
        })
        .transpose()?;
    Ok(DateConfig {
        prompt,
        value: optional(value, from, "value")?,
        default: optional(default, from, "default")?,
        min: optional(min, from, "min")?,
        max: optional(max, from, "max")?,
        today: None,
        seconds: false,
        twelve_hour: false,
        mouse,
        validator,
    })
}

fn date_arg(value: &Bound<'_, PyAny>) -> PyResult<DateTime> {
    date_from(value).map(day)
}

fn time_arg(value: &Bound<'_, PyAny>) -> PyResult<DateTime> {
    time_from(value).map(clock)
}

fn date_or_none(
    py: Python<'_>,
    at: Option<DateTime>,
    to: fn(Python<'_>, DateTime) -> PyResult<Py<PyAny>>,
) -> PyResult<Py<PyAny>> {
    match at {
        Some(at) => to(py, at),
        None => Ok(py.None()),
    }
}

fn as_date(py: Python<'_>, at: DateTime) -> PyResult<Py<PyAny>> {
    date_to(py, at.date)
}

fn as_time(py: Python<'_>, at: DateTime) -> PyResult<Py<PyAny>> {
    time_to(py, at.time)
}

/// A picker class's methods: its own, then the getters and drivers every
/// picker has (`#[pymethods]` takes no macro inside it, so the macro makes
/// the whole block).
macro_rules! picker_methods {
    ($picker:ty, $to:expr, { $($own:tt)* }) => {
        #[pymethods]
        impl $picker {
        $($own)*
        fn __traverse__(&self, visit: PyVisit<'_>) -> Result<(), PyTraverseError> {
            if let Some(validator) = &self.0.validator {
                visit.call(validator)?;
            }
            Ok(())
        }

        #[getter]
        fn prompt(&self) -> String {
            self.0.prompt.clone()
        }

        #[getter]
        fn value(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
            date_or_none(py, self.0.value, $to)
        }

        #[getter]
        fn default(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
            date_or_none(py, self.0.default, $to)
        }

        #[getter]
        fn min(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
            date_or_none(py, self.0.min, $to)
        }

        #[getter]
        fn max(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
            date_or_none(py, self.0.max, $to)
        }

        #[getter]
        fn mouse(&self) -> bool {
            self.0.mouse
        }

        #[pyo3(signature = (**options))]
        fn ask(slf: &Bound<'_, Self>, options: Option<&Bound<'_, PyDict>>) -> PyResult<Py<PyAny>> {
            super::interact_ask(slf.py(), slf.as_any(), options)
        }

        #[pyo3(signature = (script=None, *, width=80, height=24))]
        fn headless(
            slf: &Bound<'_, Self>,
            script: Option<&Bound<'_, PyAny>>,
            width: u16,
            height: u16,
        ) -> PyResult<Record> {
            super::interact_headless(slf.py(), slf.as_any(), script, width, height)
        }
        }
    };
}

/// `DatePicker(prompt="Date", *, value=None, default=None, min=None,
/// max=None, today=None, validate=None, mouse=False)`: a day from a month
/// grid (`datetime.date`s). The arrows move a day and a week, PgUp/PgDn a
/// month, `t` goes to today; days outside `min` and `max` are dimmed and
/// cannot be reached. `validate(date)` returns `None`/`True` to accept, or
/// a message to refuse. `value` is where it starts, `default` or not; a
/// `default` outside `min` and `max` is a `ValueError`. Without a terminal
/// it reads `YYYY-MM-DD`.
#[pyclass(name = "DatePicker", module = "rs_rich.interact", frozen)]
pub(crate) struct DatePicker(DateConfig);

picker_methods!(DatePicker, as_date, {
    #[new]
    #[pyo3(signature = (
        prompt="Date".to_string(), *, value=None, default=None, min=None, max=None, today=None,
        validate=None, mouse=false
    ), text_signature = "(prompt='Date', *, value=None, default=None, min=None, max=None, \
        today=None, validate=None, mouse=False)")]
    #[allow(clippy::too_many_arguments)]
    fn new(
        prompt: String,
        value: Option<&Bound<'_, PyAny>>,
        default: Option<&Bound<'_, PyAny>>,
        min: Option<&Bound<'_, PyAny>>,
        max: Option<&Bound<'_, PyAny>>,
        today: Option<&Bound<'_, PyAny>>,
        validate: Option<&Bound<'_, PyAny>>,
        mouse: bool,
    ) -> PyResult<Self> {
        let mut config = config(prompt, value, default, min, max, date_arg, validate, mouse)?;
        config.today = optional(today, date_from, "today")?;
        config.check(false)?;
        Ok(DatePicker(config))
    }

    #[getter]
    fn today(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        date_or_none(py, self.0.today.map(day), as_date)
    }
});

/// `TimePicker(prompt="Time", *, value=None, default=None, min=None,
/// max=None, seconds=False, twelve_hour=False, validate=None,
/// mouse=False)`: a naive time of day (`datetime.time`s), field by field:
/// ←/→ move between the fields, ↑/↓ change one, digits type it. A time
/// outside `min` and `max` is refused on Enter with a message. Without
/// `seconds` it works to the minute. Without a terminal it reads `HH:MM`
/// (or `2:30 pm`).
#[pyclass(name = "TimePicker", module = "rs_rich.interact", frozen)]
pub(crate) struct TimePicker(DateConfig);

picker_methods!(TimePicker, as_time, {
    #[new]
    #[pyo3(signature = (
        prompt="Time".to_string(), *, value=None, default=None, min=None, max=None,
        seconds=false, twelve_hour=false, validate=None, mouse=false
    ), text_signature = "(prompt='Time', *, value=None, default=None, min=None, max=None, \
        seconds=False, twelve_hour=False, validate=None, mouse=False)")]
    #[allow(clippy::too_many_arguments)]
    fn new(
        prompt: String,
        value: Option<&Bound<'_, PyAny>>,
        default: Option<&Bound<'_, PyAny>>,
        min: Option<&Bound<'_, PyAny>>,
        max: Option<&Bound<'_, PyAny>>,
        seconds: bool,
        twelve_hour: bool,
        validate: Option<&Bound<'_, PyAny>>,
        mouse: bool,
    ) -> PyResult<Self> {
        let mut config = config(prompt, value, default, min, max, time_arg, validate, mouse)?;
        config.seconds = seconds;
        config.twelve_hour = twelve_hour;
        config.check(!seconds)?;
        Ok(TimePicker(config))
    }

    #[getter]
    fn seconds(&self) -> bool {
        self.0.seconds
    }

    #[getter]
    fn twelve_hour(&self) -> bool {
        self.0.twelve_hour
    }
});

/// `DateTimePicker(prompt="Date and time", *, value=None, default=None,
/// min=None, max=None, today=None, seconds=False, twelve_hour=False,
/// validate=None, mouse=False)`: a day and a time (naive
/// `datetime.datetime`s); Tab moves between the grid and the time. Days
/// before `min`'s or after `max`'s are dimmed; on those days a time past
/// the limit is refused on Enter. Without `seconds` it works to the
/// minute. Without a terminal it reads `YYYY-MM-DD HH:MM`.
#[pyclass(name = "DateTimePicker", module = "rs_rich.interact", frozen)]
pub(crate) struct DateTimePicker(DateConfig);

picker_methods!(DateTimePicker, datetime_to, {
    #[new]
    #[pyo3(signature = (
        prompt="Date and time".to_string(), *, value=None, default=None, min=None, max=None,
        today=None, seconds=false, twelve_hour=false, validate=None, mouse=false
    ), text_signature = "(prompt='Date and time', *, value=None, default=None, min=None, \
        max=None, today=None, seconds=False, twelve_hour=False, validate=None, mouse=False)")]
    #[allow(clippy::too_many_arguments)]
    fn new(
        prompt: String,
        value: Option<&Bound<'_, PyAny>>,
        default: Option<&Bound<'_, PyAny>>,
        min: Option<&Bound<'_, PyAny>>,
        max: Option<&Bound<'_, PyAny>>,
        today: Option<&Bound<'_, PyAny>>,
        seconds: bool,
        twelve_hour: bool,
        validate: Option<&Bound<'_, PyAny>>,
        mouse: bool,
    ) -> PyResult<Self> {
        let mut config = config(
            prompt,
            value,
            default,
            min,
            max,
            datetime_from,
            validate,
            mouse,
        )?;
        config.today = optional(today, date_from, "today")?;
        config.seconds = seconds;
        config.twelve_hour = twelve_hour;
        config.check(!seconds)?;
        Ok(DateTimePicker(config))
    }

    #[getter]
    fn today(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        date_or_none(py, self.0.today.map(day), as_date)
    }

    #[getter]
    fn seconds(&self) -> bool {
        self.0.seconds
    }

    #[getter]
    fn twelve_hour(&self) -> bool {
        self.0.twelve_hour
    }
});

// ---------------------------------------------------------------------------
// Dispatch

/// Run `component` if it is one of these classes; otherwise give `mode`
/// back for the next kind to try.
pub(crate) fn drive(
    py: Python<'_>,
    component: &Bound<'_, PyAny>,
    mode: Mode,
) -> PyResult<Result<Record, Mode>> {
    if let Ok(reorder) = component.cast::<Reorder>() {
        let reorder = reorder.get();
        let ran = execute(py, reorder.prepare(py)?, mode)?;
        return record(py, ran, |order| reorder.answer(py, order)).map(Ok);
    }
    if let Ok(picker) = component.cast::<DatePicker>() {
        let ran = execute(py, DateBuild(picker.get().0.clone_ref(py)), mode)?;
        return record(py, ran, |date| date_to(py, date)).map(Ok);
    }
    if let Ok(picker) = component.cast::<TimePicker>() {
        let ran = execute(py, TimeBuild(picker.get().0.clone_ref(py)), mode)?;
        return record(py, ran, |time| time_to(py, time)).map(Ok);
    }
    if let Ok(picker) = component.cast::<DateTimePicker>() {
        let ran = execute(py, DateTimeBuild(picker.get().0.clone_ref(py)), mode)?;
        return record(py, ran, |at| datetime_to(py, at)).map(Ok);
    }
    Ok(Err(mode))
}

/// The node for one of these, as a child of a container (see
/// `compose::leaf`), if `component` is one.
pub(super) fn leaf(component: &Bound<'_, PyAny>) -> PyResult<Option<Node>> {
    let py = component.py();
    if let Ok(reorder) = component.cast::<Reorder>() {
        let items: Vec<Py<PyAny>> = reorder
            .get()
            .items
            .iter()
            .map(|item| item.get().value.clone_ref(py))
            .collect();
        let build = reorder.get().prepare(py)?;
        return Ok(Some(compose::leaf(build, move |py, order: Vec<usize>| {
            let values = order.into_iter().map(|index| items[index].clone_ref(py));
            Ok(PyList::new(py, values)?.into_any().unbind())
        })));
    }
    if let Ok(picker) = component.cast::<DatePicker>() {
        let build = DateBuild(picker.get().0.clone_ref(py));
        return Ok(Some(compose::leaf(build, date_to)));
    }
    if let Ok(picker) = component.cast::<TimePicker>() {
        let build = TimeBuild(picker.get().0.clone_ref(py));
        return Ok(Some(compose::leaf(build, time_to)));
    }
    if let Ok(picker) = component.cast::<DateTimePicker>() {
        let build = DateTimeBuild(picker.get().0.clone_ref(py));
        return Ok(Some(compose::leaf(build, datetime_to)));
    }
    Ok(None)
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    let py = m.py();
    for (name, class) in [
        ("InteractReorder", py.get_type::<Reorder>()),
        ("InteractDatePicker", py.get_type::<DatePicker>()),
        ("InteractTimePicker", py.get_type::<TimePicker>()),
        ("InteractDateTimePicker", py.get_type::<DateTimePicker>()),
    ] {
        m.add(name, class)?;
    }
    Ok(())
}
