//! `rich.panel`: the `Panel` class.
//!
//! Owner: the foundation.

use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyString, PyTuple};
use pyo3::{PyTraverseError, PyVisit};

use rich::align::HorizontalAlign;
use rich::protocol::Renderable;
use rich::r#box::Box as CoreBox;
use rich::style::StyleType;
use rich::{Panel as CorePanel, Style as CoreStyle};

use crate::boxes::BoxArg;
use crate::convert;
use crate::limits::{check_size, MAX_CONSOLE_HEIGHT, MAX_CONSOLE_WIDTH};
use crate::renderable::{self, AsRenderable};
use crate::style::{py_style_type, resolved_style, style_type};
use crate::text::Text;

/// A panel title: console markup, or a `Text`.
#[derive(Clone)]
enum Title {
    Markup(String),
    Text(rich::Text),
}

fn title_arg(value: Option<&Bound<'_, PyAny>>) -> PyResult<Option<Title>> {
    let Some(value) = value.filter(|v| !v.is_none()) else {
        return Ok(None);
    };
    if let Ok(text) = value.extract::<PyRef<'_, Text>>() {
        return Ok(Some(Title::Text(text.inner.clone())));
    }
    match value.extract::<String>() {
        Ok(markup) => Ok(Some(Title::Markup(markup))),
        Err(_) => Err(PyTypeError::new_err(
            "a title or subtitle must be a str or a Text",
        )),
    }
}

/// A title as Rich holds it: the `str`, or a copy of the `Text`.
fn title_value(py: Python<'_>, title: Option<&Title>) -> PyResult<Option<Py<PyAny>>> {
    Ok(match title {
        None => None,
        Some(Title::Markup(markup)) => Some(PyString::new(py, markup).into_any().unbind()),
        Some(Title::Text(text)) => Some(Py::new(py, Text::from_core(text.clone()))?.into_any()),
    })
}

fn align_name(align: HorizontalAlign) -> &'static str {
    match align {
        HorizontalAlign::Left => "left",
        HorizontalAlign::Center => "center",
        HorizontalAlign::Right => "right",
    }
}

/// A style attribute's value: Rich's default is `"none"`.
fn style_value(py: Python<'_>, style: Option<&StyleType>) -> PyResult<Py<PyAny>> {
    match style {
        Some(style) => py_style_type(py, style),
        None => Ok(PyString::new(py, "none").into_any().unbind()),
    }
}

/// A `style=` / `border_style=` argument, checked now (a name must parse
/// as a style definition) and resolved when the panel prints.
fn checked_style(value: Option<&Bound<'_, PyAny>>) -> PyResult<Option<StyleType>> {
    resolved_style(value)?;
    style_type(value)
}

fn resolve(style: &Option<StyleType>) -> PyResult<Option<CoreStyle>> {
    Ok(match style {
        Some(StyleType::Style(style)) => Some(style.clone()),
        Some(StyleType::Name(name)) => Some(crate::style::parse_style(name)?),
        None => None,
    })
}

/// A panel `width` or `height`: Rich builds that many cells or lines, so
/// a huge one is its `MemoryError` (core would try, and exhaust memory).
fn check_size_arg(what: &str, value: Option<usize>) -> PyResult<Option<usize>> {
    let limit = if what == "width" {
        MAX_CONSOLE_WIDTH
    } else {
        MAX_CONSOLE_HEIGHT
    };
    value
        .map(|value| check_size(what, value, limit))
        .transpose()
}

/// `rich.panel.Panel`: a border around any renderable.
#[pyclass(name = "Panel", module = "rs_rich.panel")]
pub(crate) struct Panel {
    renderable: Option<Py<PyAny>>,
    box_set: CoreBox,
    title: Option<Title>,
    title_align: HorizontalAlign,
    subtitle: Option<Title>,
    subtitle_align: HorizontalAlign,
    #[pyo3(get, set)]
    safe_box: Option<bool>,
    #[pyo3(get, set)]
    expand: bool,
    style: Option<StyleType>,
    border_style: Option<StyleType>,
    width: Option<usize>,
    height: Option<usize>,
    padding: (usize, usize, usize, usize),
    /// `padding` as given (Rich keeps it so).
    padding_arg: Option<Py<PyAny>>,
    #[pyo3(get, set)]
    highlight: bool,
}

impl AsRenderable for Panel {
    fn to_renderable(&self, py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        // Rich parses a `str` title with `Text.from_markup`, whatever the
        // console's `markup`, and raises on bad markup.
        for title in [&self.title, &self.subtitle] {
            if let Some(Title::Markup(markup)) = title {
                if markup.contains('[') {
                    rich::Text::from_markup(markup).map_err(crate::color::markup::markup_error)?;
                }
            }
        }
        // `__clear__` (garbage collection) is the only thing that empties it.
        let child = match &self.renderable {
            Some(child) => child.bind(py).clone(),
            None => PyString::new(py, "").into_any(),
        };
        // Upstream renders a panel's child with the panel's `highlight`.
        let mut panel = CorePanel::new(renderable::to_renderable(&child, Some(self.highlight))?)
            .box_set(self.box_set)
            .expand(self.expand)
            .title_align(self.title_align)
            .subtitle_align(self.subtitle_align)
            .safe_box(self.safe_box)
            .padding(self.padding);
        match &self.title {
            Some(Title::Markup(title)) => panel = panel.title(title.clone()),
            Some(Title::Text(title)) => panel = panel.title_as_text(title.clone()),
            None => {}
        }
        if let Some(style) = resolve(&self.style)? {
            panel = panel.style(style);
        }
        if let Some(height) = check_size_arg("height", self.height)? {
            panel = panel.height(height);
        }
        if self.highlight {
            panel = panel.highlight(true);
        }
        match &self.subtitle {
            Some(Title::Markup(subtitle)) => panel = panel.subtitle(subtitle.clone()),
            Some(Title::Text(subtitle)) => panel = panel.subtitle_as_text(subtitle.clone()),
            None => {}
        }
        if let Some(style) = resolve(&self.border_style)? {
            panel = panel.border_style(style);
        }
        if let Some(width) = check_size_arg("width", self.width)? {
            panel = panel.width(width);
        }
        Ok(Box::new(panel))
    }
}

#[pymethods]
impl Panel {
    #[new]
    #[pyo3(signature = (
        renderable, r#box=BoxArg::Default, *, title=None, title_align="center", subtitle=None,
        subtitle_align="center", safe_box=None, expand=true, style=None, border_style=None,
        width=None, height=None, padding=None, highlight=false
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        renderable: Py<PyAny>,
        r#box: BoxArg,
        title: Option<&Bound<'_, PyAny>>,
        title_align: &str,
        subtitle: Option<&Bound<'_, PyAny>>,
        subtitle_align: &str,
        safe_box: Option<bool>,
        expand: bool,
        style: Option<&Bound<'_, PyAny>>,
        border_style: Option<&Bound<'_, PyAny>>,
        width: Option<usize>,
        height: Option<usize>,
        padding: Option<&Bound<'_, PyAny>>,
        highlight: bool,
    ) -> PyResult<Self> {
        Ok(Panel {
            renderable: Some(renderable),
            box_set: r#box
                .or(rich::r#box::ROUNDED)
                .ok_or_else(|| PyValueError::new_err("a Panel needs a box"))?,
            title: title_arg(title)?,
            title_align: convert::align(title_align)?,
            subtitle: title_arg(subtitle)?,
            subtitle_align: convert::align(subtitle_align)?,
            safe_box,
            expand,
            style: checked_style(style)?,
            border_style: checked_style(border_style)?,
            width,
            height,
            padding: match padding {
                Some(value) => convert::padding(value)?,
                None => (0, 1, 0, 1),
            },
            padding_arg: padding.map(|value| value.clone().unbind()),
            highlight,
        })
    }

    // The child can refer back to the panel (`holder.ref = Panel(holder)`),
    // so the garbage collector must see it to collect such a cycle.
    fn __traverse__(&self, visit: PyVisit<'_>) -> Result<(), PyTraverseError> {
        if let Some(child) = &self.renderable {
            visit.call(child)?;
        }
        if let Some(padding) = &self.padding_arg {
            visit.call(padding)?;
        }
        Ok(())
    }

    fn __clear__(&mut self) {
        self.renderable = None;
        self.padding_arg = None;
    }

    // Rich's attributes. The panel is a specification converted when it
    // prints, so each setter only updates the specification.

    #[getter]
    fn renderable(&self, py: Python<'_>) -> Py<PyAny> {
        match &self.renderable {
            Some(child) => child.clone_ref(py),
            None => PyString::new(py, "").into_any().unbind(),
        }
    }

    #[setter]
    fn set_renderable(&mut self, value: Py<PyAny>) {
        self.renderable = Some(value);
    }

    #[getter(r#box)]
    fn get_box(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        crate::boxes::to_py(py, self.box_set)
    }

    #[setter(r#box)]
    fn set_box(&mut self, value: BoxArg) -> PyResult<()> {
        self.box_set = value
            .or(rich::r#box::ROUNDED)
            .ok_or_else(|| PyValueError::new_err("a Panel needs a box"))?;
        Ok(())
    }

    #[getter]
    fn title(&self, py: Python<'_>) -> PyResult<Option<Py<PyAny>>> {
        title_value(py, self.title.as_ref())
    }

    #[setter]
    fn set_title(&mut self, value: Option<&Bound<'_, PyAny>>) -> PyResult<()> {
        self.title = title_arg(value)?;
        Ok(())
    }

    #[getter]
    fn subtitle(&self, py: Python<'_>) -> PyResult<Option<Py<PyAny>>> {
        title_value(py, self.subtitle.as_ref())
    }

    #[setter]
    fn set_subtitle(&mut self, value: Option<&Bound<'_, PyAny>>) -> PyResult<()> {
        self.subtitle = title_arg(value)?;
        Ok(())
    }

    #[getter]
    fn title_align(&self) -> &'static str {
        align_name(self.title_align)
    }

    #[setter]
    fn set_title_align(&mut self, value: &str) -> PyResult<()> {
        self.title_align = convert::align(value)?;
        Ok(())
    }

    #[getter]
    fn subtitle_align(&self) -> &'static str {
        align_name(self.subtitle_align)
    }

    #[setter]
    fn set_subtitle_align(&mut self, value: &str) -> PyResult<()> {
        self.subtitle_align = convert::align(value)?;
        Ok(())
    }

    #[getter]
    fn style(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        style_value(py, self.style.as_ref())
    }

    #[setter]
    fn set_style(&mut self, value: Option<&Bound<'_, PyAny>>) -> PyResult<()> {
        self.style = checked_style(value)?;
        Ok(())
    }

    #[getter]
    fn border_style(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        style_value(py, self.border_style.as_ref())
    }

    #[setter]
    fn set_border_style(&mut self, value: Option<&Bound<'_, PyAny>>) -> PyResult<()> {
        self.border_style = checked_style(value)?;
        Ok(())
    }

    #[getter]
    fn width(&self) -> Option<usize> {
        self.width
    }

    #[setter]
    fn set_width(&mut self, value: Option<usize>) -> PyResult<()> {
        self.width = value;
        Ok(())
    }

    #[getter]
    fn height(&self) -> Option<usize> {
        self.height
    }

    #[setter]
    fn set_height(&mut self, value: Option<usize>) -> PyResult<()> {
        self.height = value;
        Ok(())
    }

    #[getter]
    fn padding(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        match &self.padding_arg {
            Some(padding) => Ok(padding.clone_ref(py)),
            None => Ok(PyTuple::new(py, [0, 1])?.into_any().unbind()),
        }
    }

    #[setter]
    fn set_padding(&mut self, value: &Bound<'_, PyAny>) -> PyResult<()> {
        self.padding = convert::padding(value)?;
        self.padding_arg = Some(value.clone().unbind());
        Ok(())
    }

    /// `Panel.fit(...)`: a panel that fits its content (`expand=False`).
    #[classmethod]
    #[pyo3(signature = (
        renderable, r#box=BoxArg::Default, *, title=None, title_align="center", subtitle=None,
        subtitle_align="center", safe_box=None, style=None, border_style=None, width=None,
        height=None, padding=None, highlight=false
    ))]
    #[allow(clippy::too_many_arguments)]
    fn fit(
        _cls: &Bound<'_, pyo3::types::PyType>,
        renderable: Py<PyAny>,
        r#box: BoxArg,
        title: Option<&Bound<'_, PyAny>>,
        title_align: &str,
        subtitle: Option<&Bound<'_, PyAny>>,
        subtitle_align: &str,
        safe_box: Option<bool>,
        style: Option<&Bound<'_, PyAny>>,
        border_style: Option<&Bound<'_, PyAny>>,
        width: Option<usize>,
        height: Option<usize>,
        padding: Option<&Bound<'_, PyAny>>,
        highlight: bool,
    ) -> PyResult<Self> {
        Panel::new(
            renderable,
            r#box,
            title,
            title_align,
            subtitle,
            subtitle_align,
            safe_box,
            false,
            style,
            border_style,
            width,
            height,
            padding,
            highlight,
        )
    }
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    renderable::add_renderable_class::<Panel>(m)
}
