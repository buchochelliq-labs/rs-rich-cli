//! [`ChannelSink`]: an [`EventSink`] that sends each event, as a
//! [`Record`], down a bounded channel, so a log view on another thread
//! can show what `tracing` or `log` reports.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TrySendError};
use std::sync::Arc;

use super::Record;
use crate::adapters::EventSink;
use crate::event::StructuredEvent;

/// Called after each send, from the logging thread.
type Notify = Arc<dyn Fn() + Send + Sync>;

/// An [`EventSink`] over a bounded channel. Each event becomes a
/// [`Record`] ([`Record::from_event`]), turned into `T` (a `Record`
/// itself by default, or rs-rich-interact's `LogEvent` for its
/// `LogView::source`), and is sent without waiting: when the channel is
/// full the record is dropped and counted ([`dropped`](Self::dropped)), so
/// a slow reader never holds up the thread that logs.
///
/// Clones share the channel and the count: keep one to read the count
/// after handing another to `EventLayer::new` or `LogAdapter::new`.
///
/// ```
/// use rich_ext::adapters::EventSink;
/// use rich_ext::event::{EventContext, Message, Severity, StructuredEvent};
/// use rich_ext::log_stream::{ChannelSink, Record};
///
/// let (sink, records) = ChannelSink::<Record>::bounded(2);
/// for message in ["one", "two", "three"] {
///     let event = StructuredEvent::new(Message::Literal(message.into())).context(EventContext {
///         severity: Some(Severity::Info),
///         ..Default::default()
///     });
///     let _ = sink.emit(event);
/// }
/// let got: Vec<_> = records.try_iter().map(|r| r.message().unwrap().into_owned()).collect();
/// assert_eq!(got, ["one", "two"]);
/// assert_eq!(sink.dropped(), 1);
/// ```
pub struct ChannelSink<T = Record> {
    sender: SyncSender<T>,
    dropped: Arc<AtomicU64>,
    notify: Option<Notify>,
}

impl<T> Clone for ChannelSink<T> {
    fn clone(&self) -> Self {
        ChannelSink {
            sender: self.sender.clone(),
            dropped: Arc::clone(&self.dropped),
            notify: self.notify.clone(),
        }
    }
}

impl<T: From<Record> + Send> ChannelSink<T> {
    /// A sink and the receiving end of its channel, which holds at most
    /// `capacity` records (at least one).
    pub fn bounded(capacity: usize) -> (ChannelSink<T>, Receiver<T>) {
        let (sender, receiver) = sync_channel(capacity.max(1));
        let sink = ChannelSink {
            sender,
            dropped: Arc::new(AtomicU64::new(0)),
            notify: None,
        };
        (sink, receiver)
    }

    /// Call `notify` after each record is sent, or dropped because the
    /// channel was full, on the thread that logged: to wake the reader
    /// (an app's event loop). Keep it quick; it must not log.
    #[must_use]
    pub fn notify(mut self, notify: impl Fn() + Send + Sync + 'static) -> ChannelSink<T> {
        self.notify = Some(Arc::new(notify));
        self
    }

    /// Send `record` without waiting. Returns whether it was sent: `false`
    /// when the channel was full (the record is counted as dropped) or its
    /// receiver is gone.
    pub fn send(&self, record: Record) -> bool {
        let sent = match self.sender.try_send(T::from(record)) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) => {
                self.dropped.fetch_add(1, Ordering::Relaxed);
                false
            }
            Err(TrySendError::Disconnected(_)) => return false,
        };
        if let Some(notify) = &self.notify {
            notify();
        }
        sent
    }

    /// The records dropped because the channel was full, in all.
    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    /// The records dropped since the last call, resetting the count: what
    /// a reader shows as "N dropped" once.
    pub fn take_dropped(&self) -> u64 {
        self.dropped.swap(0, Ordering::Relaxed)
    }
}

impl<T: From<Record> + Send + 'static> EventSink for ChannelSink<T> {
    fn emit(&self, event: StructuredEvent) -> std::io::Result<()> {
        if self.send(Record::from_event(&event)) {
            Ok(())
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::WouldBlock,
                "the log channel is full or closed",
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{EventContext, Message, Severity};
    use std::sync::atomic::AtomicUsize;

    fn event(message: &str) -> StructuredEvent {
        StructuredEvent::new(Message::Literal(message.into())).context(EventContext {
            severity: Some(Severity::Error),
            target: Some("app".into()),
            ..Default::default()
        })
    }

    #[test]
    fn a_full_channel_drops_and_counts_without_blocking() {
        let (sink, records) = ChannelSink::<Record>::bounded(1);
        assert!(sink.emit(event("kept")).is_ok());
        // A second send would block a plain `send`; this one returns.
        assert!(sink.emit(event("lost")).is_err());
        assert!(!sink.send(Record::text("lost too")));
        assert_eq!(sink.clone().dropped(), 2);
        assert_eq!(sink.take_dropped(), 2);
        assert_eq!(sink.dropped(), 0);
        let got: Vec<Record> = records.try_iter().collect();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].message().as_deref(), Some("kept"));
        assert_eq!(got[0].severity(), Some(Severity::Error));
    }

    #[test]
    fn notify_runs_for_each_send_and_a_closed_channel_is_not_a_drop() {
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&calls);
        let (sink, records) = ChannelSink::<Record>::bounded(4);
        let sink = sink.notify(move || {
            seen.fetch_add(1, Ordering::Relaxed);
        });
        sink.send(Record::text("a"));
        sink.send(Record::text("b"));
        assert_eq!(calls.load(Ordering::Relaxed), 2);
        drop(records);
        assert!(!sink.send(Record::text("c")));
        assert_eq!(sink.dropped(), 0);
    }

    #[test]
    fn records_cross_threads() {
        let (sink, records) = ChannelSink::<Record>::bounded(64);
        let worker = std::thread::spawn(move || {
            for i in 0..10 {
                let _ = sink.emit(event(&format!("line {i}")));
            }
        });
        worker.join().unwrap();
        assert_eq!(records.try_iter().count(), 10);
    }
}
