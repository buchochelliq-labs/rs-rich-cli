//! Conversions and plumbing shared by the `rs_rich.ext` submodules: names
//! for Rust enums, seconds for durations, character offsets for byte
//! offsets, Python values for `rich-ext` values, and a render scope for
//! tools that render with a console of their own.

use std::time::Duration;

use pyo3::create_exception;
use pyo3::exceptions::{PyException, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyDict, PyFloat, PyInt, PyList, PyString, PyTuple};

use rich::console::{Console as CoreConsole, ConsoleOptions as CoreOptions};
use rich::measure::Measurement as CoreMeasurement;
use rich::protocol::Renderable;
use rich::segment::Segment as CoreSegment;
use rich::{Style as CoreStyle, Text as CoreText};

use crate::protocol::OptionsBase;
use crate::renderable::{self, Ambient};
use crate::style::resolved_style;
use crate::text::Text;

create_exception!(_native, ExtError, PyException);
create_exception!(_native, DiagnosticSpanError, ExtError);
create_exception!(_native, TransformError, ExtError);
create_exception!(_native, PipelineError, TransformError);
create_exception!(_native, DataError, ExtError);
create_exception!(_native, SelectError, ExtError);
create_exception!(_native, PatchParseError, ExtError);
create_exception!(_native, TestParseError, ExtError);
create_exception!(_native, ConstraintError, ExtError);
create_exception!(_native, LiveCoordinatorError, ExtError);
create_exception!(_native, RedactPatternError, ExtError);
create_exception!(_native, EncodingError, ExtError);
create_exception!(_native, TransferCancelled, ExtError);

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    let py = m.py();
    m.add("ExtError", py.get_type::<ExtError>())?;
    m.add("DiagnosticSpanError", py.get_type::<DiagnosticSpanError>())?;
    m.add("TransformError", py.get_type::<TransformError>())?;
    m.add("PipelineError", py.get_type::<PipelineError>())?;
    m.add("DataError", py.get_type::<DataError>())?;
    m.add("SelectError", py.get_type::<SelectError>())?;
    m.add("PatchParseError", py.get_type::<PatchParseError>())?;
    m.add("TestParseError", py.get_type::<TestParseError>())?;
    m.add("ConstraintError", py.get_type::<ConstraintError>())?;
    m.add(
        "LiveCoordinatorError",
        py.get_type::<LiveCoordinatorError>(),
    )?;
    m.add("RedactPatternError", py.get_type::<RedactPatternError>())?;
    m.add("EncodingError", py.get_type::<EncodingError>())?;
    m.add("TransferCancelled", py.get_type::<TransferCancelled>())?;
    Ok(())
}

/// A two-way table between Python names and a Rust enum:
/// `names!(level, level_name, Level, "level", { "error" => Level::Error, ... })`
/// defines `level(&str) -> PyResult<Level>` and `level_name(Level) -> &str`.
macro_rules! names {
    ($parse:ident, $name:ident, $ty:ty, $what:literal, { $($text:literal => $value:expr),+ $(,)? }) => {
        #[allow(dead_code)]
        pub(crate) fn $parse(value: &str) -> pyo3::PyResult<$ty> {
            let lowered = value.to_ascii_lowercase().replace('-', "_");
            $(if lowered == $text { return Ok($value); })+
            Err(pyo3::exceptions::PyValueError::new_err(format!(
                "invalid {} {:?}; expected {}",
                $what,
                value,
                [$($text),+].join(", ")
            )))
        }
        #[allow(dead_code, unreachable_patterns)]
        pub(crate) fn $name(value: $ty) -> &'static str {
            $(if value == $value { return $text; })+
            "unknown"
        }
    };
}
pub(crate) use names;

/// Seconds (an `int` or `float`, or a `datetime.timedelta`) as a duration.
pub(crate) fn seconds(value: &Bound<'_, PyAny>) -> PyResult<Duration> {
    let secs: f64 = if let Some(total) = value.getattr_opt("total_seconds")? {
        total.call0()?.extract()?
    } else {
        value.extract()?
    };
    if !secs.is_finite() || secs < 0.0 {
        return Err(PyValueError::new_err(format!(
            "a duration must be a finite number of seconds, at least 0, got {secs}"
        )));
    }
    Duration::try_from_secs_f64(secs).map_err(|e| PyValueError::new_err(e.to_string()))
}

pub(crate) fn opt_seconds(value: Option<&Bound<'_, PyAny>>) -> PyResult<Option<Duration>> {
    value.filter(|v| !v.is_none()).map(seconds).transpose()
}

/// A style argument resolved now (`"bold red"` or a `Style`); `None` for none.
pub(crate) fn style(value: Option<&Bound<'_, PyAny>>) -> PyResult<Option<CoreStyle>> {
    resolved_style(value)
}

/// A required style argument.
pub(crate) fn required_style(value: &Bound<'_, PyAny>) -> PyResult<CoreStyle> {
    resolved_style(Some(value))?.ok_or_else(|| PyTypeError::new_err("a style is required"))
}

/// The byte offset of character `index` in `text` (negative counts from the
/// end, as in Python), clamped to the text.
pub(crate) fn byte_offset(text: &str, index: isize) -> usize {
    let chars = text.chars().count() as isize;
    let index = if index < 0 { chars + index } else { index }.clamp(0, chars) as usize;
    text.char_indices()
        .nth(index)
        .map_or(text.len(), |(offset, _)| offset)
}

/// The character index of byte offset `offset` in `text`.
pub(crate) fn char_index(text: &str, offset: usize) -> usize {
    let mut offset = offset.min(text.len());
    while !text.is_char_boundary(offset) {
        offset -= 1;
    }
    text[..offset].chars().count()
}

/// Character offsets `start..end` (Python semantics) as a byte range.
pub(crate) fn byte_range(text: &str, start: isize, end: isize) -> std::ops::Range<usize> {
    byte_offset(text, start)..byte_offset(text, end)
}

/// A `str` or `Text` argument as a core `Text` (a `str` is plain text).
pub(crate) fn text_arg(value: &Bound<'_, PyAny>) -> PyResult<CoreText> {
    if let Ok(text) = value.extract::<PyRef<'_, Text>>() {
        return Ok(text.inner.clone());
    }
    if let Ok(string) = value.cast::<PyString>() {
        return Ok(CoreText::new(string.to_cow()?.as_ref()));
    }
    Err(PyTypeError::new_err(format!(
        "expected a str or Text, got {}",
        value.get_type().name()?
    )))
}

/// A value a Python callback returned for a cell: a `Text`, or a `str`
/// read as markup (anything else is its `str`).
pub(crate) fn markup_or_text(value: &Bound<'_, PyAny>) -> PyResult<CoreText> {
    if let Ok(text) = value.extract::<PyRef<'_, Text>>() {
        return Ok(text.inner.clone());
    }
    let string = value.str()?;
    CoreText::from_markup(string.to_cow()?.as_ref())
        .map_err(|e| crate::errors::MarkupError::new_err(e.to_string()))
}

/// A core `Text` as a Python `Text`.
pub(crate) fn py_text(py: Python<'_>, text: CoreText) -> PyResult<Py<Text>> {
    Py::new(py, Text { inner: text })
}

/// A list of strings from any iterable of `str` (a lone `str` is one item).
pub(crate) fn strings(value: &Bound<'_, PyAny>) -> PyResult<Vec<String>> {
    if let Ok(string) = value.cast::<PyString>() {
        return Ok(vec![string.to_cow()?.into_owned()]);
    }
    value
        .try_iter()?
        .map(|item| item?.extract::<String>())
        .collect()
}

/// `rich_ext::event::Value` from a Python value.
pub(crate) fn event_value(value: &Bound<'_, PyAny>) -> PyResult<rich_ext::event::Value> {
    use rich_ext::event::Value;
    let _nesting = renderable::Nesting::enter()?;
    if value.is_none() {
        return Ok(Value::Null);
    }
    if let Ok(flag) = value.cast::<PyBool>() {
        return Ok(Value::Bool(flag.is_true()));
    }
    if value.is_instance_of::<PyInt>() {
        if let Ok(v) = value.extract::<i64>() {
            return Ok(Value::Integer(v));
        }
        if let Ok(v) = value.extract::<u64>() {
            return Ok(Value::Unsigned(v));
        }
        if let Ok(v) = value.extract::<i128>() {
            return Ok(Value::Integer128(v));
        }
        if let Ok(v) = value.extract::<u128>() {
            return Ok(Value::Unsigned128(v));
        }
        return Ok(Value::String(value.str()?.to_string()));
    }
    if let Ok(v) = value.cast::<PyFloat>() {
        return Ok(Value::Float(v.value()));
    }
    if let Ok(v) = value.cast::<PyString>() {
        return Ok(Value::String(v.to_cow()?.into_owned()));
    }
    if let Ok(map) = value.cast::<PyDict>() {
        let mut entries = Vec::with_capacity(map.len());
        for (key, item) in map.iter() {
            entries.push((key.str()?.to_string(), event_value(&item)?));
        }
        return Ok(Value::Map(entries));
    }
    if value.is_instance_of::<PyList>() || value.is_instance_of::<PyTuple>() {
        let items: PyResult<Vec<_>> = value.try_iter()?.map(|item| event_value(&item?)).collect();
        return Ok(Value::List(items?));
    }
    Ok(Value::String(value.str()?.to_string()))
}

/// A Python value for a `rich_ext::event::Value`.
pub(crate) fn event_value_to_py(
    py: Python<'_>,
    value: &rich_ext::event::Value,
) -> PyResult<Py<PyAny>> {
    use rich_ext::event::Value;
    Ok(match value {
        Value::Null => py.None(),
        Value::Bool(v) => PyBool::new(py, *v).to_owned().into_any().unbind(),
        Value::Integer(v) => v.into_pyobject(py)?.into_any().unbind(),
        Value::Unsigned(v) => v.into_pyobject(py)?.into_any().unbind(),
        Value::Integer128(v) => v.into_pyobject(py)?.into_any().unbind(),
        Value::Unsigned128(v) => v.into_pyobject(py)?.into_any().unbind(),
        Value::Float(v) => v.into_pyobject(py)?.into_any().unbind(),
        Value::String(v) => v.into_pyobject(py)?.into_any().unbind(),
        Value::List(items) => {
            let items: PyResult<Vec<_>> = items.iter().map(|v| event_value_to_py(py, v)).collect();
            PyList::new(py, items?)?.into_any().unbind()
        }
        Value::Map(entries) => {
            let dict = PyDict::new(py);
            for (key, item) in entries {
                dict.set_item(key, event_value_to_py(py, item)?)?;
            }
            dict.into_any().unbind()
        }
    })
}

/// `(key, value)` pairs from a `dict` or an iterable of pairs.
pub(crate) fn pairs<'py>(value: &Bound<'py, PyAny>) -> PyResult<Vec<(String, Bound<'py, PyAny>)>> {
    if let Ok(map) = value.cast::<PyDict>() {
        return map
            .iter()
            .map(|(key, item)| Ok((key.extract::<String>()?, item)))
            .collect();
    }
    value
        .try_iter()?
        .map(|pair| {
            let (key, item): (String, Bound<'py, PyAny>) = pair?.extract()?;
            Ok((key, item))
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Rendering

/// A core console of `width` columns for tools that render on their own
/// (reports, captures). Colour only when `color` is set.
pub(crate) fn plain_console(width: usize, color: bool) -> CoreConsole {
    CoreConsole::builder()
        .width(width)
        .force_terminal(color)
        .color_system(color.then_some(rich::ColorSystem::Truecolor))
        .no_color(!color)
        .build()
}

/// Run `f` in a render scope whose ambient Python console is a fresh
/// `rs_rich` console `width` columns wide, so Python renderables can render
/// inside a core render started here.
pub(crate) fn scoped<T>(
    py: Python<'_>,
    width: usize,
    height: usize,
    is_terminal: bool,
    f: impl FnOnce() -> PyResult<T>,
) -> PyResult<T> {
    let kwargs = PyDict::new(py);
    kwargs.set_item("width", width.max(1))?;
    kwargs.set_item("height", height.max(1))?;
    kwargs.set_item("file", py.import("io")?.getattr("StringIO")?.call0()?)?;
    let console = py
        .import("rs_rich.console")?
        .getattr("Console")?
        .call((), Some(&kwargs))?;
    let ambient = Ambient {
        console: console.unbind(),
        base: OptionsBase {
            size: (width, height),
            legacy_windows: false,
            is_terminal,
            encoding: "utf-8".into(),
            max_height: height,
            highlight: None,
            markup: None,
        },
        emoji: true,
        markup: true,
        highlight: true,
    };
    renderable::scope(ambient, f)
}

/// A boxed renderable as a renderable, for `rich-ext` wrappers generic over
/// `R: Renderable` (`Degrade`, `Redacted`, `Overflowing`).
pub(crate) struct Boxed(pub(crate) Box<dyn Renderable>);

impl Renderable for Boxed {
    fn rich_render(&self, console: &CoreConsole, options: &CoreOptions) -> Vec<CoreSegment> {
        self.0.rich_render(console, options)
    }

    fn measure(&self, console: &CoreConsole, options: &CoreOptions) -> CoreMeasurement {
        self.0.measure(console, options)
    }
}
