use super::EventSink;
use crate::event::{EventContext, Message, Severity, SourceLocation, StructuredEvent, Value};
use std::sync::Arc;
use tracing::{
    field::{Field, Visit},
    Event, Subscriber,
};
use tracing_subscriber::{layer::Context, Layer};
/// Composable layer. Install with a caller-owned subscriber or scoped dispatcher.
pub struct EventLayer {
    sink: Arc<dyn EventSink>,
}
impl EventLayer {
    pub fn new(sink: Arc<dyn EventSink>) -> Self {
        Self { sink }
    }
}
struct Visitor {
    event: StructuredEvent,
}
impl Visitor {
    fn record(&mut self, field: &Field, value: Value) {
        if field.name() == "message" {
            if let Value::String(message) = value {
                self.event.message = Message::Literal(message);
                return;
            }
        }
        self.event.fields.push((field.name().into(), value));
    }
}
impl Visit for Visitor {
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
impl<S: Subscriber> Layer<S> for EventLayer {
    fn on_event(&self, event: &Event<'_>, _: Context<'_, S>) {
        let meta = event.metadata();
        let severity = match *meta.level() {
            tracing::Level::ERROR => Severity::Error,
            tracing::Level::WARN => Severity::Warn,
            tracing::Level::INFO => Severity::Info,
            tracing::Level::DEBUG => Severity::Debug,
            tracing::Level::TRACE => Severity::Trace,
        };
        let mut visitor = Visitor {
            event: StructuredEvent::new(Message::Literal(String::new())).context(EventContext {
                severity: Some(severity),
                target: Some(meta.target().into()),
                module: meta.module_path().map(str::to_owned),
                source: meta.file().map(|path| SourceLocation {
                    path: path.into(),
                    line: meta.line().unwrap_or(0) as usize,
                    column: None,
                }),
                ..Default::default()
            }),
        };
        event.record(&mut visitor);
        let _ = self.sink.emit(visitor.event);
    }
}
