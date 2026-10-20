#![cfg(feature = "tracing")]
//! `tracing` events through `EventLayer` and a `ChannelSink` arrive as log
//! records, and read back as the same events.
use std::sync::Arc;
use std::time::Duration;

use rich_ext::adapters::EventLayer;
use rich_ext::event::{Message, Severity, SpanEvent, StructuredEvent, Value};
use rich_ext::log_stream::{ChannelSink, Record};
use tracing_subscriber::prelude::*;

#[test]
fn tracing_events_arrive_as_records_in_order() {
    let (sink, records) = ChannelSink::<Record>::bounded(16);
    let layer = EventLayer::new(Arc::new(sink));
    let subscriber = tracing_subscriber::registry().with(layer);
    tracing::subscriber::with_default(subscriber, || {
        tracing::info!(target: "app::db", rows = 3u64, "loaded");
        let _span = tracing::info_span!("request", id = 7u64).entered();
        tracing::error!("failed");
    });
    let got: Vec<Record> = records.try_iter().collect();
    assert_eq!(got.len(), 2);
    let keys: Vec<&str> = got[0].fields.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(keys, ["level", "target", "message", "rows"]);
    assert_eq!(got[0].format, "event");
    assert_eq!(got[0].severity(), Some(Severity::Info));
    assert_eq!(got[0].get("target"), Some(&Value::String("app::db".into())));
    assert_eq!(got[0].get("rows"), Some(&Value::Unsigned(3)));
    assert_eq!(got[1].message().as_deref(), Some("failed"));
    assert_eq!(got[1].get("spans"), Some(&Value::String("request".into())));
}

#[test]
fn a_record_from_an_event_reads_back_as_that_event() {
    let mut event = StructuredEvent::new(Message::Literal("slow".into()))
        .field("latency_ms", Value::Integer(900))
        .span_event(SpanEvent::Close {
            elapsed: Duration::from_millis(3),
        });
    event.context.severity = Some(Severity::Warn);
    event.context.timestamp = Some("2026-10-20T10:00:00Z".into());
    event.context.correlation_id = Some("abc".into());
    let record = Record::from_event(&event);
    assert_eq!(record.time().as_deref(), Some("2026-10-20T10:00:00Z"));
    assert_eq!(
        record.get("correlation_id"),
        Some(&Value::String("abc".into()))
    );
    assert_eq!(record.get("span"), Some(&Value::String("close".into())));
    assert_eq!(record.get("elapsed_us"), Some(&Value::Unsigned(3000)));
    let back = record.to_event();
    assert_eq!(back.message, Message::Literal("slow".into()));
    assert_eq!(back.context.severity, Some(Severity::Warn));
    assert_eq!(
        back.context.timestamp.as_deref(),
        Some("2026-10-20T10:00:00Z")
    );
    assert_eq!(back.fields[0], ("latency_ms".into(), Value::Integer(900)));
}

#[test]
fn a_flood_never_blocks_the_logging_thread() {
    let (sink, records) = ChannelSink::<Record>::bounded(8);
    let counter = sink.clone();
    let subscriber = tracing_subscriber::registry().with(EventLayer::new(Arc::new(sink)));
    tracing::subscriber::with_default(subscriber, || {
        for i in 0..1000 {
            tracing::debug!(i, "tick");
        }
    });
    assert_eq!(records.try_iter().count(), 8);
    assert_eq!(counter.dropped(), 992);
}
