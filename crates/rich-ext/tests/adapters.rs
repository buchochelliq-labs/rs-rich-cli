#![cfg(all(feature = "log", feature = "tracing"))]
use rich_ext::{
    adapters::{EventLayer, EventSink, LogAdapter},
    event::{StructuredEvent, Value},
};
use std::sync::{Arc, Mutex};
#[derive(Default)]
struct Sink(Mutex<Vec<StructuredEvent>>);
impl EventSink for Sink {
    fn emit(&self, event: StructuredEvent) -> std::io::Result<()> {
        self.0.lock().unwrap().push(event);
        Err(std::io::Error::other("failed sink"))
    }
}
#[test]
fn log_filters_and_failed_delivery_does_not_recurse_or_install() {
    use log::Log;
    let sink = Arc::new(Sink::default());
    let before = log::max_level();
    let adapter = LogAdapter::new(sink.clone(), log::LevelFilter::Warn);
    adapter.log(
        &log::Record::builder()
            .args(format_args!("ignored"))
            .level(log::Level::Info)
            .build(),
    );
    adapter.log(
        &log::Record::builder()
            .args(format_args!("failed"))
            .level(log::Level::Error)
            .target("test")
            .build(),
    );
    assert_eq!(sink.0.lock().unwrap().len(), 1);
    assert_eq!(log::max_level(), before);
}
#[test]
fn scoped_tracing_preserves_primitive_fields_and_debug_strings() {
    use tracing_subscriber::prelude::*;
    let sink = Arc::new(Sink::default());
    let subscriber = tracing_subscriber::registry().with(EventLayer::new(sink.clone()));
    tracing::subscriber::with_default(
        subscriber,
        || tracing::info!(count=3i64,ready=true,debug=?vec![1,2],"hello"),
    );
    let events = sink.0.lock().unwrap();
    assert_eq!(events.len(), 1);
    assert!(events[0]
        .fields
        .contains(&("count".into(), Value::Integer(3))));
    assert!(events[0]
        .fields
        .contains(&("ready".into(), Value::Bool(true))));
    assert!(events[0]
        .fields
        .contains(&("debug".into(), Value::String("[1, 2]".into()))));
}
