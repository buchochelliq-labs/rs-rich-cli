//! The plugin works through public items alone: it registers with the ext
//! host like any third-party plugin, and its fence renderer draws Markdown
//! ```` ```mermaid ```` blocks.

use rich::markdown::Markdown;
use rich::Console;
use rich_ext::plugin::Capability;
use rich_ext::ExtensionRegistry;
use rich_mermaid::MermaidPlugin;

#[test]
fn registers_a_fence_renderer_and_a_source_renderer() {
    let mut registry = ExtensionRegistry::with_defaults();
    registry.add_plugin(&MermaidPlugin::default()).unwrap();
    let plugin = &registry.plugins()[1];
    assert_eq!(plugin.metadata.id, "mermaid");
    assert_eq!(
        plugin.capabilities,
        [
            Capability::FenceRenderer("mermaid".into()),
            Capability::Renderer("mermaid".into())
        ]
    );

    let console = Console::builder().width(60).color_system(None).build();
    let markdown = Markdown::new(
        "# Flow\n\n```mermaid\ngraph LR\n  A --> B\n```\n\n```rust\nfn main() {}\n```\n",
    )
    .fence_renderer(registry.fences().unwrap());
    let out = console.render_to_string(&markdown);
    assert!(out.contains("│ A ├─►│ B │"), "{out}");
    assert!(out.contains("fn main() {}"), "{out}");

    let rendered = registry
        .renderer("mermaid")
        .unwrap()
        .render("graph TD\n  X --> Y")
        .unwrap();
    let out = console.render_to_string(rendered.as_ref());
    assert!(out.contains("│ X │") && out.contains('▼'), "{out}");
}

#[test]
fn markdown_without_the_plugin_is_unchanged() {
    let console = Console::builder().width(60).color_system(None).build();
    let source = "```mermaid\ngraph LR\n  A --> B\n```";
    let plain = console.render_to_string(&Markdown::new(source));
    assert!(plain.contains("A --> B"), "{plain}");
    let registry = ExtensionRegistry::with_defaults();
    assert!(registry.fences().is_none());
}
