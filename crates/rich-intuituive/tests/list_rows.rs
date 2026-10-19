//! Lists' and tables' row controls (gap batch U, round 1 of the demo
//! gaps): trailing text, a marker column, a selected style per row, a
//! table without a header, `virtual_list`'s scroll padding, row styles and
//! offset, lazy lists, ratatui's cell rules, the screen rect of a row, and
//! a list sized `Size::Auto`.

use std::cell::Cell;
use std::rc::Rc;

use intuituive::interact::{Button, Event, Mouse, MouseKind};
use intuituive::prelude::*;
use intuituive::widgets::{
    cell, list_lazy, table_with, virtual_list, Column, RowRects, TableOptions,
};
use intuituive::Driver;

fn names() -> Vec<String> {
    ["Cargo.toml", "README.md", "src", "target"]
        .into_iter()
        .map(String::from)
        .collect()
}

fn shown(app: App, width: u16, height: u16) -> Driver {
    let mut driver = app.driver(width, height);
    driver.settle();
    driver
}

fn rows(driver: &Driver) -> Vec<String> {
    driver
        .screen()
        .plain()
        .iter()
        .map(|row| row.trim_end().to_string())
        .collect()
}

fn style_at(driver: &Driver, x: u16, y: u16) -> String {
    let screen = driver.screen();
    screen
        .style(screen.cell(x, y).style)
        .map(|s| s.definition())
        .unwrap_or_default()
}

fn click(driver: &mut Driver, column: u16, row: u16) {
    for kind in [MouseKind::Down(Button::Left), MouseKind::Up(Button::Left)] {
        driver.event(Event::Mouse(Mouse::new(kind, column, row)));
        driver.settle();
    }
}

// 1. Row controls on `list`.

#[test]
fn trailing_text_sits_at_the_right_of_each_row() {
    let app =
        App::new(|| list(names, signal(0)).trailing(|i| ["1.2K", "", "4", "96M"][i].to_string()));
    let driver = shown(app, 16, 4);
    let screen = driver.screen().plain();
    assert_eq!(screen[0], "Cargo.toml  1.2K");
    // An empty trailing text leaves the row as it was.
    assert_eq!(screen[1].trim_end(), "README.md");
    assert_eq!(screen[2], "src            4");
    assert_eq!(screen[3], "target       96M");
}

#[test]
fn trailing_text_cuts_the_item_short_of_it() {
    let app = App::new(|| {
        let items = || vec!["a-very-long-file-name.txt".to_string()];
        list(items, signal(0)).trailing(|_| "12K".into())
    });
    let driver = shown(app, 14, 1);
    assert_eq!(driver.screen().plain()[0], "a-very-lo… 12K");
}

#[test]
fn the_highlight_covers_the_trailing_text() {
    let app = App::new(|| list(names, signal(2)).trailing(|_| "4".into()));
    let driver = shown(app, 10, 4);
    assert!(
        style_at(&driver, 9, 2).contains("reverse"),
        "{}",
        style_at(&driver, 9, 2)
    );
    assert!(!style_at(&driver, 9, 1).contains("reverse"));
}

#[test]
fn trailing_text_is_asked_only_for_rows_drawn() {
    let asked = Rc::new(Cell::new(0usize));
    let count = asked.clone();
    let app = App::new(move || {
        let count = count.clone();
        let items = || (0..1000).map(|i| format!("item {i}")).collect();
        list(items, signal(0)).trailing(move |i| {
            count.set(count.get() + 1);
            i.to_string()
        })
    });
    let _driver = shown(app, 20, 5);
    assert!(asked.get() > 0 && asked.get() < 50, "asked {}", asked.get());
}

#[test]
fn the_marker_column_is_kept_outside_the_highlight() {
    let app = App::new(|| {
        list(names, signal(1)).marker(|i| match i {
            0 => Some("on yellow".into()),
            1 => Some("on red".into()),
            _ => None,
        })
    });
    let driver = shown(app, 12, 4);
    let screen = rows(&driver);
    // The text starts after the column, marked or not.
    assert_eq!(screen[0], " Cargo.toml");
    assert_eq!(screen[2], " src");
    assert!(style_at(&driver, 0, 0).contains("on yellow"));
    // The selected row's marker keeps its own style; the highlight starts
    // after it.
    assert!(style_at(&driver, 0, 1).contains("on red"));
    assert!(!style_at(&driver, 0, 1).contains("reverse"));
    assert!(style_at(&driver, 1, 1).contains("reverse"));
    // Unmarked: a plain blank cell.
    assert_eq!(style_at(&driver, 0, 2), "");
}

#[test]
fn the_marker_column_does_not_move_the_text_when_nothing_is_marked() {
    let app = App::new(move || {
        let marked = signal(false);
        list(names, signal(0))
            .symbol("> ")
            .marker(move |i| (marked.get() && i == 2).then(|| "on cyan".into()))
            .on_key("m", move |_| marked.set(true))
    });
    let mut driver = shown(app, 14, 4);
    let before = rows(&driver);
    assert_eq!(style_at(&driver, 0, 2), "");
    driver.press("m");
    assert_eq!(rows(&driver), before);
    assert_eq!(before[0], " > Cargo.toml");
    assert_eq!(before[2], "   src");
    assert!(style_at(&driver, 0, 2).contains("on cyan"));
}

#[test]
fn a_press_on_the_marker_column_selects_the_row() {
    let app = App::new(move || list(names, signal(0)).marker(|_| None));
    let mut driver = shown(app, 12, 4);
    click(&mut driver, 0, 3);
    assert!(style_at(&driver, 1, 3).contains("reverse"));
    assert!(!style_at(&driver, 1, 0).contains("reverse"));
}

#[test]
fn the_selected_style_is_chosen_per_row() {
    let app = App::new(move || {
        let selected = signal(2);
        let colour = |i: usize| if i == 2 { "blue" } else { "green" };
        list(names, selected)
            .row_style(move |i| Some(colour(i).into()))
            .selected_style_for(move |i| (i == 2).then(|| format!("reverse {}", colour(i))))
            .on_key("x", move |_| selected.set(0))
    });
    let mut driver = shown(app, 12, 4);
    // A directory: a blue bar.
    let hovered = style_at(&driver, 0, 2);
    assert!(
        hovered.contains("reverse") && hovered.contains("blue"),
        "{hovered}"
    );
    // `None`: the theme's `selected`, still over the row's own colour.
    driver.press("x");
    let other = style_at(&driver, 0, 0);
    assert!(
        other.contains("reverse") && other.contains("green"),
        "{other}"
    );
    assert_eq!(style_at(&driver, 0, 2), "blue");
}

// 2. A table without a header.

#[test]
fn a_table_without_a_header_starts_with_its_rows() {
    let app = App::new(move || {
        let selected = signal(0);
        let columns = vec![
            Column::new("Name", Size::Flex(1)),
            Column::new("Size", Size::Auto).align(Align::End),
        ];
        let rows = || {
            names()
                .into_iter()
                .zip(["1.2K", "8K", "4", "96M"])
                .map(|(n, s)| vec![n, s.to_string()])
                .collect()
        };
        table_with(
            columns,
            rows,
            selected,
            TableOptions::default().header(false),
        )
    });
    let mut driver = shown(app, 16, 4);
    let screen = rows(&driver);
    assert_eq!(screen[0], "Cargo.toml  1.2K");
    assert_eq!(screen[2], "src            4");
    // A press on the top line selects the first row; on the third the third.
    click(&mut driver, 3, 2);
    assert!(style_at(&driver, 0, 2).contains("reverse"));
    click(&mut driver, 3, 0);
    assert!(style_at(&driver, 0, 0).contains("reverse"));
    assert!(!style_at(&driver, 0, 2).contains("reverse"));
}

// 3. `virtual_list` options.

#[test]
fn virtual_list_keeps_scroll_padding() {
    let app =
        App::new(move || virtual_list(|| 100, |i| format!("row {i}"), signal(0)).scroll_padding(2));
    let mut driver = shown(app, 10, 6);
    for _ in 0..5 {
        driver.press("down");
    }
    // Row 5 selected, two rows below it in view.
    let screen = rows(&driver);
    assert_eq!(screen[5], "row 7");
    assert_eq!(screen[3], "row 5");
}

#[test]
fn virtual_list_takes_a_row_style() {
    let app = App::new(|| {
        virtual_list(|| 10, |i| format!("row {i}"), signal(0))
            .row_style(|i| (i % 2 == 1).then(|| "on grey15".into()))
    });
    let driver = shown(app, 10, 4);
    assert!(style_at(&driver, 8, 1).contains("on grey15"));
    assert!(!style_at(&driver, 8, 2).contains("on grey15"));
}

#[test]
fn virtual_list_offset_reads_and_sets_the_view() {
    let app = App::new(move || {
        let first = signal(0usize);
        let selected = signal(0usize);
        column([
            virtual_list(|| 100, |i| format!("row {i}"), selected).offset(first),
            text!("{first}").fixed(1),
        ])
        .on_key("x", move |_| {
            selected.set(50);
            first.set(48);
        })
    });
    let mut driver = shown(app, 10, 5);
    driver.press("end");
    // The view followed the selection to the end.
    assert_eq!(rows(&driver)[4], "96");
    // Set from outside, it scrolls; the selection is kept in view.
    driver.press("x");
    assert_eq!(rows(&driver)[0], "row 48");
    assert_eq!(rows(&driver)[4], "48");
}

#[test]
fn a_lazy_list_asks_only_for_the_items_drawn() {
    let asked = Rc::new(Cell::new(0usize));
    let count = asked.clone();
    let app = App::new(move || {
        let count = count.clone();
        list_lazy(
            || 1_000_000,
            move |i| {
                count.set(count.get() + 1);
                format!("item {i}")
            },
            signal(0),
        )
        .trailing(|i| (i % 7).to_string())
        .marker(|i| (i == 999_999).then(|| "on red".into()))
        .on_key("q", |cx| cx.quit())
    });
    let mut driver = shown(app, 16, 5);
    driver.press("end");
    let screen = driver.screen().plain();
    assert_eq!(screen[4], " item 999999   0");
    assert!(asked.get() < 100, "asked {}", asked.get());
}

// 4. Ratatui's cell rules.

#[test]
fn a_cropped_column_cuts_without_an_ellipsis() {
    let app = App::new(|| {
        let columns = vec![
            Column::new("A", Size::Fixed(5)).overflow(Overflow::Crop),
            Column::new("B", Size::Fixed(5)),
        ];
        let rows = || vec![vec!["abcdefgh".into(), "abcdefgh".into()]];
        table_with(columns, rows, signal(0), TableOptions::default())
    });
    let driver = shown(app, 11, 2);
    assert_eq!(driver.screen().plain()[1], "abcde abcd…");
}

#[test]
fn cells_in_a_row_are_cut_at_their_edge() {
    let app = App::new(|| {
        row([
            cell(|| "[bold]oscilloscope[/]".into()).percent(40),
            cell(|| "a long trigger".into()).percent(40),
            cell(|| "60fps".into()).auto(),
        ])
        .gap(1)
    });
    let driver = shown(app, 23, 2);
    let screen = rows(&driver);
    assert_eq!(screen[0], "oscillos a long t 60fps");
    // One line: nothing wraps onto the next.
    assert_eq!(screen[1], "");
}

// 5. The screen rect of a row.

#[test]
fn row_rects_are_on_the_screen() {
    let rects = RowRects::new();
    let given = rects.clone();
    let app = App::new(move || {
        let selected = signal(0usize);
        let items = || (0..30).map(|n| format!("item {n}")).collect();
        column([
            label("header").fixed(1),
            row([
                label("|").fixed(2),
                list(items, selected)
                    .marker(|_| None)
                    .row_rects(&given)
                    .panel("List"),
            ]),
        ])
        .on_key("x", move |_| selected.set(10))
    });
    let mut driver = shown(app, 20, 8);
    // Inside the panel the list starts at x 3 (2 + its border), y 2; its
    // rows start after the marker column, at x 4.
    assert_eq!(
        rects.get(0).map(|r| (r.x, r.y, r.width, r.height)),
        Some((4, 2, 15, 1))
    );
    driver.press("x");
    // Scrolled: item 10 on the panel's last row.
    let rect = rects.get(10).unwrap();
    assert_eq!((rect.x, rect.y), (4, 6));
    assert_eq!(rects.get(0), None);
    assert_eq!(rects.shown().len(), 5);
}

#[test]
fn row_rects_of_a_bottom_to_top_multiline_list() {
    let rects = RowRects::new();
    let given = rects.clone();
    let app = App::new(move || {
        let items = || vec!["one".to_string(), "two\nlines".into()];
        list(items, signal(0))
            .multiline()
            .direction(intuituive::widgets::ListDirection::BottomToTop)
            .row_rects(&given)
    });
    let _driver = shown(app, 10, 5);
    assert_eq!(rects.get(0).map(|r| (r.y, r.height)), Some((4, 1)));
    assert_eq!(rects.get(1).map(|r| (r.y, r.height)), Some((2, 2)));
}

#[test]
fn row_rects_of_a_table() {
    let rects = RowRects::new();
    let given = rects.clone();
    let app = App::new(move || {
        let columns = vec![Column::new("Name", Size::Flex(1))];
        let rows = || names().into_iter().map(|n| vec![n]).collect();
        column([
            label("top").fixed(1),
            table_with(columns, rows, signal(0), TableOptions::default()).row_rects(&given),
        ])
    });
    let _driver = shown(app, 12, 6);
    // The header is line 1; the first row is line 2.
    assert_eq!(rects.get(0).map(|r| (r.x, r.y, r.width)), Some((0, 2, 12)));
    assert_eq!(rects.get(3).map(|r| r.y), Some(5));
}

// 6. A list sized `Size::Auto`.

#[test]
fn an_auto_list_is_as_wide_as_its_widest_row() {
    let app = App::new(|| {
        let a = || vec!["left".to_string()];
        let b = || vec!["right".to_string(), "rightmost".into()];
        row([
            list(a, signal(0)).flex(1),
            list(b, signal(0)).no_focus().auto(),
        ])
    });
    let driver = shown(app, 20, 2);
    assert_eq!(rows(&driver)[1], "           rightmost");
    assert_eq!(rows(&driver)[0], "left       right");
}

#[test]
fn an_auto_list_counts_its_chrome() {
    let app = App::new(|| {
        let b = || vec!["abc".to_string()];
        row([
            label("x").flex(1),
            list(b, signal(0))
                .symbol("> ")
                .marker(|_| None)
                .trailing(|_| "12".into())
                .auto(),
        ])
    });
    let driver = shown(app, 20, 1);
    // A marker column, the symbol, the text, a blank and the trailing
    // text: 1 + 2 + 3 + 1 + 2.
    assert_eq!(driver.screen().plain()[0], "x           > abc 12");
}

#[test]
fn a_popup_opens_under_a_row() {
    let rects = RowRects::new();
    let given = rects.clone();
    let app = App::new(move || {
        let selected = signal(2usize);
        let rects = given.clone();
        column([
            label("top").fixed(1),
            list(names, selected)
                .row_rects(&given)
                .on_key("r", move |cx| {
                    let Some(rect) = rects.get(selected.get_untracked()) else {
                        return;
                    };
                    cx.popup(
                        rect,
                        intuituive::Placement::Below,
                        Size::Fixed(6),
                        Size::Fixed(1),
                        || label("rename"),
                    );
                }),
        ])
    });
    let mut driver = shown(app, 12, 6);
    driver.press("r");
    // `src` is on line 3, the popup on line 4.
    assert_eq!(rows(&driver)[3], "src");
    assert!(
        rows(&driver)[4].starts_with("rename"),
        "{:?}",
        rows(&driver)
    );
}
