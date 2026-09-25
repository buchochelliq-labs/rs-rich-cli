//! A plugin written the way a third-party crate would write it: only public
//! items of `rich` and `rich_plugin_api` (integration tests cannot see anything
//! else), checked against a recording registrar.

use std::sync::Arc;

use rich::console::ConsoleOptions;
use rich::protocol::{HighlightError, HighlightedCode};
use rich::r#box::ROUNDED;
use rich::segment::Segment;
use rich::{CodeHighlighter, Console, FenceRenderer, Highlighter, Renderable, Text, Theme};
use rich_plugin_api::{
    Capability, Plugin, PluginError, PluginMetadata, PluginRegistrar, SourceRenderer,
    PLUGIN_API_VERSION,
};

struct Shout;
impl Highlighter for Shout {
    fn highlight(&self, text: &mut Text) {
        let len = text.plain().len();
        text.stylize(rich::Style::parse("bold").unwrap(), 0, len);
    }
}

struct Plain;
impl CodeHighlighter for Plain {
    fn highlight(
        &self,
        code: &str,
        _language: Option<&str>,
        _theme: &str,
    ) -> Result<HighlightedCode, HighlightError> {
        Ok(HighlightedCode {
            lines: code.split('\n').map(|_| Default::default()).collect(),
            ..Default::default()
        })
    }
    fn default_theme(&self) -> &str {
        "plain"
    }
    fn themes(&self) -> Vec<String> {
        vec!["plain".into()]
    }
    fn languages(&self) -> Vec<String> {
        Vec::new()
    }
}

struct Upper;
impl SourceRenderer for Upper {
    fn render(&self, source: &str) -> Result<Box<dyn Renderable + Send + Sync>, PluginError> {
        if source.is_empty() {
            return Err(PluginError::Other("nothing to render".into()));
        }
        Ok(Box::new(Text::new(source.to_uppercase())))
    }
}

struct Stars;
impl FenceRenderer for Stars {
    fn render_fence(
        &self,
        _language: &str,
        code: &str,
        _console: &Console,
        _options: &ConsoleOptions,
    ) -> Option<Vec<Segment>> {
        Some(vec![
            Segment::new("*".repeat(code.len()), None),
            Segment::line(),
        ])
    }
}

struct Everything;
impl Plugin for Everything {
    fn metadata(&self) -> PluginMetadata {
        PluginMetadata::new("everything", "Everything", env!("CARGO_PKG_VERSION"))
            .description("one of each capability")
    }
    fn register(&self, registrar: &mut dyn PluginRegistrar) -> Result<(), PluginError> {
        registrar.highlighter(Box::new(|| Box::new(Shout)));
        registrar.code_highlighter("plain", Arc::new(Plain));
        registrar.theme("calm", Theme::new());
        registrar.box_style("round", ROUNDED);
        registrar.renderer("upper", Arc::new(Upper));
        registrar.fence_renderer("stars", Arc::new(Stars));
        Ok(())
    }
}

/// Records what a plugin registers.
#[derive(Default)]
struct Recorder {
    capabilities: Vec<Capability>,
    renderers: Vec<Arc<dyn SourceRenderer>>,
}

impl PluginRegistrar for Recorder {
    fn highlighter(&mut self, factory: rich_plugin_api::HighlighterFactory) {
        let _ = factory();
        self.capabilities.push(Capability::Highlighter);
    }
    fn code_highlighter(&mut self, name: &str, _highlighter: Arc<dyn CodeHighlighter>) {
        self.capabilities
            .push(Capability::CodeHighlighter(name.into()));
    }
    fn theme(&mut self, name: &str, _theme: Theme) {
        self.capabilities.push(Capability::Theme(name.into()));
    }
    fn box_style(&mut self, name: &str, _style: rich::r#box::Box) {
        self.capabilities.push(Capability::BoxStyle(name.into()));
    }
    fn renderer(&mut self, name: &str, renderer: Arc<dyn SourceRenderer>) {
        self.capabilities.push(Capability::Renderer(name.into()));
        self.renderers.push(renderer);
    }
    fn fence_renderer(&mut self, language: &str, _renderer: Arc<dyn FenceRenderer>) {
        self.capabilities
            .push(Capability::FenceRenderer(language.into()));
    }
}

#[test]
fn a_plugin_registers_every_capability_through_public_items() {
    let plugin = Everything;
    let meta = plugin.metadata();
    assert_eq!(meta.id, "everything");
    assert_eq!(meta.api_version, PLUGIN_API_VERSION);
    let mut recorder = Recorder::default();
    plugin.register(&mut recorder).unwrap();
    assert_eq!(
        recorder.capabilities,
        [
            Capability::Highlighter,
            Capability::CodeHighlighter("plain".into()),
            Capability::Theme("calm".into()),
            Capability::BoxStyle("round".into()),
            Capability::Renderer("upper".into()),
            Capability::FenceRenderer("stars".into()),
        ]
    );
    let console = Console::builder().width(20).color_system(None).build();
    let rendered = recorder.renderers[0].render("hi").unwrap();
    assert_eq!(console.render_to_string(rendered.as_ref()).trim_end(), "HI");
    assert!(recorder.renderers[0].render("").is_err());
}
