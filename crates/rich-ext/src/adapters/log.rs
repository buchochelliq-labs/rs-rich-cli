use super::EventSink;
use crate::event::{EventContext, Message, Severity, SourceLocation, StructuredEvent};
use std::sync::Arc;
/// Explicit log facade adapter. Construction never installs a logger.
pub struct LogAdapter {
    sink: Arc<dyn EventSink>,
    level: log::LevelFilter,
}
impl LogAdapter {
    pub fn new(sink: Arc<dyn EventSink>, level: log::LevelFilter) -> Self {
        Self { sink, level }
    }
}
impl log::Log for LogAdapter {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= self.level
    }
    fn log(&self, record: &log::Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let severity = match record.level() {
            log::Level::Error => Severity::Error,
            log::Level::Warn => Severity::Warn,
            log::Level::Info => Severity::Info,
            log::Level::Debug => Severity::Debug,
            log::Level::Trace => Severity::Trace,
        };
        let event = StructuredEvent::new(Message::Literal(record.args().to_string())).context(
            EventContext {
                severity: Some(severity),
                target: Some(record.target().into()),
                module: record.module_path().map(str::to_owned),
                source: record.file().map(|path| SourceLocation {
                    path: path.into(),
                    line: record.line().unwrap_or(0) as usize,
                    column: None,
                }),
                ..Default::default()
            },
        );
        let _ = self.sink.emit(event);
    }
    fn flush(&self) {}
}
