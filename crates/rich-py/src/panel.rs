//! `rich.panel`: the `Panel` class.
//!
//! Owner: the foundation.

use pyo3::exceptions::PyValueError;
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

/// `rich.panel.Panel`: a border around any renderable.
#[pyclass(name = "Panel", module = "rs_rich.panel")]
pub(crate) struct Panel {
    renderable: Option<Py<PyAny>>,
    box_set: CoreBox,
    title: Option<String>,
    title_align: HorizontalAlign,
    subtitle: Option<String>,
    subtitle_align: HorizontalAlign,
    expand: bool,
    border_style: Option<CoreStyle>,
    width: Option<usize>,
    padding: (usize, usize, usize, usize),
}

impl AsRenderable for Panel {
    fn to_renderable(&self, py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        // `__clear__` (garbage collection) is the only thing that empties it.
        let child = match &self.renderable {
            Some(child) => child.bind(py).clone(),
            None => PyString::new(py, "").into_any(),
        };
        // Upstream renders a panel's child with `highlight=False`.
        let mut panel = CorePanel::new(renderable::to_renderable(&child, Some(false))?)
            .box_set(self.box_set)
            .expand(self.expand)
            .title_align(self.title_align)
            .subtitle_align(self.subtitle_align)
            .padding(self.padding);
        if let Some(title) = &self.title {
            panel = panel.title(title.clone());
        }
        if let Some(subtitle) = &self.subtitle {
            panel = panel.subtitle(subtitle.clone());
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
        subtitle_align="center", expand=true, border_style=None, width=None, padding=None
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        renderable: Py<PyAny>,
        r#box: BoxArg,
        title: Option<String>,
        title_align: &str,
        subtitle: Option<String>,
        subtitle_align: &str,
        expand: bool,
        border_style: Option<&Bound<'_, PyAny>>,
        width: Option<usize>,
        padding: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        Ok(Panel {
            renderable: Some(renderable),
            box_set: r#box
                .or(rich::r#box::ROUNDED)
                .ok_or_else(|| PyValueError::new_err("a Panel needs a box"))?,
            title,
            title_align: convert::align(title_align)?,
            subtitle,
            subtitle_align: convert::align(subtitle_align)?,
            expand,
            border_style: resolved_style(border_style)?,
            width,
            padding: match padding {
                Some(value) => convert::padding(value)?,
                None => (0, 1, 0, 1),
            },
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
        subtitle_align="center", border_style=None, width=None, padding=None
    ))]
    #[allow(clippy::too_many_arguments)]
    fn fit(
        _cls: &Bound<'_, pyo3::types::PyType>,
        renderable: Py<PyAny>,
        r#box: BoxArg,
        title: Option<String>,
        title_align: &str,
        subtitle: Option<String>,
        subtitle_align: &str,
        border_style: Option<&Bound<'_, PyAny>>,
        width: Option<usize>,
        padding: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        Panel::new(
            renderable,
            r#box,
            title,
            title_align,
            subtitle,
            subtitle_align,
            false,
            border_style,
            width,
            padding,
        )
    }
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    renderable::add_renderable_class::<Panel>(m)
}
