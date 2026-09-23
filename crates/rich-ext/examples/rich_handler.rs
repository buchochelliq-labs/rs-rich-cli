//! `log` and `tracing` records printed through `RichHandler`, upstream's
//! `logging.RichHandler` layout.
use std::sync::Arc;

use rich_ext::{
    adapters::{EventLayer, LogAdapter},
    RichHandler,
};
use tracing_subscriber::prelude::*;

fn main() {
    let handler = Arc::new(RichHandler::new(rich::Console::new()));

    let logger = Box::leak(Box::new(LogAdapter::new(
        handler.clone(),
        log::LevelFilter::Debug,
    )));
    log::set_logger(logger).expect("caller installs the logger");
    log::set_max_level(log::LevelFilter::Debug);
    log::info!("Server starting on 127.0.0.1:8080");
    log::debug!("GET /index.html 200 in 0.4ms");
    log::warn!("cache miss for key {:?}", "user:42");

    let subscriber = tracing_subscriber::registry().with(EventLayer::new(handler));
    tracing::subscriber::with_default(subscriber, || {
        tracing::error!(retries = 3, "POST /upload failed");
    });
}
