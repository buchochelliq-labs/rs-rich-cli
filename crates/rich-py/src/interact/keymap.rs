//! `Keymap` and `Binding` (0.0.14): a component's keys, declared rather
//! than matched inline, so containers list them (help, hints) and they can
//! be rebound. Mirrors `rich_interact::keymap`.

use std::sync::Mutex;

use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyString;

use rich_interact::keymap::{try_keys, Binding as CoreBinding, Keymap as CoreKeymap};
use rich_interact::Key;

use super::components::Event;
use super::iterable;

/// Keys from a `str` of names separated by spaces or commas (`"up k"`,
/// `"ctrl+n, down"`) or an iterable of names.
pub(super) fn keys_arg(value: &Bound<'_, PyAny>) -> PyResult<Vec<Key>> {
    let parse = |names: &str| {
        try_keys(names).map_err(|name| PyValueError::new_err(format!("unknown key name {name:?}")))
    };
    if let Ok(names) = value.cast::<PyString>() {
        return parse(&names.to_cow()?);
    }
    let mut keys = Vec::new();
    for name in iterable::<String>(value, "keys")? {
        keys.extend(parse(&name)?);
    }
    Ok(keys)
}

/// The keymap a component's `keymap()` returned: a `Keymap`, or `None` for
/// none.
pub(super) fn keymap_of(value: &Bound<'_, PyAny>, what: &str) -> PyResult<CoreKeymap> {
    if value.is_none() {
        return Ok(CoreKeymap::default());
    }
    match value.cast::<Keymap>() {
        Ok(keymap) => Ok(keymap.get().inner()),
        Err(_) => Err(PyTypeError::new_err(format!(
            "{what} must return a Keymap or None, got {}",
            value.get_type().name()?
        ))),
    }
}

/// `Binding(context, action, keys, description)`: one thing a key does.
/// `keys` are the key names that do it now, the first the one to show.
#[pyclass(name = "Binding", module = "rs_rich.interact", frozen)]
pub(crate) struct Binding {
    inner: CoreBinding,
}

impl Binding {
    pub(super) fn from_core(inner: CoreBinding) -> Binding {
        Binding { inner }
    }
}

#[pymethods]
impl Binding {
    #[new]
    #[pyo3(signature = (context, action, keys, description=String::new()))]
    fn new(
        context: String,
        action: String,
        keys: &Bound<'_, PyAny>,
        description: String,
    ) -> PyResult<Self> {
        Ok(Binding {
            inner: CoreBinding::new(context, action, keys_arg(keys)?, description),
        })
    }

    /// Which component or container it belongs to: `select`, `split`.
    #[getter]
    fn context(&self) -> &str {
        &self.inner.context
    }

    /// What it does, for code and configuration: `down`.
    #[getter]
    fn action(&self) -> &str {
        &self.inner.action
    }

    /// The keys that do it, by name.
    #[getter]
    fn keys(&self) -> Vec<String> {
        self.inner.keys.iter().map(Key::to_string).collect()
    }

    /// What it does, for people: `move down`.
    #[getter]
    fn description(&self) -> &str {
        &self.inner.description
    }

    /// `context.action`: how configuration names it.
    #[getter]
    fn id(&self) -> String {
        self.inner.id()
    }

    /// The keys as people read them: `up/ctrl+p`.
    #[getter]
    fn keys_label(&self) -> String {
        self.inner.keys_label()
    }

    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        other
            .cast::<Binding>()
            .is_ok_and(|other| other.get().inner == self.inner)
    }

    fn __repr__(&self) -> String {
        format!(
            "Binding({:?}, {:?}, {:?}, {:?})",
            self.inner.context,
            self.inner.action,
            self.inner.keys_label(),
            self.inner.description
        )
    }
}

/// `Keymap(context="component")`: a component's bindings. Declare them
/// with `bind` (it returns the keymap, so calls chain), then ask what a key
/// means with `action(event)` instead of matching keys yourself. A
/// component's `keymap()` returns it, so containers list its keys and
/// `rebind` changes them.
#[pyclass(name = "Keymap", module = "rs_rich.interact", frozen)]
pub(crate) struct Keymap {
    inner: Mutex<CoreKeymap>,
}

impl Keymap {
    pub(super) fn from_core(inner: CoreKeymap) -> Keymap {
        Keymap {
            inner: Mutex::new(inner),
        }
    }

    pub(super) fn inner(&self) -> CoreKeymap {
        self.inner.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    fn with<T>(&self, f: impl FnOnce(&mut CoreKeymap) -> T) -> T {
        f(&mut self.inner.lock().unwrap_or_else(|p| p.into_inner()))
    }
}

/// A key from a key `Event` or a key name.
fn key_arg(value: &Bound<'_, PyAny>) -> PyResult<Option<Key>> {
    if let Ok(event) = value.cast::<Event>() {
        return Ok(event.get().pressed());
    }
    if let Ok(name) = value.cast::<PyString>() {
        let name = name.to_cow()?;
        return Key::parse(&name)
            .map(Some)
            .ok_or_else(|| PyValueError::new_err(format!("unknown key name {name:?}")));
    }
    Err(PyTypeError::new_err(format!(
        "expected an Event or a key name, got {}",
        value.get_type().name()?
    )))
}

#[pymethods]
impl Keymap {
    #[new]
    #[pyo3(signature = (context="component"))]
    fn new(context: &str) -> Self {
        Keymap::from_core(CoreKeymap::new(context))
    }

    /// The context `bind` puts bindings in.
    #[getter]
    fn context(&self) -> String {
        self.with(|keymap| keymap.context().to_string())
    }

    /// `bind(action, keys, description="")`: declare that `keys` (names
    /// separated by spaces or commas, or a list of names) do `action`.
    /// Returns the keymap.
    #[pyo3(signature = (action, keys, description=String::new()))]
    fn bind<'py>(
        slf: Bound<'py, Self>,
        action: String,
        keys: &Bound<'py, PyAny>,
        description: String,
    ) -> PyResult<Bound<'py, Self>> {
        let keys = keys_arg(keys)?;
        slf.get().with(|keymap| {
            let binding = CoreBinding::new(keymap.context(), action, keys, description);
            keymap.add(binding);
        });
        Ok(slf)
    }

    /// `rebind(action, keys)`: make `keys` do `action` instead of what was
    /// declared (no keys unbinds it). Returns the keymap.
    fn rebind<'py>(
        slf: Bound<'py, Self>,
        action: &str,
        keys: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, Self>> {
        let keys = keys_arg(keys)?;
        slf.get().with(|keymap| keymap.rebind(action, keys));
        Ok(slf)
    }

    /// `action(key)`: the action a key `Event` (or a key name) triggers, or
    /// `None`; `None` for any other event.
    fn action(&self, key: &Bound<'_, PyAny>) -> PyResult<Option<String>> {
        let Some(key) = key_arg(key)? else {
            return Ok(None);
        };
        Ok(self.with(|keymap| keymap.action(key).map(str::to_string)))
    }

    /// `keys(action)`: the names of the keys that trigger `action` now.
    fn keys(&self, action: &str) -> Vec<String> {
        self.with(|keymap| keymap.keys(action).iter().map(Key::to_string).collect())
    }

    /// Every binding, with the keys that do it now.
    #[getter]
    fn bindings(&self) -> Vec<Binding> {
        self.with(|keymap| keymap.bindings())
            .into_iter()
            .map(Binding::from_core)
            .collect()
    }

    fn __len__(&self) -> usize {
        self.with(|keymap| keymap.len())
    }

    fn __repr__(&self) -> String {
        self.with(|keymap| format!("<Keymap {:?} bindings={}>", keymap.context(), keymap.len()))
    }
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    let py = m.py();
    m.add("InteractKeymap", py.get_type::<Keymap>())?;
    m.add("InteractBinding", py.get_type::<Binding>())?;
    Ok(())
}
