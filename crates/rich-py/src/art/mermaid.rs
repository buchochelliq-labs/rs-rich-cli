//! `rs_rich.mermaid`: `rich-mermaid` diagrams. `Mermaid` renders a diagram
//! (flowcharts, sequence and ER diagrams as text; with an `mmdc` build and
//! `backend="mmdc"`, every type through Mermaid's CLI); `parse_flowchart`,
//! `parse_sequence` and `parse_er` (and `parse_diagram`, which picks by the
//! header) expose the parsers, and the `draw_*` functions the text drawings,
//! raising on what they refuse; `MermaidFences` is the Markdown fence
//! renderer. (`MermaidPlugin` is the plugins area's, in `plugins/plugin.rs`.)

use std::sync::Arc;

use pyo3::create_exception;
use pyo3::exceptions::{PyException, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyString};

use rich::protocol::{FenceRenderer, Renderable};
use rich_diagram::Drawing;
use rich_mermaid::er::{self, Cardinality, EntityDiagram as CoreEntityDiagram};
use rich_mermaid::flowchart::{Direction, Head, Shape, Stroke};
use rich_mermaid::sequence::{
    self, Event as CoreEvent, LineStyle, Message as CoreMessage, NotePlacement, ParticipantKind,
    Sequence as CoreSequence, Tip,
};
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
        // rs-rich-diagram's ER shape; no Mermaid source produces it.
        Shape::Table => "table",
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

/// A diagram drawn as text (`rich_mermaid::Diagram`): a flowchart, a
/// sequence diagram or an ER diagram.
#[pyclass(name = "MermaidDiagram", module = "rs_rich.mermaid", frozen)]
pub(crate) struct MermaidDiagram {
    #[pyo3(get)]
    lines: Vec<String>,
    /// The widest line, in cells.
    #[pyo3(get)]
    width: usize,
    /// What the drawing does not show, one sentence each (`Mermaid` prints
    /// them under the diagram).
    #[pyo3(get)]
    notes: Vec<String>,
}

impl From<Drawing> for MermaidDiagram {
    fn from(drawing: Drawing) -> Self {
        MermaidDiagram {
            lines: drawing.lines,
            width: drawing.width,
            notes: drawing.notes,
        }
    }
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
    // The flowchart parser names other kinds as not drawn; these two are,
    // by their own parsers.
    for (is, what, with) in [
        (
            sequence::is_sequence as fn(&str) -> bool,
            "a sequence diagram",
            "parse_sequence",
        ),
        (er::is_er, "an ER diagram", "parse_er"),
    ] {
        if is(source) {
            return Err(kinded::<MermaidParseError>(
                py,
                format!("{what} is not a flowchart; parse it with {with} or parse_diagram"),
                "unsupported",
            )
            .tap_line(py));
        }
    }
    rich_mermaid::parse(source).map_err(|error| parse_error(py, &error))
}

/// `line` is `None` on an error that is not a syntax error.
trait TapLine {
    fn tap_line(self, py: Python<'_>) -> Self;
}

impl TapLine for PyErr {
    fn tap_line(self, py: Python<'_>) -> Self {
        let _ = self.value(py).setattr("line", py.None());
        self
    }
}

fn layout_error(py: Python<'_>, reason: String) -> PyErr {
    kinded::<MermaidLayoutError>(py, format!("too large to draw: {reason}"), "too_large")
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
        .map_err(|reason| layout_error(py, reason))?;
    Ok(diagram.into())
}

// ---------------------------------------------------------------------------
// Sequence diagrams

fn tip_name(tip: Tip) -> Option<&'static str> {
    match tip {
        Tip::None => None,
        Tip::Arrow => Some("arrow"),
        Tip::Cross => Some("cross"),
        Tip::Open => Some("open"),
    }
}

/// A participant or actor of a sequence diagram.
#[pyclass(name = "SequenceParticipant", module = "rs_rich.mermaid", frozen)]
pub(crate) struct SequenceParticipant {
    #[pyo3(get)]
    id: String,
    /// The text in its head; `\n` breaks a line.
    #[pyo3(get)]
    label: String,
    /// `"participant"` (a box) or `"actor"` (a stick figure).
    #[pyo3(get)]
    kind: &'static str,
}

#[pymethods]
impl SequenceParticipant {
    fn __repr__(&self) -> String {
        format!(
            "SequenceParticipant(id={}, label={}, kind={})",
            repr_str(&self.id),
            repr_str(&self.label),
            repr_str(self.kind)
        )
    }
}

/// A message: `source` and `target` index `participants`.
#[pyclass(name = "SequenceMessage", module = "rs_rich.mermaid", frozen)]
pub(crate) struct SequenceMessage {
    #[pyo3(get)]
    source: usize,
    #[pyo3(get)]
    target: usize,
    #[pyo3(get)]
    text: String,
    /// `"solid"` (`->>`) or `"dotted"` (`-->>`).
    #[pyo3(get)]
    line: &'static str,
    /// The tip at the sender (only `<<->>` has one): `None`, `"arrow"`,
    /// `"cross"` or `"open"`.
    #[pyo3(get)]
    start: Option<&'static str>,
    /// The tip at the receiver.
    #[pyo3(get)]
    end: Option<&'static str>,
    /// Its `autonumber` number, when numbering is on.
    #[pyo3(get)]
    number: Option<u64>,
}

impl From<&CoreMessage> for SequenceMessage {
    fn from(message: &CoreMessage) -> Self {
        SequenceMessage {
            source: message.from,
            target: message.to,
            text: message.text.clone(),
            line: match message.line {
                LineStyle::Solid => "solid",
                LineStyle::Dotted => "dotted",
            },
            start: tip_name(message.start),
            end: tip_name(message.end),
            number: message.number,
        }
    }
}

#[pymethods]
impl SequenceMessage {
    fn __repr__(&self) -> String {
        format!(
            "SequenceMessage(source={}, target={}, text={}, line={}, start={}, end={}, number={})",
            self.source,
            self.target,
            repr_str(&self.text),
            repr_str(self.line),
            repr_opt(self.start),
            repr_opt(self.end),
            self.number.map_or("None".to_string(), |n| n.to_string())
        )
    }
}

/// One statement of a sequence diagram, in order. `kind` is `"message"`
/// (`message`), `"note"` (`placement` `"left_of"`, `"right_of"` or
/// `"over"`, `participants` the one or two it is beside or spans, `text`),
/// `"activate"` or `"deactivate"` (`participants`, one), `"start"` (a block
/// opens: `block` is its keyword, `label` its label), `"divider"` (`else`,
/// `and` or `option`: `label`) or `"end"`.
#[pyclass(name = "SequenceEvent", module = "rs_rich.mermaid", frozen)]
pub(crate) struct SequenceEvent {
    #[pyo3(get)]
    kind: &'static str,
    message: Option<Py<SequenceMessage>>,
    #[pyo3(get)]
    placement: Option<&'static str>,
    #[pyo3(get)]
    participants: Vec<usize>,
    #[pyo3(get)]
    text: Option<String>,
    #[pyo3(get)]
    block: Option<&'static str>,
    #[pyo3(get)]
    label: Option<String>,
}

impl SequenceEvent {
    fn new(py: Python<'_>, event: &CoreEvent) -> PyResult<Self> {
        let mut out = SequenceEvent {
            kind: "end",
            message: None,
            placement: None,
            participants: Vec::new(),
            text: None,
            block: None,
            label: None,
        };
        match event {
            CoreEvent::Message(message) => {
                out.kind = "message";
                out.message = Some(Py::new(py, SequenceMessage::from(message))?);
            }
            CoreEvent::Note { placement, text } => {
                out.kind = "note";
                let (name, participants) = match *placement {
                    NotePlacement::LeftOf(at) => ("left_of", vec![at]),
                    NotePlacement::RightOf(at) => ("right_of", vec![at]),
                    NotePlacement::Over(first, last) if first == last => ("over", vec![first]),
                    NotePlacement::Over(first, last) => ("over", vec![first, last]),
                };
                out.placement = Some(name);
                out.participants = participants;
                out.text = Some(text.clone());
            }
            CoreEvent::Activate(at) => {
                out.kind = "activate";
                out.participants = vec![*at];
            }
            CoreEvent::Deactivate(at) => {
                out.kind = "deactivate";
                out.participants = vec![*at];
            }
            CoreEvent::Start { kind, label } => {
                out.kind = "start";
                out.block = Some(kind.keyword());
                out.label = Some(label.clone());
            }
            CoreEvent::Divider { label } => {
                out.kind = "divider";
                out.label = Some(label.clone());
            }
            CoreEvent::End => {}
        }
        Ok(out)
    }
}

#[pymethods]
impl SequenceEvent {
    /// The message, for a `"message"` event.
    #[getter]
    fn message(&self, py: Python<'_>) -> Option<Py<SequenceMessage>> {
        self.message.as_ref().map(|m| m.clone_ref(py))
    }

    fn __repr__(&self) -> String {
        format!("<SequenceEvent {}>", self.kind)
    }
}

/// A parsed sequence diagram (`rich_mermaid::sequence::Sequence`).
#[pyclass(name = "SequenceDiagram", module = "rs_rich.mermaid", frozen)]
pub(crate) struct SequenceDiagram {
    inner: CoreSequence,
}

#[pymethods]
impl SequenceDiagram {
    #[getter]
    fn title(&self) -> Option<String> {
        self.inner.title.clone()
    }

    /// Left to right, in order of first appearance.
    #[getter]
    fn participants(&self) -> Vec<SequenceParticipant> {
        self.inner
            .participants
            .iter()
            .map(|participant| SequenceParticipant {
                id: participant.id.clone(),
                label: participant.label.clone(),
                kind: match participant.kind {
                    ParticipantKind::Participant => "participant",
                    ParticipantKind::Actor => "actor",
                },
            })
            .collect()
    }

    /// Every statement that draws something, in order.
    #[getter]
    fn events(&self, py: Python<'_>) -> PyResult<Vec<SequenceEvent>> {
        self.inner
            .events
            .iter()
            .map(|event| SequenceEvent::new(py, event))
            .collect()
    }

    /// The messages alone, in order.
    #[getter]
    fn messages(&self) -> Vec<SequenceMessage> {
        self.inner
            .events
            .iter()
            .filter_map(|event| match event {
                CoreEvent::Message(message) => Some(SequenceMessage::from(message)),
                _ => None,
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
            "<SequenceDiagram participants={} events={}>",
            self.inner.participants.len(),
            self.inner.events.len()
        )
    }
}

fn sequence_of(py: Python<'_>, source: &str) -> PyResult<CoreSequence> {
    sequence::parse(source).map_err(|error| parse_error(py, &error))
}

/// Parse a Mermaid sequence diagram (`rich_mermaid::sequence::parse`).
/// Raises `MermaidParseError` as `parse_flowchart` does.
#[pyfunction]
fn parse_sequence(py: Python<'_>, source: &str) -> PyResult<SequenceDiagram> {
    Ok(SequenceDiagram {
        inner: sequence_of(py, source)?,
    })
}

/// Draw a sequence diagram as text (`rich_mermaid::sequence::draw`): a
/// `SequenceDiagram`, or source to parse first. Raises `MermaidLayoutError`
/// when it is too large to draw.
#[pyfunction]
#[pyo3(signature = (diagram, ascii=false))]
fn draw_sequence(
    py: Python<'_>,
    diagram: &Bound<'_, PyAny>,
    ascii: bool,
) -> PyResult<MermaidDiagram> {
    let diagram = if let Ok(source) = diagram.cast::<PyString>() {
        sequence_of(py, source.to_cow()?.as_ref())?
    } else if let Ok(diagram) = diagram.cast::<SequenceDiagram>() {
        diagram.get().inner.clone()
    } else {
        return Err(PyTypeError::new_err(
            "expected a SequenceDiagram or Mermaid source",
        ));
    };
    py.detach(move || sequence::draw(&diagram, ascii))
        .map(MermaidDiagram::from)
        .map_err(|reason| layout_error(py, reason))
}

/// Whether `source` is a sequence diagram (its first statement is
/// `sequenceDiagram`).
#[pyfunction]
fn mermaid_is_sequence(source: &str) -> bool {
    sequence::is_sequence(source)
}

// ---------------------------------------------------------------------------
// ER diagrams

fn cardinality_name(cardinality: Cardinality) -> &'static str {
    match cardinality {
        Cardinality::ExactlyOne => "exactly_one",
        Cardinality::ZeroOrOne => "zero_or_one",
        Cardinality::ZeroOrMore => "zero_or_more",
        Cardinality::OneOrMore => "one_or_more",
    }
}

/// An entity's attribute: `type name [keys] ["comment"]`.
#[pyclass(name = "ErAttribute", module = "rs_rich.mermaid", frozen)]
pub(crate) struct ErAttribute {
    #[pyo3(get)]
    data_type: String,
    #[pyo3(get)]
    name: String,
    /// `"PK"`, `"FK"` and `"UK"`, as written.
    #[pyo3(get)]
    keys: Vec<&'static str>,
    #[pyo3(get)]
    comment: Option<String>,
}

#[pymethods]
impl ErAttribute {
    fn __repr__(&self) -> String {
        format!(
            "ErAttribute(data_type={}, name={}, keys=[{}], comment={})",
            repr_str(&self.data_type),
            repr_str(&self.name),
            self.keys
                .iter()
                .map(|key| repr_str(key))
                .collect::<Vec<_>>()
                .join(", "),
            repr_opt(self.comment.as_deref())
        )
    }
}

/// An entity and its attributes.
#[pyclass(name = "ErEntity", module = "rs_rich.mermaid", frozen)]
pub(crate) struct ErEntity {
    #[pyo3(get)]
    id: String,
    /// Its header: the alias, or the id.
    #[pyo3(get)]
    label: String,
    attributes: Vec<Py<ErAttribute>>,
}

#[pymethods]
impl ErEntity {
    #[getter]
    fn attributes(&self, py: Python<'_>) -> Vec<Py<ErAttribute>> {
        self.attributes.iter().map(|a| a.clone_ref(py)).collect()
    }

    fn __repr__(&self) -> String {
        format!(
            "ErEntity(id={}, label={}, attributes={})",
            repr_str(&self.id),
            repr_str(&self.label),
            self.attributes.len()
        )
    }
}

/// A relationship: `source` and `target` index `entities`. The
/// cardinalities are `"exactly_one"`, `"zero_or_one"`, `"zero_or_more"` or
/// `"one_or_more"`, and `*_notation` what the drawing shows (`1`, `0..1`,
/// `0..N`, `1..N`).
#[pyclass(name = "ErRelationship", module = "rs_rich.mermaid", frozen)]
pub(crate) struct ErRelationship {
    #[pyo3(get)]
    source: usize,
    #[pyo3(get)]
    target: usize,
    #[pyo3(get)]
    source_cardinality: &'static str,
    #[pyo3(get)]
    target_cardinality: &'static str,
    #[pyo3(get)]
    source_notation: &'static str,
    #[pyo3(get)]
    target_notation: &'static str,
    /// `--` (a solid line) rather than `..` (dotted).
    #[pyo3(get)]
    identifying: bool,
    #[pyo3(get)]
    label: String,
}

#[pymethods]
impl ErRelationship {
    fn __repr__(&self) -> String {
        format!(
            "ErRelationship(source={}, target={}, source_cardinality={}, \
             target_cardinality={}, identifying={}, label={})",
            self.source,
            self.target,
            repr_str(self.source_cardinality),
            repr_str(self.target_cardinality),
            if self.identifying { "True" } else { "False" },
            repr_str(&self.label)
        )
    }
}

/// A parsed entity-relationship diagram
/// (`rich_mermaid::er::EntityDiagram`).
#[pyclass(name = "EntityDiagram", module = "rs_rich.mermaid", frozen)]
pub(crate) struct EntityDiagram {
    inner: CoreEntityDiagram,
}

#[pymethods]
impl EntityDiagram {
    #[getter]
    fn title(&self) -> Option<String> {
        self.inner.title.clone()
    }

    /// `"TD"`, `"BT"`, `"LR"` or `"RL"`.
    #[getter]
    fn direction(&self) -> &'static str {
        direction_name(self.inner.direction)
    }

    /// In order of first appearance.
    #[getter]
    fn entities(&self, py: Python<'_>) -> PyResult<Vec<ErEntity>> {
        self.inner
            .entities
            .iter()
            .map(|entity| {
                Ok(ErEntity {
                    id: entity.id.clone(),
                    label: entity.label.clone(),
                    attributes: entity
                        .attributes
                        .iter()
                        .map(|attribute| {
                            Py::new(
                                py,
                                ErAttribute {
                                    data_type: attribute.data_type.clone(),
                                    name: attribute.name.clone(),
                                    keys: attribute.keys.iter().map(|key| key.code()).collect(),
                                    comment: attribute.comment.clone(),
                                },
                            )
                        })
                        .collect::<PyResult<_>>()?,
                })
            })
            .collect()
    }

    #[getter]
    fn relationships(&self) -> Vec<ErRelationship> {
        self.inner
            .relationships
            .iter()
            .map(|relationship| ErRelationship {
                source: relationship.from,
                target: relationship.to,
                source_cardinality: cardinality_name(relationship.from_cardinality),
                target_cardinality: cardinality_name(relationship.to_cardinality),
                source_notation: relationship.from_cardinality.notation(),
                target_notation: relationship.to_cardinality.notation(),
                identifying: relationship.identifying,
                label: relationship.label.clone(),
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
            "<EntityDiagram {} entities={} relationships={}>",
            direction_name(self.inner.direction),
            self.inner.entities.len(),
            self.inner.relationships.len()
        )
    }
}

fn er_of(py: Python<'_>, source: &str) -> PyResult<CoreEntityDiagram> {
    er::parse(source).map_err(|error| parse_error(py, &error))
}

/// Parse a Mermaid ER diagram (`rich_mermaid::er::parse`). Raises
/// `MermaidParseError` as `parse_flowchart` does.
#[pyfunction]
fn parse_er(py: Python<'_>, source: &str) -> PyResult<EntityDiagram> {
    Ok(EntityDiagram {
        inner: er_of(py, source)?,
    })
}

/// Draw an ER diagram as text (`rich_mermaid::er::draw`): an
/// `EntityDiagram`, or source to parse first. Raises `MermaidLayoutError`
/// when it is too large to draw.
#[pyfunction]
#[pyo3(signature = (diagram, ascii=false))]
fn draw_er(py: Python<'_>, diagram: &Bound<'_, PyAny>, ascii: bool) -> PyResult<MermaidDiagram> {
    let diagram = if let Ok(source) = diagram.cast::<PyString>() {
        er_of(py, source.to_cow()?.as_ref())?
    } else if let Ok(diagram) = diagram.cast::<EntityDiagram>() {
        diagram.get().inner.clone()
    } else {
        return Err(PyTypeError::new_err(
            "expected an EntityDiagram or Mermaid source",
        ));
    };
    py.detach(move || er::draw(&diagram, ascii))
        .map(MermaidDiagram::from)
        .map_err(|reason| layout_error(py, reason))
}

/// Whether `source` is an ER diagram (its first statement is `erDiagram`).
#[pyfunction]
fn mermaid_is_er(source: &str) -> bool {
    er::is_er(source)
}

// ---------------------------------------------------------------------------
// Any diagram drawn as text

/// Parse `source` by its header, as `Mermaid` does: a `SequenceDiagram`
/// (`sequenceDiagram`), an `EntityDiagram` (`erDiagram`) or a `Flowchart`.
/// Raises `MermaidParseError`; another diagram type is `"unsupported"`.
#[pyfunction]
fn parse_diagram(py: Python<'_>, source: &str) -> PyResult<Py<PyAny>> {
    Ok(if sequence::is_sequence(source) {
        Py::new(py, parse_sequence(py, source)?)?.into_any()
    } else if er::is_er(source) {
        Py::new(py, parse_er(py, source)?)?.into_any()
    } else {
        Py::new(py, parse_flowchart(py, source)?)?.into_any()
    })
}

/// Draw any diagram drawn as text: a `Flowchart`, `SequenceDiagram` or
/// `EntityDiagram`, or source, parsed by its header.
#[pyfunction]
#[pyo3(signature = (diagram, ascii=false))]
fn draw_diagram(
    py: Python<'_>,
    diagram: &Bound<'_, PyAny>,
    ascii: bool,
) -> PyResult<MermaidDiagram> {
    let parsed;
    let diagram = match diagram.cast::<PyString>() {
        Ok(source) => {
            parsed = parse_diagram(py, source.to_cow()?.as_ref())?.into_bound(py);
            &parsed
        }
        Err(_) => diagram,
    };
    if diagram.is_instance_of::<SequenceDiagram>() {
        draw_sequence(py, diagram, ascii)
    } else if diagram.is_instance_of::<EntityDiagram>() {
        draw_er(py, diagram, ascii)
    } else if diagram.is_instance_of::<Flowchart>() {
        draw_flowchart(py, diagram, ascii)
    } else {
        Err(PyTypeError::new_err(
            "expected a Flowchart, SequenceDiagram, EntityDiagram or Mermaid source",
        ))
    }
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
    m.add_class::<SequenceDiagram>()?;
    m.add_class::<SequenceParticipant>()?;
    m.add_class::<SequenceMessage>()?;
    m.add_class::<SequenceEvent>()?;
    m.add_class::<EntityDiagram>()?;
    m.add_class::<ErEntity>()?;
    m.add_class::<ErAttribute>()?;
    m.add_class::<ErRelationship>()?;
    m.add_function(pyo3::wrap_pyfunction!(parse_flowchart, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(draw_flowchart, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(parse_sequence, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(draw_sequence, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(mermaid_is_sequence, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(parse_er, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(draw_er, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(mermaid_is_er, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(parse_diagram, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(draw_diagram, m)?)?;
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
