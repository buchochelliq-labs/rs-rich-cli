//! Small widgets after Textual's (#813): link, progress_bar, header,
//! key_panel, masked_input and placeholder.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use intuituive::prelude::*;
use intuituive::{
    PlaceholderVariant, assert_screen, header, key_panel, link, masked_input, placeholder,
    progress_bar,
};

fn shown(driver: &intuituive::Driver) -> String {
    driver.screen().plain().join("\n")
}

#[test]
fn enter_on_a_focused_link_calls_the_opener() {
    let opened = Rc::new(RefCell::new(Vec::new()));
    let seen = opened.clone();
    let app = App::new(move || {
        let opened = opened.clone();
        column([
            label("other").focusable(),
            link("Docs", "https://example.com/docs")
                .name("docs")
                .open_with(move |url| opened.borrow_mut().push(url.to_string())),
        ])
    });
    let mut driver = app.driver(20, 2);
    driver.press("tab");
    driver.assert_focused("docs");
    driver.press("enter");
    driver.click_on("docs");
    assert_eq!(seen.borrow().len(), 2);
    assert_eq!(seen.borrow()[0], "https://example.com/docs");
}

#[test]
fn a_link_runs_its_action_instead() {
    let app = App::new(|| {
        let n = signal(0);
        column([
            text!("{n}"),
            link("more", "x").on_activate(move |_| n.update(|n| *n += 1)),
        ])
    });
    let mut driver = app.driver(10, 2);
    driver.press("enter");
    assert_eq!(driver.screen().plain()[0].trim_end(), "1");
}

#[test]
fn progress_bar_shows_its_share_and_eta_and_pulses_without_a_total() {
    let app = App::new(|| {
        let (done, total) = (signal(0.0), signal(Some(10.0)));
        column([
            progress_bar(done, total)
                .show_percentage(true)
                .show_eta(true),
            progress_bar(signal(0.0), signal(None)),
        ])
        .on_key("h", move |_| done.set(5.0))
    });
    let mut driver = app.driver(40, 2);
    driver.settle();
    driver.advance(Duration::from_secs(10));
    driver.press("h");
    let row = driver.screen().plain()[0].clone();
    assert!(row.contains(" 50%"), "{row}");
    assert!(row.trim_end().ends_with("ETA 0:00:10"), "{row}");
    // The pulse moves with the app's clock.
    let styled = |driver: &intuituive::Driver| {
        intuituive::testing::styled(driver)
            .lines()
            .nth(1)
            .unwrap()
            .to_string()
    };
    let before = styled(&driver);
    driver.advance(Duration::from_millis(300));
    assert_ne!(styled(&driver), before);
}

#[test]
fn header_shows_the_top_screens_subtitle_and_a_ticking_clock() {
    let app = App::new(|| {
        column([header().clock(), label("home")])
            .on_key("s", |cx| cx.set_subtitle("home screen"))
            .on_key("p", |cx| {
                cx.push(|| {
                    column([header(), label("detail")]).on_key("d", |cx| cx.set_subtitle("detail"))
                })
            })
    })
    .title("Demo");
    let mut driver = app.driver(60, 3);
    driver.press("s");
    let top = driver.screen().plain()[0].clone();
    assert!(top.contains("Demo — home screen"), "{top}");
    let clock =
        |driver: &intuituive::Driver| driver.screen().plain()[0].trim_end()[51..].to_string();
    let at = clock(&driver);
    driver.advance(Duration::from_secs(2));
    assert_ne!(clock(&driver), at, "the clock ticks");
    driver.press("p d");
    assert!(driver.screen().plain()[0].contains("Demo — detail"));
}

#[test]
fn key_panel_follows_the_focus_and_toggles() {
    let app = App::new(|| {
        row([
            column([
                label("files")
                    .name("files")
                    .bind("d", "Delete", |_| {})
                    .focusable(),
                label("log")
                    .name("log")
                    .bind("f", "Follow", |_| {})
                    .focusable(),
            ]),
            key_panel(),
        ])
        .bind("k", "Keys", |cx| cx.toggle_key_panel())
    });
    let mut driver = app.driver(60, 6);
    driver.settle();
    assert!(shown(&driver).contains("d  Delete"));
    assert!(!shown(&driver).contains("Follow"));
    driver.press("tab");
    assert!(shown(&driver).contains("f  Follow"));
    assert!(!shown(&driver).contains("Delete"));
    driver.press("k");
    assert!(!shown(&driver).contains("Keys"));
    driver.press("k");
    assert!(shown(&driver).contains("Keys"));
}

#[test]
fn masked_input_fixes_its_separators() {
    let app = App::new(|| {
        let date = signal(String::new());
        column([
            masked_input(date, "9999-99-99").name("date"),
            text!("[{date}]"),
        ])
    });
    let mut driver = app.driver(20, 2);
    driver.type_text("20261027");
    assert_eq!(driver.screen().plain()[0].trim_end(), "2026-10-27");
    assert_eq!(driver.screen().plain()[1].trim_end(), "[2026-10-27]");
    // Backspace skips the separators.
    driver.press("backspace backspace backspace");
    assert_eq!(driver.screen().plain()[1].trim_end(), "[2026-1]");
    // Letters do not fit; a digit does.
    driver.type_text("x5");
    assert_eq!(driver.screen().plain()[1].trim_end(), "[2026-15]");
}

#[test]
fn placeholder_snapshot() {
    let app = App::new(|| {
        row([
            placeholder("sidebar").fixed(12),
            column([
                placeholder("main"),
                placeholder("size").placeholder_variant(PlaceholderVariant::Size),
            ]),
        ])
    });
    let mut driver = app.driver(30, 6);
    driver.settle();
    assert_screen!(driver, "placeholder");
}

/// The key panel lists the help's sections (#813): a heading for each
/// named section, its bindings under it, those with none first.
#[test]
fn key_panel_lists_the_helps_sections() {
    let app = App::new(|| {
        row([
            label("files")
                .name("files")
                .bind("d", "Delete", |_| {})
                .section("selection", |node| node.bind("v", "Select", |_| {}))
                .focusable(),
            key_panel(),
        ])
        .bind("q", "Quit", |_| {})
    });
    let mut driver = app.driver(60, 8);
    driver.settle();
    let lines: Vec<String> = driver
        .screen()
        .plain()
        .iter()
        .map(|line| line.split('│').nth(1).unwrap_or("").trim().to_string())
        .filter(|cell| !cell.is_empty())
        .collect();
    let at = |text: &str| lines.iter().position(|l| l == text);
    let (files, delete) = (at("files").unwrap(), at("d  Delete").unwrap());
    let (selection, select) = (at("selection").unwrap(), at("v  Select").unwrap());
    let quit = at("q  Quit").unwrap();
    assert!(quit < files && files < delete, "{lines:?}");
    assert!(selection < select, "{lines:?}");
    // The heading is styled as a heading.
    let styled = intuituive::testing::styled(&driver);
    assert!(styled.contains("[bold underline]files"), "{styled}");
}
