use rich_ext::{
    adapters::{EventSink, LogAdapter},
    event::StructuredEvent,
};
struct Output;
impl EventSink for Output {
    fn emit(&self, event: StructuredEvent) -> std::io::Result<()> {
        rich::Console::new().print(&event);
        Ok(())
    }
}
fn main() {
    let adapter = Box::leak(Box::new(LogAdapter::new(
        std::sync::Arc::new(Output),
        log::LevelFilter::Info,
    )));
    log::set_logger(adapter).expect("caller installs the logger");
    log::set_max_level(log::LevelFilter::Info);
    log::info!("Application owns logger installation");
}
