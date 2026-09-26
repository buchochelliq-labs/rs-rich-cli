//! `Plugin` (the base a Python plugin subclasses), the built-in plugins,
//! and `PluginRegistrar`, through which a plugin registers what it adds.
//!
//! A Python plugin's `register(registrar)` runs with a Python registrar that
//! collects the registrations; only when `register` returns do they reach
//! the host's registrar, so a plugin that raises leaves nothing behind (the
//! host refuses it, as it refuses a failing Rust plugin). The registrar is
//! closed once `register` returns.

use std::sync::{Arc, Mutex};

use pyo3::exceptions::{PyNotImplementedError, PyRuntimeError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyTuple};

use rich::protocol::{CodeHighlighter as CoreCodeHighlighter, FenceRenderer as CoreFenceRenderer};
use rich::r#box::Box as BoxStyle;
use rich::Theme as CoreTheme;
use rich_plugin_api::{
    HighlighterFactory, Plugin as CorePlugin, PluginError as CorePluginError,
    PluginMetadata as CoreMetadata, PluginRegistrar as CoreRegistrar,
    SourceRenderer as CoreSourceRenderer, TextTransform as CoreTextTransform,
};

use super::adapters::{fence_renderer_arg, highlighter_factory_arg, renderer_arg, transform_arg};
use super::code::code_highlighter_arg;
use super::errors::{self, callback_failed};
use super::types::PluginMetadata;
use crate::boxes::PyBox;
use crate::theme::Theme;

/// One registration, held until the plugin's `register` returns.
pub(crate) enum Registration {
    Highlighter(HighlighterFactory),
    CodeHighlighter(String, Arc<dyn CoreCodeHighlighter>),
    Theme(String, CoreTheme),
    BoxStyle(String, BoxStyle),
    Renderer(String, Arc<dyn CoreSourceRenderer>),
    FenceRenderer(String, Arc<dyn CoreFenceRenderer>),
    Transform(String, Arc<dyn CoreTextTransform>),
}

impl Registration {
    fn replay(self, registrar: &mut dyn CoreRegistrar) {
        match self {
            Registration::Highlighter(factory) => registrar.highlighter(factory),
            Registration::CodeHighlighter(name, value) => registrar.code_highlighter(&name, value),
            Registration::Theme(name, value) => registrar.theme(&name, value),
            Registration::BoxStyle(name, value) => registrar.box_style(&name, value),
            Registration::Renderer(name, value) => registrar.renderer(&name, value),
            Registration::FenceRenderer(name, value) => registrar.fence_renderer(&name, value),
            Registration::Transform(name, value) => registrar.transform(&name, value),
        }
    }
}

/// Collects registrations: a Rust plugin registering into a Python
/// registrar.
struct Collect<'a>(&'a mut Vec<Registration>);

impl CoreRegistrar for Collect<'_> {
    fn highlighter(&mut self, factory: HighlighterFactory) {
        self.0.push(Registration::Highlighter(factory));
    }
    fn code_highlighter(&mut self, name: &str, highlighter: Arc<dyn CoreCodeHighlighter>) {
        self.0
            .push(Registration::CodeHighlighter(name.to_string(), highlighter));
    }
    fn theme(&mut self, name: &str, theme: CoreTheme) {
        self.0.push(Registration::Theme(name.to_string(), theme));
    }
    fn box_style(&mut self, name: &str, style: BoxStyle) {
        self.0.push(Registration::BoxStyle(name.to_string(), style));
    }
    fn renderer(&mut self, name: &str, renderer: Arc<dyn CoreSourceRenderer>) {
        self.0
            .push(Registration::Renderer(name.to_string(), renderer));
    }
    fn fence_renderer(&mut self, language: &str, renderer: Arc<dyn CoreFenceRenderer>) {
        self.0
            .push(Registration::FenceRenderer(language.to_string(), renderer));
    }
    fn transform(&mut self, name: &str, transform: Arc<dyn CoreTextTransform>) {
        self.0
            .push(Registration::Transform(name.to_string(), transform));
    }
}

/// What a plugin's `register(registrar)` receives. Each method adds one
/// capability; names are checked by the host when `register` returns.
#[pyclass(name = "PluginRegistrar", module = "rs_rich.plugins", frozen)]
pub(crate) struct PluginRegistrar {
    /// `None` once the plugin's `register` has returned.
    collected: Mutex<Option<Vec<Registration>>>,
}

impl PluginRegistrar {
    fn open() -> PluginRegistrar {
        PluginRegistrar {
            collected: Mutex::new(Some(Vec::new())),
        }
    }

    fn close(&self) -> Vec<Registration> {
        self.collected
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
            .unwrap_or_default()
    }

    fn push(&self, registration: Registration) -> PyResult<()> {
        let mut collected = self
            .collected
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match collected.as_mut() {
            Some(collected) => {
                collected.push(registration);
                Ok(())
            }
            None => Err(PyRuntimeError::new_err(
                "this PluginRegistrar is closed: register capabilities inside Plugin.register",
            )),
        }
    }

    /// Run a Rust plugin's `register` against this registrar.
    fn register_native(&self, plugin: &dyn CorePlugin) -> Result<(), CorePluginError> {
        let mut staged = Vec::new();
        plugin.register(&mut Collect(&mut staged))?;
        for registration in staged {
            self.push(registration)
                .map_err(|e| CorePluginError::Other(e.to_string()))?;
        }
        Ok(())
    }
}

#[pymethods]
impl PluginRegistrar {
    /// A highlighter for printed text: an object with `highlight(text)`
    /// (`rs_rich.highlighter.Highlighter` and friends), or a class of them,
    /// instantiated once per console it is installed onto.
    fn highlighter(&self, highlighter: &Bound<'_, PyAny>) -> PyResult<()> {
        self.push(Registration::Highlighter(highlighter_factory_arg(
            highlighter,
        )?))
    }

    /// A syntax-highlighting engine, selectable by `name`: a
    /// `CodeHighlighter` (subclass).
    fn code_highlighter(&self, name: String, highlighter: &Bound<'_, PyAny>) -> PyResult<()> {
        self.push(Registration::CodeHighlighter(
            name,
            code_highlighter_arg(highlighter)?,
        ))
    }

    /// A named `Theme`.
    fn theme(&self, name: String, theme: PyRef<'_, Theme>) -> PyResult<()> {
        self.push(Registration::Theme(name, theme.inner.clone()))
    }

    /// A named box style (one of `rs_rich.box`'s).
    fn box_style(&self, name: String, r#box: PyRef<'_, PyBox>) -> PyResult<()> {
        self.push(Registration::BoxStyle(name, r#box.inner))
    }

    /// A named source renderer: a callable `f(source)` (or an object with
    /// `render(source)`) returning a renderable.
    fn renderer(&self, name: String, renderer: &Bound<'_, PyAny>) -> PyResult<()> {
        self.push(Registration::Renderer(name, renderer_arg(renderer)?))
    }

    /// A renderer for Markdown fences in `language`: a callable
    /// `f(language, code)` (or an object with
    /// `render_fence(language, code, console, options)`) returning a
    /// renderable, a list of segments, or `None` to decline.
    fn fence_renderer(&self, language: String, renderer: &Bound<'_, PyAny>) -> PyResult<()> {
        self.push(Registration::FenceRenderer(
            language,
            fence_renderer_arg(renderer)?,
        ))
    }

    /// A named text transform: a callable `f(text) -> Text` (or an object
    /// with `transform(text)`).
    fn transform(&self, name: String, transform: &Bound<'_, PyAny>) -> PyResult<()> {
        self.push(Registration::Transform(name, transform_arg(transform)?))
    }

    fn __repr__(&self) -> String {
        let collected = self
            .collected
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match collected.as_ref() {
            Some(items) => format!("<PluginRegistrar open, {} registered>", items.len()),
            None => "<PluginRegistrar closed>".to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// Plugins

/// Something that extends rich. Subclass it and implement `metadata()`
/// (returning a `PluginMetadata`; a class attribute works too) and
/// `register(registrar)`.
#[pyclass(name = "Plugin", module = "rs_rich.plugins", subclass, frozen)]
pub(crate) struct Plugin {
    /// The Rust plugin, for the built-in plugin classes.
    pub(crate) native: Option<Arc<dyn CorePlugin>>,
}

#[pymethods]
impl Plugin {
    #[new]
    #[pyo3(signature = (*_args, **_kwargs))]
    fn new(_args: &Bound<'_, PyTuple>, _kwargs: Option<&Bound<'_, PyDict>>) -> Self {
        Plugin { native: None }
    }

    fn metadata(&self) -> PyResult<PluginMetadata> {
        match &self.native {
            Some(plugin) => Ok(PluginMetadata {
                inner: plugin.metadata(),
            }),
            None => Err(PyNotImplementedError::new_err(
                "a Plugin subclass must implement metadata()",
            )),
        }
    }

    fn register(&self, py: Python<'_>, registrar: PyRef<'_, PluginRegistrar>) -> PyResult<()> {
        match &self.native {
            Some(plugin) => registrar
                .register_native(plugin.as_ref())
                .map_err(|error| errors::plugin_error(py, &error)),
            None => Err(PyNotImplementedError::new_err(
                "a Plugin subclass must implement register(registrar)",
            )),
        }
    }

    fn __repr__(slf: &Bound<'_, Self>) -> PyResult<String> {
        let name = slf.get_type().name()?;
        Ok(match &slf.get().native {
            Some(plugin) => format!("<{name} {:?}>", plugin.metadata().id),
            None => format!("<{name}>"),
        })
    }
}

/// rich-ext's built-ins: the number highlighter and the `syntect` code
/// highlighter (`ExtensionRegistry.with_defaults()` adds it).
#[pyclass(name = "BuiltinPlugin", module = "rs_rich.plugins", extends = Plugin, frozen)]
pub(crate) struct BuiltinPlugin;

#[pymethods]
impl BuiltinPlugin {
    #[new]
    fn new() -> PyClassInitializer<Self> {
        PyClassInitializer::from(Plugin {
            native: Some(Arc::new(rich_ext::BuiltinPlugin)),
        })
        .add_subclass(BuiltinPlugin)
    }
}

/// Mermaid diagrams: the `mermaid` fence renderer and source renderer.
/// `ascii` draws with ASCII only (`None` follows the console); `backend` is
/// `"text"` or `"mmdc"` (Mermaid's CLI, when the wheel was built with it,
/// falling back to text).
#[pyclass(name = "MermaidPlugin", module = "rs_rich.plugins", extends = Plugin, frozen)]
pub(crate) struct MermaidPlugin;

#[pymethods]
impl MermaidPlugin {
    #[new]
    #[pyo3(signature = (*, ascii=None, backend="text"))]
    fn new(ascii: Option<bool>, backend: &str) -> PyResult<PyClassInitializer<Self>> {
        let backend = match backend {
            "text" => rich_mermaid::Backend::Text,
            "mmdc" => rich_mermaid::Backend::Mmdc,
            other => {
                return Err(PyValueError::new_err(format!(
                    "backend must be \"text\" or \"mmdc\", not {other:?}"
                )))
            }
        };
        // With the `mmdc` feature the options have one more field.
        #[allow(clippy::needless_update)]
        let options = rich_mermaid::MermaidOptions {
            backend,
            ascii,
            ..Default::default()
        };
        Ok(PyClassInitializer::from(Plugin {
            native: Some(Arc::new(rich_mermaid::MermaidPlugin::new(options))),
        })
        .add_subclass(MermaidPlugin))
    }
}

/// The lumis (tree-sitter) code highlighter, registered as `"lumis"`. Only
/// in wheels built with the `lumis` feature.
#[cfg(feature = "lumis")]
#[pyclass(name = "LumisPlugin", module = "rs_rich.plugins", extends = Plugin, frozen)]
pub(crate) struct LumisPlugin;

#[cfg(feature = "lumis")]
#[pymethods]
impl LumisPlugin {
    #[new]
    fn new() -> PyClassInitializer<Self> {
        PyClassInitializer::from(Plugin {
            native: Some(Arc::new(rich_lumis::LumisPlugin)),
        })
        .add_subclass(LumisPlugin)
    }
}

/// A Python plugin as a Rust one. Its metadata was read before the host
/// saw it (reading it can raise).
pub(crate) struct PyPlugin {
    object: Py<PyAny>,
    metadata: CoreMetadata,
}

impl PyPlugin {
    /// Wrap a Python plugin, reading its metadata now.
    pub(crate) fn new(object: &Bound<'_, PyAny>) -> PyResult<PyPlugin> {
        let metadata = match object.getattr_opt("metadata")? {
            Some(value) if value.is_callable() => value.call0()?,
            Some(value) => value,
            None => {
                return Err(PyTypeError::new_err(format!(
                    "a plugin needs metadata() and register(registrar); got {}",
                    object.repr()?
                )))
            }
        };
        let metadata = metadata
            .extract::<PyRef<'_, PluginMetadata>>()
            .map_err(|_| {
                PyTypeError::new_err("a plugin's metadata() must return a PluginMetadata")
            })?
            .inner
            .clone();
        if !object
            .getattr_opt("register")?
            .is_some_and(|m| m.is_callable())
        {
            return Err(PyTypeError::new_err("a plugin needs register(registrar)"));
        }
        Ok(PyPlugin {
            object: object.clone().unbind(),
            metadata,
        })
    }
}

impl CorePlugin for PyPlugin {
    fn metadata(&self) -> CoreMetadata {
        self.metadata.clone()
    }

    fn register(&self, registrar: &mut dyn CoreRegistrar) -> Result<(), CorePluginError> {
        let collected = Python::attach(|py| {
            let python_registrar = Bound::new(py, PluginRegistrar::open())
                .map_err(|error| CorePluginError::Other(callback_failed(py, error)))?;
            let result = self
                .object
                .bind(py)
                .call_method1("register", (&python_registrar,));
            let collected = python_registrar.get().close();
            match result {
                Ok(_) => Ok(collected),
                Err(error) => Err(CorePluginError::Other(callback_failed(py, error))),
            }
        })?;
        for registration in collected {
            registration.replay(registrar);
        }
        Ok(())
    }
}

/// The Rust plugin for a Python plugin argument: a built-in's own, or the
/// adapter.
pub(crate) fn plugin_arg(value: &Bound<'_, PyAny>) -> PyResult<Arc<dyn CorePlugin>> {
    if let Ok(plugin) = value.extract::<PyRef<'_, Plugin>>() {
        if let Some(native) = &plugin.native {
            return Ok(native.clone());
        }
    }
    Ok(Arc::new(PyPlugin::new(value)?))
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PluginRegistrar>()?;
    m.add_class::<Plugin>()?;
    m.add_class::<BuiltinPlugin>()?;
    m.add_class::<MermaidPlugin>()?;
    #[cfg(feature = "lumis")]
    m.add_class::<LumisPlugin>()?;
    Ok(())
}
