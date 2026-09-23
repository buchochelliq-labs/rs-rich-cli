//! Optional facade adapters with explicit ownership; no global installation.
use crate::event::StructuredEvent;
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
