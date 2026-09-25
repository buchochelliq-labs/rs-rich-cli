//! `ExtensionRegistry`: rich-ext's plugin host, from Python, and what
//! `install(console)` leaves on a console.

use std::sync::{Arc, Mutex};

use pyo3::exceptions::{PyKeyError, PyTypeError};
use pyo3::prelude::*;
use pyo3::types::PyDict;

use rich::console::Console as CoreConsole;
use rich::protocol::{CodeHighlighting, ConsoleCodeHighlighting, Highlighter};
use rich_ext::{ExtensionRegistry as CoreRegistry, HighlighterChoiceError as CoreChoiceError};
use rich_plugin_api::{
    HighlighterFactory, Plugin as CorePlugin, PluginError as CorePluginError, PluginMetadata,
    PluginRegistrar as CoreRegistrar,
};

use super::adapters::{
    highlighter_factory_arg, transform_arg, FenceRenderer, SourceRenderer, TextPipeline,
    TextTransform,
};
use super::code::{code_highlighter_arg, CodeHighlighter};
use super::errors::{self, HighlighterChoiceError};
use super::plugin::plugin_arg;
use super::types::{Capability, RegisteredPlugin};
use crate::boxes::PyBox;
use crate::console::Console;
use crate::style::Style;
use crate::theme::Theme;

// ---------------------------------------------------------------------------
// Recording highlighter factories

/// Rust's registry keeps its highlighter factories to itself, and
/// `install` needs them for a Python console (which builds a fresh core
/// console for every print). So each plugin registers through [`Tee`],
/// which shares every factory with this side.
struct Recording<'a> {
    plugin: &'a dyn CorePlugin,
    factories: Mutex<Vec<Arc<HighlighterFactory>>>,
}

impl CorePlugin for Recording<'_> {
    fn metadata(&self) -> PluginMetadata {
        self.plugin.metadata()
    }

    fn register(&self, registrar: &mut dyn CoreRegistrar) -> Result<(), CorePluginError> {
        let mut tee = Tee {
            inner: registrar,
            factories: Vec::new(),
        };
        let result = self.plugin.register(&mut tee);
        *self
            .factories
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = tee.factories;
        result
    }
}

struct Tee<'a> {
    inner: &'a mut dyn CoreRegistrar,
    factories: Vec<Arc<HighlighterFactory>>,
}

impl CoreRegistrar for Tee<'_> {
    fn highlighter(&mut self, factory: HighlighterFactory) {
        let shared = Arc::new(factory);
        self.factories.push(shared.clone());
        self.inner.highlighter(Box::new(move || shared()));
    }
    fn code_highlighter(&mut self, name: &str, highlighter: Arc<dyn rich::CodeHighlighter>) {
        self.inner.code_highlighter(name, highlighter)
    }
    fn theme(&mut self, name: &str, theme: rich::Theme) {
        self.inner.theme(name, theme)
    }
    fn box_style(&mut self, name: &str, style: rich::r#box::Box) {
        self.inner.box_style(name, style)
    }
    fn renderer(&mut self, name: &str, renderer: Arc<dyn rich_plugin_api::SourceRenderer>) {
        self.inner.renderer(name, renderer)
    }
    fn fence_renderer(&mut self, language: &str, renderer: Arc<dyn rich::FenceRenderer>) {
        self.inner.fence_renderer(language, renderer)
    }
    fn transform(&mut self, name: &str, transform: Arc<dyn rich_plugin_api::TextTransform>) {
        self.inner.transform(name, transform)
    }
}

// ---------------------------------------------------------------------------
// What `install` leaves on a console

/// The extensions installed onto one Python console: highlighters (fresh
/// ones from the factories for every core console) and the default code
/// highlighter. What `rich_ext::ExtensionRegistry::install` does to a core
/// console, kept for the core consoles a Python `Console` builds.
#[derive(Clone, Default)]
pub(crate) struct Installed {
    factories: Vec<Arc<HighlighterFactory>>,
    code_highlighting: Option<CodeHighlighting>,
}

#[allow(dead_code)] // called by `console.rs` once it applies installed extensions
impl Installed {
    /// New highlighters, from the factories, in the order installed.
    pub(crate) fn highlighters(&self) -> Vec<Box<dyn Highlighter + Send>> {
        self.factories.iter().map(|factory| factory()).collect()
    }

    /// The default code highlighter, if one was installed.
    pub(crate) fn code_highlighting(&self) -> Option<&CodeHighlighting> {
        self.code_highlighting.as_ref()
    }

    /// Install onto a core console, as Rust's `ExtensionRegistry::install`.
    pub(crate) fn apply(&self, console: &mut CoreConsole) {
        for highlighter in self.highlighters() {
            console.add_highlighter(highlighter);
        }
        if let Some(highlighting) = &self.code_highlighting {
            console.set_code_highlighting(Some(highlighting.clone()));
        }
    }

    fn then(&self, later: &Installed) -> Installed {
        let mut merged = self.clone();
        merged.factories.extend(later.factories.iter().cloned());
        if later.code_highlighting.is_some() {
            merged.code_highlighting = later.code_highlighting.clone();
        }
        merged
    }
}

/// Every console something was installed onto. The strong reference keeps
/// a console's address from being reused while it is listed; entries only
/// this table still holds are dropped on the next access.
static INSTALLED: Mutex<Vec<(Py<Console>, Arc<Installed>)>> = Mutex::new(Vec::new());

fn installed_table() -> std::sync::MutexGuard<'static, Vec<(Py<Console>, Arc<Installed>)>> {
    INSTALLED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn refcount(_py: Python<'_>, object: &Py<Console>) -> isize {
    // SAFETY: the pointer is a live object (we hold a reference) and the
    // GIL is held.
    unsafe { pyo3::ffi::Py_REFCNT(object.as_ptr()) }
}

/// The extensions installed onto `console`, for the console to apply to
/// each core console it builds (see [`Installed::apply`]).
#[allow(dead_code)] // called by `console.rs` once it applies installed extensions
pub(crate) fn installed(py: Python<'_>, console: &Console) -> Option<Arc<Installed>> {
    let (found, _released) = {
        let mut table = installed_table();
        let released = prune(py, &mut table, None);
        let found = table
            .iter()
            .find(|(held, _)| std::ptr::eq(held.get(), console))
            .map(|(_, installed)| installed.clone());
        (found, released)
    };
    found
}

/// Take out the entries only the table still holds (except `keep`). The
/// caller drops them after releasing the table: dropping the last reference
/// to a console runs Python code.
fn prune(
    py: Python<'_>,
    table: &mut Vec<(Py<Console>, Arc<Installed>)>,
    keep: Option<&Bound<'_, Console>>,
) -> Vec<(Py<Console>, Arc<Installed>)> {
    let (kept, released) = std::mem::take(table).into_iter().partition(|(held, _)| {
        refcount(py, held) > 1 || keep.is_some_and(|console| held.is(console))
    });
    *table = kept;
    released
}

fn add_installed(py: Python<'_>, console: &Bound<'_, Console>, added: Installed) {
    let _released = {
        let mut table = installed_table();
        let released = prune(py, &mut table, Some(console));
        match table.iter_mut().find(|(held, _)| held.is(console)) {
            Some((_, installed)) => *installed = Arc::new(installed.then(&added)),
            None => table.push((console.clone().unbind(), Arc::new(added))),
        }
        released
    };
}

// ---------------------------------------------------------------------------
// The registry

/// A collection of extensions to install onto a `Console`: plugins add
/// highlighters, code highlighters, themes, box styles, renderers, fence
/// renderers and transforms. Adding a plugin is all-or-nothing.
#[pyclass(name = "ExtensionRegistry", module = "rs_rich.plugins", unsendable)]
pub(crate) struct ExtensionRegistry {
    inner: CoreRegistry,
    /// Every highlighter factory, in registration order.
    factories: Vec<Arc<HighlighterFactory>>,
}

impl ExtensionRegistry {
    fn add(&mut self, py: Python<'_>, plugin: &dyn CorePlugin) -> PyResult<()> {
        let recording = Recording {
            plugin,
            factories: Mutex::new(Vec::new()),
        };
        let result = errors::direct(|| self.inner.add_plugin(&recording));
        match result {
            Ok(()) => {
                let factories = recording
                    .factories
                    .into_inner()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                self.factories.extend(factories);
                Ok(())
            }
            Err(error) => Err(errors::plugin_error(py, &error)),
        }
    }
}

fn choice_error(py: Python<'_>, error: &CoreChoiceError) -> PyErr {
    let raised = HighlighterChoiceError::new_err(error.to_string());
    let value = raised.value(py);
    match error {
        CoreChoiceError::UnknownHighlighter { name, available } => {
            let _ = value.setattr("name", name.clone());
            let _ = value.setattr("available", available.clone());
            let _ = value.setattr("theme", py.None());
        }
        CoreChoiceError::UnknownTheme { highlighter, theme } => {
            let _ = value.setattr("name", highlighter.clone());
            let _ = value.setattr("available", py.None());
            let _ = value.setattr("theme", theme.clone());
        }
    }
    raised
}

#[pymethods]
impl ExtensionRegistry {
    /// An empty registry.
    #[new]
    fn new() -> Self {
        ExtensionRegistry {
            inner: CoreRegistry::new(),
            factories: Vec::new(),
        }
    }

    /// A registry with rich-ext's built-in plugin: the number highlighter
    /// and the `syntect` code highlighter.
    #[staticmethod]
    fn with_defaults(py: Python<'_>) -> PyResult<Self> {
        let mut registry = ExtensionRegistry::new();
        registry.add(py, &rich_ext::BuiltinPlugin)?;
        Ok(registry)
    }

    /// Add a plugin (a `Plugin` subclass, or a built-in plugin) and
    /// everything it registers. On failure `PluginError` is raised and the
    /// registry is unchanged.
    fn add_plugin(&mut self, py: Python<'_>, plugin: &Bound<'_, PyAny>) -> PyResult<()> {
        let rust = match plugin_arg(plugin) {
            Ok(rust) => rust,
            Err(error) if error.is_instance_of::<PyTypeError>(py) => return Err(error),
            // Reading the metadata raised: refused, as Rust's host refuses a
            // plugin whose `metadata` panics.
            Err(error) => {
                let message = errors::describe(py, &error);
                let raised = errors::plugin_error(
                    py,
                    &CorePluginError::Failed {
                        plugin: "(unknown)".to_string(),
                        message: format!("metadata failed: {message}"),
                    },
                );
                raised.set_cause(py, Some(error));
                return Err(raised);
            }
        };
        self.add(py, rust.as_ref())
    }

    /// Register a highlighter without a plugin (reported as `"(direct)"`).
    fn register_highlighter(&mut self, highlighter: &Bound<'_, PyAny>) -> PyResult<()> {
        let factory = Arc::new(highlighter_factory_arg(highlighter)?);
        self.factories.push(factory.clone());
        self.inner.register_highlighter(move || factory());
        Ok(())
    }

    /// Register a code highlighter without a plugin.
    fn register_code_highlighter(
        &mut self,
        py: Python<'_>,
        name: &str,
        highlighter: &Bound<'_, PyAny>,
    ) -> PyResult<()> {
        let highlighter = code_highlighter_arg(highlighter)?;
        self.inner
            .register_code_highlighter(name, highlighter)
            .map_err(|error| errors::plugin_error(py, &error))
    }

    /// Register a text transform without a plugin.
    fn register_transform(
        &mut self,
        py: Python<'_>,
        name: &str,
        transform: &Bound<'_, PyAny>,
    ) -> PyResult<()> {
        let transform = transform_arg(transform)?;
        self.inner
            .register_transform(name, transform)
            .map_err(|error| errors::plugin_error(py, &error))
    }

    /// The plugins added, in order.
    fn plugins(&self) -> Vec<RegisteredPlugin> {
        self.inner
            .plugins()
            .iter()
            .map(RegisteredPlugin::from_core)
            .collect()
    }

    /// The id of the plugin that provides `capability` (`"(direct)"` for one
    /// registered without a plugin), or `None`.
    fn provided_by(&self, capability: PyRef<'_, Capability>) -> Option<String> {
        self.inner
            .provided_by(&capability.inner)
            .map(str::to_string)
    }

    /// Every code highlighter name, sorted.
    fn code_highlighter_names(&self) -> Vec<String> {
        self.inner
            .code_highlighter_names()
            .into_iter()
            .map(str::to_string)
            .collect()
    }

    /// A code highlighter by name.
    fn code_highlighter(&self, name: &str) -> Option<CodeHighlighter> {
        self.inner.code_highlighter(name).map(CodeHighlighter::wrap)
    }

    /// Make the code highlighter `name` (and `theme`, one of its themes)
    /// the default for every console this registry is installed onto.
    #[pyo3(signature = (name, theme=None))]
    fn set_default_code_highlighter(
        &mut self,
        py: Python<'_>,
        name: &str,
        theme: Option<&str>,
    ) -> PyResult<()> {
        let result = errors::direct(|| self.inner.set_default_code_highlighter(name, theme));
        result.map_err(|error| choice_error(py, &error))
    }

    /// The default code highlighter's name, if one was chosen.
    fn default_code_highlighter(&self) -> Option<String> {
        self.inner.default_code_highlighter().map(str::to_string)
    }

    /// A theme by name.
    fn theme<'py>(&self, py: Python<'py>, name: &str) -> PyResult<Option<Bound<'py, PyAny>>> {
        let Some(theme) = self.inner.theme(name) else {
            return Ok(None);
        };
        // Through `Theme`'s constructor: the same styles, with nothing
        // inherited (the registered theme already holds any it inherited).
        let styles = PyDict::new(py);
        let mut names: Vec<&str> = theme.names().collect();
        names.sort_unstable();
        for name in names {
            if let Some(style) = theme.get(name) {
                styles.set_item(name, Style::from_core(style.clone()))?;
            }
        }
        let kwargs = PyDict::new(py);
        kwargs.set_item("inherit", false)?;
        py.get_type::<Theme>()
            .call((styles,), Some(&kwargs))
            .map(Some)
    }

    /// A box style by name (one of `rs_rich.box`'s constants).
    fn box_style(&self, py: Python<'_>, name: &str) -> PyResult<Option<Py<PyAny>>> {
        let Some(style) = self.inner.box_style(name) else {
            return Ok(None);
        };
        let module = py.import("rs_rich._native")?;
        for (_, value) in module.dict().iter() {
            if let Ok(candidate) = value.extract::<PyRef<'_, PyBox>>() {
                if candidate.inner == style {
                    return Ok(Some(value.clone().unbind()));
                }
            }
        }
        Err(pyo3::exceptions::PyNotImplementedError::new_err(
            "this box style is not one of rs_rich.box's, and rs_rich cannot build custom boxes",
        ))
    }

    /// A source renderer by name.
    fn renderer(&self, name: &str) -> Option<SourceRenderer> {
        self.inner
            .renderer(name)
            .map(|inner| SourceRenderer { inner: Some(inner) })
    }

    /// The fence renderer registered for `language`.
    fn fence_renderer(&self, language: &str) -> Option<FenceRenderer> {
        self.inner
            .fence_renderer(language)
            .map(|inner| FenceRenderer { inner: Some(inner) })
    }

    /// One fence renderer routing each fence to the one registered for its
    /// language (for Markdown), or `None` when there are none.
    fn fences(&self) -> Option<FenceRenderer> {
        self.inner
            .fences()
            .map(|inner| FenceRenderer { inner: Some(inner) })
    }

    /// A text transform by name.
    fn transform(&self, name: &str) -> Option<TextTransform> {
        self.inner
            .transform(name)
            .map(|inner| TextTransform { inner: Some(inner) })
    }

    /// Every text transform name, sorted.
    fn transform_names(&self) -> Vec<String> {
        self.inner
            .transform_names()
            .into_iter()
            .map(str::to_string)
            .collect()
    }

    /// The named transforms chained in the order given. `KeyError` names
    /// the first one no plugin registered.
    fn text_pipeline(&self, names: Vec<String>) -> PyResult<TextPipeline> {
        let pipeline = self
            .inner
            .text_pipeline(names.iter().map(String::as_str))
            .map_err(|name| PyKeyError::new_err(format!("no transform named {name:?}")))?;
        Ok(TextPipeline {
            inner: Arc::new(pipeline),
        })
    }

    /// Install every registered highlighter onto `console`, and the default
    /// code highlighter if one was chosen. What is installed stays with the
    /// console; later changes to the registry do not reach it.
    fn install(&self, py: Python<'_>, console: &Bound<'_, Console>) {
        add_installed(
            py,
            console,
            Installed {
                factories: self.factories.clone(),
                code_highlighting: self.inner.code_highlighting(),
            },
        );
    }

    fn __repr__(&self) -> String {
        let ids: Vec<&str> = self
            .inner
            .plugins()
            .iter()
            .map(|p| p.metadata.id.as_str())
            .collect();
        format!("<ExtensionRegistry plugins={ids:?}>")
    }
}

/// `install_defaults(console)`: install rich-ext's defaults onto `console`.
#[pyfunction]
fn install_defaults(py: Python<'_>, console: &Bound<'_, Console>) -> PyResult<()> {
    ExtensionRegistry::with_defaults(py)?.install(py, console);
    Ok(())
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<ExtensionRegistry>()?;
    m.add_function(pyo3::wrap_pyfunction!(install_defaults, m)?)?;
    Ok(())
}
