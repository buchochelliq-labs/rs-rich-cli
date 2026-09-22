//! Write a real diagnostic/layout SVG and HTML to the requested directory.
use rich::protocol::{Support, TargetCapabilities};
use rich::{ColorSystem, Text, Theme};
use rich_ext::{
    diagnostic::{Diagnostic, SourceSnippet},
    event::{EventContext, EventView, Message, Severity, StructuredEvent, Value},
    layout::{Axis, Constraint, LayoutNode},
    target::{RenderTarget, TargetKind},
};
fn main() -> std::io::Result<()> {
    let output = std::path::PathBuf::from(std::env::args_os().nth(1).expect("output directory"));
    std::fs::create_dir_all(&output)?;
    let event = StructuredEvent::new(Message::Literal("Configuration rejected".into()))
        .context(EventContext {
            severity: Some(Severity::Error),
            correlation_id: Some("build-42".into()),
            ..Default::default()
        })
        .field("service", Value::String("preview".into()))
        .field("attempt", Value::Integer(3))
        .diagnostic(
            Diagnostic::new("Invalid endpoint")
                .cause("port must be numeric")
                .snippet(
                    SourceSnippet::new("config.toml".into(), "port = invalid".into(), 7..14, 1)
                        .unwrap(),
                )
                .help("Use a port from 1 to 65535")
                .view(EventView::Expanded),
        )
        .view(EventView::Expanded);
    let layout = LayoutNode::split(
        Axis::Horizontal,
        vec![
            LayoutNode::leaf(Box::new(Text::styled(
                "Release\n\nTargets\nLayout\nEvents\nDiagnostics",
                "bold cyan",
            )))
            .width(Constraint::fixed(15)),
            LayoutNode::leaf(Box::new(event)),
        ],
    );
    let target = RenderTarget::new(
        TargetKind::Svg,
        TargetCapabilities {
            width: 80,
            height: 13,
            color_system: Some(ColorSystem::Truecolor),
            interactive: false,
            unicode: true,
            hyperlinks: false,
            sixel: Support::Unsupported,
        },
        Theme::default_theme(),
    );
    let segments = target.segments(&layout);
    std::fs::write(
        output.join("cli-v9-diagnostics.svg"),
        rich::svg::export_svg(
            &segments,
            &rich::terminal_theme::DEFAULT_TERMINAL_THEME,
            "Structured diagnostics and constrained layout",
            "expanded-release",
            80,
        ),
    )?;
    std::fs::write(
        output.join("cli-v9-diagnostics.html"),
        rich::export::export_html_inline(&segments, &rich::terminal_theme::DEFAULT_TERMINAL_THEME),
    )
}
