//! Composition from Python (0.0.14 workstream 4): `Component` to subclass,
//! and the containers of `rich_interact::compose` (`Column`, `Row`,
//! `Stack`, `Split`, `Tabs`, `Layers` with `Layer`s, `Label`, `Map`).
//!
//! Like the other classes here, a container is a configuration: it holds
//! its children as Python objects, and each run builds a fresh Rust tree
//! from them ([`node`]), with the built-ins as the same Rust components
//! they are on their own and a `Component` subclass bridged by
//! `components::PyComponent`. Every child answers in Python values, so the
//! tree's output is a Python object: a built-in's answer finishes the whole
//! composition unless a `Map` says otherwise.
//!
//! The tree runs through the same drivers as a single component, with the
//! GIL released between events; Python code (a component's methods, `Map`
//! and binding callbacks, a layer factory) takes it back when it runs, and
//! an exception from any of it ends the run and is raised from it.

use std::sync::Mutex;

use pyo3::exceptions::{PyNotImplementedError, PyRecursionError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyMapping, PyTuple};
use pyo3::{PyTraverseError, PyVisit};

use rich_interact::compose::{
    Child, ComponentExt, Label as CoreLabel, Layer as CoreLayer, LayerKind, Layers as CoreLayers,
    Split as CoreSplit, Stack as CoreStack, Tabs as CoreTabs,
};
use rich_interact::{Axis, Component, Context, Flow, Key, Size};

use super::components::{self, flow, Ignored};
use super::keymap::{keys_arg, Keymap};
use super::{iterable, Build, Record};
use crate::ext::common::scoped;
use crate::renderable;

/// A child of a container, not built yet: built with the GIL released,
/// once per run.
pub(crate) type Node = Box<dyn FnOnce() -> Child<'static, Py<PyAny>> + Send>;

/// How deep containers may nest: past it, a container that contains itself
/// is the likelier story.
const MAX_DEPTH: usize = 100;

/// A composition, built for a run.
pub(super) struct Tree(pub(super) Node);

impl Build for Tree {
    type C = Child<'static, Py<PyAny>>;

    fn build(self) -> Self::C {
        (self.0)()
    }
}

/// A built-in component as a child: its answer, converted to Python by
/// `convert`, finishes the container.
pub(super) fn leaf<B>(
    build: B,
    convert: impl Fn(Python<'_>, <B::C as Component>::Output) -> PyResult<Py<PyAny>> + Send + 'static,
) -> Node
where
    B: Build + 'static,
    B::C: 'static,
{
    Box::new(move || {
        let component = build.build();
        Box::new(component.map(move |value| {
            Python::attach(|py| match convert(py, value) {
                Ok(value) => Flow::Done(value),
                Err(error) => {
                    renderable::report_error(py, error);
                    Flow::Cancel
                }
            })
        }))
    })
}

/// Call a Python callback for a flow; an exception is kept for the run,
/// which it cancels.
fn call_flow(
    callable: &Py<PyAny>,
    args: impl for<'py> FnOnce(Python<'py>) -> PyResult<Bound<'py, PyTuple>>,
    what: &str,
) -> Flow<Py<PyAny>> {
    Python::attach(|py| {
        let result = args(py)
            .and_then(|args| callable.bind(py).call1(args))
            .and_then(|result| flow(&result, what));
        result.unwrap_or_else(|error| {
            renderable::report_error(py, error);
            Flow::Cancel
        })
    })
}

/// Whether `component` is a composition that runs as a tree: a container,
/// a `Label`, a `Map` or a `Component` subclass. The other components run
/// on their own, as before.
pub(super) fn tree(py: Python<'_>, component: &Bound<'_, PyAny>) -> PyResult<Option<Node>> {
    if component.is_instance_of::<Container>()
        || component.is_instance_of::<Label>()
        || component.is_instance_of::<Map>()
        || component.is_instance_of::<Base>()
    {
        return node(py, component, 0).map(Some);
    }
    Ok(None)
}

/// The node for any component, container or not.
pub(super) fn node(py: Python<'_>, component: &Bound<'_, PyAny>, depth: usize) -> PyResult<Node> {
    if depth > MAX_DEPTH {
        return Err(PyRecursionError::new_err(format!(
            "containers nest more than {MAX_DEPTH} deep: does one contain itself?"
        )));
    }
    if let Ok(stack) = component.cast::<Stack>() {
        let common = common(component)?;
        let stack = stack.get();
        let (axis, gap) = (stack.axis, stack.gap);
        // Copied out first: a stack may (wrongly) contain itself, and the
        // lock is not reentrant.
        let children: Vec<(Py<PyAny>, Size)> = stack
            .children()
            .iter()
            .map(|(child, size)| (child.clone_ref(py), *size))
            .collect();
        let children = children
            .iter()
            .map(|(child, size)| Ok((node(py, child.bind(py), depth + 1)?, *size)))
            .collect::<PyResult<Vec<_>>>()?;
        return Ok(Box::new(move || {
            let mut built = CoreStack::new(axis).gap(gap);
            for (child, size) in children {
                built.push(size, child());
            }
            Box::new(common.apply(built))
        }));
    }
    if let Ok(split) = component.cast::<Split>() {
        let common = common(component)?;
        let split = split.get();
        let first = node(py, split.first.bind(py), depth + 1)?;
        let second = node(py, split.second.bind(py), depth + 1)?;
        let (axis, ratio, at, mins) = (split.axis, split.ratio, split.at, split.mins);
        return Ok(Box::new(move || {
            let mut built = match axis {
                Axis::Horizontal => CoreSplit::horizontal(first(), second()),
                Axis::Vertical => CoreSplit::vertical(first(), second()),
            }
            .ratio(ratio);
            if let Some(cells) = at {
                built = built.at(cells);
            }
            if let Some((first, second)) = mins {
                built = built.mins(first, second);
            }
            Box::new(common.apply(built))
        }));
    }
    if let Ok(tabs) = component.cast::<Tabs>() {
        let common = common(component)?;
        let tabs = tabs.get();
        let active = tabs.active;
        let children: Vec<(String, Py<PyAny>)> = tabs
            .tabs()
            .iter()
            .map(|(title, child)| (title.clone(), child.clone_ref(py)))
            .collect();
        let children = children
            .iter()
            .map(|(title, child)| Ok((title.clone(), node(py, child.bind(py), depth + 1)?)))
            .collect::<PyResult<Vec<_>>>()?;
        return Ok(Box::new(move || {
            let mut built = CoreTabs::new();
            for (title, child) in children {
                built.push(title, child());
            }
            Box::new(common.apply(built.active(active)))
        }));
    }
    if let Ok(layers) = component.cast::<Layers>() {
        let common = common(component)?;
        let layers = layers.get();
        let base = node(py, layers.base.bind(py), depth + 1)?;
        let open: Vec<Py<Layer>> = layers
            .open
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .iter()
            .map(|layer| layer.clone_ref(py))
            .collect();
        let open = open
            .iter()
            .map(|layer| layer.get().prepare(py, depth + 1))
            .collect::<PyResult<Vec<_>>>()?;
        let openers: Vec<Opener> = layers
            .openers
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .iter()
            .map(|opener| opener.clone_ref(py))
            .collect();
        return Ok(Box::new(move || {
            let mut built = CoreLayers::new(base());
            for layer in open {
                built.open(layer.build());
            }
            for opener in openers {
                let Opener {
                    action,
                    keys,
                    description,
                    factory,
                } = opener;
                built = built.open_on(&action, keys, &description, move || open_layer(&factory));
            }
            Box::new(common.apply(built))
        }));
    }
    if let Ok(label) = component.cast::<Label>() {
        let markup = label.get().markup.clone();
        return Ok(Box::new(move || Box::new(CoreLabel::new(markup))));
    }
    if let Ok(map) = component.cast::<Map>() {
        let map = map.get();
        let inner = node(py, map.component.bind(py), depth + 1)?;
        let done = map.done.as_ref().map(|done| done.clone_ref(py));
        let cancel = map.cancel.as_ref().map(|cancel| cancel.clone_ref(py));
        return Ok(Box::new(move || {
            let mapped = inner().map(move |value| match &done {
                Some(done) => call_flow(done, |py| PyTuple::new(py, [value]), "done()"),
                None => Flow::Done(value),
            });
            match cancel {
                Some(cancel) => Box::new(mapped.on_cancel(move || {
                    call_flow(&cancel, |py| Ok(PyTuple::empty(py)), "cancel()")
                })),
                None => Box::new(mapped),
            }
        }));
    }
    match components::leaf(py, component)? {
        Some(node) => Ok(node),
        None => Err(PyTypeError::new_err(format!(
            "expected an interactive component (a built-in, a container, a Label, a Map, a \
             Component subclass, or an object with handle() and render()), got {}",
            component.get_type().name()?
        ))),
    }
}

/// A child checked when it is added, so a mistake raises where it was made
/// rather than when the composition runs.
fn check(component: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
    // Taking the node checks it; it is built again for each run.
    drop(node(component.py(), component, 0)?);
    Ok(component.clone().unbind())
}

// ---------------------------------------------------------------------------
// What a Python component sees

/// `Context`: what a component renders for and starts with: `width` and
/// `height`, the columns and rows it has.
#[pyclass(name = "Context", module = "rs_rich.interact", frozen)]
pub(crate) struct PyContext {
    #[pyo3(get)]
    width: usize,
    #[pyo3(get)]
    height: usize,
}

impl PyContext {
    pub(super) fn new(context: &Context<'_>) -> PyContext {
        PyContext {
            width: context.width,
            height: context.height,
        }
    }
}

#[pymethods]
impl PyContext {
    fn __repr__(&self) -> String {
        format!("Context(width={}, height={})", self.width, self.height)
    }
}

/// `Component`: subclass it to write a component in Python. Override
/// `render(context)` (return a renderable; a `str` is console markup) and
/// `handle(event)` (return `None` to carry on, `Done(value)` to finish,
/// `Cancel()` to cancel, or `Ignored()` to leave the event to the container,
/// the default). `keymap()` declares its keys for help and hints;
/// `focusable()`, `mouse()`, `tick()`, `start(context)` and
/// `default_value()` have defaults. It composes with the built-ins in
/// every container and runs through the same drivers.
#[pyclass(name = "Component", module = "rs_rich.interact", subclass)]
pub(crate) struct Base;

#[pymethods]
impl Base {
    #[new]
    #[pyo3(signature = (*_args, **_kwargs))]
    fn new(_args: &Bound<'_, PyTuple>, _kwargs: Option<&Bound<'_, PyDict>>) -> Self {
        Base
    }

    /// Handle one event. The default leaves every event to the container.
    fn handle(&self, event: &Bound<'_, PyAny>) -> Ignored {
        let _ = event;
        Ignored
    }

    /// Render the current state for `context`.
    fn render(slf: &Bound<'_, Self>, context: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        let _ = context;
        Err(PyNotImplementedError::new_err(format!(
            "{} must define render(context)",
            slf.get_type().name()?
        )))
    }

    /// Called once with the context before the first paint. `Done(value)`
    /// or `Cancel()` finishes without waiting for a key.
    fn start(&self, context: &Bound<'_, PyAny>) -> Option<Py<PyAny>> {
        let _ = context;
        None
    }

    /// The component's bindings now, or `None` for none.
    fn keymap(&self) -> Option<Keymap> {
        None
    }

    /// Whether Tab stops on it in a container (default: yes).
    fn focusable(&self) -> bool {
        true
    }

    /// Whether it wants mouse events (default: no).
    fn mouse(&self) -> bool {
        false
    }

    /// How often, in seconds, it wants a `"tick"` event (default: never).
    fn tick(&self) -> Option<f64> {
        None
    }

    /// The answer without a terminal, when the fallback is `"default"`.
    fn default_value(&self) -> Option<Py<PyAny>> {
        None
    }

    /// `map(done=None, *, cancel=None)`: `Map(self, done, cancel=cancel)`.
    #[pyo3(signature = (done=None, *, cancel=None))]
    fn map(
        slf: &Bound<'_, Self>,
        done: Option<Py<PyAny>>,
        cancel: Option<Py<PyAny>>,
    ) -> PyResult<Map> {
        Map::new(slf.as_any(), done, cancel)
    }

    /// `ask(**options)`: `rs_rich.interact.ask(self, **options)`.
    #[pyo3(signature = (**options))]
    fn ask(slf: &Bound<'_, Self>, options: Option<&Bound<'_, PyDict>>) -> PyResult<Py<PyAny>> {
        super::interact_ask(slf.py(), slf.as_any(), options)
    }

    /// `headless(script=None, *, width=80, height=24)`:
    /// `rs_rich.interact.headless(self, ...)`.
    #[pyo3(signature = (script=None, *, width=80, height=24))]
    fn headless(
        slf: &Bound<'_, Self>,
        script: Option<&Bound<'_, PyAny>>,
        width: u16,
        height: u16,
    ) -> PyResult<Record> {
        super::interact_headless(slf.py(), slf.as_any(), script, width, height)
    }
}

// ---------------------------------------------------------------------------
// Leaves

/// `Label(markup)`: console markup that takes no focus and uses no events:
/// a title, a note.
#[pyclass(name = "Label", module = "rs_rich.interact", frozen)]
pub(crate) struct Label {
    #[pyo3(get)]
    markup: String,
}

#[pymethods]
impl Label {
    #[new]
    fn new(markup: String) -> Self {
        Label { markup }
    }

    fn __repr__(&self) -> String {
        format!("Label({:?})", self.markup)
    }
}

/// `Map(component, done=None, *, cancel=None)`: what `component`'s answer
/// means for its container. `done(value)` returns `None` to carry on (the
/// component stays, showing its answer, and focus moves past it),
/// `Done(value)` to finish the composition, or `Cancel()`; without `done`,
/// the answer finishes the composition as it is. `cancel()` says what
/// cancelling means; without it, cancelling cancels the composition.
#[pyclass(name = "Map", module = "rs_rich.interact", frozen)]
pub(crate) struct Map {
    component: Py<PyAny>,
    done: Option<Py<PyAny>>,
    cancel: Option<Py<PyAny>>,
}

#[pymethods]
impl Map {
    fn __traverse__(&self, visit: PyVisit<'_>) -> Result<(), PyTraverseError> {
        visit.call(&self.component)?;
        if let Some(done) = &self.done {
            visit.call(done)?;
        }
        if let Some(cancel) = &self.cancel {
            visit.call(cancel)?;
        }
        Ok(())
    }

    #[new]
    #[pyo3(signature = (component, done=None, *, cancel=None))]
    fn new(
        component: &Bound<'_, PyAny>,
        done: Option<Py<PyAny>>,
        cancel: Option<Py<PyAny>>,
    ) -> PyResult<Self> {
        let py = component.py();
        for (callback, what) in [(&done, "done"), (&cancel, "cancel")] {
            if let Some(callback) = callback {
                if !callback.bind(py).is_callable() {
                    return Err(PyTypeError::new_err(format!("{what} must be callable")));
                }
            }
        }
        Ok(Map {
            component: check(component)?,
            done,
            cancel,
        })
    }

    /// The component it maps.
    #[getter]
    fn component(&self, py: Python<'_>) -> Py<PyAny> {
        self.component.clone_ref(py)
    }

    /// `ask(**options)`: `rs_rich.interact.ask(self, **options)`.
    #[pyo3(signature = (**options))]
    fn ask(slf: &Bound<'_, Self>, options: Option<&Bound<'_, PyDict>>) -> PyResult<Py<PyAny>> {
        super::interact_ask(slf.py(), slf.as_any(), options)
    }

    /// `headless(script=None, *, width=80, height=24)`:
    /// `rs_rich.interact.headless(self, ...)`.
    #[pyo3(signature = (script=None, *, width=80, height=24))]
    fn headless(
        slf: &Bound<'_, Self>,
        script: Option<&Bound<'_, PyAny>>,
        width: u16,
        height: u16,
    ) -> PyResult<Record> {
        super::interact_headless(slf.py(), slf.as_any(), script, width, height)
    }

    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        Ok(format!("Map({})", self.component.bind(py).repr()?))
    }
}

// ---------------------------------------------------------------------------
// Containers

/// A binding a container runs: `on` (after the focused child leaves the
/// key) or `shortcut` (before the child sees it).
struct Handler {
    action: String,
    keys: Vec<Key>,
    description: String,
    shortcut: bool,
    callback: Py<PyAny>,
}

/// What every container has besides its children.
#[derive(Default)]
struct Common {
    handlers: Vec<Handler>,
    rebinds: Vec<(String, Vec<Key>)>,
    mouse: bool,
}

/// [`Common`], taken for a run.
struct CommonBuild {
    handlers: Vec<Handler>,
    rebinds: Vec<(String, Vec<Key>)>,
    mouse: bool,
}

fn common(component: &Bound<'_, PyAny>) -> PyResult<CommonBuild> {
    let py = component.py();
    let container = component.cast::<Container>()?;
    let common = container
        .get()
        .common
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    Ok(CommonBuild {
        handlers: common
            .handlers
            .iter()
            .map(|handler| Handler {
                action: handler.action.clone(),
                keys: handler.keys.clone(),
                description: handler.description.clone(),
                shortcut: handler.shortcut,
                callback: handler.callback.clone_ref(py),
            })
            .collect(),
        rebinds: common.rebinds.clone(),
        mouse: common.mouse,
    })
}

/// The builder methods every Rust container has, for [`CommonBuild::apply`].
trait Bindable: Sized {
    fn on_key(
        self,
        action: &str,
        keys: Vec<Key>,
        description: &str,
        shortcut: bool,
        handler: impl FnMut() -> Flow<Py<PyAny>> + 'static,
    ) -> Self;
    fn rebind_keys(self, action: &str, keys: Vec<Key>) -> Self;
    fn mouse_on(self) -> Self;
}

macro_rules! bindable {
    ($($container:ident),*) => {$(
        impl Bindable for $container<'static, Py<PyAny>> {
            fn on_key(
                self,
                action: &str,
                keys: Vec<Key>,
                description: &str,
                shortcut: bool,
                handler: impl FnMut() -> Flow<Py<PyAny>> + 'static,
            ) -> Self {
                if shortcut {
                    self.shortcut(action, keys, description, handler)
                } else {
                    self.on(action, keys, description, handler)
                }
            }

            fn rebind_keys(self, action: &str, keys: Vec<Key>) -> Self {
                self.rebind(action, keys)
            }

            fn mouse_on(self) -> Self {
                self.with_mouse(true)
            }
        }
    )*};
}

bindable!(CoreStack, CoreSplit, CoreTabs, CoreLayers);

impl CommonBuild {
    fn apply<C: Bindable>(self, mut container: C) -> C {
        for handler in self.handlers {
            let Handler {
                action,
                keys,
                description,
                shortcut,
                callback,
            } = handler;
            let what = format!("the handler for {action:?}");
            let run = move || call_flow(&callback, |py| Ok(PyTuple::empty(py)), &what);
            container = container.on_key(&action, keys, &description, shortcut, run);
        }
        for (action, keys) in self.rebinds {
            container = container.rebind_keys(&action, keys);
        }
        if self.mouse {
            container = container.mouse_on();
        }
        container
    }
}

/// `Container`: what `Stack` (and `Column`, `Row`), `Split`, `Tabs` and
/// `Layers` share: bindings of your own, rebinding, the mouse, and the
/// drivers as methods.
#[pyclass(name = "Container", module = "rs_rich.interact", subclass, frozen)]
#[derive(Default)]
pub(crate) struct Container {
    common: Mutex<Common>,
}

impl Container {
    fn add<'py>(
        slf: Bound<'py, Self>,
        action: String,
        keys: &Bound<'py, PyAny>,
        description: String,
        callback: Py<PyAny>,
        shortcut: bool,
    ) -> PyResult<Bound<'py, Self>> {
        if !callback.bind(slf.py()).is_callable() {
            return Err(PyTypeError::new_err("the handler must be callable"));
        }
        let keys = keys_arg(keys)?;
        {
            let mut common = slf.get().common.lock().unwrap_or_else(|p| p.into_inner());
            common.handlers.retain(|handler| handler.action != action);
            common.handlers.push(Handler {
                action,
                keys,
                description,
                shortcut,
                callback,
            });
        }
        Ok(slf)
    }
}

#[pymethods]
impl Container {
    fn __traverse__(&self, visit: PyVisit<'_>) -> Result<(), PyTraverseError> {
        // Never block in the collector: a container being changed is alive.
        if let Ok(common) = self.common.try_lock() {
            for handler in &common.handlers {
                visit.call(&handler.callback)?;
            }
        }
        Ok(())
    }

    /// `on(action, keys, description, handler)`: run `handler()` when one
    /// of `keys` bubbles up unused by the focused child. It returns what
    /// happens next: `None` carries on, `Done(value)` finishes, `Cancel()`
    /// cancels. Returns the container.
    fn on<'py>(
        slf: Bound<'py, Self>,
        action: String,
        keys: &Bound<'py, PyAny>,
        description: String,
        handler: Py<PyAny>,
    ) -> PyResult<Bound<'py, Self>> {
        Container::add(slf, action, keys, description, handler, false)
    }

    /// `shortcut(action, keys, description, handler)`: as `on`, but before
    /// the focused child sees the key.
    fn shortcut<'py>(
        slf: Bound<'py, Self>,
        action: String,
        keys: &Bound<'py, PyAny>,
        description: String,
        handler: Py<PyAny>,
    ) -> PyResult<Bound<'py, Self>> {
        Container::add(slf, action, keys, description, handler, true)
    }

    /// `rebind(action, keys)`: make `keys` do one of this container's
    /// actions (`focus-next`, `focus-previous`, and its own); no keys
    /// unbinds it. Returns the container.
    fn rebind<'py>(
        slf: Bound<'py, Self>,
        action: String,
        keys: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, Self>> {
        let keys = keys_arg(keys)?;
        slf.get()
            .common
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .rebinds
            .push((action, keys));
        Ok(slf)
    }

    /// `with_mouse(on=True)`: report the mouse even when no child asks for
    /// it, for a click to focus a child or a border to drag. Returns the
    /// container.
    #[pyo3(signature = (on=true))]
    fn with_mouse(slf: Bound<'_, Self>, on: bool) -> Bound<'_, Self> {
        slf.get()
            .common
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .mouse = on;
        slf
    }

    /// `map(done=None, *, cancel=None)`: `Map(self, done, cancel=cancel)`.
    #[pyo3(signature = (done=None, *, cancel=None))]
    fn map(
        slf: &Bound<'_, Self>,
        done: Option<Py<PyAny>>,
        cancel: Option<Py<PyAny>>,
    ) -> PyResult<Map> {
        Map::new(slf.as_any(), done, cancel)
    }

    /// `keymap()`: every binding the composition has before its first
    /// event: the focused child's, then the container's own.
    fn keymap(slf: &Bound<'_, Self>) -> PyResult<Keymap> {
        keymap_of(slf.py(), slf.as_any())
    }

    /// `ask(**options)`: `rs_rich.interact.ask(self, **options)`.
    #[pyo3(signature = (**options))]
    fn ask(slf: &Bound<'_, Self>, options: Option<&Bound<'_, PyDict>>) -> PyResult<Py<PyAny>> {
        super::interact_ask(slf.py(), slf.as_any(), options)
    }

    /// `headless(script=None, *, width=80, height=24)`:
    /// `rs_rich.interact.headless(self, ...)`.
    #[pyo3(signature = (script=None, *, width=80, height=24))]
    fn headless(
        slf: &Bound<'_, Self>,
        script: Option<&Bound<'_, PyAny>>,
        width: u16,
        height: u16,
    ) -> PyResult<Record> {
        super::interact_headless(slf.py(), slf.as_any(), script, width, height)
    }
}

fn axis(name: &str, what: &str) -> PyResult<Axis> {
    match name {
        "horizontal" => Ok(Axis::Horizontal),
        "vertical" => Ok(Axis::Vertical),
        other => Err(PyValueError::new_err(format!(
            "invalid {what} {other:?}; expected horizontal or vertical"
        ))),
    }
}

fn axis_name(axis: Axis) -> &'static str {
    match axis {
        Axis::Horizontal => "horizontal",
        Axis::Vertical => "vertical",
    }
}

/// `Stack(*children, axis="vertical", gap=0)`: children along an axis, each
/// sized with `child(component, size=..., ratio=...)`. `Column` and `Row`
/// are its two axes.
#[pyclass(name = "Stack", module = "rs_rich.interact", extends = Container, subclass, frozen)]
pub(crate) struct Stack {
    axis: Axis,
    gap: usize,
    children: Mutex<Vec<(Py<PyAny>, Size)>>,
}

impl Stack {
    fn make(children: &Bound<'_, PyTuple>, axis: Axis, gap: usize) -> PyResult<Stack> {
        let children = children
            .iter()
            .map(|child| Ok((check(&child)?, Size::Auto)))
            .collect::<PyResult<Vec<_>>>()?;
        Ok(Stack {
            axis,
            gap,
            children: Mutex::new(children),
        })
    }

    fn children(&self) -> std::sync::MutexGuard<'_, Vec<(Py<PyAny>, Size)>> {
        self.children.lock().unwrap_or_else(|p| p.into_inner())
    }
}

#[pymethods]
impl Stack {
    fn __traverse__(&self, visit: PyVisit<'_>) -> Result<(), PyTraverseError> {
        if let Ok(children) = self.children.try_lock() {
            for (child, _) in children.iter() {
                visit.call(child)?;
            }
        }
        Ok(())
    }

    #[new]
    #[pyo3(signature = (*children, axis="vertical", gap=0))]
    fn new(
        children: &Bound<'_, PyTuple>,
        axis: &str,
        gap: usize,
    ) -> PyResult<PyClassInitializer<Self>> {
        let stack = Stack::make(children, self::axis(axis, "axis")?, gap)?;
        Ok(PyClassInitializer::from(Container::default()).add_subclass(stack))
    }

    /// `child(component, *, size=None, ratio=None)`: add a child. `size`
    /// is a fixed number of cells along the axis; `ratio` a share of what
    /// the fixed and content-sized children leave (`ratio=2` takes twice
    /// what `ratio=1` does); with neither, a child in a column takes the
    /// rows it renders, and in a row the same as `ratio=1`. Returns the
    /// stack.
    #[pyo3(signature = (component, *, size=None, ratio=None))]
    fn child<'py>(
        slf: Bound<'py, Self>,
        component: &Bound<'py, PyAny>,
        size: Option<usize>,
        ratio: Option<u16>,
    ) -> PyResult<Bound<'py, Self>> {
        let size = match (size, ratio) {
            (Some(_), Some(_)) => {
                return Err(PyValueError::new_err("give size or ratio, not both"));
            }
            (Some(cells), None) => Size::Fixed(cells),
            (None, Some(weight)) => Size::Flex(weight),
            (None, None) => Size::Auto,
        };
        let child = check(component)?;
        slf.get().children().push((child, size));
        Ok(slf)
    }

    /// `"vertical"` (a column) or `"horizontal"` (a row).
    #[getter]
    fn axis(&self) -> &'static str {
        axis_name(self.axis)
    }

    #[getter]
    fn gap(&self) -> usize {
        self.gap
    }

    /// The children, in order.
    #[getter(children)]
    fn children_list(&self, py: Python<'_>) -> Vec<Py<PyAny>> {
        self.children()
            .iter()
            .map(|(child, _)| child.clone_ref(py))
            .collect()
    }

    fn __len__(&self) -> usize {
        self.children().len()
    }

    fn __repr__(slf: &Bound<'_, Self>) -> PyResult<String> {
        Ok(format!(
            "<{} of {}>",
            slf.get_type().name()?,
            slf.get().children().len()
        ))
    }
}

/// `Column(*children, gap=0)`: children top to bottom. A child added
/// without a size takes the rows it renders.
#[pyclass(name = "Column", module = "rs_rich.interact", extends = Stack, frozen)]
pub(crate) struct Column;

#[pymethods]
impl Column {
    #[new]
    #[pyo3(signature = (*children, gap=0))]
    fn new(children: &Bound<'_, PyTuple>, gap: usize) -> PyResult<PyClassInitializer<Self>> {
        let stack = Stack::make(children, Axis::Vertical, gap)?;
        Ok(PyClassInitializer::from(Container::default())
            .add_subclass(stack)
            .add_subclass(Column))
    }
}

/// `Row(*children, gap=0)`: children left to right, sharing the width.
#[pyclass(name = "Row", module = "rs_rich.interact", extends = Stack, frozen)]
pub(crate) struct Row;

#[pymethods]
impl Row {
    #[new]
    #[pyo3(signature = (*children, gap=0))]
    fn new(children: &Bound<'_, PyTuple>, gap: usize) -> PyResult<PyClassInitializer<Self>> {
        let stack = Stack::make(children, Axis::Horizontal, gap)?;
        Ok(PyClassInitializer::from(Container::default())
            .add_subclass(stack)
            .add_subclass(Row))
    }
}

/// `Split(first, second, *, axis="horizontal", ratio=50, at=None, min=None,
/// mins=None)`: two panes with a border between them, side by side
/// (`Split.horizontal`) or stacked (`Split.vertical`). `ratio` is the first
/// pane's share in percent, `at` puts the border after that many cells,
/// `min` is both panes' minimum size and `mins` a `(first, second)` pair.
/// Tab moves focus between the panes; Alt+H and Alt+L (Alt+K and Alt+J
/// stacked) move the border, and so does the mouse with `with_mouse()`.
#[pyclass(name = "Split", module = "rs_rich.interact", extends = Container, frozen)]
pub(crate) struct Split {
    first: Py<PyAny>,
    second: Py<PyAny>,
    axis: Axis,
    ratio: u8,
    at: Option<usize>,
    mins: Option<(usize, usize)>,
}

impl Split {
    #[allow(clippy::too_many_arguments)]
    fn make(
        first: &Bound<'_, PyAny>,
        second: &Bound<'_, PyAny>,
        axis: Axis,
        ratio: u8,
        at: Option<usize>,
        min: Option<usize>,
        mins: Option<(usize, usize)>,
    ) -> PyResult<PyClassInitializer<Split>> {
        if ratio > 100 {
            return Err(PyValueError::new_err(format!(
                "ratio must be a percentage from 0 to 100, got {ratio}"
            )));
        }
        let mins = match (min, mins) {
            (Some(_), Some(_)) => {
                return Err(PyValueError::new_err("give min or mins, not both"));
            }
            (Some(cells), None) => Some((cells, cells)),
            (None, mins) => mins,
        };
        let split = Split {
            first: check(first)?,
            second: check(second)?,
            axis,
            ratio,
            at,
            mins,
        };
        Ok(PyClassInitializer::from(Container::default()).add_subclass(split))
    }
}

#[pymethods]
impl Split {
    fn __traverse__(&self, visit: PyVisit<'_>) -> Result<(), PyTraverseError> {
        visit.call(&self.first)?;
        visit.call(&self.second)?;
        Ok(())
    }

    #[new]
    #[pyo3(signature = (first, second, *, axis="horizontal", ratio=50, at=None, min=None, mins=None))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        first: &Bound<'_, PyAny>,
        second: &Bound<'_, PyAny>,
        axis: &str,
        ratio: u8,
        at: Option<usize>,
        min: Option<usize>,
        mins: Option<(usize, usize)>,
    ) -> PyResult<PyClassInitializer<Self>> {
        Split::make(
            first,
            second,
            self::axis(axis, "axis")?,
            ratio,
            at,
            min,
            mins,
        )
    }

    /// `Split.horizontal(left, right, **options)`: side by side.
    #[staticmethod]
    #[pyo3(signature = (left, right, *, ratio=50, at=None, min=None, mins=None))]
    fn horizontal(
        left: &Bound<'_, PyAny>,
        right: &Bound<'_, PyAny>,
        ratio: u8,
        at: Option<usize>,
        min: Option<usize>,
        mins: Option<(usize, usize)>,
    ) -> PyResult<Py<Self>> {
        let init = Split::make(left, right, Axis::Horizontal, ratio, at, min, mins)?;
        Py::new(left.py(), init)
    }

    /// `Split.vertical(top, bottom, **options)`: one above the other.
    #[staticmethod]
    #[pyo3(signature = (top, bottom, *, ratio=50, at=None, min=None, mins=None))]
    fn vertical(
        top: &Bound<'_, PyAny>,
        bottom: &Bound<'_, PyAny>,
        ratio: u8,
        at: Option<usize>,
        min: Option<usize>,
        mins: Option<(usize, usize)>,
    ) -> PyResult<Py<Self>> {
        let init = Split::make(top, bottom, Axis::Vertical, ratio, at, min, mins)?;
        Py::new(top.py(), init)
    }

    #[getter]
    fn first(&self, py: Python<'_>) -> Py<PyAny> {
        self.first.clone_ref(py)
    }

    #[getter]
    fn second(&self, py: Python<'_>) -> Py<PyAny> {
        self.second.clone_ref(py)
    }

    #[getter]
    fn axis(&self) -> &'static str {
        axis_name(self.axis)
    }

    #[getter]
    fn ratio(&self) -> u8 {
        self.ratio
    }

    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        Ok(format!(
            "Split.{}({}, {})",
            axis_name(self.axis),
            self.first.bind(py).repr()?,
            self.second.bind(py).repr()?
        ))
    }
}

/// `Tabs(tabs=(), *, active=0)`: one child at a time under a bar of
/// titles; `tabs` is `(title, component)` pairs or a mapping, and `tab`
/// adds one. Every tab keeps its state while hidden. Alt+Right and
/// Alt+Left switch tabs, Alt+1 to Alt+9 pick one.
#[pyclass(name = "Tabs", module = "rs_rich.interact", extends = Container, frozen)]
pub(crate) struct Tabs {
    tabs: Mutex<Vec<(String, Py<PyAny>)>>,
    active: usize,
}

impl Tabs {
    fn tabs(&self) -> std::sync::MutexGuard<'_, Vec<(String, Py<PyAny>)>> {
        self.tabs.lock().unwrap_or_else(|p| p.into_inner())
    }
}

#[pymethods]
impl Tabs {
    fn __traverse__(&self, visit: PyVisit<'_>) -> Result<(), PyTraverseError> {
        if let Ok(tabs) = self.tabs.try_lock() {
            for (_, child) in tabs.iter() {
                visit.call(child)?;
            }
        }
        Ok(())
    }

    #[new]
    #[pyo3(signature = (tabs=None, *, active=0))]
    fn new(tabs: Option<&Bound<'_, PyAny>>, active: usize) -> PyResult<PyClassInitializer<Self>> {
        let mut pairs = Vec::new();
        if let Some(tabs) = tabs.filter(|tabs| !tabs.is_none()) {
            let tabs = match tabs.cast::<PyMapping>() {
                Ok(mapping) => mapping.items()?.into_any(),
                Err(_) => tabs.clone(),
            };
            for pair in iterable::<(String, Bound<'_, PyAny>)>(&tabs, "tabs")? {
                let (title, child) = pair;
                pairs.push((title, check(&child)?));
            }
        }
        let tabs = Tabs {
            tabs: Mutex::new(pairs),
            active,
        };
        Ok(PyClassInitializer::from(Container::default()).add_subclass(tabs))
    }

    /// `tab(title, component)`: add a tab. Returns the tabs.
    fn tab<'py>(
        slf: Bound<'py, Self>,
        title: String,
        component: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, Self>> {
        let child = check(component)?;
        slf.get().tabs().push((title, child));
        Ok(slf)
    }

    /// The titles, in order.
    #[getter]
    fn titles(&self) -> Vec<String> {
        self.tabs().iter().map(|(title, _)| title.clone()).collect()
    }

    /// The tab shown first.
    #[getter]
    fn active(&self) -> usize {
        self.active
    }

    fn __len__(&self) -> usize {
        self.tabs().len()
    }

    fn __repr__(&self) -> String {
        format!("Tabs({:?})", self.titles())
    }
}

/// `Layer(component, *, kind="modal", title=None, size=None, border=True,
/// backdrop=None, dismissable=True)`: a component shown over a `Layers`
/// base. A `"modal"` dims the base and traps focus until dismissed; a
/// `"popover"` has no backdrop, and a click outside closes it. `size` is
/// `(width, height)` with the border (default 40 × 10), centred.
#[pyclass(name = "Layer", module = "rs_rich.interact", frozen)]
pub(crate) struct Layer {
    component: Py<PyAny>,
    kind: LayerKind,
    #[pyo3(get)]
    title: Option<String>,
    #[pyo3(get)]
    size: Option<(usize, usize)>,
    #[pyo3(get)]
    border: bool,
    #[pyo3(get)]
    backdrop: Option<bool>,
    #[pyo3(get)]
    dismissable: bool,
}

/// A [`Layer`], taken for a run.
struct LayerBuild {
    component: Node,
    kind: LayerKind,
    title: Option<String>,
    size: Option<(usize, usize)>,
    border: bool,
    backdrop: Option<bool>,
    dismissable: bool,
}

impl LayerBuild {
    fn build(self) -> CoreLayer<'static, Py<PyAny>> {
        let child = (self.component)();
        let mut layer = match self.kind {
            LayerKind::Popover => CoreLayer::popover(child),
            _ => CoreLayer::modal(child),
        }
        .border(self.border)
        .dismissable(self.dismissable);
        if let Some(title) = self.title {
            layer = layer.title(title);
        }
        if let Some((width, height)) = self.size {
            layer = layer.size(width, height);
        }
        if let Some(backdrop) = self.backdrop {
            layer = layer.backdrop(backdrop);
        }
        layer
    }
}

impl Layer {
    fn prepare(&self, py: Python<'_>, depth: usize) -> PyResult<LayerBuild> {
        Ok(LayerBuild {
            component: node(py, self.component.bind(py), depth)?,
            kind: self.kind,
            title: self.title.clone(),
            size: self.size,
            border: self.border,
            backdrop: self.backdrop,
            dismissable: self.dismissable,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn make(
        component: &Bound<'_, PyAny>,
        kind: LayerKind,
        title: Option<String>,
        size: Option<(usize, usize)>,
        border: bool,
        backdrop: Option<bool>,
        dismissable: bool,
    ) -> PyResult<Layer> {
        Ok(Layer {
            component: check(component)?,
            kind,
            title,
            size,
            border,
            backdrop,
            dismissable,
        })
    }
}

#[pymethods]
impl Layer {
    fn __traverse__(&self, visit: PyVisit<'_>) -> Result<(), PyTraverseError> {
        visit.call(&self.component)?;
        Ok(())
    }

    #[new]
    #[pyo3(signature = (component, *, kind="modal", title=None, size=None, border=true, backdrop=None, dismissable=true))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        component: &Bound<'_, PyAny>,
        kind: &str,
        title: Option<String>,
        size: Option<(usize, usize)>,
        border: bool,
        backdrop: Option<bool>,
        dismissable: bool,
    ) -> PyResult<Self> {
        let kind = match kind {
            "modal" => LayerKind::Modal,
            "popover" => LayerKind::Popover,
            other => {
                return Err(PyValueError::new_err(format!(
                    "invalid kind {other:?}; expected modal or popover"
                )))
            }
        };
        Layer::make(component, kind, title, size, border, backdrop, dismissable)
    }

    /// `Layer.modal(component, **options)`.
    #[staticmethod]
    #[pyo3(signature = (component, *, title=None, size=None, border=true, backdrop=None, dismissable=true))]
    fn modal(
        component: &Bound<'_, PyAny>,
        title: Option<String>,
        size: Option<(usize, usize)>,
        border: bool,
        backdrop: Option<bool>,
        dismissable: bool,
    ) -> PyResult<Self> {
        let kind = LayerKind::Modal;
        Layer::make(component, kind, title, size, border, backdrop, dismissable)
    }

    /// `Layer.popover(component, **options)`.
    #[staticmethod]
    #[pyo3(signature = (component, *, title=None, size=None, border=true, backdrop=None, dismissable=true))]
    fn popover(
        component: &Bound<'_, PyAny>,
        title: Option<String>,
        size: Option<(usize, usize)>,
        border: bool,
        backdrop: Option<bool>,
        dismissable: bool,
    ) -> PyResult<Self> {
        let kind = LayerKind::Popover;
        Layer::make(component, kind, title, size, border, backdrop, dismissable)
    }

    /// `"modal"` or `"popover"`.
    #[getter]
    fn kind(&self) -> &'static str {
        match self.kind {
            LayerKind::Popover => "popover",
            _ => "modal",
        }
    }

    #[getter]
    fn component(&self, py: Python<'_>) -> Py<PyAny> {
        self.component.clone_ref(py)
    }

    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        Ok(format!(
            "Layer.{}({})",
            self.kind(),
            self.component.bind(py).repr()?
        ))
    }
}

/// A key that opens a layer made by a Python factory.
struct Opener {
    action: String,
    keys: Vec<Key>,
    description: String,
    factory: Py<PyAny>,
}

impl Opener {
    fn clone_ref(&self, py: Python<'_>) -> Opener {
        Opener {
            action: self.action.clone(),
            keys: self.keys.clone(),
            description: self.description.clone(),
            factory: self.factory.clone_ref(py),
        }
    }
}

/// The layer a factory makes: a `Layer`, or a component to show as a
/// modal. An exception is kept for the run, and an empty modal stands in
/// until the run ends at once.
fn open_layer(factory: &Py<PyAny>) -> CoreLayer<'static, Py<PyAny>> {
    Python::attach(|py| {
        let made = factory
            .bind(py)
            .call0()
            .and_then(|made| match made.cast::<Layer>() {
                Ok(layer) => layer.get().prepare(py, 0).map(LayerBuild::build),
                Err(_) => node(py, &made, 0).map(|child| CoreLayer::modal(child())),
            });
        made.unwrap_or_else(|error| {
            renderable::report_error(py, error);
            CoreLayer::modal(CoreLabel::new(""))
        })
    })
}

/// `Layers(base)`: a base component with modal dialogs and popovers over
/// it. `open_on(action, keys, description, factory)` opens the layer
/// `factory()` makes (a `Layer`, or a component to show as a modal) when
/// one of `keys` reaches it unused; `open(layer)` starts with a layer open.
/// Escape dismisses the top layer; a layer whose child answers closes, and
/// the answer goes up.
#[pyclass(name = "Layers", module = "rs_rich.interact", extends = Container, frozen)]
pub(crate) struct Layers {
    base: Py<PyAny>,
    openers: Mutex<Vec<Opener>>,
    open: Mutex<Vec<Py<Layer>>>,
}

#[pymethods]
impl Layers {
    fn __traverse__(&self, visit: PyVisit<'_>) -> Result<(), PyTraverseError> {
        visit.call(&self.base)?;
        if let Ok(openers) = self.openers.try_lock() {
            for opener in openers.iter() {
                visit.call(&opener.factory)?;
            }
        }
        if let Ok(open) = self.open.try_lock() {
            for layer in open.iter() {
                visit.call(layer)?;
            }
        }
        Ok(())
    }

    #[new]
    fn new(base: &Bound<'_, PyAny>) -> PyResult<PyClassInitializer<Self>> {
        let layers = Layers {
            base: check(base)?,
            openers: Mutex::new(Vec::new()),
            open: Mutex::new(Vec::new()),
        };
        Ok(PyClassInitializer::from(Container::default()).add_subclass(layers))
    }

    /// `open_on(action, keys, description, factory)`: see the class.
    /// Returns the layers.
    fn open_on<'py>(
        slf: Bound<'py, Self>,
        action: String,
        keys: &Bound<'py, PyAny>,
        description: String,
        factory: Py<PyAny>,
    ) -> PyResult<Bound<'py, Self>> {
        if !factory.bind(slf.py()).is_callable() {
            return Err(PyTypeError::new_err("the factory must be callable"));
        }
        let opener = Opener {
            action,
            keys: keys_arg(keys)?,
            description,
            factory,
        };
        slf.get()
            .openers
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(opener);
        Ok(slf)
    }

    /// `open(layer)`: start with `layer` open, over any opened before.
    /// Returns the layers.
    fn open<'py>(slf: Bound<'py, Self>, layer: Py<Layer>) -> Bound<'py, Self> {
        slf.get()
            .open
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(layer);
        slf
    }

    #[getter]
    fn base(&self, py: Python<'_>) -> Py<PyAny> {
        self.base.clone_ref(py)
    }

    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        Ok(format!("Layers({})", self.base.bind(py).repr()?))
    }
}

/// The keymap of `component` (a composition or any component) before its
/// first event: what a help overlay would list.
pub(super) fn keymap_of(py: Python<'_>, component: &Bound<'_, PyAny>) -> PyResult<Keymap> {
    let node = node(py, component, 0)?;
    let keymap = scoped(py, 80, 24, false, || Ok(node().keymap()))?;
    Ok(Keymap::from_core(keymap))
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    let py = m.py();
    for (name, class) in [
        ("InteractComponent", py.get_type::<Base>()),
        ("InteractContext", py.get_type::<PyContext>()),
        ("InteractLabel", py.get_type::<Label>()),
        ("InteractMap", py.get_type::<Map>()),
        ("InteractContainer", py.get_type::<Container>()),
        ("InteractStack", py.get_type::<Stack>()),
        ("InteractColumn", py.get_type::<Column>()),
        ("InteractRow", py.get_type::<Row>()),
        ("InteractSplit", py.get_type::<Split>()),
        ("InteractTabs", py.get_type::<Tabs>()),
        ("InteractLayer", py.get_type::<Layer>()),
        ("InteractLayers", py.get_type::<Layers>()),
    ] {
        m.add(name, class)?;
    }
    Ok(())
}
