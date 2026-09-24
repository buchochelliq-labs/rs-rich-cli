use super::EventSink;
use crate::event::{
    EventContext, Message, Severity, SourceLocation, SpanContext, SpanEvent, StructuredEvent, Value,
};
use std::sync::Arc;
use std::time::Instant;
use tracing::{
    field::{Field, Visit},
    span::{Attributes, Id, Record},
    Event, Metadata, Subscriber,
};
use tracing_subscriber::{
    layer::Context,
    registry::{LookupSpan, SpanRef},
    Layer,
};

/// Composable layer. Install with a caller-owned subscriber or scoped dispatcher.
///
/// Every event carries the spans it happened in, outermost first, with their
/// fields as last recorded ([`StructuredEvent::span_context`]). With
/// [`span_open`](Self::span_open) and [`span_close`](Self::span_close) the
/// layer also emits an event when a span opens and when it closes (with how
/// long it was open), marked by [`StructuredEvent::span_marker`]. Spans are
/// tracked in the subscriber's registry, so it must implement `LookupSpan`,
/// as `tracing_subscriber::registry()` and `fmt()` do.
pub struct EventLayer {
    sink: Arc<dyn EventSink>,
    open: bool,
    close: bool,
}

impl EventLayer {
    pub fn new(sink: Arc<dyn EventSink>) -> Self {
        Self {
            sink,
            open: false,
            close: false,
        }
    }

    /// Emit an event when a span opens (default off).
    pub fn span_open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }

    /// Emit an event when a span closes, with how long it was open (default
    /// off).
    pub fn span_close(mut self, close: bool) -> Self {
        self.close = close;
        self
    }
}

/// What the layer keeps for each span, in the registry's extensions.
struct SpanData {
    fields: Vec<(String, Value)>,
    opened: Instant,
}

/// Collects fields; `message` is kept apart for events.
#[derive(Default)]
struct Fields {
    message: Option<String>,
    fields: Vec<(String, Value)>,
}

impl Fields {
    fn record(&mut self, field: &Field, value: Value) {
        if field.name() == "message" {
            if let Value::String(message) = value {
                self.message = Some(message);
                return;
            }
        }
        let name = field.name();
        // A span's `record` replaces the value it was created with.
        if let Some((_, old)) = self.fields.iter_mut().find(|(key, _)| key == name) {
            *old = value;
        } else {
            self.fields.push((name.into(), value));
        }
    }
}

impl Visit for Fields {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.record(field, Value::String(format!("{value:?}")));
    }
    fn record_str(&mut self, field: &Field, value: &str) {
        self.record(field, Value::String(value.into()));
    }
    fn record_i64(&mut self, field: &Field, value: i64) {
        self.record(field, Value::Integer(value));
    }
    fn record_u64(&mut self, field: &Field, value: u64) {
        self.record(field, Value::Unsigned(value));
    }
    fn record_i128(&mut self, field: &Field, value: i128) {
        self.record(field, Value::Integer128(value));
    }
    fn record_u128(&mut self, field: &Field, value: u128) {
        self.record(field, Value::Unsigned128(value));
    }
    fn record_f64(&mut self, field: &Field, value: f64) {
        self.record(field, Value::Float(value));
    }
    fn record_bool(&mut self, field: &Field, value: bool) {
        self.record(field, Value::Bool(value));
    }
}

fn severity(level: &tracing::Level) -> Severity {
    match *level {
        tracing::Level::ERROR => Severity::Error,
        tracing::Level::WARN => Severity::Warn,
        tracing::Level::INFO => Severity::Info,
        tracing::Level::DEBUG => Severity::Debug,
        tracing::Level::TRACE => Severity::Trace,
    }
}

fn context(meta: &Metadata<'_>) -> EventContext {
    EventContext {
        severity: Some(severity(meta.level())),
        target: Some(meta.target().into()),
        module: meta.module_path().map(str::to_owned),
        source: meta.file().map(|path| SourceLocation {
            path: path.into(),
            line: meta.line().unwrap_or(0) as usize,
            column: None,
        }),
        ..Default::default()
    }
}

/// A span as the events inside it report it.
fn span_context<S>(span: &SpanRef<'_, S>) -> SpanContext
where
    S: for<'a> LookupSpan<'a>,
{
    let fields = span
        .extensions()
        .get::<SpanData>()
        .map(|data| data.fields.clone())
        .unwrap_or_default();
    SpanContext {
        name: span.name().into(),
        fields,
    }
}

/// The parents of `span`, outermost first.
fn parents<S>(span: &SpanRef<'_, S>) -> Vec<SpanContext>
where
    S: for<'a> LookupSpan<'a>,
{
    let mut parents: Vec<SpanContext> = span.scope().skip(1).map(|s| span_context(&s)).collect();
    parents.reverse();
    parents
}

impl EventLayer {
    /// The event reporting `span` opening or closing.
    fn span_event<S>(&self, span: &SpanRef<'_, S>, marker: SpanEvent) -> StructuredEvent
    where
        S: for<'a> LookupSpan<'a>,
    {
        let own = span_context(span);
        let mut event = StructuredEvent::new(Message::Literal(own.name))
            .context(context(span.metadata()))
            .spans(parents(span))
            .span_event(marker);
        event.fields = own.fields;
        event
    }
}

impl<S> Layer<S> for EventLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(id) else { return };
        let mut fields = Fields::default();
        attrs.record(&mut fields);
        // A span's `message` field is an ordinary field.
        if let Some(message) = fields.message.take() {
            fields
                .fields
                .insert(0, ("message".into(), Value::String(message)));
        }
        span.extensions_mut().insert(SpanData {
            fields: fields.fields,
            opened: Instant::now(),
        });
        if self.open {
            let _ = self.sink.emit(self.span_event(&span, SpanEvent::Open));
        }
    }

    fn on_record(&self, id: &Id, values: &Record<'_>, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(id) else { return };
        let mut extensions = span.extensions_mut();
        let Some(data) = extensions.get_mut::<SpanData>() else {
            return;
        };
        let mut fields = Fields {
            message: None,
            fields: std::mem::take(&mut data.fields),
        };
        values.record(&mut fields);
        if let Some(message) = fields.message.take() {
            fields
                .fields
                .push(("message".into(), Value::String(message)));
        }
        data.fields = fields.fields;
    }

    fn on_close(&self, id: Id, ctx: Context<'_, S>) {
        if !self.close {
            return;
        }
        let Some(span) = ctx.span(&id) else { return };
        let elapsed = span
            .extensions()
            .get::<SpanData>()
            .map(|data| data.opened.elapsed())
            .unwrap_or_default();
        let _ = self
            .sink
            .emit(self.span_event(&span, SpanEvent::Close { elapsed }));
    }

    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        let mut fields = Fields::default();
        event.record(&mut fields);
        let spans = ctx
            .event_scope(event)
            .map(|scope| scope.from_root().map(|span| span_context(&span)).collect())
            .unwrap_or_default();
        let mut structured =
            StructuredEvent::new(Message::Literal(fields.message.unwrap_or_default()))
                .context(context(event.metadata()))
                .spans(spans);
        structured.fields = fields.fields;
        let _ = self.sink.emit(structured);
    }
}
