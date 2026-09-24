#![cfg(feature = "tracing")]
//! `EventLayer` span tracking and `RichHandler`'s span views, links and Live
//! output.
use rich::protocol::{Support, TargetCapabilities};
use rich::{Console, Theme};
use rich_ext::{
    adapters::{EventLayer, EventSink},
    event::{
        EventContext, Message, Severity, SourceLocation, SpanContext, SpanEvent, StructuredEvent,
        Value,
    },
    hyperlink::Hyperlinker,
    live::LiveCoordinator,
    target::{RenderTarget, TargetKind},
    RichHandler, SpanView,
};
use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tracing_subscriber::prelude::*;

#[derive(Default)]
struct Sink(Mutex<Vec<StructuredEvent>>);
impl EventSink for Sink {
    fn emit(&self, event: StructuredEvent) -> std::io::Result<()> {
        self.0.lock().unwrap().push(event);
        Ok(())
    }
}

fn names(spans: &[SpanContext]) -> Vec<&str> {
    spans.iter().map(|span| span.name.as_str()).collect()
}

#[test]
fn events_carry_their_spans_outermost_first_with_recorded_fields() {
    let sink = Arc::new(Sink::default());
    let subscriber = tracing_subscriber::registry().with(EventLayer::new(sink.clone()));
    tracing::subscriber::with_default(subscriber, || {
        tracing::info!("outside");
        let request = tracing::info_span!("request", id = 7u64, user = tracing::field::Empty);
        let _request = request.enter();
        request.record("user", "ada");
        let _query = tracing::debug_span!("query", table = "users").entered();
        tracing::warn!(rows = 3i64, "slow");
    });
    let events = sink.0.lock().unwrap();
    assert_eq!(events.len(), 2);
    assert!(events[0].span_context().is_empty());
    let spans = events[1].span_context();
    assert_eq!(names(spans), ["request", "query"]);
    assert_eq!(
        spans[0].fields,
        [
            ("id".to_string(), Value::Unsigned(7)),
            ("user".to_string(), Value::String("ada".into())),
        ]
    );
    assert_eq!(
        spans[1].fields,
        [("table".to_string(), Value::String("users".into()))]
    );
    assert!(matches!(&events[1].message, Message::Literal(m) if m == "slow"));
    assert_eq!(events[1].fields, [("rows".to_string(), Value::Integer(3))]);
    assert_eq!(events[1].span_marker(), None);
}

#[test]
fn span_open_and_close_are_reported_with_parents_and_elapsed_time() {
    let sink = Arc::new(Sink::default());
    let layer = EventLayer::new(sink.clone())
        .span_open(true)
        .span_close(true);
    let subscriber = tracing_subscriber::registry().with(layer);
    tracing::subscriber::with_default(subscriber, || {
        let outer = tracing::info_span!("outer", n = 1i64).entered();
        {
            let _inner = tracing::warn_span!("inner").entered();
            std::thread::sleep(Duration::from_millis(2));
        }
        drop(outer);
    });
    let events = sink.0.lock().unwrap();
    let summary: Vec<(String, Vec<&str>, bool)> = events
        .iter()
        .map(|event| {
            let Message::Literal(name) = &event.message else {
                panic!("literal")
            };
            (
                name.clone(),
                names(event.span_context()),
                matches!(event.span_marker(), Some(SpanEvent::Open)),
            )
        })
        .collect();
    assert_eq!(
        summary,
        [
            ("outer".into(), vec![], true),
            ("inner".into(), vec!["outer"], true),
            ("inner".into(), vec!["outer"], false),
            ("outer".into(), vec![], false),
        ]
    );
    // The span's own fields and level travel with its open and close events.
    assert_eq!(events[0].fields, [("n".to_string(), Value::Integer(1))]);
    assert_eq!(events[1].context.severity, Some(Severity::Warn));
    let Some(SpanEvent::Close { elapsed }) = events[2].span_marker() else {
        panic!("close")
    };
    assert!(elapsed >= Duration::from_millis(2), "{elapsed:?}");
}

fn console(ascii: bool) -> Console {
    Console::builder()
        .width(80)
        .color_system(None)
        .ascii_only(ascii)
        .build()
}

fn handler(view: SpanView, ascii: bool) -> RichHandler {
    RichHandler::new(console(ascii))
        .span_view(view)
        .highlighter(None)
        .time_format(|| "[12:00:00]".into())
}

fn spans() -> Vec<SpanContext> {
    vec![
        SpanContext::new("request").field("id", Value::Unsigned(7)),
        SpanContext::new("query"),
    ]
}

fn event(message: &str) -> StructuredEvent {
    StructuredEvent::new(Message::Literal(message.into()))
        .spans(spans())
        .field("rows", Value::Integer(3))
}

fn open(name: &str, parents: Vec<SpanContext>) -> StructuredEvent {
    StructuredEvent::new(Message::Literal(name.into()))
        .spans(parents)
        .span_event(SpanEvent::Open)
}

fn close(name: &str, parents: Vec<SpanContext>, micros: u64) -> StructuredEvent {
    StructuredEvent::new(Message::Literal(name.into()))
        .spans(parents)
        .span_event(SpanEvent::Close {
            elapsed: Duration::from_micros(micros),
        })
}

#[test]
fn inline_view_puts_the_span_chain_before_the_message() {
    let handler = handler(SpanView::Inline, false);
    assert_eq!(
        handler.render_message(&event("slow")).plain(),
        "request{id=7}:query: slow rows=3"
    );
    // No spans, no prefix.
    let plain = StructuredEvent::new(Message::Literal("ready".into()));
    assert_eq!(handler.render_message(&plain).plain(), "ready");
    let request = vec![SpanContext::new("request").field("id", Value::Unsigned(7))];
    assert_eq!(
        handler
            .render_message(&open("query", request.clone()))
            .plain(),
        "request{id=7}:query opened"
    );
    assert_eq!(
        handler
            .render_message(&close("query", request, 1_250))
            .plain(),
        "request{id=7}:query closed 1.25ms"
    );
    // Span names are bold.
    let styled = Console::builder()
        .width(80)
        .force_terminal(true)
        .color_system(Some(rich::ColorSystem::Truecolor))
        .build();
    let out = styled.render_to_string(&handler.render_message(&event("slow")));
    assert!(out.contains("\x1b[1mrequest\x1b[0m"), "{out:?}");
    assert!(out.contains("\x1b[1mquery\x1b[0m"), "{out:?}");
}

#[test]
fn tree_view_draws_guides_and_branches() {
    let handler = handler(SpanView::Tree, false);
    let request = vec![SpanContext::new("request").field("id", Value::Unsigned(7))];
    let lines: Vec<String> = [
        open("request", vec![]).field("id", Value::Unsigned(7)),
        open("query", request.clone()),
        event("slow"),
        close("query", request, 850),
        close("request", vec![], 2_500_000).field("id", Value::Unsigned(7)),
    ]
    .iter()
    .map(|event| handler.render_message(event).plain().to_string())
    .collect();
    assert_eq!(
        lines,
        [
            "┌ request id=7",
            "│ ┌ query",
            "│ │ slow rows=3",
            "│ └ query 850µs",
            "└ request 2.50s",
        ]
    );
    let ascii = self::handler(SpanView::Tree, true);
    assert_eq!(
        ascii.render_message(&event("slow")).plain(),
        "| | slow rows=3"
    );
    assert_eq!(
        ascii.render_message(&close("query", vec![], 5)).plain(),
        "` query 5µs"
    );
}

#[test]
fn hidden_view_shows_only_the_message() {
    let handler = handler(SpanView::Hidden, false);
    assert_eq!(
        handler.render_message(&event("slow")).plain(),
        "slow rows=3"
    );
    assert_eq!(
        handler.render_message(&open("query", spans())).plain(),
        "query opened"
    );
}

fn located(path: &str, line: usize) -> StructuredEvent {
    StructuredEvent::new(Message::Literal("ready".into())).context(EventContext {
        severity: Some(Severity::Info),
        source: Some(SourceLocation {
            path: path.into(),
            line,
            column: None,
        }),
        ..Default::default()
    })
}

/// `text` without its CSI and OSC escape sequences.
fn visible(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\x1b' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('[') => {
                for c in chars.by_ref() {
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            Some(']') => {
                while let Some(c) = chars.next() {
                    if c == '\x1b' && chars.peek() == Some(&'\\') {
                        chars.next();
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// A writer tests can read back.
#[derive(Clone, Default)]
struct Shared(Arc<Mutex<Vec<u8>>>);
impl Write for Shared {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
impl Shared {
    fn text(&self) -> String {
        String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
    }
}

fn target(width: usize, interactive: bool, hyperlinks: bool) -> RenderTarget {
    RenderTarget::new(
        TargetKind::Custom,
        TargetCapabilities {
            width,
            height: 24,
            // Links are written only with a colour system.
            color_system: hyperlinks.then_some(rich::ColorSystem::Truecolor),
            interactive,
            unicode: true,
            hyperlinks,
            sixel: Support::Unsupported,
        },
        Theme::default_theme(),
    )
}

#[test]
fn the_hyperlinker_decides_where_the_path_column_links() {
    // Rendered into a Live coordinator whose target shows hyperlinks, so the
    // OSC 8 link is visible in the bytes.
    let render = |handler: RichHandler| {
        let out = Shared::default();
        let live = Arc::new(Mutex::new(LiveCoordinator::new(
            out.clone(),
            target(80, false, true),
        )));
        let handler = handler.live(live.clone());
        handler.emit_event(&located("src/server/main.rs", 42));
        live.lock().unwrap().finish().unwrap();
        out.text()
    };
    let base =
        || RichHandler::new(target(80, false, true).console()).time_format(|| "[12:00:00]".into());
    let bare = render(base());
    assert!(bare.contains("file://src/server/main.rs#42"), "{bare:?}");
    let editor = render(
        base().hyperlinker(
            Hyperlinker::new()
                .base_dir("/work")
                .editor("vscode://file/{path}:{line}"),
        ),
    );
    assert!(
        editor.contains("vscode://file//work/src/server/main.rs:42"),
        "{editor:?}"
    );
    assert!(!editor.contains("file://src"), "{editor:?}");
    assert!(visible(&editor).contains("main.rs:42"), "{editor:?}");
    let off = render(base().hyperlinker(Hyperlinker::disabled()));
    assert!(
        !off.contains("file://") && !off.contains("\x1b]8;"),
        "{off:?}"
    );
    assert!(visible(&off).contains("main.rs:42"), "{off:?}");
}

#[test]
fn live_output_prints_above_the_regions() {
    let out = Shared::default();
    let target = target(60, true, false);
    let live = Arc::new(Mutex::new(LiveCoordinator::new(
        out.clone(),
        target.clone(),
    )));
    live.lock()
        .unwrap()
        .add(vec![rich::Segment::new("progress 50%", None)])
        .unwrap();
    live.lock().unwrap().refresh().unwrap();
    let handler = RichHandler::new(target.console())
        .time_format(|| "[12:00:00]".into())
        .show_path(false)
        .live(live.clone());
    let sink: Arc<dyn EventSink> = Arc::new(handler);
    let subscriber = tracing_subscriber::registry().with(EventLayer::new(sink));
    tracing::subscriber::with_default(subscriber, || {
        let _span = tracing::info_span!("job", n = 1i64).entered();
        tracing::info!("step done");
    });
    let text = out.text();
    let log = text.find("job{n=1}: step done").expect(&text);
    // The region was painted, cleared for the log line, then painted again.
    let before = text[..log].matches("progress 50%").count();
    let after = text[log..].matches("progress 50%").count();
    assert_eq!((before, after), (1, 1), "{text:?}");
}

/// `span.record` replaces a field in place, `message` included, as tracing's
/// record semantics say; it used to append a second `message`.
#[test]
fn recording_a_span_field_replaces_it_in_place() {
    let sink = Arc::new(Sink::default());
    let subscriber = tracing_subscriber::registry().with(EventLayer::new(sink.clone()));
    tracing::subscriber::with_default(subscriber, || {
        let span = tracing::info_span!("s", message = "first", k = 2i64);
        let _span = span.enter();
        span.record("message", "second");
        span.record("k", 3i64);
        span.record("message", "third");
        tracing::info!("inside");
    });
    let events = sink.0.lock().unwrap();
    assert_eq!(
        events[0].span_context()[0].fields,
        [
            ("message".to_string(), Value::String("third".into())),
            ("k".to_string(), Value::Integer(3)),
        ]
    );
}
