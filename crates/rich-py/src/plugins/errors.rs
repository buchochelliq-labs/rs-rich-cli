//! `PluginError`, `HighlightError` and friends, and how a Python exception
//! raised inside a plugin callback gets back to Python.
//!
//! # Exceptions in callbacks
//!
//! Rust calls a Python plugin through a trait whose method either cannot
//! fail (`Highlighter::highlight`, `FenceRenderer::render_fence`) or fails
//! with a string-like error (`PluginError`, `HighlightError`). The original
//! Python exception is kept, so it is never reduced to a message:
//!
//! - when Python called into Rust directly (a handle's `highlight`,
//!   `transform` or `render`, or `ExtensionRegistry.add_plugin`), the
//!   exception is stashed and the handle raises it (or a `PluginError` whose
//!   `__cause__` it is);
//! - when core called the plugin while rendering (a `Console.print`), the
//!   exception goes to the render's scope, like one from `__rich_console__`,
//!   and the print raises it.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Mutex;

use pyo3::create_exception;
use pyo3::exceptions::{PyException, PyRuntimeError, PyValueError};
use pyo3::prelude::*;

use rich::console::Console as CoreConsole;
use rich::protocol::{HighlightError as CoreHighlightError, Renderable};
use rich_plugin_api::{Capability, PluginError as CorePluginError};

use crate::renderable::{self, Ambient, AsRenderable, PyRenderable};

create_exception!(_native, PluginError, PyException);
create_exception!(_native, HighlightError, PyException);
create_exception!(_native, UnknownThemeError, HighlightError);
create_exception!(_native, HighlighterChoiceError, PyValueError);

/// The `kind` of a `PluginError`, one per Rust variant.
fn kind(error: &CorePluginError) -> &'static str {
    match error {
        CorePluginError::IncompatibleApi { .. } => "incompatible_api",
        CorePluginError::DuplicatePlugin { .. } => "duplicate_plugin",
        CorePluginError::Conflict { .. } => "conflict",
        CorePluginError::InvalidName { .. } => "invalid_name",
        CorePluginError::Failed { .. } => "failed",
        _ => "other",
    }
}

/// A Python `PluginError` for a Rust one: the message is Rust's, and the
/// variant's fields are attributes (`kind`, `plugin`, `name`, ...). Its
/// `__cause__` is the Python exception behind it, if one was stashed.
pub(crate) fn plugin_error(py: Python<'_>, error: &CorePluginError) -> PyErr {
    let raised = PluginError::new_err(error.to_string());
    let value = raised.value(py);
    // Setting an attribute on a fresh exception cannot fail in practice;
    // an error here would only lose a detail, never the exception.
    let set = |name: &str, item: &dyn Fn() -> PyResult<Py<PyAny>>| {
        if let Ok(item) = item() {
            let _ = value.setattr(name, item);
        }
    };
    let text =
        |text: &str| -> PyResult<Py<PyAny>> { Ok(text.into_pyobject(py)?.into_any().unbind()) };
    let number = |n: u32| -> PyResult<Py<PyAny>> { Ok(n.into_pyobject(py)?.into_any().unbind()) };
    set("kind", &|| text(kind(error)));
    for field in [
        "plugin",
        "name",
        "existing",
        "capability",
        "built_for",
        "host",
        "message",
        "stage",
    ] {
        set(field, &|| Ok(py.None()));
    }
    match error {
        CorePluginError::IncompatibleApi {
            plugin,
            built_for,
            host,
        } => {
            set("plugin", &|| text(plugin));
            set("built_for", &|| number(*built_for));
            set("host", &|| number(*host));
        }
        CorePluginError::DuplicatePlugin { id } => set("plugin", &|| text(id)),
        CorePluginError::Conflict {
            capability,
            existing,
            plugin,
        } => {
            set("plugin", &|| text(plugin));
            set("existing", &|| text(existing));
            set("capability", &|| {
                Ok(Py::new(py, super::types::Capability::from_core(capability))?.into_any())
            });
        }
        CorePluginError::InvalidName { plugin, name } => {
            set("plugin", &|| text(plugin));
            set("name", &|| text(name));
        }
        CorePluginError::Failed { plugin, message } => {
            set("plugin", &|| text(plugin));
            set("message", &|| text(message));
        }
        CorePluginError::Other(message) => set("message", &|| text(message)),
        _ => {}
    }
    if let Some(cause) = take_stashed() {
        raised.set_cause(py, Some(cause));
    }
    raised
}

/// A Python `HighlightError` (or `UnknownThemeError`) for a Rust one.
pub(crate) fn highlight_error(py: Python<'_>, error: &CoreHighlightError) -> PyErr {
    let raised = match error {
        CoreHighlightError::UnknownTheme(name) => UnknownThemeError::new_err(name.clone()),
        CoreHighlightError::Engine(_) => HighlightError::new_err(error.to_string()),
    };
    if let Some(cause) = take_stashed() {
        raised.set_cause(py, Some(cause));
    }
    raised
}

/// What a Rust message says about a Python exception: `Type: message`.
pub(crate) fn describe(py: Python<'_>, error: &PyErr) -> String {
    let name = error
        .get_type(py)
        .name()
        .map(|n| n.to_string())
        .unwrap_or_else(|_| "Exception".to_string());
    let message = error
        .value(py)
        .str()
        .map(|s| s.to_string())
        .unwrap_or_default();
    if message.is_empty() {
        name
    } else {
        format!("{name}: {message}")
    }
}

// ---------------------------------------------------------------------------
// Direct calls and the stash

thread_local! {
    /// For each direct call in progress (innermost last): the render scope
    /// it started in, if any.
    static DIRECT: RefCell<Vec<Option<Rc<Ambient>>>> = const { RefCell::new(Vec::new()) };
    /// The first Python exception a callback raised during a direct call.
    static STASH: RefCell<Option<PyErr>> = const { RefCell::new(None) };
}

struct DirectGuard;

impl Drop for DirectGuard {
    fn drop(&mut self) {
        DIRECT.with(|direct| direct.borrow_mut().pop());
    }
}

/// Run `f`, a call from Python into Rust that may call back into Python:
/// an exception a callback raises is stashed for [`take_stashed`] instead
/// of going to a render scope.
pub(crate) fn direct<T>(f: impl FnOnce() -> T) -> T {
    STASH.with(|stash| stash.borrow_mut().take());
    let scope = renderable::ambient().ok();
    DIRECT.with(|direct| direct.borrow_mut().push(scope));
    let _guard = DirectGuard;
    f()
}

/// The exception stashed by the current direct call, if any.
pub(crate) fn take_stashed() -> Option<PyErr> {
    STASH.with(|stash| stash.borrow_mut().take())
}

/// Whether the innermost direct call is still the innermost Python-facing
/// frame (no render scope was entered since it started).
fn directly_called() -> bool {
    let current = renderable::ambient().ok();
    DIRECT.with(|direct| match direct.borrow().last() {
        None => false,
        Some(started) => match (started, &current) {
            (None, None) => true,
            (Some(a), Some(b)) => Rc::ptr_eq(a, b),
            _ => false,
        },
    })
}

/// A Python callback raised `error`: keep it for the direct caller, or hand
/// it to the render in progress. Returns the message for the Rust error.
pub(crate) fn callback_failed(py: Python<'_>, error: PyErr) -> String {
    let message = describe(py, &error);
    if directly_called() {
        STASH.with(|stash| {
            let mut stash = stash.borrow_mut();
            if stash.is_none() {
                *stash = Some(error);
            }
        });
    } else {
        defer(py, error);
    }
    message
}

/// A pyclass whose conversion to a renderable raises the error it holds.
/// Rendering one inside the current scope is how an error reaches that
/// scope's pending slot through the bridge's public API.
#[pyclass(module = "rs_rich.plugins", frozen)]
pub(crate) struct DeferredError {
    error: Mutex<Option<PyErr>>,
}

impl AsRenderable for DeferredError {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        let error = self
            .error
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
        Err(error.unwrap_or_else(|| PyRuntimeError::new_err("plugin error already reported")))
    }
}

/// Hand `error` to the enclosing render scope (the first error wins); with
/// none, it is reported as unraisable, as a render error outside a console
/// is.
pub(crate) fn defer(py: Python<'_>, error: PyErr) {
    let Ok(holder) = Py::new(
        py,
        DeferredError {
            error: Mutex::new(Some(error)),
        },
    ) else {
        return;
    };
    let console = CoreConsole::builder().width(1).build();
    let options = console.options();
    let _ = PyRenderable::new(holder.into_any()).rich_render(&console, &options);
}

/// The Python class for a capability kind, for error messages.
pub(crate) fn capability_kind(capability: &Capability) -> &'static str {
    match capability {
        Capability::Highlighter => "highlighter",
        Capability::CodeHighlighter(_) => "code_highlighter",
        Capability::Theme(_) => "theme",
        Capability::BoxStyle(_) => "box_style",
        Capability::Renderer(_) => "renderer",
        Capability::FenceRenderer(_) => "fence_renderer",
        Capability::Transform(_) => "transform",
        _ => "other",
    }
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    let py = m.py();
    m.add("PluginError", py.get_type::<PluginError>())?;
    m.add("HighlightError", py.get_type::<HighlightError>())?;
    m.add("UnknownThemeError", py.get_type::<UnknownThemeError>())?;
    m.add(
        "HighlighterChoiceError",
        py.get_type::<HighlighterChoiceError>(),
    )?;
    renderable::register_renderable::<DeferredError>(py);
    Ok(())
}
