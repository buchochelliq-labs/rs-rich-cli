//! Nodes from Python: `Node` and its builders, and the functions that make
//! nodes (`text`, `label`, `renderable`, `column`, `each`, `list`,
//! `scroll`, `component`, ...).
//!
//! A `Node` holds the Rust node until it goes into a tree (a column's
//! children, a panel, an app's root), which takes it: a node is used once,
//! as in Rust, and using it again raises. Its builders change it and
//! return the same object, so they chain.

use std::cell::RefCell;

use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyString, PyTuple};

use rich::console::{Console as CoreConsole, ConsoleOptions as CoreOptions};
use rich::measure::Measurement as CoreMeasurement;
use rich::protocol::Renderable;
use rich::Segment as CoreSegment;
use rich_intuituive as intuituive;
use rich_intuituive::interact::{
    Button, Component, Context, Event, Flow, Keymap as CoreKeymap, Mouse, MouseKind, View,
};
use rich_intuituive::Node;

use super::reactive::{call_with_ctx, signal_arg, PyValue};
use super::{current, fail, size_arg, sizes, Building, Callback, Handle, PyRect, PySize};
use crate::renderable;

// ---------------------------------------------------------------------------
// The class

/// `Node`: a part of the screen. Builders change it and return it, so they
/// chain: `label("hi").panel("Greeting").fixed(3)`.
#[pyclass(name = "Node", module = "rs_rich.tui", unsendable)]
pub(crate) struct PyNode {
    node: RefCell<Option<Node>>,
    id: u64,
}

impl PyNode {
    pub(crate) fn new(node: Node) -> PyNode {
        PyNode {
            id: node.id(),
            node: RefCell::new(Some(node)),
        }
    }

    /// The node, taken: it goes into a tree.
    pub(crate) fn take(&self) -> PyResult<Node> {
        self.node.borrow_mut().take().ok_or_else(used)
    }

    /// Change the node with `f`.
    fn map(&self, f: impl FnOnce(Node) -> Node) -> PyResult<()> {
        let node = self.take()?;
        *self.node.borrow_mut() = Some(f(node));
        Ok(())
    }

    fn with<R>(&self, f: impl FnOnce(&Node) -> R) -> PyResult<R> {
        self.node.borrow().as_ref().map(f).ok_or_else(used)
    }
}

fn used() -> PyErr {
    PyValueError::new_err(
        "this node is already in a tree: a node is used once (make another for a second place)",
    )
}

/// A node argument: a `Node` (taken), a `str` (a `label`), or a terminal
/// pane or web view (its node).
pub(crate) fn take_node(value: &Bound<'_, PyAny>) -> PyResult<Node> {
    if let Ok(node) = value.cast::<PyNode>() {
        return node.borrow().take();
    }
    if let Ok(markup) = value.cast::<PyString>() {
        return Ok(intuituive::label(markup.to_cow()?.into_owned()));
    }
    if let Some(node) = super::embed::take_embedded(value)? {
        return Ok(node);
    }
    Err(PyTypeError::new_err(format!(
        "expected a Node (or a str, shown as a label), got {}",
        value.get_type().name()?
    )))
}

/// Nodes from an iterable.
pub(crate) fn take_nodes(value: &Bound<'_, PyAny>) -> PyResult<Vec<Node>> {
    if value.is_instance_of::<PyString>() {
        return Err(PyTypeError::new_err(
            "children must be an iterable of nodes, not a str",
        ));
    }
    // Collected first, so a bad child takes none of the others.
    let items: Vec<Bound<'_, PyAny>> = value.try_iter()?.collect::<PyResult<_>>()?;
    for item in &items {
        if let Ok(node) = item.cast::<PyNode>() {
            if node.borrow().node.borrow().is_none() {
                return Err(used());
            }
        }
    }
    items.iter().map(take_node).collect()
}

/// A Python function returning a node, called back from Rust: the node, or
/// an empty label once the app has failed.
pub(crate) fn node_callback(
    callback: &Callback,
    args: impl for<'py> FnOnce(Python<'py>) -> PyResult<Bound<'py, PyTuple>>,
) -> Node {
    callback
        .call(args, take_node)
        .unwrap_or_else(|| intuituive::label(""))
}

/// A screen's build function: called while the screen is built, so it may
/// call `every`.
pub(crate) fn screen_builder(callback: Callback) -> impl FnOnce() -> Node + 'static {
    move || {
        let _building = Building::new();
        node_callback(&callback, |py| Ok(PyTuple::empty(py)))
    }
}

/// A condition (`checked_when`, `class_when`): its truth.
fn condition(callback: Callback) -> impl Fn() -> bool + 'static {
    move || {
        callback
            .call(|py| Ok(PyTuple::empty(py)), |value| value.is_truthy())
            .unwrap_or(false)
    }
}

// ---------------------------------------------------------------------------
// The mouse

/// `Mouse`: a mouse event, in the node's own cells: `kind` (`"down"`,
/// `"up"`, `"drag"`, `"moved"`, `"scroll_up"` or `"scroll_down"`),
/// `button` (`"left"`, `"right"`, `"middle"` or `None`), `column`, `row`,
/// and the `shift`, `ctrl` and `alt` held.
#[pyclass(name = "Mouse", module = "rs_rich.tui", frozen, skip_from_py_object)]
#[derive(Clone, Copy)]
pub(crate) struct PyMouse(pub(crate) Mouse);

fn button_name(button: Button) -> &'static str {
    match button {
        Button::Left => "left",
        Button::Right => "right",
        Button::Middle => "middle",
    }
}

fn button_arg(name: &str) -> PyResult<Button> {
    Ok(match name {
        "left" => Button::Left,
        "right" => Button::Right,
        "middle" => Button::Middle,
        other => {
            return Err(PyValueError::new_err(format!(
                "invalid button {other:?}; expected left, right or middle"
            )))
        }
    })
}

/// A mouse event from its parts, as `Driver.mouse` takes them.
pub(crate) fn mouse_from(kind: &str, column: u16, row: u16, button: &str) -> PyResult<Mouse> {
    let kind = match kind {
        "down" => MouseKind::Down(button_arg(button)?),
        "up" => MouseKind::Up(button_arg(button)?),
        "drag" => MouseKind::Drag(button_arg(button)?),
        "moved" => MouseKind::Moved,
        "scroll_up" => MouseKind::ScrollUp,
        "scroll_down" => MouseKind::ScrollDown,
        other => {
            return Err(PyValueError::new_err(format!(
                "invalid mouse kind {other:?}; expected down, up, drag, moved, scroll_up or \
                 scroll_down"
            )))
        }
    };
    Ok(Mouse::new(kind, column, row))
}

#[pymethods]
impl PyMouse {
    #[getter]
    fn kind(&self) -> &'static str {
        match self.0.kind {
            MouseKind::Down(_) => "down",
            MouseKind::Up(_) => "up",
            MouseKind::Drag(_) => "drag",
            MouseKind::Moved => "moved",
            MouseKind::ScrollUp => "scroll_up",
            MouseKind::ScrollDown => "scroll_down",
        }
    }

    #[getter]
    fn button(&self) -> Option<&'static str> {
        match self.0.kind {
            MouseKind::Down(b) | MouseKind::Up(b) | MouseKind::Drag(b) => Some(button_name(b)),
            _ => None,
        }
    }

    #[getter]
    fn column(&self) -> u16 {
        self.0.column
    }

    #[getter]
    fn row(&self) -> u16 {
        self.0.row
    }

    #[getter]
    fn shift(&self) -> bool {
        self.0.modifiers.shift
    }

    #[getter]
    fn ctrl(&self) -> bool {
        self.0.modifiers.ctrl
    }

    #[getter]
    fn alt(&self) -> bool {
        self.0.modifiers.alt
    }

    /// A button went down.
    fn is_press(&self) -> bool {
        matches!(self.0.kind, MouseKind::Down(_))
    }

    /// The wheel turned: -1 up, 1 down, 0 otherwise.
    fn wheel(&self) -> i32 {
        match self.0.kind {
            MouseKind::ScrollUp => -1,
            MouseKind::ScrollDown => 1,
            _ => 0,
        }
    }

    fn __repr__(&self) -> String {
        match self.button() {
            Some(button) => format!(
                "Mouse({}, {button}, column={}, row={})",
                self.kind(),
                self.0.column,
                self.0.row
            ),
            None => format!(
                "Mouse({}, column={}, row={})",
                self.kind(),
                self.0.column,
                self.0.row
            ),
        }
    }
}

// ---------------------------------------------------------------------------
// Builders

#[pymethods]
impl PyNode {
    /// The node's identity: for `cx.focus(id)` and a pop-up's anchor.
    #[getter]
    fn id(&self) -> u64 {
        self.id
    }

    /// Where the node was last laid out (once it is in a tree, this object
    /// no longer reaches it).
    #[getter]
    fn rect(&self) -> PyResult<PyRect> {
        self.with(|node| PyRect(node.rect()))
    }

    /// How it asks to be sized along its parent's axis.
    fn size_hint(&self) -> PyResult<PySize> {
        self.with(|node| PySize(node.size_hint()))
    }

    /// Its states, for assistive technology.
    fn access_state(&self) -> PyResult<super::app::PyAccessState> {
        self.with(|node| super::app::PyAccessState(node.access_state()))
    }

    /// Its role, for assistive technology.
    fn access_role(&self) -> PyResult<super::Role> {
        self.with(|node| super::Role::from_rust(node.access_role()).unwrap_or(super::Role::Group))
    }

    /// Name it for the inspector, the help and the palette, and a
    /// stylesheet's `#name`.
    fn name<'py>(slf: Bound<'py, Self>, name: &str) -> PyResult<Bound<'py, Self>> {
        slf.borrow().map(|n| n.name(name))?;
        Ok(slf)
    }

    /// Say what it is, for assistive technology (a `Role` or its name).
    fn role<'py>(slf: Bound<'py, Self>, role: &Bound<'py, PyAny>) -> PyResult<Bound<'py, Self>> {
        let role = super::role_arg(role)?;
        slf.borrow().map(|n| n.role(role))?;
        Ok(slf)
    }

    /// Its accessible name.
    fn label<'py>(slf: Bound<'py, Self>, name: &str) -> PyResult<Bound<'py, Self>> {
        slf.borrow().map(|n| n.label(name))?;
        Ok(slf)
    }

    /// Announce what it shows whenever that changes (a status line).
    fn live(slf: Bound<'_, Self>) -> PyResult<Bound<'_, Self>> {
        slf.borrow().map(Node::live)?;
        Ok(slf)
    }

    /// Leave it, and what is inside, out of what assistive technology gets.
    #[pyo3(signature = (hidden=true))]
    fn access_hidden(slf: Bound<'_, Self>, hidden: bool) -> PyResult<Bound<'_, Self>> {
        slf.borrow().map(|n| n.access_hidden(hidden))?;
        Ok(slf)
    }

    /// Expanded while `condition()` holds (ARIA's `aria-expanded`).
    fn expanded_when<'py>(
        slf: Bound<'py, Self>,
        condition: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, Self>> {
        let condition = self::condition(Callback::checked(condition, "the condition")?);
        slf.borrow().map(|n| n.expanded_when(condition))?;
        Ok(slf)
    }

    /// Checked while `condition()` holds; its role becomes a check box
    /// unless set.
    fn checked_when<'py>(
        slf: Bound<'py, Self>,
        condition: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, Self>> {
        let condition = self::condition(Callback::checked(condition, "the condition")?);
        slf.borrow().map(|n| n.checked_when(condition))?;
        Ok(slf)
    }

    /// Busy while `condition()` holds (ARIA's `aria-busy`).
    fn busy_when<'py>(
        slf: Bound<'py, Self>,
        condition: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, Self>> {
        let condition = self::condition(Callback::checked(condition, "the condition")?);
        slf.borrow().map(|n| n.busy_when(condition))?;
        Ok(slf)
    }

    /// Size it along its parent's axis (a `Size`, an int or a str).
    fn size<'py>(slf: Bound<'py, Self>, size: &Bound<'py, PyAny>) -> PyResult<Bound<'py, Self>> {
        let size = size_arg(size)?;
        slf.borrow().map(|n| n.size(size))?;
        Ok(slf)
    }

    /// Exactly `cells` along the parent's axis.
    fn fixed(slf: Bound<'_, Self>, cells: u16) -> PyResult<Bound<'_, Self>> {
        slf.borrow().map(|n| n.fixed(cells))?;
        Ok(slf)
    }

    /// A share, by `weight`, of the space left over.
    fn flex(slf: Bound<'_, Self>, weight: u16) -> PyResult<Bound<'_, Self>> {
        slf.borrow().map(|n| n.flex(weight))?;
        Ok(slf)
    }

    /// `percent` of the parent's axis.
    fn percent(slf: Bound<'_, Self>, percent: u16) -> PyResult<Bound<'_, Self>> {
        slf.borrow().map(|n| n.percent(percent))?;
        Ok(slf)
    }

    /// As much as the content needs.
    fn auto(slf: Bound<'_, Self>) -> PyResult<Bound<'_, Self>> {
        slf.borrow().map(Node::auto)?;
        Ok(slf)
    }

    fn min_size(slf: Bound<'_, Self>, cells: u16) -> PyResult<Bound<'_, Self>> {
        slf.borrow().map(|n| n.min_size(cells))?;
        Ok(slf)
    }

    fn max_size(slf: Bound<'_, Self>, cells: u16) -> PyResult<Bound<'_, Self>> {
        slf.borrow().map(|n| n.max_size(cells))?;
        Ok(slf)
    }

    /// In a grid: span `columns` columns and `rows` rows.
    fn span(slf: Bound<'_, Self>, columns: u16, rows: u16) -> PyResult<Bound<'_, Self>> {
        slf.borrow().map(|n| n.span(columns, rows))?;
        Ok(slf)
    }

    /// For a column, row or grid: `cells` between neighbours.
    fn gap(slf: Bound<'_, Self>, cells: u16) -> PyResult<Bound<'_, Self>> {
        slf.borrow().map(|n| n.gap(cells))?;
        Ok(slf)
    }

    /// For a grid: the rows' sizes.
    fn rows<'py>(slf: Bound<'py, Self>, rows: &Bound<'py, PyAny>) -> PyResult<Bound<'py, Self>> {
        let rows = sizes(rows)?;
        slf.borrow().map(|n| n.rows(rows))?;
        Ok(slf)
    }

    /// Give it `classes` (space-separated) for a stylesheet. (`class` is a
    /// Python keyword, hence the underscore.)
    fn class_<'py>(slf: Bound<'py, Self>, classes: &str) -> PyResult<Bound<'py, Self>> {
        slf.borrow().map(|n| n.class(classes))?;
        Ok(slf)
    }

    /// Give it `class` while `condition()` holds.
    fn class_when<'py>(
        slf: Bound<'py, Self>,
        class: &str,
        condition: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, Self>> {
        let condition = self::condition(Callback::checked(condition, "the condition")?);
        slf.borrow().map(|n| n.class_when(class, condition))?;
        Ok(slf)
    }

    /// Match a stylesheet's `:selected` (and be selected, for assistive
    /// technology) while `condition()` holds.
    fn selected_when<'py>(
        slf: Bound<'py, Self>,
        condition: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, Self>> {
        let condition = self::condition(Callback::checked(condition, "the condition")?);
        slf.borrow().map(|n| n.selected_when(condition))?;
        Ok(slf)
    }

    /// Disabled while `condition()` holds: out of the Tab order, its
    /// handlers off, and a stylesheet's `:disabled`.
    fn disabled_when<'py>(
        slf: Bound<'py, Self>,
        condition: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, Self>> {
        let condition = self::condition(Callback::checked(condition, "the condition")?);
        slf.borrow().map(|n| n.disabled_when(condition))?;
        Ok(slf)
    }

    /// Put it in the Tab order.
    fn focusable(slf: Bound<'_, Self>) -> PyResult<Bound<'_, Self>> {
        slf.borrow().map(Node::focusable)?;
        Ok(slf)
    }

    /// Give it the focus when its screen opens.
    fn autofocus(slf: Bound<'_, Self>) -> PyResult<Bound<'_, Self>> {
        slf.borrow().map(Node::autofocus)?;
        Ok(slf)
    }

    /// Take it out of the Tab order.
    fn no_focus(slf: Bound<'_, Self>) -> PyResult<Bound<'_, Self>> {
        slf.borrow().map(Node::no_focus)?;
        Ok(slf)
    }

    /// Draw it in `style` while it has the focus (makes it focusable).
    fn focus_style<'py>(slf: Bound<'py, Self>, style: &str) -> PyResult<Bound<'py, Self>> {
        slf.borrow().map(|n| n.focus_style(style))?;
        Ok(slf)
    }

    /// Draw it in `style` while the pointer is over it.
    fn hover_style<'py>(slf: Bound<'py, Self>, style: &str) -> PyResult<Bound<'py, Self>> {
        slf.borrow().map(|n| n.hover_style(style))?;
        Ok(slf)
    }

    /// Show `markup` in a box by the pointer once it rests on the node (F1
    /// shows the focused node's at once).
    fn tooltip<'py>(slf: Bound<'py, Self>, markup: &str) -> PyResult<Bound<'py, Self>> {
        slf.borrow().map(|n| n.tooltip(markup))?;
        Ok(slf)
    }

    /// Let it be dragged with the left button, carrying `value` to a node
    /// with `on_drop`.
    fn draggable<'py>(
        slf: Bound<'py, Self>,
        value: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, Self>> {
        let value = PyValue(value.clone().unbind());
        slf.borrow().map(|n| n.draggable(value))?;
        Ok(slf)
    }

    /// Take what is dropped on it: `handler(value, cx)`. Every value
    /// dragged from Python is offered (Rust matches by type; check the
    /// value's type in the handler).
    fn on_drop<'py>(
        slf: Bound<'py, Self>,
        handler: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, Self>> {
        let handler = Callback::checked(handler, "the drop handler")?;
        slf.borrow().map(|n| {
            n.on_drop(move |value: &PyValue, cx| call_with_ctx(&handler, Some(value.clone()), cx))
        })?;
        Ok(slf)
    }

    /// Run `handler(cx)` when one of `keys` (space-separated names) is
    /// pressed while the focus is on this node or inside it.
    fn on_key<'py>(
        slf: Bound<'py, Self>,
        keys: &str,
        handler: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, Self>> {
        super::keys(keys)?;
        let handler = Callback::checked(handler, "the key handler")?;
        slf.borrow()
            .map(|n| n.on_key(keys, move |cx| call_with_ctx(&handler, None, cx)))?;
        Ok(slf)
    }

    /// `on_key` with a description, which makes it a command in the
    /// palette and a line in the help.
    fn bind<'py>(
        slf: Bound<'py, Self>,
        keys: &str,
        description: &str,
        handler: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, Self>> {
        super::keys(keys)?;
        let handler = Callback::checked(handler, "the key handler")?;
        slf.borrow().map(|n| {
            n.bind(keys, description, move |cx| {
                call_with_ctx(&handler, None, cx)
            })
        })?;
        Ok(slf)
    }

    /// Run `handler(cx)` when it is clicked (makes it focusable).
    fn on_click<'py>(
        slf: Bound<'py, Self>,
        handler: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, Self>> {
        let handler = Callback::checked(handler, "the click handler")?;
        slf.borrow()
            .map(|n| n.on_click(move |cx| call_with_ctx(&handler, None, cx)))?;
        Ok(slf)
    }

    /// Run `handler(cx, mouse)` for every mouse event over it; it returns
    /// whether it used the event (one it did not bubbles up).
    fn on_mouse<'py>(
        slf: Bound<'py, Self>,
        handler: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, Self>> {
        let handler = Callback::checked(handler, "the mouse handler")?;
        slf.borrow().map(|n| {
            n.on_mouse(move |cx, mouse| {
                let loan = super::Loan::new();
                handler
                    .call(
                        |py| {
                            let ctx = super::app::PyCtx::lend(py, cx, &loan)?;
                            let mouse = Py::new(py, PyMouse(mouse))?;
                            PyTuple::new(py, [ctx.into_any().unbind(), mouse.into_any()])
                        },
                        |used| used.is_truthy(),
                    )
                    .unwrap_or(true)
            })
        })?;
        Ok(slf)
    }

    /// For a component node: run `handler(cx)` when it is cancelled (Esc).
    fn on_cancel<'py>(
        slf: Bound<'py, Self>,
        handler: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, Self>> {
        let handler = Callback::checked(handler, "the cancel handler")?;
        slf.borrow()
            .map(|n| n.on_cancel(move |cx| call_with_ctx(&handler, None, cx)))?;
        Ok(slf)
    }

    /// Wrap it in a rounded border with `title`, highlighted while the
    /// focus is inside.
    #[pyo3(signature = (title=""))]
    fn panel<'py>(slf: Bound<'py, Self>, title: &str) -> PyResult<Bound<'py, Self>> {
        let node = slf.borrow().take()?.panel(title);
        replace(&slf, node);
        Ok(slf)
    }

    /// Leave `vertical` rows above and below it, and `horizontal` columns
    /// either side.
    fn padding(slf: Bound<'_, Self>, vertical: u16, horizontal: u16) -> PyResult<Bound<'_, Self>> {
        let node = slf.borrow().take()?.padding(vertical, horizontal);
        replace(&slf, node);
        Ok(slf)
    }

    fn __repr__(&self) -> String {
        match &*self.node.borrow() {
            Some(_) => format!("<Node {}>", self.id),
            None => format!("<Node {} (in a tree)>", self.id),
        }
    }
}

/// Put `node` in `slf` (a wrapper that has a new identity).
fn replace(slf: &Bound<'_, PyNode>, node: Node) {
    let mut this = slf.borrow_mut();
    this.id = node.id();
    *this.node.borrow_mut() = Some(node);
}

// ---------------------------------------------------------------------------
// Rendering Python renderables in a node

/// Run `f` in a render scope, so Python renderables render: the one
/// already around (an app run from Python), else one of its own (a served
/// session's thread).
pub(crate) fn render_scope<T>(
    py: Python<'_>,
    width: usize,
    height: usize,
    f: impl FnOnce() -> PyResult<T>,
) -> PyResult<T> {
    match renderable::ambient() {
        Ok(outer) => {
            let ambient = renderable::Ambient {
                console: outer.console.clone_ref(py),
                base: outer.base.clone(),
                emoji: outer.emoji,
                markup: outer.markup,
                highlight: outer.highlight,
                highlighter: outer.highlighter.as_ref().map(|h| h.clone_ref(py)),
                emoji_variant: outer.emoji_variant,
            };
            renderable::scope(ambient, f)
        }
        Err(_) => crate::ext::common::scoped(py, width, height, true, f),
    }
}

/// A Python object drawn as lines: a list of lines of `Segment`s as they
/// are, anything else as a renderable at `width` x `height` (a height of 0:
/// as tall as it likes).
pub(crate) fn draw_object(
    value: &Bound<'_, PyAny>,
    console: &CoreConsole,
    width: u16,
    height: u16,
) -> PyResult<Vec<Vec<CoreSegment>>> {
    if let Some(lines) = segment_lines(value)? {
        return Ok(lines);
    }
    let py = value.py();
    render_scope(py, width.into(), height.into(), || {
        let renderable = renderable::to_renderable(value, None)?;
        let options = console.options().update_width(width.max(1) as usize);
        let options = if height == 0 {
            options
        } else {
            options.update_height(height as usize)
        };
        Ok(console.render_lines(&*renderable, &options, false))
    })
}

/// Lines of segments, if `value` is a list of lists of `Segment`s.
pub(crate) fn segment_lines(value: &Bound<'_, PyAny>) -> PyResult<Option<Vec<Vec<CoreSegment>>>> {
    let Ok(list) = value.cast::<pyo3::types::PyList>() else {
        return Ok(None);
    };
    let mut lines = Vec::with_capacity(list.len());
    for line in list.iter() {
        let Ok(line) = line.cast::<pyo3::types::PyList>() else {
            return Ok(None);
        };
        let mut segments = Vec::with_capacity(line.len());
        for segment in line.iter() {
            let Ok(segment) = segment.cast::<crate::segment::Segment>() else {
                return Ok(None);
            };
            segments.push(segment.get().to_core());
        }
        lines.push(segments);
    }
    Ok(Some(lines))
}

/// What a `renderable` node's function returns, rendered when the node
/// draws.
struct Drawn {
    value: Option<Py<PyAny>>,
    app: Option<Handle>,
}

impl Drawn {
    fn lines(&self, console: &CoreConsole, options: &CoreOptions) -> PyResult<Vec<CoreSegment>> {
        Python::attach(|py| {
            let Some(value) = &self.value else {
                return Ok(Vec::new());
            };
            let value = value.bind(py);
            render_scope(py, options.max_width, options.height.unwrap_or(0), || {
                let renderable = renderable::to_renderable(value, None)?;
                Ok(renderable.rich_render(console, options))
            })
        })
    }
}

impl Renderable for Drawn {
    fn rich_render(&self, console: &CoreConsole, options: &CoreOptions) -> Vec<CoreSegment> {
        self.lines(console, options).unwrap_or_else(|error| {
            Python::attach(|py| fail(py, self.app.as_ref(), error));
            Vec::new()
        })
    }

    fn measure(&self, console: &CoreConsole, options: &CoreOptions) -> CoreMeasurement {
        let measured = Python::attach(|py| {
            let Some(value) = &self.value else {
                return Ok(CoreMeasurement::new(0, 0));
            };
            let value = value.bind(py);
            render_scope(py, options.max_width, options.height.unwrap_or(0), || {
                Ok(renderable::to_renderable(value, None)?.measure(console, options))
            })
        });
        measured.unwrap_or_else(|error| {
            Python::attach(|py| fail(py, self.app.as_ref(), error));
            CoreMeasurement::new(0, 0)
        })
    }
}

// ---------------------------------------------------------------------------
// Hosted components

/// A hosted `rs_rich.interact` component that stops the app when Python
/// code it runs raises (the render scope keeps the exception, which
/// `run()` raises).
struct Hosted<C> {
    component: C,
    app: Option<Handle>,
}

impl<C: Component> Hosted<C> {
    fn pending(&self) -> bool {
        let pending = renderable::has_pending();
        if pending {
            if let Some(app) = &self.app {
                app.stop();
            }
        }
        pending
    }
}

impl<C: Component> Component for Hosted<C> {
    type Output = C::Output;

    fn start(&mut self, context: &Context<'_>) -> Flow<C::Output> {
        if self.pending() {
            return Flow::Continue;
        }
        self.component.start(context)
    }

    fn handle(&mut self, event: &Event, context: &Context<'_>) -> Flow<C::Output> {
        if self.pending() {
            return Flow::Continue;
        }
        let flow = self.component.handle(event, context);
        if self.pending() {
            return Flow::Continue;
        }
        flow
    }

    fn render(&self, context: &Context<'_>) -> View {
        if self.pending() {
            return View::default();
        }
        self.component.render(context)
    }

    fn tick(&self) -> Option<std::time::Duration> {
        self.component.tick()
    }

    fn mouse(&self) -> bool {
        self.component.mouse()
    }

    fn keymap(&self) -> CoreKeymap {
        self.component.keymap()
    }

    fn focusable(&self) -> bool {
        self.component.focusable()
    }

    fn focus_step(&mut self, forward: bool) -> bool {
        self.component.focus_step(forward)
    }

    fn focus_enter(&mut self, forward: bool) -> bool {
        self.component.focus_enter(forward)
    }

    fn default_value(&self) -> Option<C::Output> {
        self.component.default_value()
    }
}

type Child = rich_intuituive::interact::compose::Child<'static, Py<PyAny>>;

/// `component` (an `rs_rich.interact` component) as a Rust one.
fn hosted(py: Python<'_>, component: &Bound<'_, PyAny>) -> PyResult<Hosted<Child>> {
    let build = crate::interact::compose::node(py, component, 0)?;
    Ok(Hosted {
        component: build(),
        app: current(),
    })
}

fn on_done(callback: Callback) -> impl FnMut(Py<PyAny>, &mut intuituive::Ctx) + 'static {
    move |value, cx| call_with_ctx(&callback, Some(PyValue(value)), cx)
}

// ---------------------------------------------------------------------------
// The functions that make nodes

/// `text(f)`: console markup from `f()`, read again whenever a signal it
/// read changes. `f` may return anything; it is shown as `str(...)`. A
/// `str` instead of a function is shown as it is.
#[pyfunction]
fn tui_text(markup: &Bound<'_, PyAny>) -> PyResult<PyNode> {
    if let Ok(markup) = markup.cast::<PyString>() {
        let markup = markup.to_cow()?.into_owned();
        return Ok(PyNode::new(intuituive::text(move || markup.clone())));
    }
    let markup = Callback::checked(markup, "text's function")?;
    Ok(PyNode::new(intuituive::text(move || {
        markup
            .call(
                |py| Ok(PyTuple::empty(py)),
                |value| match value.cast::<PyString>() {
                    Ok(text) => Ok(text.to_cow()?.into_owned()),
                    Err(_) => Ok(value.str()?.to_cow()?.into_owned()),
                },
            )
            .unwrap_or_default()
    })))
}

/// `label(markup)`: console markup that never changes.
#[pyfunction]
fn tui_label(markup: &str) -> PyNode {
    PyNode::new(intuituive::label(markup))
}

/// `renderable(f)`: any renderable `f()` returns (a `Table`, `Markdown`,
/// `Syntax`, a chart, your own `__rich_console__`), rendered at the node's
/// size and built again when a signal `f` read changes.
#[pyfunction]
fn tui_renderable(f: &Bound<'_, PyAny>) -> PyResult<PyNode> {
    let f = Callback::checked(f, "renderable's function")?;
    Ok(PyNode::new(intuituive::renderable(move || Drawn {
        value: f.call0(),
        app: f.app(),
    })))
}

/// `leaf(draw)`: a node that draws itself: `draw(width, height)` returns a
/// renderable to render at that size, or lines (a list of lists of
/// `Segment`s). A height of 0 asks how tall it would like to be.
#[pyfunction]
fn tui_leaf(draw: &Bound<'_, PyAny>) -> PyResult<PyNode> {
    let draw = Callback::checked(draw, "leaf's draw")?;
    Ok(PyNode::new(intuituive::leaf(
        move |console, width, height| {
            draw.call(
                |py| PyTuple::new(py, [width, height]),
                |value| draw_object(value, console, width, height),
            )
            .unwrap_or_default()
        },
    )))
}

/// `column(children)`: children one above the other.
#[pyfunction]
fn tui_column(children: &Bound<'_, PyAny>) -> PyResult<PyNode> {
    Ok(PyNode::new(intuituive::column(take_nodes(children)?)))
}

/// `row(children)`: children side by side.
#[pyfunction]
fn tui_row(children: &Bound<'_, PyAny>) -> PyResult<PyNode> {
    Ok(PyNode::new(intuituive::row(take_nodes(children)?)))
}

/// `grid(columns, children)`: children in a grid with `columns` (sizes),
/// filled row by row.
#[pyfunction]
fn tui_grid(columns: &Bound<'_, PyAny>, children: &Bound<'_, PyAny>) -> PyResult<PyNode> {
    let columns = sizes(columns)?;
    Ok(PyNode::new(intuituive::grid(
        columns,
        take_nodes(children)?,
    )))
}

/// Values from an iterable a callback returned.
fn values(value: &Bound<'_, PyAny>) -> PyResult<Vec<PyValue>> {
    value
        .try_iter()?
        .map(|item| Ok(PyValue(item?.unbind())))
        .collect()
}

/// `each(keys, build)`: one child per key of `keys()`, built by
/// `build(key)` and kept by key (`==` and `hash`) across reorders.
#[pyfunction]
fn tui_each(keys: &Bound<'_, PyAny>, build: &Bound<'_, PyAny>) -> PyResult<PyNode> {
    let keys = Callback::checked(keys, "each's keys")?;
    let build = Callback::checked(build, "each's build")?;
    Ok(PyNode::new(intuituive::each(
        move || {
            keys.call(|py| Ok(PyTuple::empty(py)), values)
                .unwrap_or_default()
        },
        move |key: PyValue| node_callback(&build, |py| PyTuple::new(py, [key.object(py)])),
    )))
}

/// `switch(key, build)`: the child for `key()`, built by `build(key)` the
/// first time and kept while other keys show.
#[pyfunction]
fn tui_switch(key: &Bound<'_, PyAny>, build: &Bound<'_, PyAny>) -> PyResult<PyNode> {
    let key = Callback::checked(key, "switch's key")?;
    let build = Callback::checked(build, "switch's build")?;
    Ok(PyNode::new(intuituive::switch(
        move || key.call0().map(PyValue).unwrap_or_else(PyValue::none),
        move |key: PyValue| node_callback(&build, |py| PyTuple::new(py, [key.object(py)])),
    )))
}

/// Strings from what a callback returned: each item as `str(item)`.
pub(crate) fn strings(value: &Bound<'_, PyAny>) -> PyResult<Vec<String>> {
    if value.is_instance_of::<PyString>() {
        return Err(PyTypeError::new_err(
            "expected an iterable of str, not a str",
        ));
    }
    value
        .try_iter()?
        .map(|item| {
            let item = item?;
            match item.cast::<PyString>() {
                Ok(text) => Ok(text.to_cow()?.into_owned()),
                Err(_) => Ok(item.str()?.to_cow()?.into_owned()),
            }
        })
        .collect()
}

/// A function of no arguments returning strings, or a fixed iterable.
pub(crate) fn string_source(
    value: &Bound<'_, PyAny>,
    what: &str,
) -> PyResult<impl Fn() -> Vec<String> + 'static> {
    enum Source {
        Fixed(Vec<String>),
        Call(Callback),
    }
    let source = if value.is_callable() {
        Source::Call(Callback::checked(value, what)?)
    } else {
        Source::Fixed(strings(value)?)
    };
    Ok(move || match &source {
        Source::Fixed(items) => items.clone(),
        Source::Call(f) => f
            .call(|py| Ok(PyTuple::empty(py)), strings)
            .unwrap_or_default(),
    })
}

/// `list(items, selected)`: a scrolling list of markup rows from `items()`
/// (or a fixed list), the selected row's index in the signal `selected`.
#[pyfunction]
fn tui_list(items: &Bound<'_, PyAny>, selected: &Bound<'_, PyAny>) -> PyResult<PyNode> {
    let py = items.py();
    let items = string_source(items, "list's items")?;
    let selected = signal_arg(selected, "list's selected")?
        .borrow()
        .usize(py, "a list's selection")?;
    Ok(PyNode::new(intuituive::list(items, selected)))
}

/// `scroll(child)`: a viewport onto `child`, scrolled with the keys, the
/// wheel and a scrollbar.
#[pyfunction]
fn tui_scroll(child: &Bound<'_, PyAny>) -> PyResult<PyNode> {
    super::require("scroll()")?;
    Ok(PyNode::new(intuituive::scroll(take_node(child)?)))
}

/// `scroll_with(child, offset)`: `scroll`, with the first row in view in
/// the signal `offset`.
#[pyfunction]
fn tui_scroll_with(child: &Bound<'_, PyAny>, offset: &Bound<'_, PyAny>) -> PyResult<PyNode> {
    let offset = signal_arg(offset, "scroll's offset")?
        .borrow()
        .u16(child.py(), "a scroll's offset")?;
    Ok(PyNode::new(intuituive::scroll_with(
        take_node(child)?,
        offset,
    )))
}

/// `scroll_x(child)`: a view that scrolls across.
#[pyfunction]
fn tui_scroll_x(child: &Bound<'_, PyAny>) -> PyResult<PyNode> {
    super::require("scroll_x()")?;
    Ok(PyNode::new(intuituive::scroll_x(take_node(child)?)))
}

/// `scroll_both(child)`: a view that scrolls down and across.
#[pyfunction]
fn tui_scroll_both(child: &Bound<'_, PyAny>) -> PyResult<PyNode> {
    super::require("scroll_both()")?;
    Ok(PyNode::new(intuituive::scroll_both(take_node(child)?)))
}

/// `scroll_both_with(child, x, y, across, down)`: a scrolling view with its
/// first column and row in view in the signals `x` and `y`.
#[pyfunction]
fn tui_scroll_both_with(
    child: &Bound<'_, PyAny>,
    x: &Bound<'_, PyAny>,
    y: &Bound<'_, PyAny>,
    across: bool,
    down: bool,
) -> PyResult<PyNode> {
    let py = child.py();
    let x = signal_arg(x, "scroll's x")?
        .borrow()
        .u16(py, "a scroll's offset")?;
    let y = signal_arg(y, "scroll's y")?
        .borrow()
        .u16(py, "a scroll's offset")?;
    Ok(PyNode::new(intuituive::scroll_both_with(
        take_node(child)?,
        x,
        y,
        across,
        down,
    )))
}

/// `component(component, on_done)`: an `rs_rich.interact` component (an
/// `Input`, a `Select`, a `Form`, a `Component` subclass, a container) as a
/// node; `on_done(value, cx)` gets its answer.
#[pyfunction]
fn tui_component(component: &Bound<'_, PyAny>, on_done: &Bound<'_, PyAny>) -> PyResult<PyNode> {
    let hosted = hosted(component.py(), component)?;
    let on_done = Callback::checked(on_done, "component's on_done")?;
    Ok(PyNode::new(intuituive::component(
        hosted,
        self::on_done(on_done),
    )))
}

/// `repeating(make, on_done)`: the component `make()` returns, made again
/// after each answer (an entry box that takes one entry after another).
#[pyfunction]
fn tui_repeating(make: &Bound<'_, PyAny>, on_done: &Bound<'_, PyAny>) -> PyResult<PyNode> {
    let py = make.py();
    let make = Callback::checked(make, "repeating's make")?;
    let on_done = Callback::checked(on_done, "repeating's on_done")?;
    // The first one is made now, so a mistake raises here.
    let first = make
        .func()
        .bind(py)
        .call0()
        .and_then(|component| hosted(py, &component))?;
    let first = std::cell::RefCell::new(Some(first));
    let app = make.app();
    Ok(PyNode::new(intuituive::repeating(
        move || {
            if let Some(first) = first.borrow_mut().take() {
                return first;
            }
            let made = make.call(
                |py| Ok(PyTuple::empty(py)),
                |value| hosted(value.py(), value),
            );
            made.unwrap_or_else(|| Hosted {
                component: Box::new(rich_intuituive::interact::compose::Label::new("")),
                app: app.clone(),
            })
        },
        self::on_done(on_done),
    )))
}

/// `widget(w)`: a node with the behaviour of `w`, a `Widget` subclass.
#[pyfunction]
fn tui_widget(widget: &Bound<'_, PyAny>) -> PyResult<PyNode> {
    super::widget::node_of(widget).map(PyNode::new)
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    let py = m.py();
    m.add("TuiNode", py.get_type::<PyNode>())?;
    m.add("TuiMouse", py.get_type::<PyMouse>())?;
    m.add_function(wrap_pyfunction!(tui_text, m)?)?;
    m.add_function(wrap_pyfunction!(tui_label, m)?)?;
    m.add_function(wrap_pyfunction!(tui_renderable, m)?)?;
    m.add_function(wrap_pyfunction!(tui_leaf, m)?)?;
    m.add_function(wrap_pyfunction!(tui_column, m)?)?;
    m.add_function(wrap_pyfunction!(tui_row, m)?)?;
    m.add_function(wrap_pyfunction!(tui_grid, m)?)?;
    m.add_function(wrap_pyfunction!(tui_each, m)?)?;
    m.add_function(wrap_pyfunction!(tui_switch, m)?)?;
    m.add_function(wrap_pyfunction!(tui_list, m)?)?;
    m.add_function(wrap_pyfunction!(tui_scroll, m)?)?;
    m.add_function(wrap_pyfunction!(tui_scroll_with, m)?)?;
    m.add_function(wrap_pyfunction!(tui_scroll_x, m)?)?;
    m.add_function(wrap_pyfunction!(tui_scroll_both, m)?)?;
    m.add_function(wrap_pyfunction!(tui_scroll_both_with, m)?)?;
    m.add_function(wrap_pyfunction!(tui_component, m)?)?;
    m.add_function(wrap_pyfunction!(tui_repeating, m)?)?;
    m.add_function(wrap_pyfunction!(tui_widget, m)?)?;
    Ok(())
}
