//! Facade adapters with explicit ownership; no global installation.
//! [`EventSink`] is always here; `LogAdapter` needs the `log` feature and
//! `EventLayer` the `tracing` feature.
use crate::event::StructuredEvent;
/// Where an adapter sends each event: a `RichHandler`, a
/// [`ChannelSink`](crate::log_stream::ChannelSink), a file, a test buffer.
pub trait EventSink: Send + Sync {
    fn emit(&self, event: StructuredEvent) -> std::io::Result<()>;
}
#[cfg(feature = "log")]
mod log;
#[cfg(feature = "log")]
pub use log::LogAdapter;
#[cfg(feature = "tracing")]
mod tracing;
#[cfg(feature = "tracing")]
pub use tracing::EventLayer;
