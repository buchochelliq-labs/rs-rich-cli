use rich_ext::{
    adapters::{EventLayer, EventSink},
    event::StructuredEvent,
};
use tracing_subscriber::prelude::*;
struct Output;
impl EventSink for Output {
    fn emit(&self, event: StructuredEvent) -> std::io::Result<()> {
        rich::Console::new().print(&event);
        Ok(())
    }
}
fn main() {
    let subscriber =
        tracing_subscriber::registry().with(EventLayer::new(std::sync::Arc::new(Output)));
    tracing::subscriber::with_default(subscriber, || {
        tracing::info!(items = 3i64, ready = true, "Scoped subscriber")
    });
}
