//! `PluginMetadata`, `Capability` and `RegisteredPlugin`: the contract's
//! plain data.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

use rich_plugin_api::{
    Capability as CoreCapability, PluginMetadata as CoreMetadata, PLUGIN_API_VERSION,
};

/// Who a plugin is: `PluginMetadata(id, name, version, description=None)`.
/// `api_version` is the plugin API it was built against (the host refuses
/// any other).
#[pyclass(name = "PluginMetadata", module = "rs_rich.plugins", frozen, skip_from_py_object)]
#[derive(Clone)]
pub(crate) struct PluginMetadata {
    pub(crate) inner: CoreMetadata,
}

#[pymethods]
impl PluginMetadata {
    #[new]
    #[pyo3(signature = (id, name, version, description=None, *, api_version=PLUGIN_API_VERSION))]
    fn new(
        id: String,
        name: String,
        version: String,
        description: Option<String>,
        api_version: u32,
    ) -> Self {
        let mut inner = CoreMetadata::new(id, name, version);
        if let Some(description) = description {
            inner = inner.description(description);
        }
        inner.api_version = api_version;
        PluginMetadata { inner }
    }

    #[getter]
    fn id(&self) -> &str {
        &self.inner.id
    }

    #[getter]
    fn name(&self) -> &str {
        &self.inner.name
    }

    #[getter]
    fn version(&self) -> &str {
        &self.inner.version
    }

    #[getter]
    fn api_version(&self) -> u32 {
        self.inner.api_version
    }

    #[getter]
    fn description(&self) -> Option<&str> {
        self.inner.description.as_deref()
    }

    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        other
            .extract::<PyRef<'_, PluginMetadata>>()
            .is_ok_and(|other| other.inner == self.inner)
    }

    fn __hash__(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        self.inner.id.hash(&mut hasher);
        self.inner.version.hash(&mut hasher);
        hasher.finish()
    }

    fn __repr__(&self) -> String {
        let mut repr = format!(
            "PluginMetadata(id={:?}, name={:?}, version={:?}",
            self.inner.id, self.inner.name, self.inner.version
        );
        if let Some(description) = &self.inner.description {
            repr.push_str(&format!(", description={description:?}"));
        }
        if self.inner.api_version != PLUGIN_API_VERSION {
            repr.push_str(&format!(", api_version={}", self.inner.api_version));
        }
        repr.push(')');
        repr
    }
}

const KINDS: &[&str] = &[
    "highlighter",
    "code_highlighter",
    "theme",
    "box_style",
    "renderer",
    "fence_renderer",
    "transform",
];

/// One thing a plugin registered: `Capability(kind, name=None)`, where
/// `kind` is `"highlighter"` (unnamed), `"code_highlighter"`, `"theme"`,
/// `"box_style"`, `"renderer"`, `"fence_renderer"` or `"transform"`.
#[pyclass(name = "Capability", module = "rs_rich.plugins", frozen, skip_from_py_object)]
#[derive(Clone)]
pub(crate) struct Capability {
    pub(crate) inner: CoreCapability,
}

impl Capability {
    pub(crate) fn from_core(inner: &CoreCapability) -> Capability {
        Capability {
            inner: inner.clone(),
        }
    }
}

#[pymethods]
impl Capability {
    #[new]
    #[pyo3(signature = (kind, name=None))]
    fn new(kind: &str, name: Option<String>) -> PyResult<Self> {
        let named = |name: Option<String>| {
            name.ok_or_else(|| PyValueError::new_err(format!("a {kind} capability needs a name")))
        };
        let inner = match kind {
            "highlighter" => {
                if name.is_some() {
                    return Err(PyValueError::new_err("highlighters are not named"));
                }
                CoreCapability::Highlighter
            }
            "code_highlighter" => CoreCapability::CodeHighlighter(named(name)?),
            "theme" => CoreCapability::Theme(named(name)?),
            "box_style" => CoreCapability::BoxStyle(named(name)?),
            "renderer" => CoreCapability::Renderer(named(name)?),
            "fence_renderer" => CoreCapability::FenceRenderer(named(name)?),
            "transform" => CoreCapability::Transform(named(name)?),
            other => {
                return Err(PyValueError::new_err(format!(
                    "unknown capability kind {other:?}; expected one of {}",
                    KINDS.join(", ")
                )))
            }
        };
        Ok(Capability { inner })
    }

    /// The kinds a capability can have.
    #[classattr]
    #[pyo3(name = "KINDS")]
    fn kinds() -> Vec<&'static str> {
        KINDS.to_vec()
    }

    #[getter]
    fn kind(&self) -> &'static str {
        super::errors::capability_kind(&self.inner)
    }

    #[getter]
    fn name(&self) -> Option<&str> {
        match &self.inner {
            CoreCapability::CodeHighlighter(name)
            | CoreCapability::Theme(name)
            | CoreCapability::BoxStyle(name)
            | CoreCapability::Renderer(name)
            | CoreCapability::FenceRenderer(name)
            | CoreCapability::Transform(name) => Some(name),
            _ => None,
        }
    }

    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        other
            .extract::<PyRef<'_, Capability>>()
            .is_ok_and(|other| other.inner == self.inner)
    }

    fn __lt__(&self, other: PyRef<'_, Capability>) -> bool {
        self.inner < other.inner
    }

    fn __hash__(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        self.inner.hash(&mut hasher);
        hasher.finish()
    }

    /// As Rust prints it: `theme "dark"`, `highlighter`.
    fn __str__(&self) -> String {
        self.inner.to_string()
    }

    fn __repr__(&self) -> String {
        match self.name() {
            Some(name) => format!("Capability({:?}, {:?})", self.kind(), name),
            None => format!("Capability({:?})", self.kind()),
        }
    }
}

/// A plugin a registry accepted, and what it registered
/// (`ExtensionRegistry.plugins()`).
#[pyclass(name = "RegisteredPlugin", module = "rs_rich.plugins", frozen)]
pub(crate) struct RegisteredPlugin {
    metadata: CoreMetadata,
    capabilities: Vec<CoreCapability>,
}

impl RegisteredPlugin {
    pub(crate) fn from_core(plugin: &rich_ext::RegisteredPlugin) -> RegisteredPlugin {
        RegisteredPlugin {
            metadata: plugin.metadata.clone(),
            capabilities: plugin.capabilities.clone(),
        }
    }
}

#[pymethods]
impl RegisteredPlugin {
    #[getter]
    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            inner: self.metadata.clone(),
        }
    }

    #[getter]
    fn capabilities(&self) -> Vec<Capability> {
        self.capabilities.iter().map(Capability::from_core).collect()
    }

    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        other
            .extract::<PyRef<'_, RegisteredPlugin>>()
            .is_ok_and(|other| {
                other.metadata == self.metadata && other.capabilities == self.capabilities
            })
    }

    fn __repr__(&self) -> String {
        let capabilities: Vec<String> = self.capabilities.iter().map(|c| c.to_string()).collect();
        format!(
            "<RegisteredPlugin {:?} [{}]>",
            self.metadata.id,
            capabilities.join(", ")
        )
    }
}

/// `is_valid_name(name)`: whether `name` is a valid plugin id or capability
/// name (lowercase letters, digits, `-`, `_` and `.`, starting with a letter
/// or digit, at most 64 bytes).
#[pyfunction]
pub(crate) fn is_valid_name(name: &str) -> bool {
    rich_plugin_api::is_valid_name(name)
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("PLUGIN_API_VERSION", PLUGIN_API_VERSION)?;
    m.add_class::<PluginMetadata>()?;
    m.add_class::<Capability>()?;
    m.add_class::<RegisteredPlugin>()?;
    m.add_function(pyo3::wrap_pyfunction!(is_valid_name, m)?)?;
    Ok(())
}
