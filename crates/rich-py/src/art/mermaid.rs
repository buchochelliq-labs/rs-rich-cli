//! `rs_rich.mermaid`: `rich-mermaid` diagrams. `Mermaid` renders a diagram
//! (flowcharts as text; with an `mmdc` build and `backend="mmdc"`, every
//! type through Mermaid's CLI); `parse_flowchart` and `draw_flowchart`
//! expose the parser and the text layout, raising on what they refuse;
//! `MermaidFences` is the Markdown fence renderer. (`MermaidPlugin` is the
//! plugins area's, in `plugins/plugin.rs`.)

use std::sync::Arc;

use pyo3::create_exception;
use pyo3::exceptions::{PyException, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyString};

use rich::protocol::{FenceRenderer, Renderable};
use rich_mermaid::flowchart::{Direction, Head, Shape, Stroke};
use rich_mermaid::{Backend, Flowchart as CoreFlowchart, MermaidOptions, ParseError};

use super::{bad_choice, kinded, normalized, repr_float, repr_opt, repr_str};
use crate::renderable::{self, AsRenderable};

create_exception!(_native, MermaidError, PyException);
create_exception!(_native, MermaidParseError, MermaidError);
create_exception!(_native, MermaidLayoutError, MermaidError);
create_exception!(_native, MmdcError, MermaidError);

fn backend(name: &str) -> PyResult<Backend> {
    match normalized(name).as_str() {
        "text" => Ok(Backend::Text),
        "mmdc" => Ok(Backend::Mmdc),
        _ => Err(bad_choice("backend", name, "text or mmdc")),
    }
}

fn backend_name(backend: Backend) -> &'static str {
    match backend {
        Backend::Text => "text",
        Backend::Mmdc => "mmdc",
    }
}

// ---------------------------------------------------------------------------
// Options

/// How to run Mermaid's CLI for `backend="mmdc"` (`rich_mermaid::mmdc::
/// MmdcOptions`). Accepted by every build; only an `mmdc` build runs it.
#[pyclass(name = "MmdcOptions", module = "rs_rich.mermaid", frozen)]
pub(crate) struct MmdcOptions {
    #[pyo3(get)]
    program: String,
    #[pyo3(get)]
    timeout: f64,
    #[pyo3(get)]
    max_input: usize,
    #[pyo3(get)]
    max_output: usize,
    #[pyo3(get)]
    puppeteer_config: Option<String>,
    #[pyo3(get)]
    background: String,
}

impl MmdcOptions {
    fn copy(&self) -> MmdcOptions {
        MmdcOptions {
            program: self.program.clone(),
            timeout: self.timeout,
            max_input: self.max_input,
            max_output: self.max_output,
            puppeteer_config: self.puppeteer_config.clone(),
            background: self.background.clone(),
        }
    }

    #[cfg(feature = "mmdc")]
    fn to_core(&self) -> rich_mermaid::mmdc::MmdcOptions {
        rich_mermaid::mmdc::MmdcOptions {
            program: self.program.clone().into(),
            timeout: std::time::Duration::from_secs_f64(self.timeout),
            max_input: self.max_input,
            max_output: self.max_output,
            puppeteer_config: self.puppeteer_config.clone().map(Into::into),
            background: self.background.clone(),
        }
    }
}

#[pymethods]
impl MmdcOptions {
    #[new]
    #[pyo3(signature = (
        *, program="mmdc".to_string(), timeout=20.0, max_input=64 * 1024,
        max_output=16 * 1024 * 1024, puppeteer_config=None, background="white".to_string()
    ))]
    fn new(
        program: String,
        timeout: f64,
        max_input: usize,
        max_output: usize,
        puppeteer_config: Option<String>,
        background: String,
    ) -> PyResult<Self> {
        if !(timeout.is_finite() && timeout > 0.0 && timeout < 1e9) {
            return Err(PyValueError::new_err(
                "timeout must be a positive, finite number of seconds",
            ));
        }
        Ok(MmdcOptions {
            program,
            timeout,
            max_input,
            max_output,
            puppeteer_config,
            background,
        })
    }

    fn __repr__(&self) -> String {
        format!(
            "MmdcOptions(program={}, timeout={}, max_input={}, max_output={}, \
             puppeteer_config={}, background={})",
            repr_str(&self.program),
            repr_float(self.timeout),
            self.max_input,
            self.max_output,
            repr_opt(self.puppeteer_config.as_deref()),
            repr_str(&self.background)
        )
    }
}

/// What `Mermaid` and `MermaidFences` share.
struct Settings {
    backend: Backend,
    ascii: Option<bool>,
    mmdc: Option<MmdcOptions>,
}

impl Settings {
    fn new(
        backend: &str,
        ascii: Option<bool>,
        mmdc: Option<PyRef<'_, MmdcOptions>>,
    ) -> PyResult<Self> {
        Ok(Settings {
            backend: self::backend(backend)?,
            ascii,
            mmdc: mmdc.map(|options| options.copy()),
        })
    }

    fn copy(&self) -> Settings {
        Settings {
            backend: self.backend,
            ascii: self.ascii,
            mmdc: self.mmdc.as_ref().map(MmdcOptions::copy),
        }
    }

    fn options(&self) -> MermaidOptions {
        MermaidOptions {
            backend: self.backend,
            ascii: self.ascii,
            #[cfg(feature = "mmdc")]
            mmdc: self
                .mmdc
                .as_ref()
                .map(MmdcOptions::to_core)
                .unwrap_or_default(),
        }
    }

    fn mmdc(&self) -> Option<MmdcOptions> {
        self.mmdc.as_ref().map(MmdcOptions::copy)
    }
}

// ---------------------------------------------------------------------------
// Mermaid

/// A Mermaid diagram (`rich_mermaid::Mermaid`). Flowcharts draw as text;
/// anything that cannot be drawn shows as its source under a one-line note.
#[pyclass(name = "Mermaid", module = "rs_rich.mermaid", frozen)]
pub(crate) struct Mermaid {
    source: String,
    settings: Settings,
}

impl AsRenderable for Mermaid {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(
            rich_mermaid::Mermaid::new(self.source.clone()).options(self.settings.options()),
        ))
    }
}

#[pymethods]
impl Mermaid {
    /// `backend`: `"text"` (the default) or `"mmdc"`. `ascii`: draw with
    /// ASCII only (`None` follows the console).
    #[new]
    #[pyo3(signature = (source, *, backend="text", ascii=None, mmdc=None))]
    fn new(
        source: String,
        backend: &str,
        ascii: Option<bool>,
        mmdc: Option<PyRef<'_, MmdcOptions>>,
    ) -> PyResult<Self> {
        Ok(Mermaid {
            source,
            settings: Settings::new(backend, ascii, mmdc)?,
        })
    }

    #[getter]
    fn source(&self) -> &str {
        &self.source
    }

    #[getter]
    fn backend(&self) -> &'static str {
        backend_name(self.settings.backend)
    }

    #[getter]
    fn ascii(&self) -> Option<bool> {
        self.settings.ascii
    }

    #[getter]
    fn mmdc(&self) -> Option<MmdcOptions> {
        self.settings.mmdc()
    }

    fn __repr__(&self) -> String {
        format!("<Mermaid {} bytes>", self.source.len())
    }
}

/// Renders ```` ```mermaid ```` fences in Markdown
/// (`rich_mermaid::MermaidFences`).
#[pyclass(name = "MermaidFences", module = "rs_rich.mermaid", frozen)]
pub(crate) struct MermaidFences {
    settings: Settings,
}

impl MermaidFences {
    /// The core fence renderer, for `Markdown`'s fence renderers.
    #[allow(dead_code)] // for the code area's `Markdown(fences=...)`
    pub(crate) fn fence_renderer(&self) -> Arc<dyn FenceRenderer> {
        Arc::new(rich_mermaid::MermaidFences {
            options: self.settings.options(),
        })
    }
}

#[pymethods]
impl MermaidFences {
    #[new]
    #[pyo3(signature = (*, backend="text", ascii=None, mmdc=None))]
    fn new(
        backend: &str,
        ascii: Option<bool>,
        mmdc: Option<PyRef<'_, MmdcOptions>>,
    ) -> PyResult<Self> {
        Ok(MermaidFences {
            settings: Settings::new(backend, ascii, mmdc)?,
        })
    }

    /// Whether a fence in `language` is drawn (any case of `mermaid`).
    fn accepts(&self, language: &str) -> bool {
        language.eq_ignore_ascii_case("mermaid")
    }

    /// The `Mermaid` that draws a fence's `code`, or `None` when this
    /// renderer leaves the fence to `Syntax`. `console` and `options` are
    /// accepted (and unused) so this is also a Python fence renderer for
    /// `rs_rich.plugins`.
    #[pyo3(signature = (language, code, console=None, options=None))]
    fn render_fence(
        &self,
        language: &str,
        code: &str,
        console: Option<&Bound<'_, PyAny>>,
        options: Option<&Bound<'_, PyAny>>,
    ) -> Option<Mermaid> {
        let _ = (console, options);
        self.accepts(language).then(|| Mermaid {
            source: code.to_string(),
            settings: self.settings.copy(),
        })
    }

    #[getter]
    fn backend(&self) -> &'static str {
        backend_name(self.settings.backend)
    }

    #[getter]
    fn ascii(&self) -> Option<bool> {
        self.settings.ascii
    }

    #[getter]
    fn mmdc(&self) -> Option<MmdcOptions> {
        self.settings.mmdc()
    }
}

// ---------------------------------------------------------------------------
// Parsing and drawing

fn direction_name(direction: Direction) -> &'static str {
    match direction {
        Direction::TopDown => "TD",
        Direction::BottomUp => "BT",
        Direction::LeftRight => "LR",
        Direction::RightLeft => "RL",
    }
}

fn shape_name(shape: Shape) -> &'static str {
    match shape {
        Shape::Rect => "rect",
        Shape::Round => "round",
        Shape::Stadium => "stadium",
        Shape::Subroutine => "subroutine",
        Shape::Cylinder => "cylinder",
        Shape::Circle => "circle",
        Shape::DoubleCircle => "double_circle",
        Shape::Asymmetric => "asymmetric",
        Shape::Rhombus => "rhombus",
        Shape::Hexagon => "hexagon",
        Shape::Parallelogram => "parallelogram",
        Shape::ParallelogramAlt => "parallelogram_alt",
        Shape::Trapezoid => "trapezoid",
        Shape::TrapezoidAlt => "trapezoid_alt",
    }
}

fn stroke_name(stroke: Stroke) -> &'static str {
    match stroke {
        Stroke::Solid => "solid",
        Stroke::Thick => "thick",
        Stroke::Dotted => "dotted",
        Stroke::Invisible => "invisible",
    }
}

fn head_name(head: Head) -> Option<&'static str> {
    match head {
        Head::None => None,
        Head::Arrow => Some("arrow"),
        Head::Circle => Some("circle"),
        Head::Cross => Some("cross"),
    }
}

/// A node of a parsed flowchart.
#[pyclass(name = "FlowchartNode", module = "rs_rich.mermaid", frozen)]
pub(crate) struct FlowchartNode {
    #[pyo3(get)]
    id: String,
    #[pyo3(get)]
    label: String,
    #[pyo3(get)]
    shape: &'static str,
}

#[pymethods]
impl FlowchartNode {
    fn __repr__(&self) -> String {
        format!(
            "FlowchartNode(id={}, label={}, shape={})",
            repr_str(&self.id),
            repr_str(&self.label),
            repr_str(self.shape)
        )
    }
}

/// An edge of a parsed flowchart: `source` and `target` index `nodes`.
#[pyclass(name = "FlowchartEdge", module = "rs_rich.mermaid", frozen)]
pub(crate) struct FlowchartEdge {
    #[pyo3(get)]
    source: usize,
    #[pyo3(get)]
    target: usize,
    #[pyo3(get)]
    label: Option<String>,
    #[pyo3(get)]
    stroke: &'static str,
    #[pyo3(get)]
    start: Option<&'static str>,
    #[pyo3(get)]
    end: Option<&'static str>,
    #[pyo3(get)]
    length: usize,
}

#[pymethods]
impl FlowchartEdge {
    fn __repr__(&self) -> String {
        format!(
            "FlowchartEdge(source={}, target={}, label={}, stroke={}, start={}, end={}, \
             length={})",
            self.source,
            self.target,
            repr_opt(self.label.as_deref()),
            repr_str(self.stroke),
            repr_opt(self.start),
            repr_opt(self.end),
            self.length
        )
    }
}

/// A parsed flowchart (`rich_mermaid::Flowchart`).
#[pyclass(name = "Flowchart", module = "rs_rich.mermaid", frozen)]
pub(crate) struct Flowchart {
    inner: CoreFlowchart,
}

#[pymethods]
impl Flowchart {
    /// `"TD"`, `"BT"`, `"LR"` or `"RL"` (`TB` parses as `TD`).
    #[getter]
    fn direction(&self) -> &'static str {
        direction_name(self.inner.direction)
    }

    #[getter]
    fn nodes(&self) -> Vec<FlowchartNode> {
        self.inner
            .nodes
            .iter()
            .map(|node| FlowchartNode {
                id: node.id.clone(),
                label: node.label.clone(),
                shape: shape_name(node.shape),
            })
            .collect()
    }

    #[getter]
    fn edges(&self) -> Vec<FlowchartEdge> {
        self.inner
            .edges
            .iter()
            .map(|edge| FlowchartEdge {
                source: edge.from,
                target: edge.to,
                label: edge.label.clone(),
                stroke: stroke_name(edge.stroke),
                start: head_name(edge.start),
                end: head_name(edge.end),
                length: edge.length,
            })
            .collect()
    }

    /// What the text renderer simplified (shown under the diagram).
    #[getter]
    fn notes(&self) -> Vec<String> {
        self.inner.notes.clone()
    }

    fn __repr__(&self) -> String {
        format!(
            "<Flowchart {} nodes={} edges={}>",
            self.direction(),
            self.inner.nodes.len(),
            self.inner.edges.len()
        )
    }
}

/// A flowchart drawn as text (`rich_mermaid::Diagram`).
#[pyclass(name = "MermaidDiagram", module = "rs_rich.mermaid", frozen)]
pub(crate) struct MermaidDiagram {
    #[pyo3(get)]
    lines: Vec<String>,
    /// The widest line, in cells.
    #[pyo3(get)]
    width: usize,
}

#[pymethods]
impl MermaidDiagram {
    fn __str__(&self) -> String {
        self.lines.join("\n")
    }

    fn __repr__(&self) -> String {
        format!("<MermaidDiagram {}x{}>", self.width, self.lines.len())
    }
}

fn parse_error(py: Python<'_>, error: &ParseError) -> PyErr {
    let kind = match error {
        ParseError::Empty => "empty",
        ParseError::Unsupported(_) => "unsupported",
        ParseError::TooLarge(_) => "too_large",
        ParseError::Syntax { .. } => "syntax",
    };
    let exception = kinded::<MermaidParseError>(py, error.to_string(), kind);
    let value = exception.value(py);
    let _ = match error {
        ParseError::Syntax { line, .. } => value.setattr("line", *line),
        _ => value.setattr("line", py.None()),
    };
    exception
}

fn parse(py: Python<'_>, source: &str) -> PyResult<CoreFlowchart> {
    rich_mermaid::parse(source).map_err(|error| parse_error(py, &error))
}

/// Parse a Mermaid flowchart (`rich_mermaid::parse`). Raises
/// `MermaidParseError`, whose `kind` is `"empty"`, `"unsupported"` (another
/// diagram type), `"too_large"` or `"syntax"` (with `line`).
#[pyfunction]
fn parse_flowchart(py: Python<'_>, source: &str) -> PyResult<Flowchart> {
    Ok(Flowchart {
        inner: parse(py, source)?,
    })
}

/// Lay out and draw a flowchart as text (`rich_mermaid::draw`): a
/// `Flowchart`, or source to parse first. Raises `MermaidLayoutError` when it
/// is too large to draw.
#[pyfunction]
#[pyo3(signature = (chart, ascii=false))]
fn draw_flowchart(
    py: Python<'_>,
    chart: &Bound<'_, PyAny>,
    ascii: bool,
) -> PyResult<MermaidDiagram> {
    let chart = if let Ok(source) = chart.cast::<PyString>() {
        parse(py, source.to_cow()?.as_ref())?
    } else if let Ok(chart) = chart.cast::<Flowchart>() {
        chart.get().inner.clone()
    } else {
        return Err(PyTypeError::new_err(
            "expected a Flowchart or Mermaid source",
        ));
    };
    let diagram = py
        .detach(move || rich_mermaid::draw(&chart, ascii))
        .map_err(|reason| {
            kinded::<MermaidLayoutError>(py, format!("too large to draw: {reason}"), "too_large")
        })?;
    Ok(MermaidDiagram {
        lines: diagram.lines,
        width: diagram.width,
    })
}

/// A label as the parser cleans it (`<br>` to newlines, entities decoded,
/// control characters removed).
#[pyfunction]
fn mermaid_clean_label(text: &str) -> String {
    rich_mermaid::flowchart::clean_label(text)
}

/// Render `source` to PNG bytes with Mermaid's CLI (`mmdc`); only in an
/// `mmdc` build. Raises `MmdcError`, whose `kind` is `"not_found"`,
/// `"too_large"`, `"output_too_large"`, `"timeout"`, `"failed"` or `"io"`.
#[pyfunction]
#[pyo3(signature = (source, options=None))]
fn mmdc_render_png<'py>(
    py: Python<'py>,
    source: &str,
    options: Option<PyRef<'_, MmdcOptions>>,
) -> PyResult<Bound<'py, PyBytes>> {
    #[cfg(feature = "mmdc")]
    {
        use rich_mermaid::mmdc::MmdcError as E;
        let options = options.map(|o| o.to_core()).unwrap_or_default();
        let source = source.to_string();
        let png = py
            .detach(move || rich_mermaid::mmdc::render_png(&source, &options))
            .map_err(|error| {
                let kind = match &error {
                    E::NotFound(_) => "not_found",
                    E::TooLarge { .. } => "too_large",
                    E::OutputTooLarge { .. } => "output_too_large",
                    E::Timeout(_) => "timeout",
                    E::Failed(_) => "failed",
                    E::Io(_) => "io",
                };
                kinded::<MmdcError>(py, error.to_string(), kind)
            })?;
        Ok(PyBytes::new(py, &png))
    }
    #[cfg(not(feature = "mmdc"))]
    {
        let _ = (py, source, options);
        Err(pyo3::exceptions::PyNotImplementedError::new_err(
            "this rs_rich build has no mmdc backend; build rs-rich-py with the `mmdc` feature",
        ))
    }
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    let py = m.py();
    m.add("MermaidError", py.get_type::<MermaidError>())?;
    m.add("MermaidParseError", py.get_type::<MermaidParseError>())?;
    m.add("MermaidLayoutError", py.get_type::<MermaidLayoutError>())?;
    m.add("MmdcError", py.get_type::<MmdcError>())?;
    m.add_class::<MmdcOptions>()?;
    renderable::add_renderable_class::<Mermaid>(m)?;
    m.add_class::<MermaidFences>()?;
    m.add_class::<Flowchart>()?;
    m.add_class::<FlowchartNode>()?;
    m.add_class::<FlowchartEdge>()?;
    m.add_class::<MermaidDiagram>()?;
    m.add_function(pyo3::wrap_pyfunction!(parse_flowchart, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(draw_flowchart, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(mermaid_clean_label, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(mmdc_render_png, m)?)?;
    m.add("MERMAID_HAS_MMDC", cfg!(feature = "mmdc"))?;
    m.add("MERMAID_MAX_SOURCE", rich_mermaid::flowchart::MAX_SOURCE)?;
    m.add("MERMAID_MAX_NODES", rich_mermaid::flowchart::MAX_NODES)?;
    m.add("MERMAID_MAX_EDGES", rich_mermaid::flowchart::MAX_EDGES)?;
    m.add(
        "MERMAID_MAX_LINK_LENGTH",
        rich_mermaid::flowchart::MAX_LINK_LENGTH,
    )?;
    Ok(())
}
