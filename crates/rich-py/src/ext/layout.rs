//! `rs_rich.ext.layout` (bounded layouts: constraints, `allocate`,
//! `LayoutNode`, `Overflowing`, `fit_segments`) and `rs_rich.ext.live`
//! (`LiveCoordinator`: several live regions and printed lines through one
//! writer).

use std::io::Write;

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyList, PyString, PyTuple};

use rich::protocol::Renderable;
use rich::segment::Segment as CoreSegment;
use rich_ext::countdown::{CountdownWait, Motion};
use rich_ext::layout::{
    allocate as core_allocate, fit_segments as core_fit, Alignment, Axis,
    Constraint as CoreConstraint, ConstraintError as CoreConstraintError, LayoutNode as CoreNode,
    OverflowPolicy, Overflowing as CoreOverflowing,
};
use rich_ext::live::{LiveCoordinator as CoreLive, LiveError, RegionId as CoreRegionId};

use super::common::{self, names, ConstraintError, LiveCoordinatorError};
use super::diagnostic::overflow_policy;
use super::status::Notifications;
use super::terminal::{policy_arg, RenderTarget};
use super::workflow::token_arg;
use crate::renderable::{self, AsRenderable};

names!(axis, axis_name, Axis, "axis", {
    "horizontal" => Axis::Horizontal,
    "vertical" => Axis::Vertical,
});

names!(alignment, alignment_name, Alignment, "alignment", {
    "start" => Alignment::Start,
    "center" => Alignment::Center,
    "end" => Alignment::End,
});

fn constraint_error(error: CoreConstraintError) -> PyErr {
    ConstraintError::new_err(error.to_string())
}

/// `Constraint(*, min=0, max=None, preferred=None, flex=1)`: how much of an
/// axis a slot takes. `Constraint.fixed(n)` is exactly `n` (no flex).
#[pyclass(
    name = "Constraint",
    module = "rs_rich.ext.layout",
    frozen,
    skip_from_py_object
)]
#[derive(Clone)]
pub(crate) struct Constraint {
    inner: CoreConstraint,
}

fn constraint_arg(value: &Bound<'_, PyAny>) -> PyResult<CoreConstraint> {
    if let Ok(size) = value.extract::<usize>() {
        return Ok(CoreConstraint::fixed(size));
    }
    Ok(value.extract::<PyRef<'_, Constraint>>()?.inner)
}

#[pymethods]
impl Constraint {
    #[new]
    #[pyo3(signature = (*, min=0, max=None, preferred=None, flex=None))]
    fn new(
        min: usize,
        max: Option<usize>,
        preferred: Option<usize>,
        flex: Option<usize>,
    ) -> PyResult<Self> {
        let inner = CoreConstraint {
            min,
            max,
            preferred,
            flex: flex.unwrap_or(if preferred.is_some() { 0 } else { 1 }),
        };
        inner.validate().map_err(constraint_error)?;
        Ok(Constraint { inner })
    }

    #[staticmethod]
    fn fixed(size: usize) -> Self {
        Constraint {
            inner: CoreConstraint::fixed(size),
        }
    }

    #[getter]
    fn min(&self) -> usize {
        self.inner.min
    }
    #[getter]
    fn max(&self) -> Option<usize> {
        self.inner.max
    }
    #[getter]
    fn preferred(&self) -> Option<usize> {
        self.inner.preferred
    }
    #[getter]
    fn flex(&self) -> usize {
        self.inner.flex
    }

    fn __repr__(&self) -> String {
        format!("{:?}", self.inner)
    }
}

/// `allocate(total, constraints)`: split `total` cells between slots as
/// `(sizes, padding, relaxed)`: `padding` is what was left over, `relaxed`
/// the slots whose minimum had to give way.
#[pyfunction]
fn allocate(
    total: usize,
    constraints: &Bound<'_, PyAny>,
) -> PyResult<(Vec<usize>, usize, Vec<usize>)> {
    let constraints: Vec<CoreConstraint> = constraints
        .try_iter()?
        .map(|c| constraint_arg(&c?))
        .collect::<PyResult<_>>()?;
    let allocation = core_allocate(total, &constraints).map_err(constraint_error)?;
    Ok((allocation.sizes, allocation.padding, allocation.relaxed))
}

/// `LayoutNode(renderable)` (a leaf) or `LayoutNode.split(axis, children)`,
/// with `width=`, `height=` (`Constraint`s, `int`s for fixed sizes, or
/// `"content"` to fit the content), `align=(horizontal, vertical)` and
/// `overflow=`. Sizes are allocated by constraint, not ratio, and every
/// cell stays inside its slot.
#[pyclass(name = "LayoutNode", module = "rs_rich.ext.layout")]
pub(crate) struct LayoutNode {
    content: NodeContent,
    width: Size,
    height: Size,
    content_width: bool,
    content_height: bool,
    align: (Alignment, Alignment),
    overflow: OverflowPolicy,
}

enum NodeContent {
    Leaf(Py<PyAny>),
    Split(Axis, Vec<Py<LayoutNode>>),
}

#[derive(Clone, Copy)]
enum Size {
    Default,
    Content,
    Constraint(CoreConstraint),
}

fn size_arg(value: Option<&Bound<'_, PyAny>>) -> PyResult<Size> {
    let Some(value) = value.filter(|v| !v.is_none()) else {
        return Ok(Size::Default);
    };
    if let Ok(name) = value.cast::<PyString>() {
        return match name.to_cow()?.as_ref() {
            "content" => Ok(Size::Content),
            other => Err(PyValueError::new_err(format!(
                "invalid size {other:?}; expected a Constraint, an int or \"content\""
            ))),
        };
    }
    let constraint = constraint_arg(value)?;
    constraint.validate().map_err(constraint_error)?;
    Ok(Size::Constraint(constraint))
}

impl LayoutNode {
    fn build(&self, py: Python<'_>) -> PyResult<CoreNode> {
        let _nesting = renderable::Nesting::enter()?;
        let mut node = match &self.content {
            NodeContent::Leaf(value) => {
                CoreNode::leaf(renderable::to_renderable(value.bind(py), None)?)
            }
            NodeContent::Split(axis, children) => CoreNode::split(
                *axis,
                children
                    .iter()
                    .map(|child| child.bind(py).borrow().build(py))
                    .collect::<PyResult<_>>()?,
            ),
        };
        node = match self.width {
            Size::Default => node,
            Size::Content => node.content_width(),
            Size::Constraint(c) => node.width(c),
        };
        node = match self.height {
            Size::Default => node,
            Size::Content => node.content_height(),
            Size::Constraint(c) => node.height(c),
        };
        if self.content_width {
            node = node.content_width();
        }
        if self.content_height {
            node = node.content_height();
        }
        Ok(node
            .align(self.align.0, self.align.1)
            .overflow(self.overflow))
    }
}

impl AsRenderable for LayoutNode {
    fn to_renderable(&self, py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        let node = self.build(py)?;
        node.validate().map_err(constraint_error)?;
        Ok(Box::new(node))
    }
}

fn align_arg(value: Option<(String, String)>) -> PyResult<(Alignment, Alignment)> {
    match value {
        None => Ok((Alignment::Start, Alignment::Start)),
        Some((h, v)) => Ok((alignment(&h)?, alignment(&v)?)),
    }
}

#[pymethods]
impl LayoutNode {
    #[new]
    #[pyo3(signature = (renderable, *, width=None, height=None, content_width=false, content_height=false, align=None, overflow="fold"))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        renderable: Py<PyAny>,
        width: Option<&Bound<'_, PyAny>>,
        height: Option<&Bound<'_, PyAny>>,
        content_width: bool,
        content_height: bool,
        align: Option<(String, String)>,
        overflow: &str,
    ) -> PyResult<Self> {
        Ok(LayoutNode {
            content: NodeContent::Leaf(renderable),
            width: size_arg(width)?,
            height: size_arg(height)?,
            content_width,
            content_height,
            align: align_arg(align)?,
            overflow: overflow_policy(overflow)?,
        })
    }

    /// Children side by side (`horizontal`) or stacked (`vertical`).
    #[staticmethod]
    #[pyo3(signature = (axis, children, *, width=None, height=None, content_width=false, content_height=false, align=None, overflow="fold"))]
    #[allow(clippy::too_many_arguments)]
    fn split(
        axis: &str,
        children: &Bound<'_, PyAny>,
        width: Option<&Bound<'_, PyAny>>,
        height: Option<&Bound<'_, PyAny>>,
        content_width: bool,
        content_height: bool,
        align: Option<(String, String)>,
        overflow: &str,
    ) -> PyResult<Self> {
        let py = children.py();
        let children = children
            .try_iter()?
            .map(|child| {
                let child = child?;
                match child.extract::<Py<LayoutNode>>() {
                    Ok(node) => Ok(node),
                    Err(_) => Py::new(
                        py,
                        LayoutNode::new(child.unbind(), None, None, false, false, None, "fold")?,
                    ),
                }
            })
            .collect::<PyResult<_>>()?;
        Ok(LayoutNode {
            content: NodeContent::Split(self::axis(axis)?, children),
            width: size_arg(width)?,
            height: size_arg(height)?,
            content_width,
            content_height,
            align: align_arg(align)?,
            overflow: overflow_policy(overflow)?,
        })
    }

    /// Check every constraint (raises `ConstraintError`).
    fn validate(&self, py: Python<'_>) -> PyResult<()> {
        self.build(py)?.validate().map_err(constraint_error)
    }

    fn __traverse__(&self, visit: pyo3::PyVisit<'_>) -> Result<(), pyo3::PyTraverseError> {
        match &self.content {
            NodeContent::Leaf(value) => visit.call(value),
            NodeContent::Split(_, children) => {
                for child in children {
                    visit.call(child)?;
                }
                Ok(())
            }
        }
    }
}

/// `Overflowing(renderable, overflow="fold")`: any renderable with one
/// overflow policy (`wrap`, `fold`, `crop`, `ellipsis`, `visible`) applied
/// to every line; output that fits is unchanged.
#[pyclass(name = "Overflowing", module = "rs_rich.ext.layout", frozen)]
pub(crate) struct Overflowing {
    renderable: Py<PyAny>,
    policy: OverflowPolicy,
}

impl AsRenderable for Overflowing {
    fn to_renderable(&self, py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        let inner = renderable::to_renderable(self.renderable.bind(py), None)?;
        Ok(Box::new(CoreOverflowing::new(inner, self.policy)))
    }
}

#[pymethods]
impl Overflowing {
    #[new]
    #[pyo3(signature = (renderable, overflow="fold"))]
    fn new(renderable: Py<PyAny>, overflow: &str) -> PyResult<Self> {
        Ok(Overflowing {
            renderable,
            policy: overflow_policy(overflow)?,
        })
    }

    #[getter]
    fn overflow(&self) -> &'static str {
        super::diagnostic::overflow_policy_name(self.policy)
    }

    fn __traverse__(&self, visit: pyo3::PyVisit<'_>) -> Result<(), pyo3::PyTraverseError> {
        visit.call(&self.renderable)
    }
}

fn segments_arg(value: &Bound<'_, PyAny>) -> PyResult<Vec<CoreSegment>> {
    value
        .try_iter()?
        .map(|s| {
            Ok(s?
                .extract::<PyRef<'_, crate::segment::Segment>>()?
                .to_core())
        })
        .collect()
}

/// `fit_segments(segments, width, overflow="fold")`: `Segment`s laid out
/// in lines of at most `width` cells, styles kept.
#[pyfunction]
#[pyo3(signature = (segments, width, overflow="fold"))]
fn fit_segments<'py>(
    py: Python<'py>,
    segments: &Bound<'py, PyAny>,
    width: usize,
    overflow: &str,
) -> PyResult<Vec<Bound<'py, PyList>>> {
    core_fit(&segments_arg(segments)?, width, overflow_policy(overflow)?)
        .iter()
        .map(|line| crate::segment::to_python(py, line))
        .collect()
}

// ---------------------------------------------------------------------------
// Live coordination

/// A Python text file as a Rust writer.
pub(crate) struct PyTextWriter(pub(crate) Option<Py<PyAny>>);

impl Write for PyTextWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        Python::attach(|py| {
            let file = match &self.0 {
                Some(file) => file.bind(py).clone(),
                None => py
                    .import("sys")
                    .and_then(|sys| sys.getattr("stdout"))
                    .map_err(|e| std::io::Error::other(e.to_string()))?,
            };
            let text = String::from_utf8_lossy(buf);
            file.call_method1("write", (text.as_ref(),))
                .map_err(|e| std::io::Error::other(e.to_string()))?;
            Ok(buf.len())
        })
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Python::attach(|py| {
            let file = match &self.0 {
                Some(file) => file.bind(py).clone(),
                None => py
                    .import("sys")
                    .and_then(|sys| sys.getattr("stdout"))
                    .map_err(|e| std::io::Error::other(e.to_string()))?,
            };
            if let Some(flush) = file
                .getattr_opt("flush")
                .map_err(|e| std::io::Error::other(e.to_string()))?
            {
                flush
                    .call0()
                    .map_err(|e| std::io::Error::other(e.to_string()))?;
            }
            Ok(())
        })
    }
}

fn live_error(error: LiveError) -> PyErr {
    LiveCoordinatorError::new_err(error.to_string())
}

/// A region of a `LiveCoordinator` (`add` returns one).
#[pyclass(name = "RegionId", module = "rs_rich.ext.live", frozen)]
pub(crate) struct RegionId {
    inner: CoreRegionId,
}

/// `LiveCoordinator(file=None, *, target=None)`: several live regions
/// (progress, status, toasts) and ordinary printed lines, all through one
/// writer so nothing tears. `target` is a `RenderTarget` (default: an
/// interactive 80x24 terminal). Use as a context manager, or call
/// `finish()`.
#[pyclass(name = "LiveCoordinator", module = "rs_rich.ext.live")]
pub(crate) struct LiveCoordinator {
    inner: CoreLive<PyTextWriter>,
    target: rich_ext::target::RenderTarget,
}

impl LiveCoordinator {
    /// A Python renderable as segments for this coordinator's target.
    fn segments(
        &self,
        py: Python<'_>,
        renderable: &Bound<'_, PyAny>,
    ) -> PyResult<Vec<CoreSegment>> {
        let c = rich::protocol::RenderEnvironment::capabilities(&self.target);
        common::scoped(py, c.width, c.height, c.interactive, || {
            let value = renderable::to_renderable(renderable, None)?;
            Ok(self.target.segments(value.as_ref()))
        })
    }
}

fn default_target() -> rich_ext::target::RenderTarget {
    rich_ext::target::RenderTarget::new(
        rich_ext::target::TargetKind::Terminal,
        rich::protocol::TargetCapabilities {
            width: 80,
            height: 24,
            color_system: Some(rich::ColorSystem::Truecolor),
            interactive: true,
            unicode: true,
            hyperlinks: true,
            sixel: rich::protocol::Support::Unsupported,
        },
        rich::theme::Theme::default_theme(),
    )
}

#[pymethods]
impl LiveCoordinator {
    #[new]
    #[pyo3(signature = (file=None, *, target=None))]
    fn new(file: Option<Py<PyAny>>, target: Option<PyRef<'_, RenderTarget>>) -> Self {
        let target = target.map_or_else(default_target, |t| t.inner.clone());
        LiveCoordinator {
            inner: CoreLive::new(PyTextWriter(file), target.clone()),
            target,
        }
    }

    /// Add a region showing `renderable`; returns its id.
    fn add(&mut self, py: Python<'_>, renderable: &Bound<'_, PyAny>) -> PyResult<RegionId> {
        let segments = self.segments(py, renderable)?;
        Ok(RegionId {
            inner: self.inner.add(segments).map_err(live_error)?,
        })
    }

    /// Replace a region's content.
    fn update(
        &mut self,
        py: Python<'_>,
        region: PyRef<'_, RegionId>,
        renderable: &Bound<'_, PyAny>,
    ) -> PyResult<()> {
        let segments = self.segments(py, renderable)?;
        self.inner
            .update(region.inner.clone(), segments)
            .map_err(live_error)
    }

    fn remove(&mut self, region: PyRef<'_, RegionId>) -> PyResult<()> {
        self.inner.remove(region.inner.clone()).map_err(live_error)
    }

    /// Repaint the regions that changed.
    fn refresh(&mut self) -> PyResult<()> {
        self.inner.refresh().map_err(live_error)
    }

    fn resize(&mut self, width: usize, height: usize) -> PyResult<()> {
        self.inner.resize(width, height).map_err(live_error)
    }

    /// Print `renderable` above the regions.
    fn print(&mut self, py: Python<'_>, renderable: &Bound<'_, PyAny>) -> PyResult<()> {
        let segments = self.segments(py, renderable)?;
        self.inner.print(&segments).map_err(live_error)
    }

    /// Clear the regions (or, not interactive, write their final state).
    fn finish(&mut self) -> PyResult<()> {
        self.inner.finish().map_err(live_error)
    }

    /// Show `notifications` in one region (log lines printed above), as of
    /// `now` seconds.
    fn present(
        &mut self,
        notifications: &Bound<'_, Notifications>,
        now: &Bound<'_, PyAny>,
    ) -> PyResult<()> {
        let now = common::seconds(now)?;
        let mut notifications = notifications.borrow_mut();
        let mut region = notifications.region.take();
        let result = notifications
            .inner
            .present(&mut self.inner, &self.target, &mut region, now);
        notifications.region = region;
        result.map_err(live_error)
    }

    /// Count down `total` seconds in a region, drawing `frame(seconds_left)`
    /// each tick (printed once, not animated, with `animate=False`, a
    /// non-interactive target or a reduced-motion `policy`).
    /// `"elapsed"` or `"cancelled"`.
    #[pyo3(signature = (total, frame, *, tick=None, cancel=None, animate=true, policy=None, sleep=None))]
    #[allow(clippy::too_many_arguments)]
    fn countdown(
        &mut self,
        py: Python<'_>,
        total: &Bound<'_, PyAny>,
        frame: Py<PyAny>,
        tick: Option<&Bound<'_, PyAny>>,
        cancel: Option<&Bound<'_, PyAny>>,
        animate: bool,
        policy: Option<&Bound<'_, PyAny>>,
        sleep: Option<Py<PyAny>>,
    ) -> PyResult<&'static str> {
        let mut wait = CountdownWait::new(common::seconds(total)?);
        if let Some(tick) = common::opt_seconds(tick)? {
            wait = wait.tick(tick);
        }
        if let Some(token) = token_arg(cancel)? {
            wait = wait.cancel(token);
        }
        let policy = policy_arg(policy)?.unwrap_or_default();
        let motion = if animate {
            Motion::for_target(&self.target, &policy)
        } else {
            Motion::Static
        };
        let error: std::cell::RefCell<Option<PyErr>> = std::cell::RefCell::new(None);
        let wait = wait.sleeper(|step| {
            if error.borrow().is_some() {
                return;
            }
            let result = match &sleep {
                Some(sleep) => sleep.bind(py).call1((step.as_secs_f64(),)).map(|_| ()),
                None => {
                    py.detach(|| std::thread::sleep(step));
                    py.check_signals()
                }
            };
            if let Err(e) = result {
                *error.borrow_mut() = Some(e);
            }
        });
        let target = self.target.clone();
        let frame_of = |left: std::time::Duration| -> common::Boxed {
            let rendered = frame
                .bind(py)
                .call1((left.as_secs_f64(),))
                .and_then(|value| {
                    let c = rich::protocol::RenderEnvironment::capabilities(&target);
                    common::scoped(py, c.width, c.height, c.interactive, || {
                        let value = renderable::to_renderable(&value, None)?;
                        Ok(target.segments(value.as_ref()))
                    })
                });
            let segments = rendered.unwrap_or_else(|e| {
                if error.borrow().is_none() {
                    *error.borrow_mut() = Some(e);
                }
                Vec::new()
            });
            common::Boxed(Box::new(Prerendered(segments)))
        };
        let outcome = wait.run_live(&mut self.inner, &self.target, motion, frame_of);
        if let Some(error) = error.into_inner() {
            return Err(error);
        }
        Ok(match outcome.map_err(live_error)? {
            rich_ext::countdown::WaitOutcome::Elapsed => "elapsed",
            rich_ext::countdown::WaitOutcome::Cancelled => "cancelled",
        })
    }

    fn __enter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    #[pyo3(signature = (*_args))]
    fn __exit__(&mut self, _args: &Bound<'_, PyTuple>) -> PyResult<bool> {
        self.finish()?;
        Ok(false)
    }
}

/// Segments already rendered for a target, as a renderable.
struct Prerendered(Vec<CoreSegment>);

impl Renderable for Prerendered {
    fn rich_render(
        &self,
        _console: &rich::Console,
        _options: &rich::ConsoleOptions,
    ) -> Vec<CoreSegment> {
        self.0.clone()
    }
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Constraint>()?;
    m.add_function(pyo3::wrap_pyfunction!(allocate, m)?)?;
    renderable::add_renderable_class::<LayoutNode>(m)?;
    renderable::add_renderable_class::<Overflowing>(m)?;
    m.add_function(pyo3::wrap_pyfunction!(fit_segments, m)?)?;
    m.add_class::<RegionId>()?;
    m.add_class::<LiveCoordinator>()?;
    Ok(())
}
