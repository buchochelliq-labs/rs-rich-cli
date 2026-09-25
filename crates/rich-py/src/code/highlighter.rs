//! `rich.highlighter`: `Highlighter`, `NullHighlighter`, `RegexHighlighter`,
//! `ReprHighlighter`, `JSONHighlighter` and `ISO8601Highlighter`.
//!
//! The classes can be subclassed from Python, as in Rich: a subclass of
//! `RegexHighlighter` sets `highlights` and `base_style`, one of `Highlighter`
//! overrides `highlight(text)`. Patterns a Python class supplies are Python
//! regular expressions, so they run through Python's `re` (the binding only
//! turns the match offsets into core spans). The built-in `ReprHighlighter`
//! and `ISO8601Highlighter` patterns run through core's parity-tested ports
//! unless the instance's `highlights` were replaced.

use pyo3::exceptions::{PyNotImplementedError, PyTypeError};
use pyo3::prelude::*;
use pyo3::sync::PyOnceLock;
use pyo3::types::{PyDict, PyList, PyString, PyTuple};

use rich::protocol::Highlighter as _;
use rich::style::StyleType;
use rich::Text as CoreText;

use crate::text::Text;

/// A new Python `Text` holding `inner` (built through the class, so it stays
/// valid whatever fields the text area adds).
pub(crate) fn new_text<'py>(py: Python<'py>, inner: CoreText) -> PyResult<Bound<'py, Text>> {
    let object = py.get_type::<Text>().call0()?.cast_into::<Text>()?;
    object.borrow_mut().inner = inner;
    Ok(object)
}

/// The byte offset of every character of `plain`, plus its length: Python's
/// character indices to core's byte offsets.
pub(crate) fn char_offsets(plain: &str) -> Vec<usize> {
    let mut offsets: Vec<usize> = plain.char_indices().map(|(offset, _)| offset).collect();
    offsets.push(plain.len());
    offsets
}

/// Rich's `Text.highlight_regex(pattern, style_prefix=prefix)` with Python's
/// `re`: each named group that matched something is styled with the style
/// named `{prefix}{group}`.
pub(crate) fn highlight_regex(
    py: Python<'_>,
    text: &mut CoreText,
    pattern: &Bound<'_, PyAny>,
    prefix: &str,
) -> PyResult<()> {
    let plain = text.plain().to_string();
    let offsets = char_offsets(&plain);
    let matches = py
        .import("re")?
        .call_method1("finditer", (pattern, plain.as_str()))?;
    for found in matches.try_iter()? {
        let found = found?;
        let groups = found.call_method0("groupdict")?;
        let groups = groups.cast::<PyDict>()?;
        for name in groups.keys() {
            let (start, end): (isize, isize) = found.call_method1("span", (&name,))?.extract()?;
            if start > -1 && end > start {
                let name: String = name.extract()?;
                text.stylize(
                    StyleType::Name(format!("{prefix}{name}")),
                    offsets[start as usize],
                    offsets[end as usize],
                );
            }
        }
    }
    Ok(())
}

fn not_text(value: &Bound<'_, PyAny>) -> PyResult<PyErr> {
    Ok(PyTypeError::new_err(format!(
        "str or Text instance required, not {}",
        value.repr()?
    )))
}

/// `rich.highlighter.Highlighter`: calling one highlights a copy of a `str`
/// or `Text` with `highlight(text)`, which subclasses implement.
#[pyclass(name = "Highlighter", module = "rs_rich.highlighter", subclass)]
pub(crate) struct Highlighter;

#[pymethods]
impl Highlighter {
    #[new]
    #[pyo3(signature = (*_args, **_kwargs))]
    fn new(_args: &Bound<'_, PyTuple>, _kwargs: Option<&Bound<'_, PyDict>>) -> Self {
        Highlighter
    }

    fn __call__<'py>(
        slf: &Bound<'py, Self>,
        text: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, Text>> {
        let py = slf.py();
        let inner = if let Ok(string) = text.cast::<PyString>() {
            CoreText::new(string.to_cow()?.as_ref())
        } else if let Ok(text) = text.extract::<PyRef<'_, Text>>() {
            text.inner.clone()
        } else {
            return Err(not_text(text)?);
        };
        let copy = new_text(py, inner)?;
        slf.call_method1("highlight", (&copy,))?;
        Ok(copy)
    }

    /// Apply highlighting in place to `text`; subclasses implement it.
    fn highlight(&self, _text: &Bound<'_, PyAny>) -> PyResult<()> {
        Err(PyNotImplementedError::new_err(
            "Highlighter is abstract: subclasses implement highlight(text)",
        ))
    }
}

/// `rich.highlighter.NullHighlighter`: highlights nothing.
#[pyclass(name = "NullHighlighter", module = "rs_rich.highlighter", extends = Highlighter, subclass)]
pub(crate) struct NullHighlighter;

#[pymethods]
impl NullHighlighter {
    #[new]
    #[pyo3(signature = (*_args, **_kwargs))]
    fn new(
        _args: &Bound<'_, PyTuple>,
        _kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyClassInitializer<Self> {
        PyClassInitializer::from(Highlighter).add_subclass(NullHighlighter)
    }

    fn highlight(&self, _text: &Bound<'_, PyAny>) {}
}

/// Which core highlighter an instance's patterns are, when they are still
/// the built-in ones.
enum Builtin {
    Repr,
    Iso8601,
}

fn builtin(slf: &Bound<'_, PyAny>, highlights: &Bound<'_, PyAny>, base: &str) -> Option<Builtin> {
    let py = slf.py();
    let is = |class: Bound<'_, pyo3::types::PyType>| {
        class
            .getattr("highlights")
            .map(|default| default.is(highlights))
            .unwrap_or(false)
    };
    if base == "repr." && is(py.get_type::<ReprHighlighter>()) {
        Some(Builtin::Repr)
    } else if base == "iso8601." && is(py.get_type::<ISO8601Highlighter>()) {
        Some(Builtin::Iso8601)
    } else {
        None
    }
}

/// `RegexHighlighter.highlight` on a core text, reading `highlights` and
/// `base_style` from `slf` (so Python subclasses and instances can set them).
fn regex_highlight(slf: &Bound<'_, PyAny>, text: &mut CoreText) -> PyResult<()> {
    let py = slf.py();
    let highlights = slf.getattr("highlights")?;
    let base: String = slf.getattr("base_style")?.extract()?;
    match builtin(slf, &highlights, &base) {
        Some(Builtin::Repr) => rich::ReprHighlighter::new().highlight(text),
        Some(Builtin::Iso8601) => rich::ISO8601Highlighter::new().highlight(text),
        None => {
            for pattern in highlights.try_iter()? {
                highlight_regex(py, text, &pattern?, &base)?;
            }
        }
    }
    Ok(())
}

/// Run `f` on the core text inside a Python `Text` argument.
fn with_text(
    text: &Bound<'_, PyAny>,
    f: impl FnOnce(&mut CoreText) -> PyResult<()>,
) -> PyResult<()> {
    let text = text.cast::<Text>().map_err(|_| {
        PyTypeError::new_err(format!(
            "highlight() needs a Text, not {}",
            text.repr().map(|r| r.to_string()).unwrap_or_default()
        ))
    })?;
    // Highlight a copy, so a pattern's Python code never runs while the
    // text is borrowed.
    let mut inner = text.borrow().inner.clone();
    f(&mut inner)?;
    text.borrow_mut().inner = inner;
    Ok(())
}

/// `rich.highlighter.RegexHighlighter`: applies the named-group patterns in
/// `highlights`, styling each group `{base_style}{name}`.
#[pyclass(name = "RegexHighlighter", module = "rs_rich.highlighter", extends = Highlighter, subclass)]
pub(crate) struct RegexHighlighter;

#[pymethods]
impl RegexHighlighter {
    #[new]
    #[pyo3(signature = (*_args, **_kwargs))]
    fn new(
        _args: &Bound<'_, PyTuple>,
        _kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyClassInitializer<Self> {
        PyClassInitializer::from(Highlighter).add_subclass(RegexHighlighter)
    }

    #[classattr]
    fn highlights(py: Python<'_>) -> PyResult<Py<PyList>> {
        Ok(PyList::empty(py).unbind())
    }

    #[classattr]
    fn base_style() -> &'static str {
        ""
    }

    fn highlight(slf: &Bound<'_, Self>, text: &Bound<'_, PyAny>) -> PyResult<()> {
        with_text(text, |inner| regex_highlight(slf.as_any(), inner))
    }
}

/// Upstream's `ReprHighlighter.highlights`, verbatim (the Python-visible
/// value; highlighting uses core's port of the same patterns).
const REPR_HIGHLIGHTS: &[&str] = &[
    r"(?P<tag_start><)(?P<tag_name>[-\w.:|]*)(?P<tag_contents>[\w\W]*)(?P<tag_end>>)",
    r#"(?P<attrib_name>[\w_]{1,50})=(?P<attrib_value>"?[\w_]+"?)?"#,
    r"(?P<brace>[][{}()])",
    concat!(
        r"(?P<ipv4>[0-9]{1,3}\.[0-9]{1,3}\.[0-9]{1,3}\.[0-9]{1,3})|",
        r"(?P<ipv6>([A-Fa-f0-9]{1,4}::?){1,7}[A-Fa-f0-9]{1,4})|",
        r"(?P<eui64>(?:[0-9A-Fa-f]{1,2}-){7}[0-9A-Fa-f]{1,2}|(?:[0-9A-Fa-f]{1,2}:){7}[0-9A-Fa-f]{1,2}|(?:[0-9A-Fa-f]{4}\.){3}[0-9A-Fa-f]{4})|",
        r"(?P<eui48>(?:[0-9A-Fa-f]{1,2}-){5}[0-9A-Fa-f]{1,2}|(?:[0-9A-Fa-f]{1,2}:){5}[0-9A-Fa-f]{1,2}|(?:[0-9A-Fa-f]{4}\.){2}[0-9A-Fa-f]{4})|",
        r"(?P<uuid>[a-fA-F0-9]{8}-[a-fA-F0-9]{4}-[a-fA-F0-9]{4}-[a-fA-F0-9]{4}-[a-fA-F0-9]{12})|",
        r"(?P<call>[\w.]*?)\(|",
        r"\b(?P<bool_true>True)\b|\b(?P<bool_false>False)\b|\b(?P<none>None)\b|",
        r"(?P<ellipsis>\.\.\.)|",
        r"(?P<number_complex>(?<!\w)(?:\-?[0-9]+\.?[0-9]*(?:e[-+]?\d+?)?)(?:[-+](?:[0-9]+\.?[0-9]*(?:e[-+]?\d+)?))?j)|",
        r"(?P<number>(?<!\w)\-?[0-9]+\.?[0-9]*(e[-+]?\d+?)?\b|0x[0-9a-fA-F]*)|",
        r"(?P<path>\B(/[-\w._+]+)*\/)(?P<filename>[-\w._+]*)?|",
        r#"(?<![\\\w])(?P<str>b?'''.*?(?<!\\)'''|b?'.*?(?<!\\)'|b?""".*?(?<!\\)"""|b?".*?(?<!\\)")|"#,
        r"(?P<url>(file|https|http|ws|wss)://[-0-9a-zA-Z$_+!`(),.?/;:&=%#~@]*)",
    ),
];

/// `rich.highlighter.ReprHighlighter`: highlights `repr`-style text.
#[pyclass(name = "ReprHighlighter", module = "rs_rich.highlighter", extends = RegexHighlighter, subclass)]
pub(crate) struct ReprHighlighter;

#[pymethods]
impl ReprHighlighter {
    #[new]
    #[pyo3(signature = (*_args, **_kwargs))]
    fn new(
        _args: &Bound<'_, PyTuple>,
        _kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyClassInitializer<Self> {
        PyClassInitializer::from(Highlighter)
            .add_subclass(RegexHighlighter)
            .add_subclass(ReprHighlighter)
    }

    #[classattr]
    fn highlights(py: Python<'_>) -> PyResult<Py<PyList>> {
        Ok(PyList::new(py, REPR_HIGHLIGHTS)?.unbind())
    }

    #[classattr]
    fn base_style() -> &'static str {
        "repr."
    }
}

const JSON_STR: &str = r#"(?<![\\\w])(?P<str>b?".*?(?<!\\)")"#;

/// `rich.highlighter.JSONHighlighter`: highlights JSON, keys included.
#[pyclass(name = "JSONHighlighter", module = "rs_rich.highlighter", extends = RegexHighlighter, subclass)]
pub(crate) struct JSONHighlighter;

/// JSON highlighting on a core text: `highlights`, then keys (a string
/// followed, past whitespace, by `:`), as upstream's `JSONHighlighter`.
pub(crate) fn json_highlight(slf: &Bound<'_, PyAny>, text: &mut CoreText) -> PyResult<()> {
    let py = slf.py();
    regex_highlight(slf, text)?;
    let plain = text.plain().to_string();
    let chars: Vec<char> = plain.chars().collect();
    let offsets = char_offsets(&plain);
    let pattern = slf.getattr("JSON_STR")?;
    let matches = py
        .import("re")?
        .call_method1("finditer", (pattern, plain.as_str()))?;
    for found in matches.try_iter()? {
        let (start, end): (usize, usize) = found?.call_method0("span")?.extract()?;
        let mut cursor = end;
        while cursor < chars.len() {
            let char = chars[cursor];
            cursor += 1;
            if char == ':' {
                text.stylize(
                    StyleType::Name("json.key".to_string()),
                    offsets[start],
                    offsets[end],
                );
            } else if matches!(char, ' ' | '\n' | '\r' | '\t') {
                continue;
            }
            break;
        }
    }
    Ok(())
}

#[pymethods]
impl JSONHighlighter {
    #[new]
    #[pyo3(signature = (*_args, **_kwargs))]
    fn new(
        _args: &Bound<'_, PyTuple>,
        _kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyClassInitializer<Self> {
        PyClassInitializer::from(Highlighter)
            .add_subclass(RegexHighlighter)
            .add_subclass(JSONHighlighter)
    }

    #[classattr]
    #[pyo3(name = "JSON_STR")]
    fn json_str() -> &'static str {
        JSON_STR
    }

    #[classattr]
    #[pyo3(name = "JSON_WHITESPACE")]
    fn json_whitespace(py: Python<'_>) -> PyResult<Py<PyAny>> {
        Ok(pyo3::types::PySet::new(py, [" ", "\n", "\r", "\t"])?
            .into_any()
            .unbind())
    }

    #[classattr]
    fn highlights(py: Python<'_>) -> PyResult<Py<PyList>> {
        let combined = [
            r"(?P<brace>[\{\[\(\)\]\}])",
            r"\b(?P<bool_true>true)\b|\b(?P<bool_false>false)\b|\b(?P<null>null)\b",
            r"(?P<number>(?<!\w)\-?[0-9]+\.?[0-9]*(e[\-\+]?\d+?)?\b|0x[0-9a-fA-F]*)",
            JSON_STR,
        ]
        .join("|");
        Ok(PyList::new(py, [combined])?.unbind())
    }

    #[classattr]
    fn base_style() -> &'static str {
        "json."
    }

    fn highlight(slf: &Bound<'_, Self>, text: &Bound<'_, PyAny>) -> PyResult<()> {
        with_text(text, |inner| json_highlight(slf.as_any(), inner))
    }
}

/// `rich.highlighter.ISO8601Highlighter`: highlights ISO 8601 dates and times.
#[pyclass(name = "ISO8601Highlighter", module = "rs_rich.highlighter", extends = RegexHighlighter, subclass)]
pub(crate) struct ISO8601Highlighter;

/// Upstream's `ISO8601Highlighter.highlights`, verbatim. They are Python
/// patterns (one uses a conditional group); highlighting uses core's port.
const ISO8601_HIGHLIGHTS: &[&str] = &[
    r"^(?P<year>[0-9]{4})-(?P<month>1[0-2]|0[1-9])$",
    r"^(?P<date>(?P<year>[0-9]{4})(?P<month>1[0-2]|0[1-9])(?P<day>3[01]|0[1-9]|[12][0-9]))$",
    r"^(?P<date>(?P<year>[0-9]{4})-?(?P<day>36[0-6]|3[0-5][0-9]|[12][0-9]{2}|0[1-9][0-9]|00[1-9]))$",
    r"^(?P<date>(?P<year>[0-9]{4})-?W(?P<week>5[0-3]|[1-4][0-9]|0[1-9]))$",
    r"^(?P<date>(?P<year>[0-9]{4})-?W(?P<week>5[0-3]|[1-4][0-9]|0[1-9])-?(?P<day>[1-7]))$",
    r"^(?P<time>(?P<hour>2[0-3]|[01][0-9]):?(?P<minute>[0-5][0-9]))$",
    r"^(?P<time>(?P<hour>2[0-3]|[01][0-9])(?P<minute>[0-5][0-9])(?P<second>[0-5][0-9]))$",
    r"^(?P<timezone>(Z|[+-](?:2[0-3]|[01][0-9])(?::?(?:[0-5][0-9]))?))$",
    r"^(?P<time>(?P<hour>2[0-3]|[01][0-9])(?P<minute>[0-5][0-9])(?P<second>[0-5][0-9]))(?P<timezone>Z|[+-](?:2[0-3]|[01][0-9])(?::?(?:[0-5][0-9]))?)$",
    r"^(?P<date>(?P<year>[0-9]{4})(?P<hyphen>-)?(?P<month>1[0-2]|0[1-9])(?(hyphen)-)(?P<day>3[01]|0[1-9]|[12][0-9])) (?P<time>(?P<hour>2[0-3]|[01][0-9])(?(hyphen):)(?P<minute>[0-5][0-9])(?(hyphen):)(?P<second>[0-5][0-9]))$",
    r"^(?P<date>(?P<year>-?(?:[1-9][0-9]*)?[0-9]{4})-(?P<month>1[0-2]|0[1-9])-(?P<day>3[01]|0[1-9]|[12][0-9]))(?P<timezone>Z|[+-](?:2[0-3]|[01][0-9]):[0-5][0-9])?$",
    r"^(?P<time>(?P<hour>2[0-3]|[01][0-9]):(?P<minute>[0-5][0-9]):(?P<second>[0-5][0-9])(?P<frac>\.[0-9]+)?)(?P<timezone>Z|[+-](?:2[0-3]|[01][0-9]):[0-5][0-9])?$",
    r"^(?P<date>(?P<year>-?(?:[1-9][0-9]*)?[0-9]{4})-(?P<month>1[0-2]|0[1-9])-(?P<day>3[01]|0[1-9]|[12][0-9]))T(?P<time>(?P<hour>2[0-3]|[01][0-9]):(?P<minute>[0-5][0-9]):(?P<second>[0-5][0-9])(?P<ms>\.[0-9]+)?)(?P<timezone>Z|[+-](?:2[0-3]|[01][0-9]):[0-5][0-9])?$",
];

#[pymethods]
impl ISO8601Highlighter {
    #[new]
    #[pyo3(signature = (*_args, **_kwargs))]
    fn new(
        _args: &Bound<'_, PyTuple>,
        _kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyClassInitializer<Self> {
        PyClassInitializer::from(Highlighter)
            .add_subclass(RegexHighlighter)
            .add_subclass(ISO8601Highlighter)
    }

    #[classattr]
    fn highlights(py: Python<'_>) -> PyResult<Py<PyList>> {
        Ok(PyList::new(py, ISO8601_HIGHLIGHTS)?.unbind())
    }

    #[classattr]
    fn base_style() -> &'static str {
        "iso8601."
    }
}

/// A highlighter argument (`highlighter=` of `Pretty`, `Console`, ...),
/// resolved once so the common cases never call into Python.
pub(crate) enum Highlight {
    /// `ReprHighlighter` (or `None`, which means it).
    Repr,
    /// `NullHighlighter`: leave the text alone.
    Null,
    /// Anything else: a Python callable taking and returning a `Text`.
    Python(Py<PyAny>),
}

impl Highlight {
    /// `None` is the repr highlighter, as Rich's `highlighter or ReprHighlighter()`.
    pub(crate) fn from_arg(value: Option<&Bound<'_, PyAny>>) -> PyResult<Highlight> {
        let Some(value) = value.filter(|value| !value.is_none()) else {
            return Ok(Highlight::Repr);
        };
        let py = value.py();
        let class = value.get_type();
        if class.is(py.get_type::<NullHighlighter>()) {
            return Ok(Highlight::Null);
        }
        if class.is(py.get_type::<ReprHighlighter>()) {
            let highlights = value.getattr("highlights")?;
            let base: String = value.getattr("base_style")?.extract()?;
            if matches!(builtin(value, &highlights, &base), Some(Builtin::Repr)) {
                return Ok(Highlight::Repr);
            }
        }
        if !value.is_callable() {
            return Err(PyTypeError::new_err(format!(
                "highlighter must be callable, not {}",
                value.repr()?
            )));
        }
        Ok(Highlight::Python(value.clone().unbind()))
    }

    /// Whether highlighting calls Python code.
    pub(crate) fn is_python(&self) -> bool {
        matches!(self, Highlight::Python(_))
    }

    pub(crate) fn clone_ref(&self, py: Python<'_>) -> Highlight {
        match self {
            Highlight::Repr => Highlight::Repr,
            Highlight::Null => Highlight::Null,
            Highlight::Python(object) => Highlight::Python(object.clone_ref(py)),
        }
    }

    /// Highlight `text` (Rich's `highlighter(text)`).
    pub(crate) fn apply(&self, py: Python<'_>, mut text: CoreText) -> PyResult<CoreText> {
        match self {
            Highlight::Repr => {
                rich::ReprHighlighter::new().highlight(&mut text);
                Ok(text)
            }
            Highlight::Null => Ok(text),
            Highlight::Python(callable) => {
                let argument = new_text(py, text)?;
                let result = callable.bind(py).call1((argument,))?;
                let result = result.extract::<PyRef<'_, Text>>().map_err(|_| {
                    PyTypeError::new_err(format!(
                        "highlighter must return a Text, not {}",
                        result.repr().map(|r| r.to_string()).unwrap_or_default()
                    ))
                })?;
                Ok(result.inner.clone())
            }
        }
    }

    /// Highlight a `str`.
    pub(crate) fn apply_str(&self, py: Python<'_>, text: &str) -> PyResult<CoreText> {
        self.apply(py, CoreText::new(text))
    }
}

/// `JSONHighlighter()` on a core text, without a Python instance.
pub(crate) fn json(py: Python<'_>, text: &mut CoreText) -> PyResult<()> {
    static INSTANCE: PyOnceLock<Py<JSONHighlighter>> = PyOnceLock::new();
    let instance = INSTANCE.get_or_try_init(py, || {
        Py::new(
            py,
            PyClassInitializer::from(Highlighter)
                .add_subclass(RegexHighlighter)
                .add_subclass(JSONHighlighter),
        )
    })?;
    json_highlight(instance.bind(py).as_any(), text)
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Highlighter>()?;
    m.add_class::<NullHighlighter>()?;
    m.add_class::<RegexHighlighter>()?;
    m.add_class::<ReprHighlighter>()?;
    m.add_class::<JSONHighlighter>()?;
    m.add_class::<ISO8601Highlighter>()?;
    Ok(())
}
