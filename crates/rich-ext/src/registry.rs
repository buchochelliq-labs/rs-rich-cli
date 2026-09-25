//! The extension registry: where plugins are hosted.
//!
//! A caller builds a registry, adds plugins, then installs it onto a
//! [`Console`]. Registration is **explicit**: no compile-time discovery and no
//! dynamic loading, for debuggability. Plugins implement the contract in
//! [`rich_plugin_api`], which depends only on core; this module is the host.
//! See docs/PLUGINS.md.

use std::collections::BTreeMap;
use std::sync::Arc;

use rich::r#box::Box as BoxStyle;
use rich::{CodeHighlighter, Console, Highlighter, SyntectHighlighter, Theme};
use rich_plugin_api::{
    is_valid_name, Capability, HighlighterFactory, Plugin, PluginError, PluginMetadata,
    PluginRegistrar, SourceRenderer, PLUGIN_API_VERSION,
};

/// A factory that produces a fresh highlighter each time it installs. The
/// highlighter is `Send` so the [`Console`] it installs onto stays `Send`.
type LocalHighlighterFactory = Box<dyn Fn() -> Box<dyn Highlighter + Send>>;

/// The id under which [`ExtensionRegistry::register_highlighter`] records
/// highlighters added without a plugin.
const DIRECT: &str = "(direct)";

/// A plugin a registry accepted, and what it registered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegisteredPlugin {
    pub metadata: PluginMetadata,
    pub capabilities: Vec<Capability>,
}

/// A collection of extensions to install onto a [`Console`].
#[derive(Default)]
pub struct ExtensionRegistry {
    highlighters: Vec<LocalHighlighterFactory>,
    code_highlighters: BTreeMap<String, (String, Arc<dyn CodeHighlighter>)>,
    themes: BTreeMap<String, (String, Theme)>,
    box_styles: BTreeMap<String, (String, BoxStyle)>,
    renderers: BTreeMap<String, (String, Arc<dyn SourceRenderer>)>,
    plugins: Vec<RegisteredPlugin>,
}

impl ExtensionRegistry {
    /// An empty registry.
    pub fn new() -> Self {
        ExtensionRegistry::default()
    }

    /// A registry pre-loaded with this crate's defaults: the [`BuiltinPlugin`].
    pub fn with_defaults() -> Self {
        let mut registry = ExtensionRegistry::new();
        registry
            .add_plugin(&BuiltinPlugin)
            .expect("the built-in plugin registers cleanly");
        registry
    }

    /// Register a highlighter factory directly, without a plugin. The factory
    /// is invoked at [`install`] time so a registry can be installed onto
    /// multiple consoles.
    ///
    /// [`install`]: ExtensionRegistry::install
    pub fn register_highlighter<F>(&mut self, factory: F) -> &mut Self
    where
        F: Fn() -> Box<dyn Highlighter + Send> + 'static,
    {
        self.highlighters.push(Box::new(factory));
        self
    }

    /// Add a plugin and everything it registers.
    ///
    /// Refused, with nothing kept, when the plugin was built for another
    /// [`PLUGIN_API_VERSION`], its id is invalid or already registered, a name
    /// it uses is invalid, it registers a capability another plugin already
    /// provides under the same name, or its own `register` fails.
    pub fn add_plugin(&mut self, plugin: &dyn Plugin) -> Result<(), PluginError> {
        let metadata = plugin.metadata();
        let id = metadata.id.clone();
        if !is_valid_name(&id) {
            return Err(PluginError::InvalidName {
                plugin: id.clone(),
                name: id,
            });
        }
        if metadata.api_version != PLUGIN_API_VERSION {
            return Err(PluginError::IncompatibleApi {
                plugin: id,
                built_for: metadata.api_version,
                host: PLUGIN_API_VERSION,
            });
        }
        if self.plugins.iter().any(|p| p.metadata.id == id) {
            return Err(PluginError::DuplicatePlugin { id });
        }

        let mut staged = Staged::default();
        plugin
            .register(&mut staged)
            .map_err(|error| PluginError::Failed {
                plugin: id.clone(),
                message: error.to_string(),
            })?;
        if let Some(name) = staged.invalid_names.into_iter().next() {
            return Err(PluginError::InvalidName { plugin: id, name });
        }
        // Conflicts with other plugins, then within this one.
        let mut seen = Vec::new();
        for capability in &staged.capabilities {
            if let Some(existing) = self.provider(capability) {
                return Err(PluginError::Conflict {
                    capability: capability.clone(),
                    existing: existing.to_string(),
                    plugin: id,
                });
            }
            if *capability != Capability::Highlighter && seen.contains(&capability) {
                return Err(PluginError::Conflict {
                    capability: capability.clone(),
                    existing: id.clone(),
                    plugin: id,
                });
            }
            seen.push(capability);
        }

        for factory in staged.highlighters {
            self.highlighters.push(Box::new(move || factory()));
        }
        for (name, value) in staged.code_highlighters {
            self.code_highlighters.insert(name, (id.clone(), value));
        }
        for (name, value) in staged.themes {
            self.themes.insert(name, (id.clone(), value));
        }
        for (name, value) in staged.box_styles {
            self.box_styles.insert(name, (id.clone(), value));
        }
        for (name, value) in staged.renderers {
            self.renderers.insert(name, (id.clone(), value));
        }
        self.plugins.push(RegisteredPlugin {
            metadata,
            capabilities: staged.capabilities,
        });
        Ok(())
    }

    /// The plugin that provides a named capability, if any.
    fn provider(&self, capability: &Capability) -> Option<&str> {
        match capability {
            Capability::CodeHighlighter(name) => self.code_highlighters.get(name).map(|e| &e.0),
            Capability::Theme(name) => self.themes.get(name).map(|e| &e.0),
            Capability::BoxStyle(name) => self.box_styles.get(name).map(|e| &e.0),
            Capability::Renderer(name) => self.renderers.get(name).map(|e| &e.0),
            _ => None,
        }
        .map(String::as_str)
    }

    /// The plugins added, in the order they were added.
    pub fn plugins(&self) -> &[RegisteredPlugin] {
        &self.plugins
    }

    /// A code highlighter by name.
    pub fn code_highlighter(&self, name: &str) -> Option<Arc<dyn CodeHighlighter>> {
        self.code_highlighters.get(name).map(|e| e.1.clone())
    }

    /// Every code highlighter name, sorted.
    pub fn code_highlighter_names(&self) -> Vec<&str> {
        self.code_highlighters.keys().map(String::as_str).collect()
    }

    /// A theme by name.
    pub fn theme(&self, name: &str) -> Option<&Theme> {
        self.themes.get(name).map(|e| &e.1)
    }

    /// A box style by name.
    pub fn box_style(&self, name: &str) -> Option<BoxStyle> {
        self.box_styles.get(name).map(|e| e.1)
    }

    /// A source renderer by name.
    pub fn renderer(&self, name: &str) -> Option<Arc<dyn SourceRenderer>> {
        self.renderers.get(name).map(|e| e.1.clone())
    }

    /// The plugin id that provided a capability, or `"(direct)"` for
    /// highlighters registered without a plugin.
    pub fn provided_by(&self, capability: &Capability) -> Option<&str> {
        match capability {
            Capability::Highlighter => Some(DIRECT),
            other => self.provider(other),
        }
    }

    /// Install every registered highlighter onto `console`.
    pub fn install(&self, console: &mut Console) {
        for factory in &self.highlighters {
            console.add_highlighter(factory());
        }
    }
}

/// Collects one plugin's registrations until they are checked.
#[derive(Default)]
struct Staged {
    capabilities: Vec<Capability>,
    invalid_names: Vec<String>,
    highlighters: Vec<HighlighterFactory>,
    code_highlighters: Vec<(String, Arc<dyn CodeHighlighter>)>,
    themes: Vec<(String, Theme)>,
    box_styles: Vec<(String, BoxStyle)>,
    renderers: Vec<(String, Arc<dyn SourceRenderer>)>,
}

impl Staged {
    fn check(&mut self, name: &str) {
        if !is_valid_name(name) {
            self.invalid_names.push(name.to_string());
        }
    }
}

impl PluginRegistrar for Staged {
    fn highlighter(&mut self, factory: HighlighterFactory) {
        self.capabilities.push(Capability::Highlighter);
        self.highlighters.push(factory);
    }

    fn code_highlighter(&mut self, name: &str, highlighter: Arc<dyn CodeHighlighter>) {
        self.check(name);
        self.capabilities
            .push(Capability::CodeHighlighter(name.to_string()));
        self.code_highlighters.push((name.to_string(), highlighter));
    }

    fn theme(&mut self, name: &str, theme: Theme) {
        self.check(name);
        self.capabilities.push(Capability::Theme(name.to_string()));
        self.themes.push((name.to_string(), theme));
    }

    fn box_style(&mut self, name: &str, style: BoxStyle) {
        self.check(name);
        self.capabilities
            .push(Capability::BoxStyle(name.to_string()));
        self.box_styles.push((name.to_string(), style));
    }

    fn renderer(&mut self, name: &str, renderer: Arc<dyn SourceRenderer>) {
        self.check(name);
        self.capabilities
            .push(Capability::Renderer(name.to_string()));
        self.renderers.push((name.to_string(), renderer));
    }
}

/// This crate's defaults as a plugin: the number highlighter, and the default
/// code highlighter registered as `"syntect"`.
pub struct BuiltinPlugin;

impl Plugin for BuiltinPlugin {
    fn metadata(&self) -> PluginMetadata {
        PluginMetadata::new("rich-ext", "rich-ext built-ins", env!("CARGO_PKG_VERSION"))
            .description("number highlighting and the syntect code highlighter")
    }

    fn register(&self, registrar: &mut dyn PluginRegistrar) -> Result<(), PluginError> {
        registrar.highlighter(Box::new(|| {
            Box::new(crate::highlighter::NumberHighlighter::new())
        }));
        registrar.code_highlighter("syntect", SyntectHighlighter::shared());
        Ok(())
    }
}

/// Convenience: install this crate's default extensions onto a console.
pub fn install_defaults(console: &mut Console) {
    ExtensionRegistry::with_defaults().install(console);
}

#[cfg(test)]
mod tests {
    use super::*;
    use rich::ColorSystem;

    #[test]
    fn number_highlighter_installs_and_styles_digits() {
        let mut console = Console::builder()
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .build();
        install_defaults(&mut console);
        // "42" should be wrapped in bold cyan (1;36); surrounding text plain.
        let out = console.render_str_to_string("n=42");
        assert!(out.contains("\x1b[1;36m42\x1b[0m"), "got: {out:?}");
    }

    #[test]
    fn defaults_come_from_the_builtin_plugin() {
        let registry = ExtensionRegistry::with_defaults();
        assert_eq!(registry.plugins().len(), 1);
        assert_eq!(registry.plugins()[0].metadata.id, "rich-ext");
        assert_eq!(registry.code_highlighter_names(), ["syntect"]);
        assert_eq!(
            registry.provided_by(&Capability::CodeHighlighter("syntect".into())),
            Some("rich-ext")
        );
    }

    /// A plugin that registers whatever it is given.
    struct Test {
        id: &'static str,
        api: u32,
        themes: Vec<&'static str>,
        fail: bool,
    }

    impl Plugin for Test {
        fn metadata(&self) -> PluginMetadata {
            let mut meta = PluginMetadata::new(self.id, self.id, "0.0.0");
            meta.api_version = self.api;
            meta
        }
        fn register(&self, registrar: &mut dyn PluginRegistrar) -> Result<(), PluginError> {
            for name in &self.themes {
                registrar.theme(name, Theme::new());
            }
            if self.fail {
                return Err(PluginError::Other("broken".into()));
            }
            Ok(())
        }
    }

    fn plugin(id: &'static str, themes: &[&'static str]) -> Test {
        Test {
            id,
            api: PLUGIN_API_VERSION,
            themes: themes.to_vec(),
            fail: false,
        }
    }

    #[test]
    fn plugins_register_and_are_listed_in_order() {
        let mut registry = ExtensionRegistry::new();
        registry.add_plugin(&plugin("a", &["dark"])).unwrap();
        registry.add_plugin(&plugin("b", &["light"])).unwrap();
        let ids: Vec<_> = registry
            .plugins()
            .iter()
            .map(|p| p.metadata.id.as_str())
            .collect();
        assert_eq!(ids, ["a", "b"]);
        assert!(registry.theme("dark").is_some() && registry.theme("light").is_some());
    }

    #[test]
    fn an_incompatible_api_version_is_refused_with_both_versions() {
        let mut registry = ExtensionRegistry::new();
        let mut old = plugin("old", &["x"]);
        old.api = PLUGIN_API_VERSION + 1;
        let error = registry.add_plugin(&old).unwrap_err();
        assert_eq!(
            error,
            PluginError::IncompatibleApi {
                plugin: "old".into(),
                built_for: PLUGIN_API_VERSION + 1,
                host: PLUGIN_API_VERSION,
            }
        );
        assert!(registry.plugins().is_empty() && registry.theme("x").is_none());
    }

    #[test]
    fn duplicate_ids_and_conflicting_names_are_refused_and_nothing_is_kept() {
        let mut registry = ExtensionRegistry::new();
        registry.add_plugin(&plugin("a", &["dark"])).unwrap();
        assert_eq!(
            registry.add_plugin(&plugin("a", &["other"])).unwrap_err(),
            PluginError::DuplicatePlugin { id: "a".into() }
        );
        let error = registry
            .add_plugin(&plugin("b", &["fresh", "dark"]))
            .unwrap_err();
        assert_eq!(
            error,
            PluginError::Conflict {
                capability: Capability::Theme("dark".into()),
                existing: "a".into(),
                plugin: "b".into(),
            }
        );
        // `fresh` was staged before the conflict and must not have been kept.
        assert!(registry.theme("fresh").is_none());
        assert_eq!(registry.plugins().len(), 1);
        // The same name twice inside one plugin is a conflict too.
        assert!(matches!(
            registry.add_plugin(&plugin("c", &["twice", "twice"])),
            Err(PluginError::Conflict { .. })
        ));
    }

    #[test]
    fn invalid_names_and_failed_registration_are_refused() {
        let mut registry = ExtensionRegistry::new();
        assert!(matches!(
            registry.add_plugin(&plugin("Bad Id", &[])),
            Err(PluginError::InvalidName { .. })
        ));
        assert!(matches!(
            registry.add_plugin(&plugin("ok", &["\u{1b}]0;x"])),
            Err(PluginError::InvalidName { .. })
        ));
        let mut broken = plugin("broken", &["kept-not"]);
        broken.fail = true;
        let error = registry.add_plugin(&broken).unwrap_err();
        assert_eq!(
            error,
            PluginError::Failed {
                plugin: "broken".into(),
                message: "broken".into(),
            }
        );
        assert!(registry.theme("kept-not").is_none() && registry.plugins().is_empty());
    }
}
