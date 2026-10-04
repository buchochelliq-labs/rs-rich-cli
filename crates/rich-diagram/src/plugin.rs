//! [`DotPlugin`]: DOT through the plugin API, like rs-rich-mermaid's
//! `MermaidPlugin`. A fence renderer draws ```` ```dot ```` (and
//! ```` ```graphviz ````) blocks in Markdown, and a `dot` source renderer
//! draws a source. Uses only public items of `rich` and `rich_plugin_api`.

use std::sync::Arc;

use rich::console::{Console, ConsoleOptions};
use rich::protocol::{FenceRenderer, Renderable};
use rich::segment::Segment;
use rich_plugin_api::{Plugin, PluginError, PluginMetadata, PluginRegistrar, SourceRenderer};

use crate::dot::Dot;

/// The fence languages drawn as DOT.
pub const FENCE_LANGUAGES: [&str; 2] = ["dot", "graphviz"];

/// How DOT sources are drawn.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DotOptions {
    /// Draw with ASCII only. `None` follows [`Console::ascii_only`].
    pub ascii: Option<bool>,
}

/// Renders ```` ```dot ```` and ```` ```graphviz ```` fences in
/// [`rich::markdown::Markdown`], and DOT sources.
#[derive(Clone, Debug, Default)]
pub struct DotFences {
    pub options: DotOptions,
}

impl FenceRenderer for DotFences {
    fn render_fence(
        &self,
        language: &str,
        code: &str,
        console: &Console,
        options: &ConsoleOptions,
    ) -> Option<Vec<Segment>> {
        if !FENCE_LANGUAGES
            .iter()
            .any(|known| language.eq_ignore_ascii_case(known))
        {
            return None;
        }
        let diagram = Dot::new(code).ascii_option(self.options.ascii);
        Some(diagram.rich_render(console, options))
    }
}

impl SourceRenderer for DotFences {
    fn render(&self, source: &str) -> Result<Box<dyn Renderable + Send + Sync>, PluginError> {
        Ok(Box::new(Dot::new(source).ascii_option(self.options.ascii)))
    }
}

/// Registers the `dot` and `graphviz` fence renderers and the `dot` source
/// renderer.
///
/// ```
/// use rich_diagram::plugin::DotPlugin;
/// use rich_plugin_api::Plugin;
///
/// assert_eq!(DotPlugin::default().metadata().id, "dot");
/// ```
#[derive(Clone, Debug, Default)]
pub struct DotPlugin {
    pub options: DotOptions,
}

impl DotPlugin {
    pub fn new(options: DotOptions) -> Self {
        DotPlugin { options }
    }
}

impl Plugin for DotPlugin {
    fn metadata(&self) -> PluginMetadata {
        PluginMetadata::new("dot", "DOT diagrams", env!("CARGO_PKG_VERSION"))
            .description("Graphviz DOT graphs drawn as text through rs-rich-diagram's layout")
    }

    fn register(&self, registrar: &mut dyn PluginRegistrar) -> Result<(), PluginError> {
        let fences = Arc::new(DotFences {
            options: self.options.clone(),
        });
        for language in FENCE_LANGUAGES {
            registrar.fence_renderer(language, fences.clone());
        }
        registrar.renderer("dot", fences);
        Ok(())
    }
}
