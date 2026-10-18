//! Scrollbars everywhere (gap batch B3): on any side, in any symbols, on
//! lists, tables, trees, text areas and logs, on their own, and with the
//! mouse.

use intuituive::interact::{Button, Event, Mouse, MouseKind};
use intuituive::prelude::*;
use intuituive::widgets::{
    scrollbar, table, tree, Column, Scrollbar, ScrollbarSymbols, Show, Side, TreeItem,
};
use intuituive::{assert_screen, Driver};

fn numbered(n: usize) -> Vec<String> {
    (0..n).map(|i| format!("line {i}")).collect()
}

fn rows(driver: &Driver) -> Vec<String> {
    driver
        .screen()
        .plain()
        .iter()
        .map(|row| row.trim_end().to_string())
        .collect()
}

/// The last column of each row.
fn last_column(driver: &Driver) -> String {
    driver
        .screen()
        .plain()
        .iter()
        .map(|row| row.chars().last().unwrap_or(' '))
        .collect()
}

fn press(driver: &mut Driver, column: u16, row: u16) {
    driver.event(Event::Mouse(Mouse::new(
        MouseKind::Down(Button::Left),
        column,
        row,
    )));
    driver.settle();
}

#[test]
fn the_default_bar_is_unchanged() {
    let app = App::new(|| scroll(label(numbered(20).join("\n"))));
    let mut driver = app.driver(10, 5);
    driver.settle();
    // As before: a thin line with a heavy thumb at the top.
    assert_eq!(last_column(&driver), "┃││││");
}

#[test]
fn scrollbar_arrows() {
    let app = App::new(|| {
        scroll(label(numbered(20).join("\n"))).scrollbar(
            Scrollbar::new()
                .symbols(ScrollbarSymbols::VERTICAL)
                .ends(Some("↑"), Some("↓")),
        )
    });
    let mut driver = app.driver(10, 6);
    driver.settle();
    assert_screen!(driver, "scrollbar_arrows");
    assert_eq!(last_column(&driver), "↑█│││↓");
    // A press on the bottom arrow steps a line.
    press(&mut driver, 9, 5);
    assert!(rows(&driver)[0].starts_with("line 1 "));
}

#[test]
fn scrollbar_left_no_track() {
    let app = App::new(|| {
        let items = || numbered(20);
        list(items, signal(0)).scrollbar(Scrollbar::new().side(Side::Left).thumb("▐").track(None))
    });
    let mut driver = app.driver(10, 4);
    driver.settle();
    assert_screen!(driver, "scrollbar_left_no_track");
    let screen = rows(&driver);
    assert_eq!(screen[0], "▐line 0");
    assert_eq!(screen[1], " line 1");
}

#[test]
fn scrollbar_bottom_custom() {
    let app = App::new(|| {
        scroll_x(label("0123456789abcdefghijklmnopqrstuvwxyz")).scrollbar(
            Scrollbar::new()
                .side(Side::Top)
                .symbols(ScrollbarSymbols::DOUBLE_HORIZONTAL),
        )
    });
    let mut driver = app.driver(12, 2);
    driver.settle();
    assert_screen!(driver, "scrollbar_bottom_custom");
    let screen = rows(&driver);
    // Along the top, with triangles; the text below it.
    assert!(screen[0].starts_with("◄███"), "{screen:?}");
    assert!(screen[0].ends_with('►'), "{screen:?}");
    assert_eq!(screen[1], "0123456789ab");
}

#[test]
fn list_scrollbar() {
    let app = App::new(|| list(|| numbered(10), signal(0)).scrollbar(Scrollbar::new()));
    let mut driver = app.driver(10, 5);
    driver.settle();
    assert_screen!(driver, "list_scrollbar");
    assert_eq!(last_column(&driver), "┃┃┃││");
    // A short list keeps no column.
    let app = App::new(|| list(|| numbered(3), signal(0)).scrollbar(Scrollbar::new()));
    let mut driver = app.driver(10, 5);
    driver.settle();
    assert_eq!(last_column(&driver), "     ");
}

#[test]
fn table_scrollbar() {
    let app = App::new(|| {
        let rows = || (0..20).map(|i| vec![format!("row {i}")]).collect();
        table(vec![Column::new("Name", Size::Flex(1))], rows, signal(0)).scrollbar(Scrollbar::new())
    });
    let mut driver = app.driver(10, 5);
    driver.settle();
    assert_screen!(driver, "table_scrollbar");
    // Down the rows, under the header.
    assert_eq!(last_column(&driver), " ┃│││");
}

#[test]
fn trees_text_areas_and_logs_take_bars() {
    let app = App::new(|| {
        let items = || {
            (0..10)
                .map(|i| TreeItem::new(format!("item {i}")))
                .collect()
        };
        tree(items, signal(vec![0])).scrollbar(Scrollbar::new())
    });
    let mut driver = app.driver(10, 4);
    driver.settle();
    assert_eq!(last_column(&driver), "┃┃││");
    let app = App::new(|| text_area(signal(numbered(8).join("\n"))).scrollbar(Scrollbar::new()));
    let mut driver = app.driver(10, 4);
    driver.settle();
    assert_eq!(last_column(&driver), "┃┃││");
    let app = App::new(|| {
        let log = Log::new(100);
        for line in numbered(8) {
            log.push(line);
        }
        log.view().scrollbar(Scrollbar::new())
    });
    let mut driver = app.driver(10, 4);
    driver.settle();
    // The log shows its last lines: the thumb at the bottom.
    assert_eq!(last_column(&driver), "││┃┃");
    assert_eq!(rows(&driver)[3], "line 7   ┃");
}

#[test]
fn dragging_the_thumb_scrolls_and_a_click_on_the_track_pages() {
    let app = App::new(|| scroll(label(numbered(40).join("\n"))));
    let mut driver = app.driver(10, 10);
    driver.settle();
    // A press on the track below the thumb pages down.
    press(&mut driver, 9, 8);
    assert!(rows(&driver)[0].starts_with("line 10 "));
    // Grab the thumb (now at rows 2-4) and drag it to the bottom.
    press(&mut driver, 9, 3);
    driver.event(Event::Mouse(Mouse::new(
        MouseKind::Drag(Button::Left),
        9,
        9,
    )));
    driver.event(Event::Mouse(Mouse::new(MouseKind::Up(Button::Left), 9, 9)));
    driver.settle();
    assert!(rows(&driver)[0].starts_with("line 30 "));
}

#[test]
fn dragging_a_list_bar_carries_the_selection_into_view() {
    let app = App::new(|| {
        let selected = signal(0usize);
        column([
            list(|| numbered(50), selected).scrollbar(Scrollbar::new()),
            text!("{selected}").fixed(1),
        ])
    });
    let mut driver = app.driver(10, 6);
    driver.settle();
    press(&mut driver, 9, 4);
    let screen = rows(&driver);
    assert!(screen[0].starts_with("line 5 "), "{screen:?}");
    assert_eq!(screen[5], "5");
}

#[test]
fn show_never_keeps_no_column() {
    let app = App::new(|| {
        scroll(label(numbered(20).join("\n"))).scrollbar(Scrollbar::new().show(Show::Never))
    });
    let mut driver = app.driver(10, 4);
    driver.settle();
    assert_eq!(last_column(&driver), "    ");
    // Always, a full thumb when everything fits.
    let app =
        App::new(|| list(|| numbered(2), signal(0)).scrollbar(Scrollbar::new().show(Show::Always)));
    let mut driver = app.driver(10, 4);
    driver.settle();
    assert_eq!(last_column(&driver), "┃┃┃┃");
}

#[test]
fn a_bar_of_its_own() {
    let app = App::new(|| {
        let top = signal(0usize);
        row([
            text(move || format!("from {}", top.get())),
            scrollbar(top, || 100, || 10, Scrollbar::new()),
        ])
    });
    let mut driver = app.driver(12, 10);
    driver.settle();
    assert_eq!(last_column(&driver), "┃│││││││││");
    // A press low on the track pages, which the app reads.
    press(&mut driver, 11, 8);
    assert_eq!(rows(&driver)[0], "from 10    │");
}

#[test]
fn a_bar_without_a_track_is_blank_off_the_thumb() {
    let app = App::new(|| list(|| numbered(20), signal(0)).scrollbar(Scrollbar::new().track(None)));
    let mut driver = app.driver(10, 4);
    driver.settle();
    assert_eq!(last_column(&driver), "┃   ");
}

#[test]
fn text_mode_leaves_the_bar_blank() {
    // For a screen reader, the bar is no text to read: its column is kept,
    // and blank.
    let app = App::new(|| {
        list(|| numbered(20), signal(0)).scrollbar(Scrollbar::new().ends(Some("↑"), Some("↓")))
    })
    .accessible(true);
    let mut driver = app.driver(12, 4);
    driver.settle();
    assert_eq!(last_column(&driver), "    ");
    assert_eq!(rows(&driver)[0], "> line 0");
}
