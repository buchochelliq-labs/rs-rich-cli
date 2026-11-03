//! Lists whose rows are nodes: `list_of` (#811).

use std::cell::RefCell;
use std::rc::Rc;

use intuituive::prelude::*;
use intuituive::{button, list_of, list_of_virtual};

/// Three cards, each a title and a button; the selection and the presses
/// are written to `log`.
fn cards(log: Rc<RefCell<Vec<String>>>) -> App {
    App::new(move || {
        let selected = signal(0usize);
        let (pressed, picked) = (log.clone(), log.clone());
        column([
            text!("selected {selected}").fixed(1),
            list_of(
                || 3,
                move |i| {
                    let pressed = pressed.clone();
                    row([
                        label(format!("card {i}")).class("title").fixed(8),
                        button("Go", move |_| pressed.borrow_mut().push(format!("go {i}")))
                            .name(&format!("go-{i}")),
                    ])
                    .fixed(3)
                },
                selected,
            )
            .on_select(move |i, _| picked.borrow_mut().push(format!("select {i}")))
            .name("cards"),
        ])
    })
}

#[test]
fn down_and_down_selects_the_third_card() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut driver = cards(log.clone()).driver(30, 12);
    driver.settle();
    driver.assert_focused("cards");
    driver.press("down down");
    assert_eq!(driver.screen().plain()[0].trim_end(), "selected 2");
    driver.press("enter");
    assert_eq!(*log.borrow(), vec!["select 2".to_string()]);
}

#[test]
fn a_cards_button_runs_the_button_not_the_selection() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut driver = cards(log.clone()).driver(30, 12);
    driver.click_on("go-1");
    assert_eq!(*log.borrow(), vec!["go 1".to_string()]);
    assert_eq!(driver.screen().plain()[0].trim_end(), "selected 0");
    // A click elsewhere on the card selects it.
    driver.click_text("card 1");
    assert_eq!(driver.screen().plain()[0].trim_end(), "selected 1");
    assert_eq!(log.borrow().last().unwrap(), "select 1");
}

#[test]
fn separators_and_disabled_rows_are_skipped() {
    let app = App::new(|| {
        let selected = signal(0usize);
        column([
            text!("at {selected}").fixed(1),
            list_of(|| 4, |i| label(format!("item {i}")), selected)
                .separator_after(|i| i == 0)
                .item_disabled_when(|i| i == 1 || i == 2),
        ])
    });
    let mut driver = app.driver(12, 8);
    driver.settle();
    let lines = driver.screen().plain();
    assert!(lines[2].contains('─'), "{lines:?}");
    driver.press("down");
    assert_eq!(driver.screen().plain()[0].trim_end(), "at 3");
    driver.press("up");
    assert_eq!(driver.screen().plain()[0].trim_end(), "at 0");
    // Disabled rows are drawn dim.
    let styled = intuituive::testing::styled(&driver);
    let row = styled.lines().find(|line| line.contains("item 1")).unwrap();
    assert!(row.contains("dim"), "{styled}");
    let row = styled.lines().find(|line| line.contains("item 3")).unwrap();
    assert!(!row.contains("dim"), "{styled}");
}

#[test]
fn a_stylesheet_styles_the_selected_cards_title() {
    let app = cards(Rc::default()).stylesheet(":selected .title { text-style: bold; }");
    let mut driver = app.driver(30, 12);
    driver.press("down");
    let styled = intuituive::testing::styled(&driver);
    let bold: Vec<&str> = styled
        .lines()
        .filter(|line| line.contains("card") && line.contains("bold"))
        .collect();
    assert_eq!(bold.len(), 1, "{styled}");
    assert!(bold[0].contains("card 1"), "{styled}");
}

#[test]
fn the_cursor_stays_in_view() {
    let app = App::new(|| {
        let selected = signal(0usize);
        list_of(|| 20, |i| label(format!("row {i}")).fixed(2), selected).scroll_padding(1)
    });
    let mut driver = app.driver(12, 6);
    driver.settle();
    driver.press("end");
    assert!(
        driver
            .screen()
            .plain()
            .iter()
            .any(|line| line.starts_with("row 19")),
        "{:?}",
        driver.screen().plain()
    );
    driver.press("home");
    assert!(driver.screen().plain()[0].starts_with("row 0"));
}

/// `list_of_virtual` (#811): the same cursor, skips and selection over a
/// million rows, building only those near the view.
#[test]
fn a_virtual_list_of_nodes_builds_only_the_rows_near_the_view() {
    let built = Rc::new(std::cell::Cell::new(0usize));
    let log = Rc::new(RefCell::new(Vec::new()));
    let (count, picked) = (built.clone(), log.clone());
    let app = App::new(move || {
        let selected = signal(0usize);
        let (count, picked) = (count.clone(), picked.clone());
        column([
            text!("at {selected}").fixed(1),
            list_of_virtual(
                || 1_000_000,
                |_| 1,
                move |i| {
                    count.set(count.get() + 1);
                    label(format!("row {i}")).class("title")
                },
                selected,
            )
            .separator_after(|i| i == 0)
            .item_disabled_when(|i| i == 1)
            .on_select(move |i, _| picked.borrow_mut().push(i))
            .name("rows"),
        ])
    });
    let mut driver = app.driver(16, 6);
    driver.settle();
    driver.assert_focused("rows");
    // The separator after row 0, and the disabled row 1, are skipped.
    driver.press("down");
    assert_eq!(driver.screen().plain()[0].trim_end(), "at 2");
    driver.press("end");
    let lines = driver.screen().plain();
    assert_eq!(lines[0].trim_end(), "at 999999");
    assert!(
        lines.iter().any(|l| l.starts_with("row 999999")),
        "{lines:?}"
    );
    driver.press("pageup");
    let lines = driver.screen().plain();
    let at: usize = lines[0].trim_end()[3..].parse().unwrap();
    assert!(at < 999_999 && at > 999_990, "{lines:?}");
    assert!(
        lines.iter().any(|l| l.starts_with(&format!("row {at}"))),
        "{lines:?}"
    );
    driver.press("enter");
    assert_eq!(*log.borrow(), vec![at]);
    assert!(built.get() < 200, "built {} rows", built.get());
    driver.press("home");
    assert!(driver.screen().plain()[1].starts_with("row 0"));
}

/// The selected row of a virtual list matches `:selected`.
#[test]
fn a_virtual_lists_selected_row_matches_selected() {
    let app = App::new(|| {
        list_of_virtual(
            || 100,
            |_| 1,
            |i| label(format!("row {i}")).class("title"),
            signal(0usize),
        )
    })
    .stylesheet(":selected .title { text-style: bold; }");
    let mut driver = app.driver(12, 4);
    driver.press("down down");
    let styled = intuituive::testing::styled(&driver);
    let bold: Vec<&str> = styled.lines().filter(|l| l.contains("bold")).collect();
    assert_eq!(bold.len(), 1, "{styled}");
    assert!(bold[0].contains("row 2"), "{styled}");
}
