//! The extension registry: where plugins are hosted.
//!
//! A caller builds a registry, adds plugins, then installs it onto a
//! [`Console`]. Registration is **explicit**: no compile-time discovery and no
//! dynamic loading, for debuggability. Plugins implement the contract in
//! [`rich_plugin_api`], which depends only on core; this module is the host.
//! See docs/PLUGINS.md.

use std::collections::BTreeMap;
use std::sync::Arc;

use rich::console::ConsoleOptions;
use rich::r#box::Box as BoxStyle;
use rich::segment::Segment;
use rich::{
    CodeHighlighter, CodeHighlighting, Console, ConsoleCodeHighlighting, FenceRenderer,
    Highlighter, SyntectHighlighter, Theme,
};
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
    fence_renderers: BTreeMap<String, (String, Arc<dyn FenceRenderer>)>,
    plugins: Vec<RegisteredPlugin>,
    /// The code highlighter chosen as every console's default, and its theme.
    default_code_highlighter: Option<(String, Option<String>)>,
}

/// Why a code highlighter or theme could not be chosen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HighlighterChoiceError {
    /// No registered code highlighter has this name.
    UnknownHighlighter {
        name: String,
        available: Vec<String>,
    },
    /// The highlighter has no theme of this name.
    UnknownTheme { highlighter: String, theme: String },
}

impl std::fmt::Display for HighlighterChoiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HighlighterChoiceError::UnknownHighlighter { name, available } => write!(
                f,
                "unknown code highlighter {name:?}; available: {}",
                available.join(", ")
            ),
            HighlighterChoiceError::UnknownTheme { highlighter, theme } => write!(
                f,
                "the {highlighter} code highlighter has no theme {theme:?}"
            ),
        }
    }
}

impl std::error::Error for HighlighterChoiceError {}

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
        for (language, value) in staged.fence_renderers {
            self.fence_renderers.insert(language, (id.clone(), value));
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
            Capability::FenceRenderer(language) => self.fence_renderers.get(language).map(|e| &e.0),
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

    /// Make the code highlighter `name` (and `theme`, one of its themes; `None`
    /// is its default) the default for every console this registry is
    /// installed onto: `Syntax`, Markdown code, source views and diffs without
    /// a highlighter of their own use it.
    pub fn set_default_code_highlighter(
        &mut self,
        name: &str,
        theme: Option<&str>,
    ) -> Result<(), HighlighterChoiceError> {
        let highlighter = self.code_highlighter(name).ok_or_else(|| {
            HighlighterChoiceError::UnknownHighlighter {
                name: name.to_string(),
                available: self
                    .code_highlighter_names()
                    .into_iter()
                    .map(str::to_string)
                    .collect(),
            }
        })?;
        if let Some(theme) = theme {
            if !highlighter.themes().iter().any(|t| t == theme) {
                return Err(HighlighterChoiceError::UnknownTheme {
                    highlighter: name.to_string(),
                    theme: theme.to_string(),
                });
            }
        }
        self.default_code_highlighter = Some((name.to_string(), theme.map(str::to_string)));
        Ok(())
    }

    /// The default code highlighter's name, if one was chosen.
    pub fn default_code_highlighter(&self) -> Option<&str> {
        self.default_code_highlighter
            .as_ref()
            .map(|(name, _)| name.as_str())
    }

    /// The chosen default, ready for a console.
    pub fn code_highlighting(&self) -> Option<CodeHighlighting> {
        let (name, theme) = self.default_code_highlighter.as_ref()?;
        Some(CodeHighlighting {
            highlighter: self.code_highlighter(name)?,
            theme: theme.clone(),
        })
    }

    /// The fence renderer registered for `language`.
    pub fn fence_renderer(&self, language: &str) -> Option<Arc<dyn FenceRenderer>> {
        self.fence_renderers.get(language).map(|e| e.1.clone())
    }

    /// One [`FenceRenderer`] that routes each fence to the renderer registered
    /// for its language, for [`rich::markdown::Markdown::fence_renderer`]. `None` when no
    /// plugin registered one, so a caller can leave Markdown untouched.
    pub fn fences(&self) -> Option<Arc<dyn FenceRenderer>> {
        if self.fence_renderers.is_empty() {
            return None;
        }
        let routes = self
            .fence_renderers
            .iter()
            .map(|(language, (_, renderer))| (language.clone(), renderer.clone()))
            .collect();
        Some(Arc::new(FenceRoutes(routes)))
    }

    /// The plugin id that provided a capability, or `"(direct)"` for
    /// highlighters registered without a plugin.
    pub fn provided_by(&self, capability: &Capability) -> Option<&str> {
        match capability {
            Capability::Highlighter => Some(DIRECT),
            other => self.provider(other),
        }
    }

    /// Install every registered highlighter onto `console`, and the default
    /// code highlighter if one was chosen.
    pub fn install(&self, console: &mut Console) {
        for factory in &self.highlighters {
            console.add_highlighter(factory());
        }
        if let Some(highlighting) = self.code_highlighting() {
            console.set_code_highlighting(Some(highlighting));
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
    fence_renderers: Vec<(String, Arc<dyn FenceRenderer>)>,
}

/// Routes fences by language; see [`ExtensionRegistry::fences`].
struct FenceRoutes(BTreeMap<String, Arc<dyn FenceRenderer>>);

impl FenceRenderer for FenceRoutes {
    fn render_fence(
        &self,
        language: &str,
        code: &str,
        console: &Console,
        options: &ConsoleOptions,
    ) -> Option<Vec<Segment>> {
        self.0
            .get(language)?
            .render_fence(language, code, console, options)
    }
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

    fn fence_renderer(&mut self, language: &str, renderer: Arc<dyn FenceRenderer>) {
        self.check(language);
        self.capabilities
            .push(Capability::FenceRenderer(language.to_string()));
        self.fence_renderers.push((language.to_string(), renderer));
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

    /// Fences reach Markdown through `fences()`, routed by language, and two
    /// plugins cannot claim the same language.
    #[test]
    fn fence_renderers_route_by_language_and_conflict_by_language() {
        struct Tag(&'static str);
        impl FenceRenderer for Tag {
            fn render_fence(
                &self,
                _language: &str,
                _code: &str,
                _console: &Console,
                _options: &ConsoleOptions,
            ) -> Option<Vec<Segment>> {
                Some(vec![Segment::new(self.0, None), Segment::line()])
            }
        }
        struct Fences(&'static str, &'static str);
        impl Plugin for Fences {
            fn metadata(&self) -> PluginMetadata {
                PluginMetadata::new(self.0, self.0, "0.0.0")
            }
            fn register(&self, registrar: &mut dyn PluginRegistrar) -> Result<(), PluginError> {
                registrar.fence_renderer(self.1, Arc::new(Tag(self.1)));
                Ok(())
            }
        }

        let mut registry = ExtensionRegistry::with_defaults();
        assert!(registry.fences().is_none());
        registry.add_plugin(&Fences("diagrams", "mermaid")).unwrap();
        registry.add_plugin(&Fences("charts", "vega")).unwrap();
        let conflict = registry
            .add_plugin(&Fences("other", "mermaid"))
            .unwrap_err();
        assert!(
            matches!(conflict, PluginError::Conflict { .. }),
            "{conflict}"
        );
        assert_eq!(
            registry.provided_by(&Capability::FenceRenderer("mermaid".into())),
            Some("diagrams")
        );

        let md = rich::markdown::Markdown::new(
            "```mermaid\nx\n```\n\n```vega\ny\n```\n\n```rust\nz\n```",
        )
        .fence_renderer(registry.fences().unwrap());
        let console = Console::builder().width(30).color_system(None).build();
        let out = console.render_to_string(&md);
        assert!(out.contains("mermaid") && out.contains("vega"), "{out}");
        assert!(out.contains('z'), "the rust fence is still code: {out}");
    }

    /// A chosen default reaches consoles on install; unknown names and themes
    /// are refused with what is available.
    #[test]
    fn a_default_code_highlighter_is_chosen_by_name_and_installed() {
        let mut registry = ExtensionRegistry::with_defaults();
        assert!(registry.code_highlighting().is_none());
        let error = registry
            .set_default_code_highlighter("nope", None)
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            "unknown code highlighter \"nope\"; available: syntect"
        );
        assert!(matches!(
            registry.set_default_code_highlighter("syntect", Some("no-theme")),
            Err(HighlighterChoiceError::UnknownTheme { .. })
        ));
        registry
            .set_default_code_highlighter("syntect", Some("ansi_dark"))
            .unwrap();
        assert_eq!(registry.default_code_highlighter(), Some("syntect"));

        let mut console = Console::builder()
            .width(30)
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .build();
        registry.install(&mut console);
        assert_eq!(
            console.code_highlighting().and_then(|h| h.theme.as_deref()),
            Some("ansi_dark")
        );
        // Code now renders in upstream's ANSI colours, with no RGB.
        let out = console.render_to_string(&rich::Syntax::new("def f(): pass", "python"));
        assert!(out.contains("\x1b[94mdef"), "{out:?}");
        assert!(!out.contains("38;2;"), "{out:?}");
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
