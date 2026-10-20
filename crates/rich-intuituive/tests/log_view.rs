//! `Log`: scrollback, follow mode, levels, the level, target and text
//! filters, keys and the wheel, the targets panel, and what a screen
//! reader hears.

use intuituive::a11y::Role;
use intuituive::interact::{Event, Mouse, MouseKind};
use intuituive::log::{LogLine, Severity, TargetLevel};
use intuituive::prelude::*;
use intuituive::widgets::Scrollbar;
use intuituive::Driver;

/// An app of a focusable log `rows` tall holding `lines` numbered lines,
/// and the log.
fn app(lines: usize, capacity: usize, rows: u16) -> (Driver, Log) {
    let mut made = None;
    let app = App::new(|| {
        let log = Log::new(capacity);
        for i in 0..lines {
            log.push(format!("line {i}"));
        }
        made = Some(log);
        log.view().focusable().on_key("q", |cx| cx.quit())
    });
    let mut driver = app.driver(20, rows);
    driver.settle();
    (driver, made.unwrap())
}

fn rows(driver: &Driver) -> Vec<String> {
    driver
        .screen()
        .plain()
        .iter()
        .map(|row| row.trim_end().to_string())
        .collect()
}

#[test]
fn it_follows_the_end_until_scrolled_back_and_follows_again_on_end() {
    let (mut driver, log) = app(10, 100, 3);
    assert_eq!(rows(&driver), ["line 7", "line 8", "line 9"]);
    assert!(log.is_following());
    driver.press("up");
    assert_eq!(rows(&driver), ["line 6", "line 7", "line 8"]);
    assert!(!log.is_following());
    // New lines arrive below without moving the view, and are counted.
    log.push("line 10");
    log.push("line 11");
    driver.settle();
    assert_eq!(rows(&driver), ["line 6", "line 7", "line 8"]);
    assert_eq!(log.unseen(), 3);
    driver.press("end");
    assert_eq!(rows(&driver), ["line 9", "line 10", "line 11"]);
    assert!(log.is_following());
    assert_eq!(log.unseen(), 0);
}

#[test]
fn page_and_home_keys_stop_at_the_top_and_down_to_the_last_line_follows() {
    let (mut driver, log) = app(10, 100, 3);
    driver.press("home");
    assert_eq!(rows(&driver), ["line 0", "line 1", "line 2"]);
    driver.press("pageup");
    assert_eq!(rows(&driver), ["line 0", "line 1", "line 2"]);
    driver.press("pagedown");
    assert_eq!(rows(&driver), ["line 2", "line 3", "line 4"]);
    driver.press("j j j j");
    assert_eq!(rows(&driver), ["line 6", "line 7", "line 8"]);
    assert!(!log.is_following());
    driver.press("j");
    assert!(log.is_following());
    log.push("line 10");
    driver.settle();
    assert_eq!(rows(&driver), ["line 8", "line 9", "line 10"]);
}

#[test]
fn the_wheel_scrolls_three_lines() {
    let (mut driver, log) = app(10, 100, 3);
    driver.event(Event::Mouse(Mouse::new(MouseKind::ScrollUp, 1, 1)));
    driver.settle();
    assert_eq!(rows(&driver), ["line 4", "line 5", "line 6"]);
    driver.event(Event::Mouse(Mouse::new(MouseKind::ScrollDown, 1, 1)));
    driver.settle();
    assert!(log.is_following());
    assert_eq!(rows(&driver), ["line 7", "line 8", "line 9"]);
}

#[test]
fn a_log_that_fits_leaves_the_keys_to_its_ancestors() {
    let mut made = None;
    let app = App::new(|| {
        let log = Log::new(10);
        log.push("only");
        made = Some(log);
        let hits = signal(0);
        column([log.view().focusable().fixed(3), text!("{hits}").fixed(1)])
            .on_key("up", move |_| hits.update(|n| *n += 1))
    });
    let mut driver = app.driver(10, 4);
    driver.press("up");
    assert_eq!(rows(&driver)[3], "1");
    assert!(made.unwrap().is_following());
}

#[test]
fn the_scrollback_drops_the_oldest_and_a_view_scrolled_past_it_shows_the_top() {
    let (mut driver, log) = app(5, 5, 2);
    driver.press("home");
    assert_eq!(rows(&driver), ["line 0", "line 1"]);
    for i in 5..9 {
        log.push(format!("line {i}"));
    }
    driver.settle();
    assert_eq!(log.len(), 5);
    assert_eq!(rows(&driver), ["line 4", "line 5"]);
    log.set_capacity(2);
    driver.settle();
    assert_eq!(log.len(), 2);
    assert_eq!(rows(&driver), ["line 7", "line 8"]);
}

fn leveled() -> (Driver, Log) {
    let mut made = None;
    let app = App::new(|| {
        let log = Log::new(100);
        let lines = [
            (Severity::Debug, "app::db", "query took 3ms"),
            (Severity::Info, "app::http", "GET /items"),
            (Severity::Warn, "app::db", "pool [b]low[/]"),
            (Severity::Error, "app::http::tls", "handshake failed"),
            (Severity::Trace, "hyper", "frame"),
        ];
        for (level, target, message) in lines {
            log.push_line(LogLine::new(message).level(level).target(target));
        }
        log.push("no level");
        made = Some(log);
        log.view().focusable()
    });
    let mut driver = app.driver(40, 6);
    driver.settle();
    (driver, made.unwrap())
}

#[test]
fn lines_show_their_level_and_target_in_theme_styles() {
    let (driver, log) = leveled();
    assert_eq!(
        rows(&driver),
        [
            "DEBUG app::db query took 3ms",
            "INFO  app::http GET /items",
            "WARN  app::db pool low",
            "ERROR app::http::tls handshake failed",
            "TRACE hyper frame",
            "no level",
        ]
    );
    let styled = intuituive::testing::styled(&driver);
    assert!(
        styled.contains("[bold red]ERROR[/] [dim]app::http::tls[/]"),
        "{styled}"
    );
    log.show_target(false);
    log.show_level(false);
    let mut driver = driver;
    driver.settle();
    assert_eq!(rows(&driver)[0], "query took 3ms");
}

#[test]
fn the_level_filter_keeps_lines_at_or_above_it_and_lines_without_one() {
    let (mut driver, log) = leveled();
    log.set_level(Some(Severity::Warn));
    driver.settle();
    assert_eq!(
        rows(&driver)[..3],
        [
            "WARN  app::db pool low",
            "ERROR app::http::tls handshake failed",
            "no level",
        ]
    );
    assert_eq!(log.shown(), 3);
    assert_eq!(log.level(), Some(Severity::Warn));
}

#[test]
fn target_levels_cover_the_targets_under_them_and_the_most_specific_wins() {
    let (mut driver, log) = leveled();
    log.set_target_level("app", TargetLevel::Off);
    log.set_target_level("app::http::tls", TargetLevel::From(Severity::Error));
    driver.settle();
    assert_eq!(
        rows(&driver)[..3],
        [
            "ERROR app::http::tls handshake failed",
            "TRACE hyper frame",
            "no level",
        ]
    );
    assert_eq!(log.target_level("app::db"), TargetLevel::Off);
    assert_eq!(log.target_level("hyper"), TargetLevel::Inherit);
    // `app` does not cover `apple`.
    assert_eq!(log.target_level("apple"), TargetLevel::Inherit);
    log.set_target_level("app", TargetLevel::Inherit);
    driver.settle();
    assert_eq!(log.shown(), 6);
}

#[test]
fn the_text_filter_is_a_query_over_the_message_level_and_target() {
    let (mut driver, log) = leveled();
    log.set_filter("target:app::http").unwrap();
    driver.settle();
    assert_eq!(log.shown(), 2);
    log.set_filter("LOW or level:error").unwrap();
    driver.settle();
    assert_eq!(
        rows(&driver)[..2],
        [
            "WARN  app::db pool low",
            "ERROR app::http::tls handshake failed"
        ]
    );
    assert!(log.set_filter("(").is_err());
    assert_eq!(log.filter(), "LOW or level:error");
    log.set_filter("").unwrap();
    driver.settle();
    assert_eq!(log.shown(), 6);
}

#[test]
fn a_filtered_log_keeps_filtering_what_arrives_and_scrolls_by_what_shows() {
    let (mut driver, log) = app(0, 100, 2);
    log.set_filter("even").unwrap();
    for i in 0..10 {
        let parity = if i % 2 == 0 { "even" } else { "odd" };
        log.push(format!("{parity} {i}"));
    }
    driver.settle();
    assert_eq!(rows(&driver), ["even 6", "even 8"]);
    driver.press("up");
    assert_eq!(rows(&driver), ["even 4", "even 6"]);
    driver.press("home");
    assert_eq!(rows(&driver), ["even 0", "even 2"]);
}

#[test]
fn the_targets_panel_sets_each_targets_level() {
    let mut made = None;
    let app = App::new(|| {
        let log = Log::new(100);
        log.push_line(LogLine::new("a").level(Severity::Info).target("app"));
        log.push_line(LogLine::new("b").level(Severity::Debug).target("db"));
        log.push_line(LogLine::new("c").level(Severity::Info).target("app"));
        made = Some(log);
        row([log.targets_view().fixed(14), log.view().flex(1)])
    });
    let mut driver = app.driver(40, 3);
    driver.settle();
    let log = made.unwrap();
    let left = |driver: &Driver| -> Vec<String> {
        rows(driver)
            .iter()
            .map(|row| {
                row.chars()
                    .take(14)
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect()
    };
    assert_eq!(left(&driver)[..2], ["TRACE app    2", "TRACE db     1"]);
    // Left from everything: debug and above.
    driver.press("left");
    assert_eq!(log.target_level("app"), TargetLevel::From(Severity::Debug));
    driver.press("down space");
    assert_eq!(log.target_level("db"), TargetLevel::Off);
    assert_eq!(left(&driver)[1], "OFF   db     1");
    assert_eq!(log.shown(), 2);
    driver.press("space");
    assert_eq!(log.target_level("db"), TargetLevel::Inherit);
    driver.press("up left left");
    assert_eq!(log.target_level("app"), TargetLevel::From(Severity::Warn));
    assert_eq!(log.shown(), 1);
    driver.press("backspace");
    assert_eq!(log.target_level("app"), TargetLevel::Inherit);
    let panel = driver
        .accessibility()
        .into_iter()
        .find(|node| node.role == Role::List)
        .unwrap();
    assert_eq!(panel.name, "app: everything, as the log");
    assert_eq!(panel.state.position, Some((1, 2)));
}

#[test]
fn a_screen_reader_hears_the_newest_line_in_view() {
    let (mut driver, log) = app(4, 100, 2);
    let node = |driver: &Driver| {
        driver
            .accessibility()
            .into_iter()
            .find(|node| node.role == Role::Log)
            .unwrap()
    };
    assert_eq!(node(&driver).name, "line 3");
    log.push("[b]line[/] 4");
    driver.settle();
    assert_eq!(node(&driver).name, "line 4");
    driver.press("up");
    assert_eq!(node(&driver).name, "line 3");
}

#[test]
fn the_scrollbar_counts_only_what_shows() {
    let mut made = None;
    let app = App::new(|| {
        let log = Log::new(100);
        for i in 0..20 {
            log.push(format!("line {i}"));
        }
        made = Some(log);
        log.view().scrollbar(Scrollbar::new()).focusable()
    });
    let mut driver = app.driver(10, 4);
    driver.settle();
    // At the end, the thumb at the bottom.
    assert_eq!(rows(&driver)[3].chars().last(), Some('┃'));
    driver.press("home");
    assert_eq!(rows(&driver)[0], "line 0   ┃");
    let log = made.unwrap();
    log.set_filter("line 1").unwrap();
    driver.settle();
    // `line 1` and `line 10` to `line 19`: eleven lines, from the top.
    assert_eq!(log.shown(), 11);
    assert_eq!(rows(&driver)[0], "line 1   ┃");
}
