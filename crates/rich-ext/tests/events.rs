use rich::{Console, Renderable, Segment};
use rich_ext::event::{EventContext, EventView, Message, Severity, StructuredEvent, Value};
fn text(event: &StructuredEvent, width: usize) -> String {
    let c = Console::builder()
        .width(width)
        .height(30)
        .no_color(true)
        .build();
    event
        .rich_render(&c, &c.options())
        .iter()
        .map(|s| s.text.as_str())
        .collect()
}
#[test]
fn fields_replace_without_reordering_and_literal_markup_stays_literal() {
    let e = StructuredEvent::new(Message::Literal("[red]hello[/red]".into()))
        .field("a", Value::Integer(1))
        .field("b", Value::Integer(2))
        .field("a", Value::Integer(3))
        .field_order(vec!["b".into()])
        .hide_fields(vec!["b".into()]);
    let out = text(&e, 80);
    assert!(out.contains("[red]hello[/red]"));
    assert_eq!(out.matches("a=3").count(), 1);
    assert!(!out.contains("a=1"));
    assert!(!out.contains("b=2"));
    let e = StructuredEvent::new(Message::Literal("message".into()))
        .field("a", Value::Bool(true))
        .field("b", Value::Null)
        .field_order(vec!["b".into(), "b".into()]);
    assert_eq!(text(&e, 80), "message b=null a=true");
}
#[test]
fn narrow_typed_events_preserve_values_without_hidden_clock() {
    let e = StructuredEvent::new(Message::Literal("event".into()))
        .field("n", Value::Float(f64::NAN))
        .field("inf", Value::Float(f64::INFINITY))
        .field(
            "map",
            Value::Map(vec![(
                "x".into(),
                Value::List(vec![Value::Integer(-2), Value::String("界\né".into())]),
            )]),
        )
        .view(EventView::Expanded)
        .context(EventContext {
            severity: Some(Severity::Warn),
            ..Default::default()
        });
    assert!(text(&e, 80).contains("NaN"));
    assert!(text(&e, 80).contains("inf"));
    for width in [1, 2, 20, 80] {
        let c = Console::builder().width(width).height(80).build();
        let lines = Segment::split_lines(&e.rich_render(&c, &c.options()));
        assert!(lines
            .iter()
            .all(|r| r.iter().map(Segment::cell_length).sum::<usize>() <= width));
    }
}

#[test]
fn destination_exports_keep_themed_severity_and_order() {
    use rich::protocol::{Support, TargetCapabilities};
    use rich_ext::target::{RenderTarget, TargetKind};
    let mut theme = rich::Theme::default_theme();
    theme.insert(
        "event.severity.error",
        rich::Style::parse("#123456").unwrap(),
    );
    let target = RenderTarget::new(
        TargetKind::Html,
        TargetCapabilities {
            width: 80,
            height: 20,
            color_system: Some(rich::ColorSystem::Truecolor),
            interactive: false,
            unicode: true,
            hyperlinks: false,
            sixel: Support::Unsupported,
        },
        theme,
    );
    let event = StructuredEvent::new(Message::Literal("failed".into()))
        .field("second", Value::Integer(2))
        .field("first", Value::Integer(1))
        .field_order(vec!["first".into()])
        .context(EventContext {
            severity: Some(Severity::Error),
            ..Default::default()
        });
    let segments = target.segments(&event);
    let html =
        rich::export::export_html_inline(&segments, &rich::terminal_theme::DEFAULT_TERMINAL_THEME);
    assert!(html.contains("#123456"));
    assert!(html.find("first=").unwrap() < html.find("second=").unwrap());
    let svg = rich::svg::export_svg(
        &segments,
        &rich::terminal_theme::DEFAULT_TERMINAL_THEME,
        "event",
        "fixed",
        80,
    );
    assert!(svg.contains("#123456"));
}
