//! `rich.style`: the `Style` and `StyleStack` classes and the `style=`
//! argument conversions.
//!
//! Owner: the text/style area (with `text.rs`, `theme.rs` and `color.rs`).
//!
//! A `Style` wraps a core style. The Python object keeps its `meta` as Rich
//! does, `marshal`-encoded, for equality, hashing and `repr`; the core style
//! carries the same meta (core's `Meta`) whenever its values are ones core
//! holds (`None`, `bool`, `int`, `float`, `str`, and lists or tuples of
//! them), so it survives in a `Text`'s spans. Other values stay on the
//! Python object only.

use std::sync::atomic::{AtomicU64, Ordering};

use pyo3::exceptions::{PyStopIteration, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict, PyTuple, PyType};

use rich::style::{Meta as CoreMeta, MetaValue};
use rich::{RichError, Style as CoreStyle, StyleType};

use crate::color::{core_color, core_system, py_color, python_repr};
use crate::errors::StyleSyntaxError;

/// Rich's attribute names, in bit order (`bold` is bit 0).
const ATTRIBUTES: [&str; 13] = [
    "bold",
    "dim",
    "italic",
    "underline",
    "blink",
    "blink2",
    "reverse",
    "conceal",
    "strike",
    "underline2",
    "frame",
    "encircle",
    "overline",
];

/// `rich.style.Style`: attributes, colours, a link and meta data.
#[pyclass(name = "Style", module = "rs_rich.style", frozen, skip_from_py_object)]
#[derive(Clone)]
pub(crate) struct Style {
    pub(crate) inner: CoreStyle,
    /// `marshal.dumps(meta)`, as Rich keeps it.
    meta: Option<Vec<u8>>,
    /// Whether `meta` was a non-empty dict (Rich's `bool(meta)`).
    meta_truthy: bool,
    link_id: String,
}

/// Rich numbers links from a counter (plus the meta's hash).
fn next_link_id() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed).to_string()
}

impl Style {
    /// Wrap a core style; its definition is core's normalised one.
    pub(crate) fn from_core(inner: CoreStyle) -> Style {
        let meta = inner.meta_ref().and_then(|meta| {
            Python::attach(|py| {
                let dict = meta_to_py(py, meta).ok()?;
                Some((dump_meta(dict.as_any()).ok()?, !meta.is_empty()))
            })
        });
        match meta {
            Some((bytes, truthy)) => Style::with_meta(inner, Some(bytes), truthy),
            None => Style::with_meta(inner, None, false),
        }
    }

    fn with_meta(inner: CoreStyle, meta: Option<Vec<u8>>, meta_truthy: bool) -> Style {
        // The core style carries the meta too, when core can hold it.
        let inner = match &meta {
            Some(bytes) => match Python::attach(|py| core_meta_from_bytes(py, bytes)) {
                Ok(core) => inner.with_meta(core),
                Err(_) => inner,
            },
            None => inner,
        };
        let link_id = if inner.link().is_some() || meta.is_some() {
            next_link_id()
        } else {
            String::new()
        };
        Style {
            inner,
            meta,
            meta_truthy,
            link_id,
        }
    }

    /// Rich's `_null`: nothing set, and no meta data.
    fn is_null(&self) -> bool {
        self.inner.is_null() && !self.meta_truthy
    }

    fn meta_dict<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        match &self.meta {
            None => Ok(PyDict::new(py)),
            Some(bytes) => Ok(py
                .import("marshal")?
                .call_method1("loads", (PyBytes::new(py, bytes),))?
                .cast_into::<PyDict>()?),
        }
    }

    /// Rich's `Style._add`: `other` wins where it sets a value, and the meta
    /// data merges.
    fn add(&self, py: Python<'_>, other: &Style) -> PyResult<Style> {
        if other.is_null() {
            return Ok(self.clone());
        }
        if self.is_null() {
            return Ok(other.clone());
        }
        let inner = self.inner.combine(&other.inner);
        let (meta, meta_truthy) = match (&self.meta, &other.meta) {
            (Some(_), Some(_)) if self.meta_truthy && other.meta_truthy => {
                let merged = self.meta_dict(py)?;
                merged.update(other.meta_dict(py)?.as_mapping())?;
                (Some(dump_meta(&merged)?), true)
            }
            _ if self.meta_truthy => (self.meta.clone(), true),
            _ => (other.meta.clone(), other.meta_truthy),
        };
        Ok(Style::with_meta(inner, meta, meta_truthy))
    }
}

/// A Python meta value as core's, if core can hold it.
fn meta_value(value: &Bound<'_, PyAny>) -> PyResult<MetaValue> {
    use pyo3::types::{PyBool, PyFloat, PyInt, PyList, PyString};
    if value.is_none() {
        Ok(MetaValue::None)
    } else if let Ok(flag) = value.cast::<PyBool>() {
        Ok(MetaValue::Bool(flag.is_true()))
    } else if value.is_exact_instance_of::<PyInt>() {
        Ok(MetaValue::Int(value.extract()?))
    } else if value.is_exact_instance_of::<PyFloat>() {
        Ok(MetaValue::Float(value.extract()?))
    } else if value.is_exact_instance_of::<PyString>() {
        Ok(MetaValue::Str(value.extract()?))
    } else if value.is_exact_instance_of::<PyList>() || value.is_exact_instance_of::<PyTuple>() {
        value
            .try_iter()?
            .map(|item| meta_value(&item?))
            .collect::<PyResult<Vec<_>>>()
            .map(MetaValue::List)
    } else {
        Err(PyTypeError::new_err(format!(
            "rs_rich keeps meta values of type None, bool, int, float, str, list and tuple \
             in rendered styles, not {}",
            value.get_type().name()?
        )))
    }
}

/// A Python meta dict as core's `Meta` (a `TypeError` for values core
/// cannot hold, or keys that are not `str`).
pub(crate) fn core_meta(meta: &Bound<'_, PyAny>) -> PyResult<CoreMeta> {
    let dict = meta.cast::<PyDict>()?;
    let mut core = CoreMeta::new();
    for (key, value) in dict.iter() {
        let key: String = key
            .extract()
            .map_err(|_| PyTypeError::new_err("rs_rich keeps meta data with str keys only"))?;
        core.insert(key, meta_value(&value)?);
    }
    Ok(core)
}

fn core_meta_from_bytes(py: Python<'_>, bytes: &[u8]) -> PyResult<CoreMeta> {
    let meta = py
        .import("marshal")?
        .call_method1("loads", (PyBytes::new(py, bytes),))?;
    core_meta(&meta)
}

fn meta_value_to_py(py: Python<'_>, value: &MetaValue) -> PyResult<Py<PyAny>> {
    Ok(match value {
        MetaValue::None => py.None(),
        MetaValue::Bool(flag) => flag.into_pyobject(py)?.to_owned().into_any().unbind(),
        MetaValue::Int(number) => number.into_pyobject(py)?.into_any().unbind(),
        MetaValue::Float(number) => number.into_pyobject(py)?.into_any().unbind(),
        MetaValue::Str(text) => text.into_pyobject(py)?.into_any().unbind(),
        MetaValue::List(items) => {
            let list = pyo3::types::PyList::empty(py);
            for item in items {
                list.append(meta_value_to_py(py, item)?)?;
            }
            list.into_any().unbind()
        }
    })
}

/// Core's `Meta` as a Python dict.
fn meta_to_py<'py>(py: Python<'py>, meta: &CoreMeta) -> PyResult<Bound<'py, PyDict>> {
    let dict = PyDict::new(py);
    for (key, value) in meta.iter() {
        dict.set_item(key, meta_value_to_py(py, value)?)?;
    }
    Ok(dict)
}

fn dump_meta(meta: &Bound<'_, PyAny>) -> PyResult<Vec<u8>> {
    meta.py()
        .import("marshal")?
        .call_method1("dumps", (meta,))?
        .extract()
}

/// A core style error as `StyleSyntaxError`, without core's prefix.
fn syntax_error(error: RichError) -> PyErr {
    match error {
        RichError::StyleSyntax(message) | RichError::ColorParse(message) => {
            StyleSyntaxError::new_err(message)
        }
        other => StyleSyntaxError::new_err(other.to_string()),
    }
}

pub(crate) fn parse_style(definition: &str) -> PyResult<CoreStyle> {
    CoreStyle::parse(definition).map_err(|error| match parse_error_message(definition) {
        Some(message) => StyleSyntaxError::new_err(message),
        None => syntax_error(error),
    })
}

/// The message Rich's `Style.parse` raises for `definition`, found by
/// scanning it as Rich does.
fn parse_error_message(definition: &str) -> Option<String> {
    let color_error = |word: &str| crate::color::parse(word).err().map(|e| e.to_string());
    let mut words = definition.split_whitespace();
    while let Some(original) = words.next() {
        let word = original.to_lowercase();
        match word.as_str() {
            "on" => {
                let Some(word) = words.next() else {
                    return Some("color expected after 'on'".into());
                };
                if let Some(error) = color_error(word) {
                    let error = error.trim_start_matches("ColorParseError: ");
                    return Some(format!(
                        "unable to parse {} as background color; {error}",
                        python_repr(word)
                    ));
                }
            }
            "not" => {
                let word = words.next().unwrap_or("");
                let known = ATTRIBUTES.contains(&word)
                    || ["b", "d", "i", "u", "r", "c", "s", "uu", "o"].contains(&word);
                if !known {
                    return Some(format!(
                        "expected style attribute after 'not', found {}",
                        python_repr(word)
                    ));
                }
            }
            "link" => {
                if words.next().is_none() {
                    return Some("URL expected after 'link'".into());
                }
            }
            _ => {
                let known = ATTRIBUTES.contains(&word.as_str())
                    || ["b", "d", "i", "u", "r", "c", "s", "uu", "o"].contains(&word.as_str());
                if !known {
                    if let Some(error) = color_error(&word) {
                        let error = error.trim_start_matches("ColorParseError: ");
                        return Some(format!(
                            "unable to parse {} as color; {error}",
                            python_repr(&word)
                        ));
                    }
                }
            }
        }
    }
    None
}

/// A core style with the given attribute values (`None` leaves one unset).
fn with_attributes(values: &[(usize, bool)]) -> CoreStyle {
    let words: Vec<String> = values
        .iter()
        .map(|&(index, on)| {
            if on {
                ATTRIBUTES[index].to_string()
            } else {
                format!("not {}", ATTRIBUTES[index])
            }
        })
        .collect();
    CoreStyle::parse(&words.join(" ")).unwrap_or_default()
}

#[pymethods]
impl Style {
    #[new]
    #[pyo3(signature = (
        *, color=None, bgcolor=None, bold=None, dim=None, italic=None, underline=None,
        blink=None, blink2=None, reverse=None, conceal=None, strike=None, underline2=None,
        frame=None, encircle=None, overline=None, link=None, meta=None
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        color: Option<&Bound<'_, PyAny>>,
        bgcolor: Option<&Bound<'_, PyAny>>,
        bold: Option<&Bound<'_, PyAny>>,
        dim: Option<&Bound<'_, PyAny>>,
        italic: Option<&Bound<'_, PyAny>>,
        underline: Option<&Bound<'_, PyAny>>,
        blink: Option<&Bound<'_, PyAny>>,
        blink2: Option<&Bound<'_, PyAny>>,
        reverse: Option<&Bound<'_, PyAny>>,
        conceal: Option<&Bound<'_, PyAny>>,
        strike: Option<&Bound<'_, PyAny>>,
        underline2: Option<&Bound<'_, PyAny>>,
        frame: Option<&Bound<'_, PyAny>>,
        encircle: Option<&Bound<'_, PyAny>>,
        overline: Option<&Bound<'_, PyAny>>,
        link: Option<String>,
        meta: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        let flags = [
            bold, dim, italic, underline, blink, blink2, reverse, conceal, strike, underline2,
            frame, encircle, overline,
        ];
        let mut values = Vec::new();
        for (index, flag) in flags.iter().enumerate() {
            if let Some(flag) = flag.filter(|flag| !flag.is_none()) {
                values.push((index, flag.is_truthy()?));
            }
        }
        let mut inner = with_attributes(&values);
        if let Some(color) = color.filter(|c| !c.is_none()) {
            inner = inner.with_color(core_color(color)?);
        }
        if let Some(bgcolor) = bgcolor.filter(|c| !c.is_none()) {
            inner = inner.with_bgcolor(core_color(bgcolor)?);
        }
        if let Some(link) = link.filter(|link| !link.is_empty()) {
            inner = inner.with_link(link);
        }
        let (meta, meta_truthy) = match meta.filter(|m| !m.is_none()) {
            Some(meta) => (Some(dump_meta(meta)?), meta.is_truthy()?),
            None => (None, false),
        };
        Ok(Style::with_meta(inner, meta, meta_truthy))
    }

    /// `Style.null()`: the style that sets nothing.
    #[classmethod]
    fn null(_cls: &Bound<'_, PyType>) -> Style {
        Style::from_core(CoreStyle::new())
    }

    /// A style with colours and no attributes.
    #[classmethod]
    #[pyo3(signature = (color=None, bgcolor=None))]
    fn from_color(
        _cls: &Bound<'_, PyType>,
        color: Option<&Bound<'_, PyAny>>,
        bgcolor: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Style> {
        let color = color.filter(|c| !c.is_none()).map(core_color).transpose()?;
        let bgcolor = bgcolor
            .filter(|c| !c.is_none())
            .map(core_color)
            .transpose()?;
        Ok(Style::from_core(CoreStyle::from_color(color, bgcolor)))
    }

    /// A style carrying only meta data.
    #[classmethod]
    fn from_meta(_cls: &Bound<'_, PyType>, meta: &Bound<'_, PyAny>) -> PyResult<Style> {
        Ok(Style::with_meta(
            CoreStyle::new(),
            Some(dump_meta(meta)?),
            meta.is_truthy()?,
        ))
    }

    /// A blank style with meta data; keyword handlers become `@name` keys.
    #[classmethod]
    #[pyo3(signature = (meta=None, **handlers))]
    fn on(
        cls: &Bound<'_, PyType>,
        meta: Option<&Bound<'_, PyDict>>,
        handlers: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Style> {
        let py = cls.py();
        let meta = match meta {
            Some(meta) => meta.clone(),
            None => PyDict::new(py),
        };
        if let Some(handlers) = handlers {
            for (key, value) in handlers.iter() {
                meta.set_item(format!("@{key}"), value)?;
            }
        }
        Style::from_meta(cls, meta.as_any())
    }

    /// `Style.parse("bold red on white")`.
    #[classmethod]
    fn parse(_cls: &Bound<'_, PyType>, style_definition: &str) -> PyResult<Self> {
        Ok(Style::from_core(parse_style(style_definition)?))
    }

    /// A definition in its canonical form (or trimmed and lower-cased when it
    /// does not parse).
    #[classmethod]
    fn normalize(_cls: &Bound<'_, PyType>, style: &str) -> String {
        CoreStyle::normalize(style)
    }

    /// The first value that is not `None`.
    #[classmethod]
    #[pyo3(signature = (*values))]
    fn pick_first<'py>(
        _cls: &Bound<'py, PyType>,
        values: &Bound<'py, PyTuple>,
    ) -> PyResult<Bound<'py, PyAny>> {
        values
            .iter()
            .find(|value| !value.is_none())
            .ok_or_else(|| PyValueError::new_err("expected at least one non-None style"))
    }

    /// Combine an iterable of styles, later ones winning.
    #[classmethod]
    fn combine(cls: &Bound<'_, PyType>, styles: &Bound<'_, PyAny>) -> PyResult<Style> {
        let py = cls.py();
        let mut styles = styles.try_iter()?;
        let first = styles
            .next()
            .ok_or_else(|| PyStopIteration::new_err(()))??;
        let mut combined = first.cast::<Style>()?.get().clone();
        for style in styles {
            let style = style?;
            combined = combined.add(py, style.cast::<Style>()?.get())?;
        }
        Ok(combined)
    }

    /// Combine styles given as arguments, later ones winning.
    #[classmethod]
    #[pyo3(signature = (*styles))]
    fn chain(cls: &Bound<'_, PyType>, styles: &Bound<'_, PyTuple>) -> PyResult<Style> {
        Style::combine(cls, styles.as_any())
    }

    #[getter]
    fn bold(&self) -> Option<bool> {
        self.inner.attr(0)
    }

    #[getter]
    fn dim(&self) -> Option<bool> {
        self.inner.attr(1)
    }

    #[getter]
    fn italic(&self) -> Option<bool> {
        self.inner.attr(2)
    }

    #[getter]
    fn underline(&self) -> Option<bool> {
        self.inner.attr(3)
    }

    #[getter]
    fn blink(&self) -> Option<bool> {
        self.inner.attr(4)
    }

    #[getter]
    fn blink2(&self) -> Option<bool> {
        self.inner.attr(5)
    }

    #[getter]
    fn reverse(&self) -> Option<bool> {
        self.inner.attr(6)
    }

    #[getter]
    fn conceal(&self) -> Option<bool> {
        self.inner.attr(7)
    }

    #[getter]
    fn strike(&self) -> Option<bool> {
        self.inner.attr(8)
    }

    #[getter]
    fn underline2(&self) -> Option<bool> {
        self.inner.attr(9)
    }

    #[getter]
    fn frame(&self) -> Option<bool> {
        self.inner.attr(10)
    }

    #[getter]
    fn encircle(&self) -> Option<bool> {
        self.inner.attr(11)
    }

    #[getter]
    fn overline(&self) -> Option<bool> {
        self.inner.attr(12)
    }

    /// The foreground `Color`, or `None`.
    #[getter]
    fn color<'py>(&self, py: Python<'py>) -> PyResult<Option<Bound<'py, PyAny>>> {
        self.inner.color().map(|c| py_color(py, c)).transpose()
    }

    /// The background `Color`, or `None`.
    #[getter]
    fn bgcolor<'py>(&self, py: Python<'py>) -> PyResult<Option<Bound<'py, PyAny>>> {
        self.inner.bgcolor().map(|c| py_color(py, c)).transpose()
    }

    #[getter]
    fn link(&self) -> Option<String> {
        self.inner.link().map(str::to_string)
    }

    #[getter]
    fn link_id(&self) -> String {
        self.link_id.clone()
    }

    /// Whether the background is unset or the terminal's default.
    #[getter]
    fn transparent_background(&self) -> bool {
        self.inner.bgcolor().is_none_or(|c| c.is_default())
    }

    /// A style with only this one's background.
    #[getter]
    fn background_style(&self) -> Style {
        Style::from_core(CoreStyle::from_color(None, self.inner.bgcolor().cloned()))
    }

    /// The meta data (a new dict each time; it cannot be changed).
    #[getter]
    fn meta<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        self.meta_dict(py)
    }

    /// A copy without colours (and without meta data).
    #[getter]
    fn without_color(&self) -> Style {
        if self.is_null() {
            return Style::from_core(CoreStyle::new());
        }
        // Rich's copy keeps the link but not the meta data.
        let link = self.inner.link().map(str::to_string);
        Style::from_core(
            self.inner
                .without_color()
                .clear_meta_and_links()
                .update_link(link),
        )
    }

    fn copy(&self) -> Style {
        if self.is_null() {
            return Style::from_core(CoreStyle::new());
        }
        Style::with_meta(self.inner.clone(), self.meta.clone(), self.meta_truthy)
    }

    /// A copy with the link and meta data removed.
    fn clear_meta_and_links(&self) -> Style {
        if self.is_null() {
            return Style::from_core(CoreStyle::new());
        }
        Style::from_core(self.inner.clear_meta_and_links())
    }

    /// A copy with a different link.
    #[pyo3(signature = (link=None))]
    fn update_link(&self, link: Option<String>) -> Style {
        let inner = self.inner.update_link(link.filter(|link| !link.is_empty()));
        Style::with_meta(inner, self.meta.clone(), self.meta_truthy)
    }

    /// The CSS for this style under `theme` (default: the default theme).
    #[pyo3(signature = (theme=None))]
    fn get_html_style(
        &self,
        theme: Option<PyRef<'_, crate::terminal_theme::TerminalTheme>>,
    ) -> String {
        match theme {
            Some(theme) => self.inner.get_html_style(&theme.inner),
            None => self
                .inner
                .get_html_style(&rich::terminal_theme::DEFAULT_TERMINAL_THEME),
        }
    }

    /// `text` wrapped in this style's ANSI codes.
    #[pyo3(signature = (text="", *, color_system=Some(3), legacy_windows=false))]
    fn render(
        &self,
        py: Python<'_>,
        text: &str,
        color_system: Option<i64>,
        legacy_windows: bool,
    ) -> PyResult<String> {
        // `None` means no colour; the default is `ColorSystem.TRUECOLOR` (3).
        let system = match color_system {
            None => None,
            Some(system) => Some(core_system(&system.into_pyobject(py)?.into_any())?),
        };
        let Some(system) = system else {
            return Ok(text.to_string());
        };
        if text.is_empty() {
            return Ok(String::new());
        }
        let codes = if system == rich::ColorSystem::Windows {
            // Core reads Windows as standard colours; Rich has its own palette.
            let mut codes: Vec<String> = Vec::new();
            let attributes = self.inner.without_color().ansi_codes(system);
            if !attributes.is_empty() {
                codes.push(attributes);
            }
            for (color, foreground) in [(self.inner.color(), true), (self.inner.bgcolor(), false)] {
                if let Some(color) = color {
                    codes.extend(crate::color::downgrade(color, system).ansi_codes(foreground));
                }
            }
            codes.join(";")
        } else {
            self.inner.ansi_codes(system)
        };
        let rendered = if codes.is_empty() {
            text.to_string()
        } else {
            format!("\x1b[{codes}m{text}\x1b[0m")
        };
        Ok(match self.inner.link() {
            Some(link) if !legacy_windows => format!(
                "\x1b]8;id={};{link}\x1b\\{rendered}\x1b]8;;\x1b\\",
                self.link_id
            ),
            _ => rendered,
        })
    }

    /// Write `text` (default: the definition) in this style to stdout.
    #[pyo3(signature = (text=None))]
    fn test(&self, py: Python<'_>, text: Option<String>) -> PyResult<()> {
        let text = text
            .filter(|text| !text.is_empty())
            .unwrap_or_else(|| self.__str__());
        let rendered = self.render(py, &text, Some(3), false)?;
        py.import("sys")?
            .getattr("stdout")?
            .call_method1("write", (format!("{rendered}\n"),))?;
        Ok(())
    }

    /// Combine: `other`'s attributes and colours win where it sets them.
    fn __add__(&self, py: Python<'_>, other: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        if other.is_none() {
            return Ok(Py::new(py, self.clone())?.into_any());
        }
        match other.cast::<Style>() {
            Ok(other) => Ok(Py::new(py, self.add(py, other.get())?)?.into_any()),
            Err(_) => Ok(py.NotImplemented()),
        }
    }

    fn __eq__(&self, py: Python<'_>, other: &Bound<'_, PyAny>) -> Py<PyAny> {
        match other.cast::<Style>() {
            Ok(other) => {
                let other = other.get();
                let equal = self.inner == other.inner && self.meta == other.meta;
                pyo3::types::PyBool::new(py, equal)
                    .to_owned()
                    .into_any()
                    .unbind()
            }
            Err(_) => py.NotImplemented(),
        }
    }

    fn __ne__(&self, py: Python<'_>, other: &Bound<'_, PyAny>) -> Py<PyAny> {
        match other.cast::<Style>() {
            Ok(other) => {
                let other = other.get();
                let equal = self.inner == other.inner && self.meta == other.meta;
                pyo3::types::PyBool::new(py, !equal)
                    .to_owned()
                    .into_any()
                    .unbind()
            }
            Err(_) => py.NotImplemented(),
        }
    }

    /// Hashable, as upstream's `Style` is: equal styles hash alike.
    fn __hash__(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        format!("{:?}", self.inner).hash(&mut hasher);
        self.meta.hash(&mut hasher);
        hasher.finish()
    }

    fn __bool__(&self) -> bool {
        !self.is_null()
    }

    fn __str__(&self) -> String {
        self.inner.definition()
    }

    /// Rich's repr: `Style(color=Color(...), bold=True, link='...')`.
    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        let mut parts: Vec<String> = Vec::new();
        if let Some(color) = self.color(py)? {
            parts.push(format!("color={}", color.repr()?));
        }
        if let Some(color) = self.bgcolor(py)? {
            parts.push(format!("bgcolor={}", color.repr()?));
        }
        // Rich's `__rich_repr__` lists every attribute but `overline`.
        for (index, name) in ATTRIBUTES.iter().enumerate().take(12) {
            if let Some(value) = self.inner.attr(index) {
                parts.push(format!("{name}={}", if value { "True" } else { "False" }));
            }
        }
        if let Some(link) = self.inner.link() {
            parts.push(format!("link={}", python_repr(link)));
        }
        if self.meta.is_some() {
            parts.push(format!("meta={}", self.meta_dict(py)?.repr()?));
        }
        Ok(format!("Style({})", parts.join(", ")))
    }
}

/// `rich.style.StyleStack`: a stack of styles, each combined with the one
/// below it.
#[pyclass(name = "StyleStack", module = "rs_rich.style")]
pub(crate) struct StyleStack {
    stack: Vec<Style>,
}

#[pymethods]
impl StyleStack {
    #[new]
    fn new(default_style: PyRef<'_, Style>) -> Self {
        StyleStack {
            stack: vec![default_style.clone()],
        }
    }

    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        let styles: Vec<String> = self
            .stack
            .iter()
            .map(|style| style.__repr__(py))
            .collect::<PyResult<_>>()?;
        Ok(format!("<stylestack [{}]>", styles.join(", ")))
    }

    /// The style at the top of the stack.
    #[getter]
    fn current(&self) -> Style {
        self.stack
            .last()
            .cloned()
            .unwrap_or_else(|| Style::from_core(CoreStyle::new()))
    }

    /// Push `style`, combined with the current style.
    fn push(&mut self, py: Python<'_>, style: PyRef<'_, Style>) -> PyResult<()> {
        let combined = self.current().add(py, &style)?;
        self.stack.push(combined);
        Ok(())
    }

    /// Pop the top style and return the new current style.
    fn pop(&mut self) -> PyResult<Style> {
        self.stack.pop();
        self.stack
            .last()
            .cloned()
            .ok_or_else(|| pyo3::exceptions::PyIndexError::new_err("list index out of range"))
    }
}

/// A `style=` argument: a string (a theme name or a definition) or a `Style`.
pub(crate) fn style_type(value: Option<&Bound<'_, PyAny>>) -> PyResult<Option<StyleType>> {
    let Some(value) = value.filter(|v| !v.is_none()) else {
        return Ok(None);
    };
    if let Ok(style) = value.cast::<Style>() {
        return Ok(Some(StyleType::Style(style.get().inner.clone())));
    }
    if let Ok(name) = value.extract::<String>() {
        return Ok(Some(StyleType::Name(name)));
    }
    Err(PyTypeError::new_err("style must be a str or a Style"))
}

/// A `style=` argument that core wants resolved now (table columns, borders).
pub(crate) fn resolved_style(value: Option<&Bound<'_, PyAny>>) -> PyResult<Option<CoreStyle>> {
    Ok(match style_type(value)? {
        Some(StyleType::Style(style)) => Some(style),
        Some(StyleType::Name(name)) => Some(parse_style(&name)?),
        None => None,
    })
}

/// A Python value for a core `StyleType`: a `str` for a name, a `Style`
/// otherwise.
pub(crate) fn py_style_type(py: Python<'_>, style: &StyleType) -> PyResult<Py<PyAny>> {
    Ok(match style {
        StyleType::Name(name) => pyo3::types::PyString::new(py, name).into_any().unbind(),
        StyleType::Style(style) => Py::new(py, Style::from_core(style.clone()))?.into_any(),
    })
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Style>()?;
    m.add_class::<StyleStack>()
}
