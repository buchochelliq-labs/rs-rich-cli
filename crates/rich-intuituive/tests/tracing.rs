//! ratatui's tracing app, rebuilt (examples/tracing.rs): the app's own
//! `tracing` events, from its thread and a worker's, in its log pane.

#[path = "../examples/tracing.rs"]
#[allow(dead_code)]
mod example;

use std::time::Duration;

use example::{dispatch, tracing_app, worker};
use intuituive::log::{tracing_layer, LogLine, Severity, TracingFeed};
use intuituive::prelude::*;
use intuituive::Driver;
use tracing_subscriber::prelude::*;

fn rows(driver: &Driver) -> Vec<String> {
    driver
        .screen()
        .plain()
        .iter()
        .map(|r| r.trim_end().to_string())
        .collect()
}

fn find(driver: &Driver, needle: &str) -> Option<String> {
    rows(driver).into_iter().find(|row| row.contains(needle))
}

#[test]
fn the_apps_own_events_show_in_its_log_with_level_and_target() {
    let (app, log) = tracing_app();
    let dispatch = dispatch(&app, log);
    tracing::dispatcher::with_default(&dispatch, || {
        tracing::info!(target: "tracing_app", "started");
        tracing::trace!(target: "tracing_app", "hidden at debug");
        tracing::warn!(target: "tracing_app::net", retries = 3u64, "slow [link]");
    });
    let mut driver = app.driver(100, 12);
    driver.settle();
    assert!(
        find(&driver, "INFO  tracing_app started").is_some(),
        "{:#?}",
        rows(&driver)
    );
    // Markup in a message shows as written; fields follow it.
    assert!(
        find(&driver, "WARN  tracing_app::net slow [link] retries=3").is_some(),
        "{:#?}",
        rows(&driver)
    );
    assert!(find(&driver, "hidden at debug").is_none());
    assert_eq!(log.len(), 3);
    assert_eq!(log.shown(), 2);
    let status = rows(&driver)[11].clone();
    assert!(
        status.starts_with("following · 2 of 3 lines · level debug+"),
        "{status}"
    );
    // The targets panel lists both.
    assert!(find(&driver, "tracing_app::net").is_some());
}

#[test]
fn a_worker_threads_events_arrive_and_the_level_keys_filter_them() {
    let (app, log) = tracing_app();
    let dispatch = dispatch(&app, log);
    worker(dispatch, Duration::ZERO, 7, Some(40))
        .join()
        .unwrap();
    let mut driver = app.driver(100, 14);
    driver.settle();
    assert_eq!(log.len(), 40);
    let targets = log.targets();
    let names: Vec<&str> = targets.iter().map(|(name, _)| name.as_str()).collect();
    assert!(
        names.iter().all(|name| name.starts_with("worker::")),
        "{names:?}"
    );
    assert_eq!(targets.iter().map(|(_, n)| n).sum::<u64>(), 40);
    // `-` three times: errors only, and a line saying so (from the app).
    driver.press("- - -");
    assert_eq!(log.level(), Some(Severity::Error));
    let errors = rows(&driver)
        .iter()
        .filter(|row| row.contains("ERROR") && row.contains("failed"))
        .count();
    assert!(errors > 0);
    assert!(rows(&driver)
        .iter()
        .all(|row| !row.contains("WARN  worker")));
}

#[test]
fn typing_in_the_box_filters_the_log_as_a_query() {
    let (app, log) = tracing_app();
    let dispatch = dispatch(&app, log);
    worker(dispatch, Duration::ZERO, 11, Some(60))
        .join()
        .unwrap();
    let mut driver = app.driver(100, 14);
    driver.settle();
    // Targets, log, box.
    driver.press("tab tab");
    driver.type_text("target:worker::db and level:error");
    driver.settle();
    assert_eq!(log.filter(), "target:worker::db and level:error");
    let shown = log.shown();
    assert!(shown > 0 && shown < 60, "{shown}");
    let log_rows: Vec<String> = rows(&driver)
        .into_iter()
        .filter(|row| row.contains("worker::"))
        .filter(|row| row.contains("job="))
        .collect();
    assert!(
        log_rows
            .iter()
            .all(|row| row.contains("ERROR worker::db failed")),
        "{log_rows:#?}"
    );
}

#[test]
fn a_flood_past_the_channel_is_counted_in_one_line() {
    let mut made = None;
    let app = App::new(|| {
        let log = Log::new(100);
        made = Some(log);
        log.view()
    });
    let log = made.unwrap();
    let layer = TracingFeed::new(log, app.proxy())
        .capacity(4)
        .fields(false)
        .layer();
    let subscriber = tracing_subscriber::registry().with(layer);
    tracing::subscriber::with_default(subscriber, || {
        for i in 0..10 {
            tracing::info!(i, "tick");
        }
    });
    let mut driver = app.driver(30, 6);
    driver.settle();
    assert_eq!(log.len(), 5);
    assert_eq!(rows(&driver)[4], "… 6 events dropped");
    assert_eq!(rows(&driver)[0], "INFO  tracing tick");
}

#[test]
fn tracing_layer_feeds_a_log_next_to_lines_pushed_by_hand() {
    let mut made = None;
    let app = App::new(|| {
        let log = Log::new(10);
        log.push_line(LogLine::text("[by hand]").level(Severity::Info));
        made = Some(log);
        log.view()
    });
    let log = made.unwrap();
    let subscriber = tracing_subscriber::registry().with(tracing_layer(log, app.proxy()));
    tracing::subscriber::with_default(subscriber, || {
        let _span = tracing::info_span!("request").entered();
        tracing::error!(target: "api", "boom");
    });
    let mut driver = app.driver(40, 2);
    driver.settle();
    assert_eq!(
        rows(&driver),
        ["INFO  [by hand]", "ERROR api request: boom"]
    );
}

#[test]
fn scrolled_back_the_status_line_counts_what_is_new() {
    let (app, log) = tracing_app();
    let dispatch = dispatch(&app, log);
    tracing::dispatcher::with_default(
        &dispatch,
        || tracing::info!(target: "tracing_app", "started"),
    );
    worker(dispatch.clone(), Duration::ZERO, 2026, Some(30))
        .join()
        .unwrap();
    let mut driver = app.driver(78, 14);
    driver.settle();
    // From the targets panel to the log, then back three lines.
    driver.press("tab up up up");
    worker(dispatch, Duration::ZERO, 99, Some(5))
        .join()
        .unwrap();
    driver.settle();
    assert!(!log.is_following());
    assert_eq!(log.unseen(), 7);
    intuituive::assert_screen!(driver, "tracing_scrolled_back");
    driver.press("end");
    assert!(log.is_following());
    assert!(rows(&driver)[13].starts_with("following"));
}
