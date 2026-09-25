//! `rich.bar.Bar`. Port of upstream `rich/bar.py`.
//!
//! Core's `Bar` has no `color` or `bgcolor`, so the renderer is ported here.

use pyo3::prelude::*;
use pyo3::types::PyString;

use rich::console::{Console as CoreConsole, ConsoleOptions as CoreOptions};
use rich::measure::Measurement as CoreMeasurement;
use rich::protocol::Renderable;
use rich::segment::Segment as CoreSegment;
use rich::{Color as CoreColor, Style as CoreStyle};

use crate::errors::StyleSyntaxError;
use crate::renderable::{self, AsRenderable};
use crate::style::Style;

const BEGIN_BLOCK_ELEMENTS: [&str; 8] = ["█", "█", "█", "▐", "▐", "▐", "▕", "▕"];
const END_BLOCK_ELEMENTS: [&str; 8] = [" ", "▏", "▎", "▍", "▌", "▋", "▊", "▉"];
const FULL_BLOCK: &str = "█";

struct BarRender {
    size: f64,
    begin: f64,
    end: f64,
    width: Option<usize>,
    style: CoreStyle,
}

impl Renderable for BarRender {
    fn rich_render(&self, _console: &CoreConsole, options: &CoreOptions) -> Vec<CoreSegment> {
        let width = self.width.unwrap_or(options.max_width).min(options.max_width);
        let style = Some(self.style.clone());
        if self.begin >= self.end {
            return vec![CoreSegment::new(" ".repeat(width), style)];
        }
        let eighths = |value: f64| (width as f64 * 8.0 * value / self.size) as i64;
        let prefix_complete = eighths(self.begin).max(0) as usize;
        let body_complete = eighths(self.end).max(0) as usize;
        let mut prefix = " ".repeat(prefix_complete / 8);
        if prefix_complete % 8 != 0 {
            prefix.push_str(BEGIN_BLOCK_ELEMENTS[prefix_complete % 8]);
        }
        let mut body = FULL_BLOCK.repeat(body_complete / 8);
        if body_complete % 8 != 0 {
            body.push_str(END_BLOCK_ELEMENTS[body_complete % 8]);
        }
        let body_len = body.chars().count();
        let suffix = " ".repeat(width.saturating_sub(body_len));
        let tail: String = body.chars().skip(prefix.chars().count()).collect();
        vec![CoreSegment::new(format!("{prefix}{tail}{suffix}"), style)]
    }

    fn measure(&self, _console: &CoreConsole, options: &CoreOptions) -> CoreMeasurement {
        match self.width {
            Some(width) => CoreMeasurement::new(width, width),
            None => CoreMeasurement::new(4, options.max_width),
        }
    }
}

/// A `Union[Color, str]` argument as a core colour.
fn color(value: &Bound<'_, PyAny>) -> PyResult<CoreColor> {
    let name = match value.cast::<PyString>() {
        Ok(name) => name.to_cow()?.into_owned(),
        Err(_) => match value.getattr_opt("name")? {
            Some(name) => name.extract()?,
            None => value.str()?.to_cow()?.into_owned(),
        },
    };
    CoreColor::parse(&name).map_err(|e| StyleSyntaxError::new_err(e.to_string()))
}

/// `rich.bar.Bar`: a solid block bar spanning `begin` to `end` of `size`.
#[pyclass(name = "Bar", module = "rs_rich.bar")]
pub(crate) struct Bar {
    #[pyo3(get, set)]
    size: Py<PyAny>,
    #[pyo3(get, set)]
    begin: Py<PyAny>,
    #[pyo3(get, set)]
    end: Py<PyAny>,
    #[pyo3(get, set)]
    width: Option<usize>,
    style: CoreStyle,
}

impl AsRenderable for Bar {
    fn to_renderable(&self, py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(BarRender {
            size: self.size.bind(py).extract()?,
            begin: self.begin.bind(py).extract()?,
            end: self.end.bind(py).extract()?,
            width: self.width,
            style: self.style.clone(),
        }))
    }
}

#[pymethods]
impl Bar {
    #[new]
    #[pyo3(signature = (size, begin, end, *, width=None, color=None, bgcolor=None))]
    fn new(
        py: Python<'_>,
        size: &Bound<'_, PyAny>,
        begin: &Bound<'_, PyAny>,
        end: &Bound<'_, PyAny>,
        width: Option<usize>,
        color: Option<&Bound<'_, PyAny>>,
        bgcolor: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        size.extract::<f64>()?;
        // `max(begin, 0)` and `min(end, size)`, keeping the argument's type.
        let begin = if begin.lt(0)? {
            0i64.into_pyobject(py)?.into_any()
        } else {
            begin.clone()
        };
        let end = if size.lt(end)? { size.clone() } else { end.clone() };
        let default = PyString::new(py, "default").into_any();
        let style = CoreStyle::new()
            .with_color(self::color(color.unwrap_or(&default))?)
            .with_bgcolor(self::color(bgcolor.unwrap_or(&default))?);
        Ok(Bar {
            size: size.clone().unbind(),
            begin: begin.unbind(),
            end: end.unbind(),
            width,
            style,
        })
    }

    #[getter]
    fn style(&self) -> Style {
        Style::from_core(self.style.clone())
    }

    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        Ok(format!(
            "Bar({}, {}, {})",
            self.size.bind(py).str()?,
            self.begin.bind(py).str()?,
            self.end.bind(py).str()?
        ))
    }
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    renderable::add_renderable_class::<Bar>(m)
}
