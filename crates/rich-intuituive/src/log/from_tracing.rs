//! The `tracing` plugin (the off-by-default `tracing` feature): a
//! `tracing_subscriber` layer that feeds a [`Log`], so an app shows its
//! own `tracing::info!` lines as they happen, from any thread.
//!
//! Events go through rs-rich-ext's `EventLayer` into a bounded
//! [`ChannelSink`]: the thread that logs never waits for the app, and what
//! a full channel cannot take is counted and shown as one `… N dropped`
//! line. The app's thread takes what waits, a batch at a time, through a
//! [`Proxy`].

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};

use rich_ext::adapters::EventLayer;
use rich_ext::event::Value;
use rich_ext::log_stream::{ChannelSink, Record};
use tracing_subscriber::layer::SubscriberExt;

use super::{Log, LogLine};
use crate::reactive::Proxy;

/// The fields a line shows apart, or not at all.
const OWN_KEYS: &[&str] = &["timestamp", "level", "target", "message", "spans"];

/// What feeds a [`Log`] from `tracing`: how many events may wait for the
/// app, and what of each shows. Make its layer with
/// [`layer`](Self::layer), or install it with [`install`](Self::install).
///
/// ```
/// use intuituive::log::TracingFeed;
/// use intuituive::prelude::*;
/// use tracing_subscriber::prelude::*;
///
/// let mut feed = None;
/// let app = App::new(|| {
///     let log = Log::new(1_000);
///     feed = Some(log);
///     log.view().on_key("q", |cx| cx.quit())
/// });
/// let layer = TracingFeed::new(feed.unwrap(), app.proxy()).capacity(256).layer();
/// let subscriber = tracing_subscriber::registry().with(layer);
/// tracing::subscriber::with_default(subscriber, || tracing::info!(port = 8080, "listening"));
/// let mut driver = app.driver(40, 2);
/// driver.settle();
/// assert!(driver.screen().plain()[0].starts_with("INFO "), "{:?}", driver.screen().plain());
/// ```
pub struct TracingFeed {
    log: Log,
    proxy: Proxy,
    capacity: usize,
    fields: bool,
    spans: bool,
}

impl TracingFeed {
    /// The events that may wait for the app by default.
    pub const CAPACITY: usize = 1_024;

    /// A feed into `log`, through `proxy` (from
    /// [`App::proxy`](crate::App::proxy)).
    pub fn new(log: Log, proxy: Proxy) -> TracingFeed {
        TracingFeed {
            log,
            proxy,
            capacity: TracingFeed::CAPACITY,
            fields: true,
            spans: true,
        }
    }

    /// How many events may wait for the app before more are dropped
    /// (default [`CAPACITY`](Self::CAPACITY)).
    #[must_use]
    pub fn capacity(mut self, events: usize) -> TracingFeed {
        self.capacity = events.max(1);
        self
    }

    /// Show an event's fields after its message, `key=value` (default on).
    #[must_use]
    pub fn fields(mut self, on: bool) -> TracingFeed {
        self.fields = on;
        self
    }

    /// Show the spans an event happened in before its message,
    /// `outer:inner:` (default on).
    #[must_use]
    pub fn spans(mut self, on: bool) -> TracingFeed {
        self.spans = on;
        self
    }

    /// The layer: add it to your subscriber
    /// (`tracing_subscriber::registry().with(layer)`), with any filter.
    pub fn layer(self) -> EventLayer {
        let (sink, records) = ChannelSink::<Record>::bounded(self.capacity);
        let records = Arc::new(Mutex::new(records));
        let pending = Arc::new(AtomicBool::new(false));
        let counter = sink.clone();
        let TracingFeed {
            log,
            proxy,
            fields,
            spans,
            ..
        } = self;
        let sink = sink.notify(move || {
            // One job at a time: it takes everything waiting.
            if pending.swap(true, Ordering::AcqRel) {
                return;
            }
            let (records, pending, counter) = (records.clone(), pending.clone(), counter.clone());
            proxy.run(move || {
                pending.store(false, Ordering::Release);
                drain(&records, &counter, log, fields, spans);
            });
        });
        EventLayer::new(Arc::new(sink))
    }

    /// Make a subscriber of the [`layer`](Self::layer) alone the global
    /// default. An error if one is set already: then add the layer to
    /// yours.
    pub fn install(self) -> Result<(), tracing::subscriber::SetGlobalDefaultError> {
        let subscriber = tracing_subscriber::registry().with(self.layer());
        tracing::subscriber::set_global_default(subscriber)
    }
}

/// A layer feeding `log` (see [`TracingFeed`]), for your subscriber:
///
/// ```no_run
/// use intuituive::log::tracing_layer;
/// use intuituive::prelude::*;
/// use tracing_subscriber::prelude::*;
///
/// let mut made = None;
/// let app = App::new(|| {
///     let log = Log::new(5_000);
///     made = Some(log);
///     log.view().panel("Log")
/// });
/// let subscriber = tracing_subscriber::registry().with(tracing_layer(made.unwrap(), app.proxy()));
/// tracing::subscriber::set_global_default(subscriber).unwrap();
/// app.run().unwrap();
/// ```
pub fn tracing_layer(log: Log, proxy: Proxy) -> EventLayer {
    TracingFeed::new(log, proxy).layer()
}

/// Make a subscriber feeding `log` the global default (see
/// [`TracingFeed::install`]).
pub fn install_tracing(
    log: Log,
    proxy: Proxy,
) -> Result<(), tracing::subscriber::SetGlobalDefaultError> {
    TracingFeed::new(log, proxy).install()
}

/// On the app's thread: everything waiting into the log, then a line for
/// what was dropped.
fn drain(
    records: &Mutex<Receiver<Record>>,
    counter: &ChannelSink<Record>,
    log: Log,
    fields: bool,
    spans: bool,
) {
    let records = records.lock().unwrap_or_else(|e| e.into_inner());
    for record in records.try_iter() {
        log.push_line(line(&record, fields, spans));
    }
    let dropped = counter.take_dropped();
    if dropped > 0 {
        let s = if dropped == 1 { "" } else { "s" };
        log.push(format!("[dim]… {dropped} event{s} dropped[/]"));
    }
}

/// A record as a log line: its level and target apart, then its spans,
/// its message and its fields.
pub(crate) fn line(record: &Record, fields: bool, spans: bool) -> LogLine {
    let mut markup = String::new();
    if let (true, Some(Value::String(names))) = (spans, record.get("spans")) {
        markup.push_str(&format!("[dim]{}:[/] ", rich::markup::escape(names)));
    }
    markup.push_str(&rich::markup::escape(
        record.message().as_deref().unwrap_or_default(),
    ));
    if fields {
        for (key, value) in &record.fields {
            if OWN_KEYS.contains(&key.as_str()) {
                continue;
            }
            let value = match value {
                Value::String(text) => text.clone(),
                Value::Null => "null".to_string(),
                Value::Bool(b) => b.to_string(),
                Value::Integer(n) => n.to_string(),
                Value::Unsigned(n) => n.to_string(),
                Value::Integer128(n) => n.to_string(),
                Value::Unsigned128(n) => n.to_string(),
                Value::Float(n) => n.to_string(),
                other => format!("{other:?}"),
            };
            markup.push_str(&format!(
                " [dim]{}=[/]{}",
                rich::markup::escape(key),
                rich::markup::escape(&value)
            ));
        }
    }
    let mut line = LogLine::new(markup);
    if let Some(level) = record.severity() {
        line = line.level(level);
    }
    if let Some(Value::String(target)) = record.get("target") {
        line = line.target(target.clone());
    }
    line
}
