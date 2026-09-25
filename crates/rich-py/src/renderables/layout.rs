//! `rich.layout`: `Layout`, its splitters and errors. Port of upstream
//! `rich/layout.py`.
//!
//! Core's `Layout` owns its children, draws an empty leaf as blank space
//! and keeps no render map; upstream's layouts are shared Python objects
//! (`layout["name"].update(...)`), an empty leaf shows a placeholder panel,
//! and `layout.map` holds the last render. So the layout here is ported in
//! Rust over Python `Layout` objects: region division (`ratio_resolve`),
//! the placeholder, `tree` and the render all live here, and each leaf is
//! rendered through the Python console's `render_lines`, as upstream does.

use std::sync::Arc;

use pyo3::create_exception;
use pyo3::exceptions::{PyException, PyKeyError, PyNotImplementedError};
use pyo3::prelude::*;
use pyo3::sync::PyOnceLock;
use pyo3::types::{PyDict, PyList, PyString, PyTuple, PyType};
use pyo3::{PyTraverseError, PyVisit};

use rich::console::{Console as CoreConsole, ConsoleOptions as CoreOptions};
use rich::measure::Measurement as CoreMeasurement;
use rich::panel::Panel as CorePanel;
use rich::protocol::{Highlighter, Renderable};
use rich::ratio::{ratio_resolve, Edge};
use rich::segment::Segment as CoreSegment;
use rich::style::{Style as CoreStyle, StyleType};
use rich::table::{Cell, Table as CoreTable};
use rich::{Color as CoreColor, ReprHighlighter, Text as CoreText};

use crate::renderable::{self, AsRenderable};
use crate::segment::Segment;

use super::align::{AlignRender, Horizontal, Vertical};
use super::constrain::StyledRender;
use super::{screen_height, text_markup};

create_exception!(_native, LayoutError, PyException, "Layout related error.");
create_exception!(_native, NoSplitter, LayoutError, "Requested splitter does not exist.");

#[derive(Clone, Copy, PartialEq, Eq)]
enum Split {
    Row,
    Column,
}

impl Split {
    fn name(self) -> &'static str {
        match self {
            Split::Row => "row",
            Split::Column => "column",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Split::Row => "[layout.tree.row]⬌",
            Split::Column => "[layout.tree.column]⬍",
        }
    }
}

// ---------------------------------------------------------------------------
// Named tuples

static REGION: PyOnceLock<Py<PyType>> = PyOnceLock::new();
static LAYOUT_RENDER: PyOnceLock<Py<PyType>> = PyOnceLock::new();

fn named_tuple<'py>(
    py: Python<'py>,
    cell: &'static PyOnceLock<Py<PyType>>,
    name: &str,
    fields: &[&str],
) -> PyResult<&'py Bound<'py, PyType>> {
    cell.get_or_try_init(py, || {
        let namedtuple = py.import("collections")?.getattr("namedtuple")?;
        let class = namedtuple.call1((name, fields.to_vec()))?;
        class.setattr("__module__", "rs_rich.layout")?;
        Ok::<_, PyErr>(class.cast_into::<PyType>()?.unbind())
    })
    .map(|class| class.bind(py))
}

/// `rich.region.Region(x, y, width, height)`.
fn region_type(py: Python<'_>) -> PyResult<&Bound<'_, PyType>> {
    named_tuple(py, &REGION, "Region", &["x", "y", "width", "height"])
}

/// `rich.layout.LayoutRender(region, render)`.
fn layout_render_type(py: Python<'_>) -> PyResult<&Bound<'_, PyType>> {
    named_tuple(py, &LAYOUT_RENDER, "LayoutRender", &["region", "render"])
}

type Region = (usize, usize, usize, usize);

// ---------------------------------------------------------------------------
// Splitters

/// `rich.layout.Splitter`: the base class of the splitters.
#[pyclass(name = "Splitter", module = "rs_rich.layout", subclass)]
pub(crate) struct Splitter;

#[pymethods]
impl Splitter {
    #[new]
    fn new() -> Self {
        Splitter
    }

    #[classattr]
    fn name() -> &'static str {
        ""
    }
}

fn divide<'py>(
    split: Split,
    children: &Bound<'py, PyAny>,
    region: &Bound<'py, PyAny>,
) -> PyResult<Bound<'py, PyList>> {
    let py = children.py();
    let (x, y, width, height): Region = region.extract()?;
    let children: Vec<Bound<'py, PyAny>> = children.try_iter()?.collect::<PyResult<_>>()?;
    let mut edges = Vec::with_capacity(children.len());
    for child in &children {
        edges.push(Edge::new(
            child.getattr("size")?.extract()?,
            child.getattr("ratio")?.extract()?,
            child.getattr("minimum_size")?.extract()?,
        ));
    }
    let result = PyList::empty(py);
    let total = match split {
        Split::Row => width,
        Split::Column => height,
    };
    let mut offset = 0;
    for (child, size) in children.iter().zip(ratio_resolve(total, &edges)) {
        let region = match split {
            Split::Row => (x + offset, y, size, height),
            Split::Column => (x, y + offset, width, size),
        };
        result.append((child, region_type(py)?.call1(region)?))?;
        offset += size;
    }
    Ok(result)
}

/// `rich.layout.RowSplitter`: split a region into side-by-side children.
#[pyclass(name = "RowSplitter", module = "rs_rich.layout", extends = Splitter)]
pub(crate) struct RowSplitter;

#[pymethods]
impl RowSplitter {
    #[new]
    fn new() -> PyClassInitializer<Self> {
        PyClassInitializer::from(Splitter).add_subclass(RowSplitter)
    }

    #[classattr]
    fn name() -> &'static str {
        "row"
    }

    fn get_tree_icon(&self) -> &'static str {
        Split::Row.icon()
    }

    fn divide<'py>(
        &self,
        children: &Bound<'py, PyAny>,
        region: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyList>> {
        divide(Split::Row, children, region)
    }
}

/// `rich.layout.ColumnSplitter`: split a region into stacked children.
#[pyclass(name = "ColumnSplitter", module = "rs_rich.layout", extends = Splitter)]
pub(crate) struct ColumnSplitter;

#[pymethods]
impl ColumnSplitter {
    #[new]
    fn new() -> PyClassInitializer<Self> {
        PyClassInitializer::from(Splitter).add_subclass(ColumnSplitter)
    }

    #[classattr]
    fn name() -> &'static str {
        "column"
    }

    fn get_tree_icon(&self) -> &'static str {
        Split::Column.icon()
    }

    fn divide<'py>(
        &self,
        children: &Bound<'py, PyAny>,
        region: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyList>> {
        divide(Split::Column, children, region)
    }
}

fn splitter_arg(splitter: &Bound<'_, PyAny>) -> PyResult<Split> {
    if splitter.is_instance_of::<RowSplitter>() {
        return Ok(Split::Row);
    }
    if splitter.is_instance_of::<ColumnSplitter>() {
        return Ok(Split::Column);
    }
    if splitter.is_instance_of::<Splitter>() {
        return Err(PyNotImplementedError::new_err(
            "rs_rich layouts split only with RowSplitter and ColumnSplitter",
        ));
    }
    match splitter.extract::<String>()?.as_str() {
        "row" => Ok(Split::Row),
        "column" => Ok(Split::Column),
        other => Err(NoSplitter::new_err(format!(
            "No splitter called {}",
            PyString::new(splitter.py(), other).repr()?
        ))),
    }
}

// ---------------------------------------------------------------------------
// Pretty-printing a layout, for the placeholder and `tree`

/// `Pretty(layout)`: the layout's `rich_repr`, on one line when it fits,
/// else one argument per line, highlighted.
struct PrettyRepr {
    parts: Vec<String>,
}

impl PrettyRepr {
    fn lines(&self, max_width: usize) -> Vec<String> {
        let one_line = format!("Layout({})", self.parts.join(", "));
        if self.parts.is_empty() || rich::cells::cell_len(&one_line) <= max_width {
            return vec![one_line];
        }
        let mut lines = vec!["Layout(".to_string()];
        let last = self.parts.len() - 1;
        for (index, part) in self.parts.iter().enumerate() {
            let comma = if index == last { "" } else { "," };
            lines.push(format!("    {part}{comma}"));
        }
        lines.push(")".to_string());
        lines
    }

    fn text(&self, max_width: usize) -> CoreText {
        let mut text = CoreText::styled(self.lines(max_width).join("\n"), "pretty");
        ReprHighlighter::new().highlight(&mut text);
        text
    }
}

impl Renderable for PrettyRepr {
    fn rich_render(&self, console: &CoreConsole, options: &CoreOptions) -> Vec<CoreSegment> {
        self.text(options.max_width).rich_render(console, options)
    }

    fn measure(&self, _console: &CoreConsole, options: &CoreOptions) -> CoreMeasurement {
        let width = self
            .lines(options.max_width)
            .iter()
            .map(|line| rich::cells::cell_len(line))
            .max()
            .unwrap_or(0);
        CoreMeasurement::new(width, width)
    }
}

/// `Layout._Placeholder`: a panel naming the empty region and its size.
struct PlaceholderRender {
    name: Option<String>,
    parts: Vec<String>,
}

impl Renderable for PlaceholderRender {
    fn rich_render(&self, console: &CoreConsole, options: &CoreOptions) -> Vec<CoreSegment> {
        let width = options.max_width;
        let height = options.height.unwrap_or_else(|| screen_height(console));
        let size = format!("({width} x {height})");
        let mut title = CoreText::new(match &self.name {
            Some(name) => format!("{name} {size}"),
            None => size,
        });
        ReprHighlighter::new().highlight(&mut title);
        let mut body = AlignRender::new(
            Box::new(PrettyRepr {
                parts: self.parts.clone(),
            }) as Box<dyn Renderable>,
            Horizontal::Center,
        );
        body.vertical = Some(Vertical::Middle);
        let blue = CoreStyle::new().with_color(CoreColor::parse("blue").expect("a valid colour"));
        let panel = CorePanel::new(Box::new(body))
            .title(text_markup(&title))
            .border_style(blue);
        let mut options = options.clone();
        options.height = Some(height);
        panel.rich_render(console, &options)
    }
}

/// The renderable a layout without content shows.
#[pyclass(name = "_Placeholder", module = "rs_rich.layout")]
struct Placeholder {
    layout: Py<Layout>,
}

impl AsRenderable for Placeholder {
    fn to_renderable(&self, py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        let layout = self.layout.bind(py).borrow();
        Ok(Box::new(PlaceholderRender {
            name: layout.name_repr(py)?,
            parts: layout.repr_parts(py)?,
        }))
    }
}

#[pymethods]
impl Placeholder {
    #[getter]
    fn layout(&self, py: Python<'_>) -> Py<Layout> {
        self.layout.clone_ref(py)
    }

    fn __traverse__(&self, visit: PyVisit<'_>) -> Result<(), PyTraverseError> {
        visit.call(&self.layout)
    }
}

/// A `layout.tree` label: the splitter's icon beside `Pretty(layout)`, dim
/// when the layout is hidden.
#[pyclass(name = "_LayoutSummary", module = "rs_rich.layout")]
struct Summary {
    layout: Py<Layout>,
}

impl AsRenderable for Summary {
    fn to_renderable(&self, py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        let layout = self.layout.bind(py).borrow();
        let pretty = PrettyRepr {
            parts: layout.repr_parts(py)?,
        };
        let pretty: Arc<dyn Renderable + Send + Sync> = if layout.visible {
            Arc::new(pretty)
        } else {
            Arc::new(StyledRender {
                child: Arc::new(pretty) as Arc<dyn Renderable + Send + Sync>,
                style: StyleType::Name("dim".to_string()),
            })
        };
        let mut table = CoreTable::grid().padding(0, 1, 0, 0);
        table.add_column("");
        table.add_column("");
        table.add_row_cells(vec![
            Cell::Markup(layout.splitter.icon().to_string()),
            Cell::Renderable(pretty),
        ]);
        Ok(Box::new(table))
    }
}

#[pymethods]
impl Summary {
    fn __traverse__(&self, visit: PyVisit<'_>) -> Result<(), PyTraverseError> {
        visit.call(&self.layout)
    }
}

// ---------------------------------------------------------------------------
// Layout

/// `rich.layout.Layout`: divide a fixed height into rows or columns.
#[pyclass(name = "Layout", module = "rs_rich.layout")]
pub(crate) struct Layout {
    /// `None`: the placeholder.
    renderable: Option<Py<PyAny>>,
    #[pyo3(get, set)]
    size: Option<usize>,
    #[pyo3(get, set)]
    minimum_size: usize,
    #[pyo3(get, set)]
    ratio: usize,
    #[pyo3(get, set)]
    name: Option<String>,
    #[pyo3(get, set)]
    visible: bool,
    splitter: Split,
    children: Vec<Py<Layout>>,
    render_map: Py<PyDict>,
}

impl Layout {
    fn name_repr(&self, py: Python<'_>) -> PyResult<Option<String>> {
        self.name
            .as_ref()
            .map(|name| Ok(PyString::new(py, name).repr()?.to_string()))
            .transpose()
    }

    /// `__rich_repr__` as `key=value` strings (defaults left out).
    fn repr_parts(&self, py: Python<'_>) -> PyResult<Vec<String>> {
        let mut parts = Vec::new();
        if let Some(name) = self.name_repr(py)? {
            parts.push(format!("name={name}"));
        }
        if let Some(size) = self.size {
            parts.push(format!("size={size}"));
        }
        if self.minimum_size != 1 {
            parts.push(format!("minimum_size={}", self.minimum_size));
        }
        if self.ratio != 1 {
            parts.push(format!("ratio={}", self.ratio));
        }
        Ok(parts)
    }

    fn visible_children(&self, py: Python<'_>) -> Vec<Py<Layout>> {
        self.children
            .iter()
            .filter(|child| child.bind(py).borrow().visible)
            .map(|child| child.clone_ref(py))
            .collect()
    }

    fn wrap(py: Python<'_>, layouts: &Bound<'_, PyTuple>) -> PyResult<Vec<Py<Layout>>> {
        layouts
            .iter()
            .map(|layout| match layout.cast::<Layout>() {
                Ok(layout) => Ok(layout.clone().unbind()),
                Err(_) => Py::new(py, Layout::build(py, Some(layout.unbind()))),
            })
            .collect()
    }

    fn build(py: Python<'_>, renderable: Option<Py<PyAny>>) -> Layout {
        Layout {
            renderable,
            size: None,
            minimum_size: 1,
            ratio: 1,
            name: None,
            visible: true,
            splitter: Split::Column,
            children: Vec::new(),
            render_map: PyDict::new(py).unbind(),
        }
    }

    /// `Layout._make_region_map`: every layout's region, sorted by region.
    fn region_map(
        slf: &Bound<'_, Layout>,
        width: usize,
        height: usize,
    ) -> PyResult<Vec<(Py<Layout>, Region)>> {
        let py = slf.py();
        let mut stack: Vec<(Py<Layout>, Region)> = vec![(slf.clone().unbind(), (0, 0, width, height))];
        let mut regions: Vec<(Py<Layout>, Region)> = Vec::new();
        let mut guard = 0usize;
        while let Some((layout, region)) = stack.pop() {
            guard += 1;
            if guard > 1_000_000 {
                return Err(pyo3::exceptions::PyRecursionError::new_err(
                    "a Layout cannot contain itself",
                ));
            }
            let (x, y, width, height) = region;
            let (split, children) = {
                let layout = layout.bind(py).borrow();
                (layout.splitter, layout.visible_children(py))
            };
            regions.push((layout, region));
            if children.is_empty() {
                continue;
            }
            let edges: Vec<Edge> = children
                .iter()
                .map(|child| {
                    let child = child.bind(py).borrow();
                    Edge::new(child.size, child.ratio, child.minimum_size)
                })
                .collect();
            let total = match split {
                Split::Row => width,
                Split::Column => height,
            };
            let mut offset = 0;
            for (child, size) in children.into_iter().zip(ratio_resolve(total, &edges)) {
                let child_region = match split {
                    Split::Row => (x + offset, y, size, height),
                    Split::Column => (x, y + offset, width, size),
                };
                stack.push((child, child_region));
                offset += size;
            }
        }
        regions.sort_by_key(|(_, region)| *region);
        Ok(regions)
    }

    /// `Layout.render(console, options)`: each leaf rendered into its region.
    fn render_map<'py>(
        slf: &Bound<'py, Layout>,
        console: &Bound<'py, PyAny>,
        options: &Bound<'py, PyAny>,
    ) -> PyResult<Vec<(Py<Layout>, Region, Bound<'py, PyAny>)>> {
        let py = slf.py();
        let width: usize = options.getattr("max_width")?.extract()?;
        let height: Option<usize> = options.getattr("height")?.extract()?;
        let height = match height {
            Some(height) => height,
            None => console.getattr("height")?.extract()?,
        };
        let mut rendered = Vec::new();
        for (layout, region) in Layout::region_map(slf, width, height)? {
            let leaf = layout.bind(py);
            if !leaf.borrow().visible_children(py).is_empty() {
                continue;
            }
            let (_, _, width, height) = region;
            let renderable = Layout::renderable(leaf)?;
            let options = options.call_method1("update_dimensions", (width, height))?;
            let kwargs = PyDict::new(py);
            kwargs.set_item("options", options)?;
            let lines = console.call_method("render_lines", (renderable,), Some(&kwargs))?;
            rendered.push((layout, region, lines));
        }
        Ok(rendered)
    }
}

#[pymethods]
impl Layout {
    #[classattr]
    fn splitters(py: Python<'_>) -> PyResult<Py<PyDict>> {
        let splitters = PyDict::new(py);
        splitters.set_item("row", py.get_type::<RowSplitter>())?;
        splitters.set_item("column", py.get_type::<ColumnSplitter>())?;
        Ok(splitters.unbind())
    }

    #[new]
    #[pyo3(signature = (
        renderable=None, *, name=None, size=None, minimum_size=1, ratio=1, visible=true
    ))]
    fn new(
        py: Python<'_>,
        renderable: Option<Py<PyAny>>,
        name: Option<String>,
        size: Option<usize>,
        minimum_size: usize,
        ratio: usize,
        visible: bool,
    ) -> PyResult<Self> {
        // `renderable or _Placeholder(self)`.
        let renderable = match renderable {
            Some(renderable) if renderable.bind(py).is_truthy()? => Some(renderable),
            _ => None,
        };
        Ok(Layout {
            size,
            minimum_size,
            ratio,
            name,
            visible,
            ..Layout::build(py, renderable)
        })
    }

    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        Ok(format!("Layout({})", self.repr_parts(py)?.join(", ")))
    }

    fn __rich_repr__<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyList>> {
        let none = py.None();
        let items = PyList::empty(py);
        items.append(("name", self.name.clone(), none.clone_ref(py)))?;
        items.append(("size", self.size, none))?;
        items.append(("minimum_size", self.minimum_size, 1))?;
        items.append(("ratio", self.ratio, 1))?;
        Ok(items)
    }

    /// The layout's content: itself when split, else its renderable (or the
    /// placeholder).
    #[getter]
    fn renderable(slf: &Bound<'_, Self>) -> PyResult<Py<PyAny>> {
        let py = slf.py();
        let layout = slf.borrow();
        if !layout.children.is_empty() {
            return Ok(slf.clone().into_any().unbind());
        }
        match &layout.renderable {
            Some(renderable) => Ok(renderable.clone_ref(py)),
            None => Ok(Py::new(
                py,
                Placeholder {
                    layout: slf.clone().unbind(),
                },
            )?
            .into_any()),
        }
    }

    /// The visible children.
    #[getter]
    fn children(&self, py: Python<'_>) -> Vec<Py<Layout>> {
        self.visible_children(py)
    }

    /// The splitter this layout divides its region with.
    #[getter]
    fn splitter(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        Ok(match self.splitter {
            Split::Row => Py::new(py, RowSplitter::new())?.into_any(),
            Split::Column => Py::new(py, ColumnSplitter::new())?.into_any(),
        })
    }

    #[setter]
    fn set_splitter(&mut self, splitter: &Bound<'_, PyAny>) -> PyResult<()> {
        self.splitter = splitter_arg(splitter)?;
        Ok(())
    }

    /// The last render: `{layout: LayoutRender(region, lines)}`.
    #[getter]
    fn map(&self, py: Python<'_>) -> Py<PyDict> {
        self.render_map.clone_ref(py)
    }

    /// The layout called `name` (this one or a descendant), or `None`.
    fn get(slf: &Bound<'_, Self>, name: &str) -> Option<Py<Layout>> {
        let py = slf.py();
        let mut pending = vec![slf.clone().unbind()];
        let mut seen = 0usize;
        while let Some(layout) = pending.pop() {
            seen += 1;
            if seen > 1_000_000 {
                return None;
            }
            let bound = layout.bind(py).borrow();
            if bound.name.as_deref() == Some(name) {
                drop(bound);
                return Some(layout);
            }
            pending.extend(bound.children.iter().rev().map(|child| child.clone_ref(py)));
        }
        None
    }

    fn __getitem__(slf: &Bound<'_, Self>, name: &str) -> PyResult<Py<Layout>> {
        Layout::get(slf, name).ok_or_else(|| {
            PyKeyError::new_err(format!(
                "No layout with name {}",
                PyString::new(slf.py(), name)
                    .repr()
                    .map(|r| r.to_string())
                    .unwrap_or_default()
            ))
        })
    }

    /// A `Tree` showing the layout's structure.
    #[getter]
    fn tree(slf: &Bound<'_, Self>) -> PyResult<Py<PyAny>> {
        let py = slf.py();
        let tree_type = py.get_type::<super::tree::Tree>();
        let summary = |layout: &Bound<'_, Layout>| {
            Py::new(
                py,
                Summary {
                    layout: layout.clone().unbind(),
                },
            )
        };
        let guide = |layout: &Bound<'_, Layout>| {
            format!("layout.tree.{}", layout.borrow().splitter.name())
        };
        let kwargs = PyDict::new(py);
        kwargs.set_item("guide_style", guide(slf))?;
        kwargs.set_item("highlight", true)?;
        let tree = tree_type.call((summary(slf)?,), Some(&kwargs))?;
        let mut pending: Vec<(Bound<'_, PyAny>, Py<Layout>)> = vec![(tree.clone(), slf.clone().unbind())];
        let mut seen = 0usize;
        while let Some((node, layout)) = pending.pop() {
            seen += 1;
            if seen > 1_000_000 {
                return Err(pyo3::exceptions::PyRecursionError::new_err(
                    "a Layout cannot contain itself",
                ));
            }
            let children: Vec<Py<Layout>> = layout
                .bind(py)
                .borrow()
                .children
                .iter()
                .map(|child| child.clone_ref(py))
                .collect();
            let mut added = Vec::new();
            for child in children {
                let child_bound = child.bind(py);
                let kwargs = PyDict::new(py);
                kwargs.set_item("guide_style", guide(child_bound))?;
                let branch = node.call_method("add", (summary(child_bound)?,), Some(&kwargs))?;
                added.push((branch, child));
            }
            pending.extend(added.into_iter().rev());
        }
        Ok(tree.unbind())
    }

    /// Split into sub-layouts (layouts, or renderables to wrap in one).
    #[pyo3(signature = (*layouts, splitter=None))]
    fn split(
        &mut self,
        py: Python<'_>,
        layouts: &Bound<'_, PyTuple>,
        splitter: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<()> {
        let layouts = Layout::wrap(py, layouts)?;
        self.splitter = match splitter {
            Some(splitter) => splitter_arg(splitter)?,
            None => Split::Column,
        };
        self.children = layouts;
        Ok(())
    }

    /// Add layouts (or renderables) to the existing split.
    #[pyo3(signature = (*layouts))]
    fn add_split(&mut self, py: Python<'_>, layouts: &Bound<'_, PyTuple>) -> PyResult<()> {
        let layouts = Layout::wrap(py, layouts)?;
        self.children.extend(layouts);
        Ok(())
    }

    /// Split into a row: layouts side by side.
    #[pyo3(signature = (*layouts))]
    fn split_row(&mut self, py: Python<'_>, layouts: &Bound<'_, PyTuple>) -> PyResult<()> {
        self.children = Layout::wrap(py, layouts)?;
        self.splitter = Split::Row;
        Ok(())
    }

    /// Split into a column: layouts stacked.
    #[pyo3(signature = (*layouts))]
    fn split_column(&mut self, py: Python<'_>, layouts: &Bound<'_, PyTuple>) -> PyResult<()> {
        self.children = Layout::wrap(py, layouts)?;
        self.splitter = Split::Column;
        Ok(())
    }

    /// Remove the split.
    fn unsplit(&mut self) {
        self.children.clear();
    }

    /// Replace the renderable.
    fn update(&mut self, renderable: Py<PyAny>) {
        self.renderable = Some(renderable);
    }

    /// Upstream redraws one sub-layout in place on the alternate screen;
    /// the bindings' console cannot update screen lines.
    fn refresh_screen(&self, _console: &Bound<'_, PyAny>, _layout_name: &str) -> PyResult<()> {
        Err(PyNotImplementedError::new_err(
            "Layout.refresh_screen is not supported by rs_rich: print the layout (or use Live) \
             to redraw it",
        ))
    }

    /// `{layout: LayoutRender(region, lines)}` for each leaf.
    fn render<'py>(
        slf: &Bound<'py, Self>,
        console: &Bound<'py, PyAny>,
        options: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyDict>> {
        let py = slf.py();
        let map = PyDict::new(py);
        for (layout, region, lines) in Layout::render_map(slf, console, options)? {
            let region = region_type(py)?.call1(region)?;
            map.set_item(layout, layout_render_type(py)?.call1((region, lines))?)?;
        }
        Ok(map)
    }

    fn __rich_console__<'py>(
        slf: &Bound<'py, Self>,
        console: &Bound<'py, PyAny>,
        options: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyList>> {
        let py = slf.py();
        let max_width: usize = options.getattr("max_width")?.extract()?;
        let width = if max_width == 0 {
            console.getattr("width")?.extract()?
        } else {
            max_width
        };
        let height: Option<usize> = options.getattr("height")?.extract()?;
        let height = match height {
            Some(height) if height > 0 => height,
            _ => console.getattr("height")?.extract()?,
        };
        let options = options.call_method1("update_dimensions", (width, height))?;
        let map = Layout::render(slf, console, &options)?;
        slf.borrow_mut().render_map = map.clone().unbind();
        let mut rows: Vec<Vec<Bound<'py, PyAny>>> = (0..height).map(|_| Vec::new()).collect();
        for (_, value) in map.iter() {
            let region = value.get_item(0)?;
            let (_, y, _, layout_height): Region = region.extract()?;
            let lines = value.get_item(1)?;
            for (row, line) in rows
                .iter_mut()
                .skip(y)
                .take(layout_height)
                .zip(lines.try_iter()?)
            {
                for segment in line?.try_iter()? {
                    row.push(segment?);
                }
            }
        }
        let new_line = Py::new(py, Segment::from_core(py, &CoreSegment::line()))?;
        let result = PyList::empty(py);
        for row in rows {
            for segment in row {
                result.append(segment)?;
            }
            result.append(new_line.clone_ref(py))?;
        }
        Ok(result)
    }

    fn __traverse__(&self, visit: PyVisit<'_>) -> Result<(), PyTraverseError> {
        if let Some(renderable) = &self.renderable {
            visit.call(renderable)?;
        }
        for child in &self.children {
            visit.call(child)?;
        }
        visit.call(&self.render_map)
    }

    fn __clear__(&mut self) {
        self.renderable = None;
        self.children.clear();
    }
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    let py = m.py();
    m.add("LayoutError", py.get_type::<LayoutError>())?;
    m.add("NoSplitter", py.get_type::<NoSplitter>())?;
    m.add("Region", region_type(py)?)?;
    m.add("LayoutRender", layout_render_type(py)?)?;
    m.add_class::<Splitter>()?;
    m.add_class::<RowSplitter>()?;
    m.add_class::<ColumnSplitter>()?;
    m.add_class::<Layout>()?;
    renderable::register_renderable::<Placeholder>(py);
    renderable::register_renderable::<Summary>(py);
    Ok(())
}
