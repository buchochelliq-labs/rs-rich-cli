//! `rich.text`: the `Text` class, with `Span` (see `color.rs`) and `Lines`.
//!
//! Owner: the text/style area (with `style.rs`, `theme.rs` and `color.rs`).
//!
//! A `Text` wraps a core text, which holds the plain string, spans, base
//! style, justify, overflow, no-wrap and tab size; core renders it. Rich's
//! `end` has no place in a core text, so it is a field beside it; other
//! modules build a `Text` with [`Text::from_core`].
//!
//! Offsets are Python character offsets, as in Rich; core spans hold byte
//! offsets, and `ops` converts. Styles on spans are core `StyleType`s: a
//! `str` stays a name (resolved by the console's theme when printed) and a
//! `Style` is kept resolved, with its meta data (core's `Meta`), so
//! `apply_meta`, `on` and `assemble(meta=)` work as in Rich.

mod lines;
pub(crate) mod ops;

use pyo3::exceptions::{PyAssertionError, PyIndexError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList, PySlice, PyString, PyTuple, PyType};

use rich::protocol::Renderable;
use rich::{StyleType, Text as CoreText};

use crate::convert::{self, Index};
use crate::limits::{check_alloc, check_size, MAX_TAB_SIZE, MAX_TEXT_LENGTH};
use crate::renderable::{self, AsRenderable};
use crate::style::{py_style_type, Style};

pub(crate) use lines::Lines;
use ops::{boundaries, byte_at, char_len, char_spans, CharSpan};

/// `rich.text.Text`: a string with styled spans.
#[pyclass(name = "Text", module = "rs_rich.text", skip_from_py_object)]
#[derive(Clone)]
pub(crate) struct Text {
    pub(crate) inner: CoreText,
    /// What ends the text when it renders (Rich's `end`, default `"\n"`).
    pub(crate) end: String,
}

impl Text {
    /// A `Text` holding a core text, ending with `"\n"`.
    pub(crate) fn from_core(inner: CoreText) -> Text {
        Text {
            inner,
            end: "\n".to_string(),
        }
    }
}

/// A base `style=` argument: `""`/`None` is no style, a `str` a name (or
/// definition), a `Style` itself.
pub(crate) fn base_style(value: Option<&Bound<'_, PyAny>>) -> PyResult<StyleType> {
    let Some(value) = value.filter(|v| !v.is_none()) else {
        return Ok(StyleType::default());
    };
    if let Ok(style) = value.cast::<Style>() {
        return Ok(StyleType::Style(style.get().inner.clone()));
    }
    match value.extract::<String>() {
        Ok(name) if name.is_empty() => Ok(StyleType::default()),
        Ok(name) => Ok(StyleType::Name(name)),
        Err(_) => Err(PyTypeError::new_err("style must be a str or a Style")),
    }
}

/// A span's style: a `str` (kept as a name) or a `Style`.
fn span_style(value: &Bound<'_, PyAny>) -> PyResult<StyleType> {
    if let Ok(style) = value.cast::<Style>() {
        return Ok(StyleType::Style(style.get().inner.clone()));
    }
    value
        .extract::<String>()
        .map(StyleType::Name)
        .map_err(|_| PyTypeError::new_err("style must be a str or a Style"))
}

/// The Python value of a text's base style: `""` when there is none.
fn py_base_style(py: Python<'_>, style: &StyleType) -> PyResult<Py<PyAny>> {
    if style.is_null_style() {
        return Ok(PyString::new(py, "").into_any().unbind());
    }
    py_style_type(py, style)
}

/// `(start, end, style)` spans from Python `Span`s (or 3-tuples).
fn spans_from(value: &Bound<'_, PyAny>) -> PyResult<Vec<CharSpan>> {
    let mut spans = Vec::new();
    for item in value.try_iter()? {
        let item = item?;
        let (start, end, style): (Index, Index, Bound<'_, PyAny>) = item.extract()?;
        spans.push((
            start.0.max(0) as usize,
            end.0.max(0) as usize,
            span_style(&style)?,
        ));
    }
    Ok(spans)
}

/// A Python value for an optional `JustifyMethod` / `OverflowMethod`.
fn overflow_arg(value: Option<&str>) -> PyResult<Option<rich::Overflow>> {
    value.map(convert::overflow).transpose()
}

/// A new Python `Text` with `end`.
pub(crate) fn new_text<'py>(
    py: Python<'py>,
    inner: CoreText,
    end: &str,
) -> PyResult<Bound<'py, Text>> {
    Bound::new(
        py,
        Text {
            inner,
            end: end.to_string(),
        },
    )
}

/// The `end` of a Python `Text`.
fn end_of(text: &Bound<'_, Text>) -> PyResult<String> {
    Ok(text.borrow().end.clone())
}

/// A new `Text` carrying `like`'s `end`.
fn like<'py>(like: &Bound<'py, Text>, inner: CoreText) -> PyResult<Bound<'py, Text>> {
    let end = end_of(like)?;
    new_text(like.py(), inner, &end)
}

/// A size a `Text` method pads to or by: Rich builds a string that long, so
/// a huge one is Rich's `MemoryError` (or `OverflowError` past `sys.maxsize`).
fn text_size(what: &str, value: usize) -> PyResult<usize> {
    check_alloc(what, value, MAX_TEXT_LENGTH)
}

/// A length a `Text` method pads *to*: Rich pads by the difference, which
/// is below `sys.maxsize`, so a huge one is only ever its `MemoryError`.
fn pad_size(what: &str, value: usize) -> PyResult<usize> {
    check_size(what, value, MAX_TEXT_LENGTH)
}

/// A tab size argument: `None`, or a positive `int` (at most the widest
/// console: tabs expand to it when the text renders).
fn tab_size_arg(value: Option<i64>) -> PyResult<Option<usize>> {
    match value {
        None => Ok(None),
        Some(size) if size > 0 => Ok(Some(check_alloc("tab_size", size as usize, MAX_TAB_SIZE)?)),
        Some(size) => Err(PyValueError::new_err(format!(
            "tab_size must be a positive int or None, got {size}"
        ))),
    }
}

/// The `Text` in `value` (a `Text`, or any object that is not one).
fn as_text(value: &Bound<'_, PyAny>) -> Option<CoreText> {
    value
        .cast::<Text>()
        .ok()
        .map(|text| text.borrow().inner.clone())
}

/// Rich's `Text.markup`: console markup that renders this text.
fn markup_of(text: &CoreText) -> String {
    let plain = text.plain();
    let base = text.base_style();
    let spans = char_spans(text);
    let length = plain.chars().count();
    let mut events: Vec<(usize, bool, StyleType)> = Vec::with_capacity(spans.len() * 2 + 2);
    events.push((0, false, base.clone()));
    events.extend(
        spans
            .iter()
            .map(|(start, _, style)| (*start, false, style.clone())),
    );
    events.extend(
        spans
            .iter()
            .map(|(_, end, style)| (*end, true, style.clone())),
    );
    events.push((length, true, base.clone()));
    events.sort_by_key(|(offset, closing, _)| (*offset, *closing));
    let bounds = boundaries(plain);
    let mut output = String::new();
    let mut position = 0;
    for (offset, closing, style) in events {
        if offset > position {
            output.push_str(&rich::markup::escape(
                &plain[byte_at(&bounds, position)..byte_at(&bounds, offset)],
            ));
            position = offset;
        }
        let name = match &style {
            StyleType::Name(name) => name.clone(),
            StyleType::Style(style) if style.is_null() => String::new(),
            StyleType::Style(style) => style.definition(),
        };
        if !name.is_empty() {
            if closing {
                output.push_str(&format!("[/{name}]"));
            } else {
                output.push_str(&format!("[{name}]"));
            }
        }
    }
    output
}

impl AsRenderable for Text {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        if self.end == "\n" {
            return Ok(Box::new(self.inner.clone()));
        }
        // Rich yields `end` after the text, inside a container too.
        Ok(Box::new(crate::renderable::TextWithEnd {
            text: self.inner.clone(),
            end: self.end.clone(),
        }))
    }
}

/// Check a pad character, as Rich asserts.
fn pad_character(character: &str) -> PyResult<char> {
    let mut chars = character.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => Ok(c),
        _ => Err(PyAssertionError::new_err(
            "Character must be a string of length 1",
        )),
    }
}

/// A non-negative count (a negative one does nothing, as `" " * -1`).
fn count(value: Index) -> usize {
    value.0.max(0) as usize
}

#[pymethods]
impl Text {
    #[new]
    #[pyo3(signature = (
        text="", style=None, *, justify=None, overflow=None, no_wrap=None, end="\n",
        tab_size=None, spans=None
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        py: Python<'_>,
        text: &str,
        style: Option<&Bound<'_, PyAny>>,
        justify: Option<&str>,
        overflow: Option<&str>,
        no_wrap: Option<bool>,
        end: &str,
        tab_size: Option<i64>,
        spans: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Py<Text>> {
        let mut inner = CoreText::styled(text, base_style(style)?);
        inner.set_tab_size(tab_size_arg(tab_size)?);
        inner.set_justify(convert::justify(justify)?);
        inner.set_overflow(overflow_arg(overflow)?);
        inner.set_no_wrap(no_wrap);
        if let Some(spans) = spans.filter(|s| !s.is_none()) {
            let spans = spans_from(spans)?;
            let plain = inner.plain().to_string();
            inner = ops::rebuild(&inner, &plain, &spans);
        }
        Ok(new_text(py, inner, end)?.unbind())
    }

    /// `Text.from_markup("[bold]hi[/]")`.
    #[classmethod]
    #[pyo3(signature = (
        text, *, style=None, emoji=true, emoji_variant=None, justify=None, overflow=None,
        end="\n"
    ))]
    #[allow(clippy::too_many_arguments)]
    fn from_markup(
        cls: &Bound<'_, PyType>,
        text: &str,
        style: Option<&Bound<'_, PyAny>>,
        emoji: bool,
        emoji_variant: Option<&str>,
        justify: Option<&str>,
        overflow: Option<&str>,
        end: &str,
    ) -> PyResult<Py<Text>> {
        let mut inner = crate::color::markup::render(text, emoji, emoji_variant)?;
        inner.set_base_style(base_style(style)?);
        inner.set_justify(convert::justify(justify)?);
        inner.set_overflow(overflow_arg(overflow)?);
        Ok(new_text(cls.py(), inner, end)?.unbind())
    }

    /// A `Text` from a string with ANSI escape codes.
    #[classmethod]
    #[pyo3(signature = (
        text, *, style=None, justify=None, overflow=None, no_wrap=None, end="\n",
        tab_size=Some(8)
    ))]
    #[allow(clippy::too_many_arguments)]
    fn from_ansi(
        cls: &Bound<'_, PyType>,
        text: &str,
        style: Option<&Bound<'_, PyAny>>,
        justify: Option<&str>,
        overflow: Option<&str>,
        no_wrap: Option<bool>,
        end: &str,
        tab_size: Option<i64>,
    ) -> PyResult<Py<Text>> {
        let py = cls.py();
        let mut inner = CoreText::from_ansi(text, base_style(style)?);
        inner.set_justify(convert::justify(justify)?);
        inner.set_overflow(overflow_arg(overflow)?);
        inner.set_no_wrap(no_wrap);
        inner.set_tab_size(tab_size_arg(tab_size)?);
        Ok(new_text(py, inner, end)?.unbind())
    }

    /// A `Text` with `style` applied to the whole string as a span.
    #[classmethod]
    #[pyo3(signature = (text, style=None, *, justify=None, overflow=None))]
    fn styled(
        cls: &Bound<'_, PyType>,
        text: &str,
        style: Option<&Bound<'_, PyAny>>,
        justify: Option<&str>,
        overflow: Option<&str>,
    ) -> PyResult<Py<Text>> {
        let mut inner = CoreText::new(text);
        inner.set_justify(convert::justify(justify)?);
        inner.set_overflow(overflow_arg(overflow)?);
        if let Some(style) = style.filter(|s| s.is_truthy().unwrap_or(false)) {
            ops::stylize(&mut inner, span_style(style)?, 0, None);
        }
        Ok(new_text(cls.py(), inner, "\n")?.unbind())
    }

    /// A `Text` from strings, `Text`s and `(str, style)` tuples.
    #[classmethod]
    #[pyo3(signature = (
        *parts, style=None, justify=None, overflow=None, no_wrap=None, end="\n",
        tab_size=Some(8), meta=None
    ))]
    #[allow(clippy::too_many_arguments)]
    fn assemble<'py>(
        cls: &Bound<'py, PyType>,
        parts: &Bound<'py, PyTuple>,
        style: Option<&Bound<'py, PyAny>>,
        justify: Option<&str>,
        overflow: Option<&str>,
        no_wrap: Option<bool>,
        end: &str,
        tab_size: Option<i64>,
        meta: Option<&Bound<'py, PyAny>>,
    ) -> PyResult<Bound<'py, Text>> {
        let py = cls.py();

        let mut inner = CoreText::styled("", base_style(style)?);
        inner.set_justify(convert::justify(justify)?);
        inner.set_overflow(overflow_arg(overflow)?);
        inner.set_no_wrap(no_wrap);
        inner.set_tab_size(tab_size_arg(tab_size)?);
        let text = new_text(py, inner, end)?;
        for part in parts.iter() {
            if part.cast::<Text>().is_ok() || part.cast::<PyString>().is_ok() {
                Text::append(text.borrow_mut(), &part, None)?;
            } else {
                let tuple = part.cast::<PyTuple>()?;
                let style = if tuple.len() > 1 {
                    Some(tuple.get_item(1)?)
                } else {
                    None
                };
                Text::append(text.borrow_mut(), &tuple.get_item(0)?, style.as_ref())?;
            }
        }
        if let Some(meta) = meta.filter(|m| !m.is_none()) {
            if meta.is_truthy()? {
                Text::apply_meta(&text, meta, Index(0), None)?;
            }
        }
        Ok(text)
    }

    #[getter]
    fn get_plain(&self) -> String {
        self.inner.plain().to_string()
    }

    #[setter(plain)]
    fn set_plain(&mut self, plain: &str) {
        ops::set_plain(&mut self.inner, plain);
    }

    /// The spans, as a new list of `Span`s (assign to change them).
    #[getter]
    fn get_spans<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyList>> {
        let class = crate::color::span_class(py);
        let spans = char_spans(&self.inner)
            .into_iter()
            .map(|(start, end, style)| class.call1((start, end, py_style_type(py, &style)?)))
            .collect::<PyResult<Vec<_>>>()?;
        PyList::new(py, spans)
    }

    #[setter(spans)]
    fn set_spans(&mut self, spans: &Bound<'_, PyAny>) -> PyResult<()> {
        let spans = spans_from(spans)?;
        let plain = self.inner.plain().to_string();
        self.inner = ops::rebuild(&self.inner, &plain, &spans);
        Ok(())
    }

    /// The base style: `""`, a style name or definition, or a `Style`.
    #[getter]
    fn get_style(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        py_base_style(py, self.inner.base_style())
    }

    #[setter(style)]
    fn set_style(&mut self, style: Option<&Bound<'_, PyAny>>) -> PyResult<()> {
        self.inner.set_base_style(base_style(style)?);
        Ok(())
    }

    #[getter]
    fn get_justify(&self) -> Option<&'static str> {
        convert::justify_name(self.inner.get_justify())
    }

    #[setter(justify)]
    fn set_justify(&mut self, justify: Option<&str>) -> PyResult<()> {
        self.inner.set_justify(convert::justify(justify)?);
        Ok(())
    }

    #[getter]
    fn get_overflow(&self) -> Option<&'static str> {
        self.inner.get_overflow().map(convert::overflow_name)
    }

    #[setter(overflow)]
    fn set_overflow(&mut self, overflow: Option<&str>) -> PyResult<()> {
        self.inner.set_overflow(overflow_arg(overflow)?);
        Ok(())
    }

    #[getter]
    fn get_no_wrap(&self) -> Option<bool> {
        self.inner.get_no_wrap()
    }

    #[setter(no_wrap)]
    fn set_no_wrap(&mut self, no_wrap: Option<bool>) {
        self.inner.set_no_wrap(no_wrap);
    }

    /// What ends the text when it renders (default `"\n"`).
    #[getter]
    fn get_end(&self) -> String {
        self.end.clone()
    }

    #[setter(end)]
    fn set_end(&mut self, end: String) {
        self.end = end;
    }

    /// Spaces per tab, or `None` for the console's.
    #[getter]
    fn get_tab_size(&self) -> Option<usize> {
        self.inner.get_tab_size()
    }

    #[setter(tab_size)]
    fn set_tab_size(&mut self, tab_size: Option<i64>) -> PyResult<()> {
        self.inner.set_tab_size(tab_size_arg(tab_size)?);
        Ok(())
    }

    /// The cells the text takes in a terminal.
    #[getter]
    fn cell_len(&self) -> usize {
        self.inner.cell_len()
    }

    /// Console markup that renders this text.
    #[getter]
    fn markup(&self) -> String {
        markup_of(&self.inner)
    }

    /// A new, empty `Text` with this one's settings (and `plain`).
    #[pyo3(signature = (plain=""))]
    fn blank_copy<'py>(slf: &Bound<'py, Self>, plain: &str) -> PyResult<Bound<'py, Text>> {
        let mut inner = slf.borrow().inner.blank_copy();
        inner.append(plain, None);
        like(slf, inner)
    }

    fn copy<'py>(slf: &Bound<'py, Self>) -> PyResult<Bound<'py, Text>> {
        let inner = slf.borrow().inner.clone();
        like(slf, inner)
    }

    /// Style characters `start..end` (negative from the end); offsets past
    /// the text are clamped.
    #[pyo3(signature = (style, start=Index(0), end=None))]
    fn stylize(
        &mut self,
        style: &Bound<'_, PyAny>,
        start: Index,
        end: Option<Index>,
    ) -> PyResult<()> {
        if !style.is_truthy()? {
            return Ok(());
        }
        let style = span_style(style)?;
        ops::stylize(&mut self.inner, style, start.0, end.map(|end| end.0));
        Ok(())
    }

    /// As `stylize`, but under the styles already applied.
    #[pyo3(signature = (style, start=Index(0), end=None))]
    fn stylize_before(
        &mut self,
        style: &Bound<'_, PyAny>,
        start: Index,
        end: Option<Index>,
    ) -> PyResult<()> {
        if !style.is_truthy()? {
            return Ok(());
        }
        let mut first = self.inner.blank_copy();
        first.append(self.inner.plain(), None);
        ops::stylize(
            &mut first,
            span_style(style)?,
            start.0,
            end.map(|end| end.0),
        );
        let mut spans = char_spans(&first);
        spans.extend(char_spans(&self.inner));
        let plain = self.inner.plain().to_string();
        self.inner = ops::rebuild(&self.inner, &plain, &spans);
        Ok(())
    }

    /// Apply meta data to a range: a span of `Style.from_meta(meta)`.
    #[pyo3(signature = (meta, start=Index(0), end=None))]
    fn apply_meta(
        slf: &Bound<'_, Self>,
        meta: &Bound<'_, PyAny>,
        start: Index,
        end: Option<Index>,
    ) -> PyResult<()> {
        let style = meta_style(meta)?;
        slf.call_method1("stylize", (style, start.0, end.map(|end| end.0)))?;
        Ok(())
    }

    /// Apply event handlers (Textual's meta data, as `@name` keys) to the
    /// whole text. Returns the text.
    #[pyo3(signature = (meta=None, **handlers))]
    fn on<'py>(
        slf: &Bound<'py, Self>,
        meta: Option<&Bound<'py, PyAny>>,
        handlers: Option<&Bound<'py, PyDict>>,
    ) -> PyResult<Bound<'py, Self>> {
        let py = slf.py();
        let merged = match meta.filter(|m| !m.is_none()) {
            Some(meta) => meta.cast::<PyDict>()?.clone(),
            None => PyDict::new(py),
        };
        if let Some(handlers) = handlers {
            for (key, value) in handlers.iter() {
                merged.set_item(format!("@{key}"), value)?;
            }
        }
        let style = meta_style(merged.as_any())?;
        slf.call_method1("stylize", (style,))?;
        Ok(slf.clone())
    }

    fn remove_suffix(&mut self, suffix: &str) {
        if self.inner.plain().ends_with(suffix) {
            ops::right_crop(&mut self.inner, suffix.chars().count());
        }
    }

    /// Remove characters from the end.
    #[pyo3(signature = (amount=Index(1)))]
    fn right_crop(&mut self, amount: Index) {
        ops::right_crop(&mut self.inner, count(amount));
    }

    /// The style of the character at `offset`, resolved by `console`.
    fn get_style_at_offset(&self, console: &Bound<'_, PyAny>, offset: Index) -> PyResult<Style> {
        Ok(Style::from_core(ops::style_at_offset(
            console,
            &self.inner,
            offset.0,
        )?))
    }

    /// Append spaces in the style of the spans that reach the end.
    fn extend_style(&mut self, spaces: Index) -> PyResult<()> {
        let spaces = text_size("spaces", count(spaces))?;
        ops::extend_style(&mut self.inner, spaces);
        Ok(())
    }

    /// Style the matches of a regular expression (Python's `re`), and each
    /// named group with `style_prefix + name`. Returns the match count.
    #[pyo3(signature = (re_highlight, style=None, *, style_prefix=""))]
    fn highlight_regex(
        &mut self,
        py: Python<'_>,
        re_highlight: &Bound<'_, PyAny>,
        style: Option<&Bound<'_, PyAny>>,
        style_prefix: &str,
    ) -> PyResult<usize> {
        let pattern = if re_highlight.is_instance_of::<PyString>() {
            py.import("re")?.call_method1("compile", (re_highlight,))?
        } else {
            re_highlight.clone()
        };
        let plain = self.inner.plain().to_string();
        let style = style.filter(|s| s.is_truthy().unwrap_or(false));
        let mut spans: Vec<CharSpan> = Vec::new();
        let mut matches = 0;
        for found in pattern
            .call_method1("finditer", (plain.as_str(),))?
            .try_iter()?
        {
            let found = found?;
            if let Some(style) = style {
                let (start, end): (usize, usize) = found.call_method0("span")?.extract()?;
                let match_style = if style.is_callable() {
                    let matched = found.call_method1("group", (0,))?;
                    style.call1((matched,))?
                } else {
                    style.clone()
                };
                if !match_style.is_none() && end > start {
                    spans.push((start, end, span_style(&match_style)?));
                }
            }
            matches += 1;
            let names = found.call_method0("groupdict")?;
            for name in names.cast::<PyDict>()?.keys() {
                let (start, end): (isize, isize) =
                    found.call_method1("span", (&name,))?.extract()?;
                if start != -1 && end > start {
                    spans.push((
                        start as usize,
                        end as usize,
                        StyleType::Name(format!("{style_prefix}{name}")),
                    ));
                }
            }
        }
        self.push_spans(&spans);
        Ok(matches)
    }

    /// Style every occurrence of the given words.
    #[pyo3(signature = (words, style, *, case_sensitive=true))]
    fn highlight_words(
        &mut self,
        py: Python<'_>,
        words: &Bound<'_, PyAny>,
        style: &Bound<'_, PyAny>,
        case_sensitive: bool,
    ) -> PyResult<usize> {
        let re = py.import("re")?;
        let escaped = words
            .try_iter()?
            .map(|word| re.call_method1("escape", (word?,))?.extract::<String>())
            .collect::<PyResult<Vec<_>>>()?;
        let flags = if case_sensitive {
            0
        } else {
            re.getattr("IGNORECASE")?.extract::<i64>()?
        };
        let style = span_style(style)?;
        let plain = self.inner.plain().to_string();
        let mut spans: Vec<CharSpan> = Vec::new();
        for found in re
            .call_method1("finditer", (escaped.join("|"), plain.as_str(), flags))?
            .try_iter()?
        {
            let (start, end): (usize, usize) = found?.call_method1("span", (0,))?.extract()?;
            spans.push((start, end, style.clone()));
        }
        self.push_spans(&spans);
        Ok(spans.len())
    }

    fn rstrip(&mut self) {
        ops::rstrip(&mut self.inner);
    }

    /// Remove trailing whitespace beyond `size` characters.
    fn rstrip_end(&mut self, size: Index) {
        ops::rstrip_end(&mut self.inner, count(size));
    }

    /// Pad with spaces or crop to `new_length` characters.
    fn set_length(&mut self, new_length: Index) -> PyResult<()> {
        let new_length = pad_size("new_length", count(new_length))?;
        ops::set_length(&mut self.inner, new_length);
        Ok(())
    }

    /// Render through the console, as `Console.render(text, options)`.
    fn __rich_console__<'py>(
        slf: &Bound<'py, Self>,
        console: &Bound<'py, PyAny>,
        options: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        console.call_method1("render", (slf, options))
    }

    /// `(widest word, widest line)` in cells.
    fn __rich_measure__(
        &self,
        console: &Bound<'_, PyAny>,
        options: &Bound<'_, PyAny>,
    ) -> crate::protocol::Measurement {
        let _ = (console, options);
        let (minimum, maximum) = self.inner.measurement();
        crate::protocol::Measurement::from_core(rich::measure::Measurement::new(minimum, maximum))
    }

    /// The text as `Segment`s (no wrapping), styles resolved by `console`,
    /// then `end` if it is not empty.
    #[pyo3(signature = (console, end=""))]
    fn render<'py>(&self, console: &Bound<'py, PyAny>, end: &str) -> PyResult<Bound<'py, PyList>> {
        let py = console.py();
        let plain = self.inner.plain();
        let mut segments: Vec<rich::segment::Segment> = Vec::new();
        let spans = self.inner.spans();
        if spans.is_empty() {
            segments.push(rich::segment::Segment::new(plain, None));
        } else {
            let null = Style::from_core(rich::Style::new());
            let get_style = |style: &StyleType| -> PyResult<rich::Style> {
                let kwargs = PyDict::new(py);
                kwargs.set_item("default", null.clone())?;
                let resolved = console.call_method(
                    "get_style",
                    (py_style_type(py, style)?,),
                    Some(&kwargs),
                )?;
                Ok(resolved.cast::<Style>()?.get().inner.clone())
            };
            let mut style_map = vec![get_style(self.inner.base_style())?];
            for span in spans {
                style_map.push(get_style(&span.style)?);
            }
            let mut events: Vec<(usize, bool, usize)> = vec![(0, false, 0)];
            events.extend(
                spans
                    .iter()
                    .enumerate()
                    .map(|(i, s)| (s.start, false, i + 1)),
            );
            events.extend(spans.iter().enumerate().map(|(i, s)| (s.end, true, i + 1)));
            events.push((plain.len(), true, 0));
            events.sort_by_key(|(offset, leaving, _)| (*offset, *leaving));
            let mut stack: Vec<usize> = Vec::new();
            for window in events.windows(2) {
                let ((offset, leaving, id), (next_offset, _, _)) = (window[0], window[1]);
                if leaving {
                    if let Some(position) = stack.iter().position(|&s| s == id) {
                        stack.remove(position);
                    }
                } else {
                    stack.push(id);
                }
                if next_offset > offset {
                    let mut ids = stack.clone();
                    ids.sort_unstable();
                    let mut style = rich::Style::new();
                    for id in ids {
                        style = style.combine(&style_map[id]);
                    }
                    segments.push(rich::segment::Segment::new(
                        &plain[offset..next_offset],
                        Some(style),
                    ));
                }
            }
        }
        if !end.is_empty() {
            segments.push(rich::segment::Segment::new(end, None));
        }
        crate::segment::to_python(py, &segments)
    }

    /// Join `lines` with this text between them.
    fn join<'py>(slf: &Bound<'py, Self>, lines: &Bound<'py, PyAny>) -> PyResult<Bound<'py, Text>> {
        let lines = lines
            .try_iter()?
            .map(|line| {
                let line = line?;
                as_text(&line).ok_or_else(|| PyTypeError::new_err("can only join Text instances"))
            })
            .collect::<PyResult<Vec<_>>>()?;
        let joined = slf.borrow().inner.join(&lines);
        like(slf, joined)
    }

    /// Replace tabs with spaces (`tab_size` defaults to the text's, else 8).
    #[pyo3(signature = (tab_size=None))]
    fn expand_tabs(slf: &Bound<'_, Self>, tab_size: Option<Index>) -> PyResult<()> {
        let tab_size = match tab_size {
            Some(size) => size.0,
            None => slf.borrow().inner.get_tab_size().unwrap_or(8) as isize,
        };
        if !slf.borrow().inner.plain().contains('\t') {
            return Ok(());
        }
        if tab_size <= 0 {
            return Err(pyo3::exceptions::PyZeroDivisionError::new_err(
                "integer modulo by zero",
            ));
        }
        let tab_size = check_alloc("tab_size", tab_size as usize, MAX_TAB_SIZE)?;
        slf.borrow_mut().inner.expand_tabs(tab_size);
        Ok(())
    }

    /// Cut to `max_width` cells, per `overflow` (default: the text's).
    #[pyo3(signature = (max_width, *, overflow=None, pad=false))]
    fn truncate(&mut self, max_width: Index, overflow: Option<&str>, pad: bool) -> PyResult<()> {
        let overflow = overflow_arg(overflow)?;
        let max_width = count(max_width);
        if pad {
            pad_size("max_width", max_width)?;
        }
        self.inner.truncate(max_width, overflow, pad);
        Ok(())
    }

    #[pyo3(signature = (count, character=" "))]
    fn pad(&mut self, count: Index, character: &str) -> PyResult<()> {
        let character = pad_character(character)?;
        let count = text_size("count", self::count(count))?;
        self.inner.pad(count, character);
        Ok(())
    }

    #[pyo3(signature = (count, character=" "))]
    fn pad_left(&mut self, count: Index, character: &str) -> PyResult<()> {
        let character = pad_character(character)?;
        let count = text_size("count", self::count(count))?;
        self.inner.pad_left(count, character);
        Ok(())
    }

    #[pyo3(signature = (count, character=" "))]
    fn pad_right(&mut self, count: Index, character: &str) -> PyResult<()> {
        let character = pad_character(character)?;
        let count = text_size("count", self::count(count))?;
        self.inner.pad_right(count, character);
        Ok(())
    }

    /// Truncate and pad to `width` cells, aligned.
    #[pyo3(signature = (align, width, character=" "))]
    fn align(&mut self, align: &str, width: Index, character: &str) -> PyResult<()> {
        let character = pad_character(character)?;
        let width = pad_size("width", count(width))?;
        self.inner.truncate(width, None, false);
        let excess = width.saturating_sub(self.inner.cell_len());
        if excess > 0 {
            match align {
                "left" => self.inner.pad_right(excess, character),
                "center" => {
                    let left = excess / 2;
                    self.inner.pad_left(left, character);
                    self.inner.pad_right(excess - left, character);
                }
                _ => self.inner.pad_left(excess, character),
            }
        }
        Ok(())
    }

    /// Append a string (with an optional style) or another `Text`.
    #[pyo3(signature = (text, style=None))]
    fn append<'py>(
        mut slf: PyRefMut<'py, Self>,
        text: &Bound<'py, PyAny>,
        style: Option<&Bound<'py, PyAny>>,
    ) -> PyResult<PyRefMut<'py, Self>> {
        let style = style.filter(|s| !s.is_none());
        // `t.append(t)`: `t` is already borrowed mutably here, so it cannot
        // be extracted again; append a copy of itself, as upstream does.
        let other = if text.as_ptr() == slf.as_ptr() {
            Some(slf.inner.clone())
        } else if let Ok(other) = text.cast::<Text>() {
            Some(other.borrow().inner.clone())
        } else {
            None
        };
        if let Some(other) = other {
            if style.is_some() {
                return Err(PyValueError::new_err(
                    "style must not be set when appending Text instance",
                ));
            }
            if !other.plain().is_empty() {
                let joined = slf.inner.clone().append_text(&other);
                slf.inner = joined;
            }
        } else if let Ok(string) = text.extract::<String>() {
            if !string.is_empty() {
                let style = match style {
                    Some(style) if style.is_truthy()? => Some(span_style(style)?),
                    _ => None,
                };
                slf.inner.append(&string, style);
            }
        } else {
            return Err(PyTypeError::new_err(
                "Only str or Text can be appended to Text",
            ));
        }
        Ok(slf)
    }

    /// Append another `Text`.
    fn append_text<'py>(
        mut slf: PyRefMut<'py, Self>,
        text: &Bound<'py, PyAny>,
    ) -> PyResult<PyRefMut<'py, Self>> {
        let other = if text.as_ptr() == slf.as_ptr() {
            slf.inner.clone()
        } else {
            as_text(text).ok_or_else(|| PyTypeError::new_err("append_text takes a Text"))?
        };
        let joined = slf.inner.clone().append_text(&other);
        slf.inner = joined;
        Ok(slf)
    }

    /// Append `(content, style)` pairs.
    fn append_tokens<'py>(
        mut slf: PyRefMut<'py, Self>,
        tokens: &Bound<'py, PyAny>,
    ) -> PyResult<PyRefMut<'py, Self>> {
        for token in tokens.try_iter()? {
            let (content, style): (String, Option<Bound<'py, PyAny>>) = token?.extract()?;
            let style = match style {
                Some(style) if style.is_truthy()? => Some(span_style(&style)?),
                _ => None,
            };
            slf.inner.append(&content, style);
        }
        Ok(slf)
    }

    /// Add another text's spans (at the same offsets) to this one.
    fn copy_styles(&mut self, text: &Bound<'_, PyAny>) -> PyResult<()> {
        let other =
            as_text(text).ok_or_else(|| PyTypeError::new_err("copy_styles takes a Text"))?;
        self.push_spans(&char_spans(&other));
        Ok(())
    }

    /// Split into lines at `separator`.
    #[pyo3(signature = (separator="\n", *, include_separator=false, allow_blank=false))]
    fn split<'py>(
        slf: &Bound<'py, Self>,
        separator: &str,
        include_separator: bool,
        allow_blank: bool,
    ) -> PyResult<Lines> {
        if separator.is_empty() {
            return Err(PyAssertionError::new_err("separator must not be empty"));
        }
        let inner = slf.borrow().inner.clone();
        if !inner.plain().contains(separator) {
            return Lines::from_objects(vec![Text::copy(slf)?.unbind()]);
        }
        Lines::from_core(
            slf.py(),
            ops::split(&inner, separator, include_separator, allow_blank),
        )
    }

    /// Divide into lines at character `offsets`.
    fn divide<'py>(slf: &Bound<'py, Self>, offsets: &Bound<'py, PyAny>) -> PyResult<Lines> {
        let offsets = offsets
            .try_iter()?
            .map(|offset| Ok(offset?.extract::<Index>()?.0))
            .collect::<PyResult<Vec<isize>>>()?;
        if offsets.is_empty() {
            return Lines::from_objects(vec![Text::copy(slf)?.unbind()]);
        }
        let inner = slf.borrow().inner.clone();
        Lines::from_core(slf.py(), ops::divide(&inner, &offsets))
    }

    /// Word-wrap to `width` cells.
    #[pyo3(signature = (console, width, *, justify=None, overflow=None, tab_size=Index(8), no_wrap=None))]
    #[allow(clippy::too_many_arguments)]
    fn wrap(
        &self,
        py: Python<'_>,
        console: &Bound<'_, PyAny>,
        width: Index,
        justify: Option<&str>,
        overflow: Option<&str>,
        tab_size: Index,
        no_wrap: Option<bool>,
    ) -> PyResult<Lines> {
        let lines = ops::wrap(
            console,
            &self.inner,
            count(width),
            ops::justify_arg(justify)?,
            overflow_arg(overflow)?,
            count(tab_size).max(1),
            no_wrap,
        )?;
        Lines::from_core(py, lines)
    }

    /// Split into lines, each padded or cropped to `width` characters.
    fn fit(&self, py: Python<'_>, width: Index) -> PyResult<Lines> {
        let width = pad_size("width", count(width))?;
        let mut lines = ops::split(&self.inner, "\n", false, false);
        for line in &mut lines {
            ops::set_length(line, width);
        }
        Lines::from_core(py, lines)
    }

    /// The indentation step of code (the gcd of the even indents, or 1).
    fn detect_indentation(&self) -> usize {
        detect_indentation(self.inner.plain())
    }

    /// A copy with indent guide characters in the indentation.
    #[pyo3(signature = (indent_size=None, *, character="│", style=None))]
    fn with_indent_guides<'py>(
        slf: &Bound<'py, Self>,
        indent_size: Option<Index>,
        character: &str,
        style: Option<&Bound<'py, PyAny>>,
    ) -> PyResult<Bound<'py, Text>> {
        let py = slf.py();
        let style = match style {
            Some(style) => span_style(style)?,
            None => StyleType::Name("dim green".to_string()),
        };
        let size = match indent_size {
            Some(size) => size.0,
            None => detect_indentation(slf.borrow().inner.plain()) as isize,
        };
        if size <= 0 {
            return Err(pyo3::exceptions::PyZeroDivisionError::new_err(
                "integer division or modulo by zero",
            ));
        }
        // Rich builds an indent guide `indent_size` wide.
        check_alloc("indent_size", size as usize, MAX_TEXT_LENGTH)?;
        let inner = slf
            .borrow()
            .inner
            .with_indent_guides(Some(size as usize), character, style);
        let _ = py;
        like(slf, inner)
    }

    fn __len__(&self) -> usize {
        char_len(&self.inner)
    }

    fn __bool__(&self) -> bool {
        !self.inner.plain().is_empty()
    }

    fn __str__(&self) -> String {
        self.inner.plain().to_string()
    }

    /// Rich's `<text 'plain' [spans] 'style'>`.
    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        let plain = PyString::new(py, self.inner.plain()).repr()?;
        let spans = self.get_spans(py)?.repr()?;
        let style = py_base_style(py, self.inner.base_style())?
            .bind(py)
            .repr()?;
        Ok(format!("<text {plain} {spans} {style}>"))
    }

    fn __add__<'py>(slf: &Bound<'py, Self>, other: &Bound<'py, PyAny>) -> PyResult<Py<PyAny>> {
        let py = slf.py();
        if other.cast::<Text>().is_err() && !other.is_instance_of::<PyString>() {
            return Ok(py.NotImplemented());
        }
        let result = Text::copy(slf)?;
        Text::append(result.borrow_mut(), other, None)?;
        Ok(result.into_any().unbind())
    }

    fn __eq__(&self, py: Python<'_>, other: &Bound<'_, PyAny>) -> Py<PyAny> {
        match as_text(other) {
            Some(other) => {
                let equal =
                    self.inner.plain() == other.plain() && self.inner.spans() == other.spans();
                pyo3::types::PyBool::new(py, equal)
                    .to_owned()
                    .into_any()
                    .unbind()
            }
            None => py.NotImplemented(),
        }
    }

    fn __contains__(&self, other: &Bound<'_, PyAny>) -> bool {
        if let Ok(string) = other.extract::<String>() {
            return self.inner.plain().contains(&string);
        }
        match as_text(other) {
            Some(text) => self.inner.plain().contains(text.plain()),
            None => false,
        }
    }

    /// One character (with the styles over it) or a slice (step 1 only).
    fn __getitem__<'py>(
        slf: &Bound<'py, Self>,
        index: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, Text>> {
        let py = slf.py();
        let inner = slf.borrow().inner.clone();
        let length = char_len(&inner) as isize;
        if let Ok(slice) = index.cast::<PySlice>() {
            let indices = slice.indices(length)?;
            if indices.step != 1 {
                return Err(PyTypeError::new_err(
                    "slices with step!=1 are not supported",
                ));
            }
            let lines = ops::divide(&inner, &[indices.start, indices.stop]);
            let line = lines
                .into_iter()
                .nth(1)
                .unwrap_or_else(|| inner.blank_copy());
            return new_text(py, line, "\n");
        }
        let offset = index.extract::<isize>()?;
        let position = if offset < 0 { length + offset } else { offset };
        if position < 0 || position >= length {
            return Err(PyIndexError::new_err("string index out of range"));
        }
        let character: String = inner
            .plain()
            .chars()
            .nth(position as usize)
            .into_iter()
            .collect();
        let spans: Vec<CharSpan> = char_spans(&inner)
            .into_iter()
            .filter(|(start, end, _)| (*end as isize) > offset && offset >= *start as isize)
            .map(|(_, _, style)| (0, 1, style))
            .collect();
        let text = ops::rebuild(&CoreText::new(""), &character, &spans);
        new_text(py, text, "")
    }
}

impl Text {
    /// Add spans given in character offsets, in order.
    fn push_spans(&mut self, spans: &[CharSpan]) {
        let bounds = boundaries(self.inner.plain());
        for (start, end, style) in spans {
            self.inner.stylize(
                style.clone(),
                byte_at(&bounds, *start),
                byte_at(&bounds, *end),
            );
        }
    }
}

/// `Style.from_meta(meta)`, checked to be meta data a span can carry.
fn meta_style<'py>(meta: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyAny>> {
    crate::style::core_meta(meta)?;
    let py = meta.py();
    py.get_type::<Style>().call_method1("from_meta", (meta,))
}

/// Rich's `Text.detect_indentation`.
fn detect_indentation(plain: &str) -> usize {
    let mut indentations: Vec<usize> = plain
        .split('\n')
        .map(|line| line.chars().take_while(|c| *c == ' ').count())
        .collect();
    indentations.sort_unstable();
    indentations.dedup();
    let even: Vec<usize> = indentations.into_iter().filter(|i| i % 2 == 0).collect();
    if even.is_empty() {
        return 1;
    }
    let gcd = even.into_iter().fold(0, |a, b| {
        let (mut a, mut b) = (a, b);
        while b != 0 {
            (a, b) = (b, a % b);
        }
        a
    });
    if gcd == 0 {
        1
    } else {
        gcd
    }
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    renderable::add_renderable_class::<Text>(m)?;
    lines::register(m)
}
