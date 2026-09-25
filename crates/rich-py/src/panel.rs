//! `rich.panel`: the `Panel` class.
//!
//! Owner: the foundation.

use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyString;
use pyo3::{PyTraverseError, PyVisit};

use rich::align::HorizontalAlign;
use rich::protocol::Renderable;
use rich::r#box::Box as CoreBox;
use rich::{Panel as CorePanel, Style as CoreStyle};

use crate::boxes::BoxArg;
use crate::convert;
use crate::renderable::{self, AsRenderable};
use crate::style::resolved_style;
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

/// `rich.panel.Panel`: a border around any renderable.
#[pyclass(name = "Panel", module = "rs_rich.panel")]
pub(crate) struct Panel {
    renderable: Option<Py<PyAny>>,
    box_set: CoreBox,
    title: Option<Title>,
    title_align: HorizontalAlign,
    subtitle: Option<Title>,
    subtitle_align: HorizontalAlign,
    safe_box: Option<bool>,
    expand: bool,
    style: Option<CoreStyle>,
    border_style: Option<CoreStyle>,
    width: Option<usize>,
    height: Option<usize>,
    padding: (usize, usize, usize, usize),
    highlight: bool,
}

impl AsRenderable for Panel {
    fn to_renderable(&self, py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
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
        if let Some(style) = &self.style {
            panel = panel.style(style.clone());
        }
        if let Some(height) = self.height {
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
        if let Some(style) = &self.border_style {
            panel = panel.border_style(style.clone());
        }
        if let Some(width) = self.width {
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
            style: resolved_style(style)?,
            border_style: resolved_style(border_style)?,
            width,
            height,
            padding: match padding {
                Some(value) => convert::padding(value)?,
                None => (0, 1, 0, 1),
            },
            highlight,
        })
    }

    // The child can refer back to the panel (`holder.ref = Panel(holder)`),
    // so the garbage collector must see it to collect such a cycle.
    fn __traverse__(&self, visit: PyVisit<'_>) -> Result<(), PyTraverseError> {
        if let Some(child) = &self.renderable {
            visit.call(child)?;
        }
        Ok(())
    }

    fn __clear__(&mut self) {
        self.renderable = None;
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
