//! Tables wider than their view (#717): with `TableOptions::scroll_x`,
//! columns keep their natural widths and the table scrolls across, the
//! header over its columns, frozen columns in place and the cell cursor
//! followed.

use std::time::Duration;

use intuituive::interact::headless::{Headless, Script};
use intuituive::interact::{Event, Modifiers, Mouse, MouseKind};
use intuituive::prelude::*;
use intuituive::widgets::{table, table_with, virtual_table_with, Column, TableOptions};

/// Natural widths 2, 12, 8, 14 and 16: `id` at 0, `name` 3, `city` 16,
/// `country` 25 and `notes` 40, 56 cells in all.
fn columns() -> Vec<Column> {
    ["id", "name", "city", "country", "notes"]
        .iter()
        .map(|title| Column::new(title, Size::Auto))
        .collect()
}

fn people() -> Vec<Vec<String>> {
    let row = |cells: [&str; 5]| cells.iter().map(|c| c.to_string()).collect();
    vec![
        row([
            "1",
            "Ada Lovelace",
            "London",
            "United Kingdom",
            "first programmer",
        ]),
        row(["2", "Grace Hopper", "New York", "United States", "COBOL"]),
    ]
}

/// The screen after `keys`, 24 columns by 4 rows: the table, and under it
/// a line showing the cell cursor's column.
fn shown(options: impl Fn(Signal<usize>) -> TableOptions + 'static, keys: &str) -> Vec<String> {
    let app = App::new(move || {
        let column = signal(0usize);
        intuituive::column([
            table_with(columns(), people, signal(0), options(column)).fixed(3),
            text!("cell {column}"),
        ])
        .on_key("q", |cx| cx.quit())
    });
    let mut keys: Vec<&str> = keys.split_whitespace().collect();
    keys.push("q");
    app.render_with(&keys, 24, 4).unwrap()
}

fn scrolling(_: Signal<usize>) -> TableOptions {
    TableOptions::default().scroll_x()
}

fn frozen(_: Signal<usize>) -> TableOptions {
    TableOptions::default().freeze_columns(1)
}

fn with_cursor(column: Signal<usize>) -> TableOptions {
    TableOptions::default().scroll_x().cells(column)
}

/// The cell `needle` starts at in `line`.
fn at(line: &str, needle: &str) -> usize {
    let start = line
        .find(needle)
        .unwrap_or_else(|| panic!("no {needle:?} in {line:?}"));
    line[..start].chars().count()
}

#[test]
fn a_wide_table_keeps_its_columns_and_says_there_are_more() {
    let rows = shown(scrolling, "");
    assert!(rows[0].starts_with("id name         city"), "{rows:?}");
    assert!(rows[0].ends_with('›'), "{rows:?}");
    assert!(rows[1].starts_with("1  Ada Lovelace London"), "{rows:?}");
    assert_eq!(at(&rows[0], "city"), at(&rows[1], "London"));
}

#[test]
fn right_and_left_scroll_a_column_at_a_time() {
    let rows = shown(scrolling, "right");
    assert!(rows[0].starts_with("‹name         city"), "{rows:?}");
    assert!(rows[0].ends_with('›'), "{rows:?}");
    assert!(rows[1].starts_with(" Ada Lovelace London"), "{rows:?}");
    assert_eq!(at(&rows[0], "city"), at(&rows[1], "London"));
    assert_eq!(at(&rows[0], "city"), at(&rows[2], "New York"));
    // h and l too, and back to the start.
    let rows = shown(scrolling, "l l h h");
    assert!(rows[0].starts_with("id name"), "{rows:?}");
}

#[test]
fn shift_home_and_end_go_to_the_ends() {
    let rows = shown(scrolling, "shift+end");
    assert!(rows[0].starts_with("‹notes"), "{rows:?}");
    assert!(!rows[0].contains('›'), "{rows:?}");
    assert!(rows[1].starts_with(" first programmer"), "{rows:?}");
    // Right at the end changes nothing; Shift+Home goes back.
    let rows = shown(scrolling, "shift+end right");
    assert!(rows[0].starts_with("‹notes"), "{rows:?}");
    let rows = shown(scrolling, "shift+end shift+home");
    assert!(rows[0].starts_with("id name"), "{rows:?}");
}

#[test]
fn frozen_columns_stay_in_place() {
    let rows = shown(frozen, "right");
    // `id` stays, `name` scrolled away, `‹` before `city`.
    assert!(rows[0].starts_with("id ‹city"), "{rows:?}");
    assert!(rows[1].starts_with("1   London"), "{rows:?}");
    assert_eq!(at(&rows[0], "country"), at(&rows[1], "United"));
    let rows = shown(frozen, "shift+end");
    assert!(rows[0].starts_with("id ‹notes"), "{rows:?}");
    assert_eq!(at(&rows[0], "notes"), at(&rows[1], "first programmer"));
    // Left does not scroll the frozen column away.
    let rows = shown(frozen, "left");
    assert!(rows[0].starts_with("id name"), "{rows:?}");
}

#[test]
fn the_view_follows_the_cell_cursor() {
    // Right moves the cursor to `country` (25..39): `city` leads.
    let rows = shown(with_cursor, "right right right");
    assert_eq!(rows[3].trim_end(), "cell 3");
    assert!(rows[0].starts_with("‹city     country"), "{rows:?}");
    // And back to `id`.
    let rows = shown(with_cursor, "right right right left left left");
    assert_eq!(rows[3].trim_end(), "cell 0");
    assert!(rows[0].starts_with("id name"), "{rows:?}");
    // Shift+Right scrolls without moving the cursor.
    let rows = shown(with_cursor, "shift+right");
    assert_eq!(rows[3].trim_end(), "cell 0");
    assert!(rows[0].starts_with("‹name"), "{rows:?}");
}

#[test]
fn a_table_that_fits_lays_out_as_before() {
    let render = |scroll: bool| {
        App::new(move || {
            let options = TableOptions {
                scroll_x: scroll,
                ..TableOptions::default()
            };
            table_with(columns(), people, signal(0), options).on_key("q", |cx| cx.quit())
        })
        .render_with(&["right", "q"], 80, 3)
        .unwrap()
    };
    let scrolled = render(true);
    assert_eq!(scrolled, render(false));
    assert!(!scrolled[0].contains('›'), "{scrolled:?}");
    // And a plain table is cut at its view's edge as before.
    let rows = App::new(|| table(columns(), people, signal(0)).on_key("q", |cx| cx.quit()))
        .render_with(&["right", "q"], 24, 3)
        .unwrap();
    assert_eq!(rows[0], "id name         city    ");
}

/// The first row after `script`, on a table 20 cells wide.
fn first_row(app: App, script: Script) -> String {
    let mut backend = Headless::new(script, 20, 3);
    let record = backend.record();
    let _ = app.run_on(&mut backend);
    let record = record.borrow();
    record
        .last_frame()
        .lines()
        .next()
        .unwrap_or_default()
        .to_string()
}

#[test]
fn a_squeezed_column_can_be_dragged_wider() {
    // `A` is squeezed to nothing by the wide title after it; the gap after
    // it, at the left edge, is still its right edge.
    let app = App::new(|| {
        let columns = vec![
            Column::new("A", Size::Flex(1)),
            Column::new("Bbbbbbbbbbbbbbbbbbbb", Size::Auto),
        ];
        let rows = || vec![vec!["a".to_string(), "b".to_string()]];
        table_with(
            columns,
            rows,
            signal(0),
            TableOptions::default().resizable(),
        )
    });
    let row = first_row(app, Script::new().drag((0, 0), (5, 0)));
    assert!(row.starts_with('A'), "{row:?}");
}

#[test]
fn a_column_scrolled_out_of_view_has_no_edge() {
    // Scrolled across, the left edge is `‹`, not the edge of `id`, which
    // is out of view: dragging it resizes nothing.
    let app = App::new(|| {
        let options = TableOptions::default().scroll_x().resizable();
        table_with(columns(), people, signal(0), options)
    });
    let script = Script::new()
        .keys("right")
        .drag((0, 0), (5, 0))
        .keys("shift+home");
    let row = first_row(app, script);
    assert!(row.starts_with("id name"), "{row:?}");
}

#[test]
fn shift_and_the_wheel_scroll_across() {
    let app = App::new(|| {
        table_with(
            columns(),
            people,
            signal(0),
            TableOptions::default().scroll_x(),
        )
    });
    let mut driver = app.driver(24, 3);
    driver.update(Duration::ZERO);
    driver.render();
    let mut wheel = Mouse::new(MouseKind::ScrollDown, 4, 1);
    wheel.modifiers = Modifiers {
        shift: true,
        ..Modifiers::NONE
    };
    driver.event(Event::Mouse(wheel));
    driver.event(Event::Mouse(wheel));
    driver.update(Duration::ZERO);
    driver.render();
    let rows = driver.screen().plain();
    assert!(rows[0].starts_with("‹city"), "{rows:?}");
    // The selection did not move.
    assert!(rows[1].starts_with(" London"), "{rows:?}");
}

#[test]
fn a_virtual_table_scrolls_across_too() {
    // Columns 13 cells wide: the first, and the last after it.
    let rows = virtual_wide()
        .render_with(&["end", "shift+end", "q"], 30, 4)
        .unwrap();
    assert!(rows[0].starts_with("column0       ‹column7"), "{rows:?}");
    assert!(
        rows[3].starts_with("r99999c0-cell  r99999c7-cell"),
        "{rows:?}"
    );
    assert_eq!(at(&rows[0], "column7"), at(&rows[3], "r99999c7"));
    let rows = virtual_wide().render_with(&["right", "q"], 30, 4).unwrap();
    assert!(rows[0].starts_with("column0   ‹column2"), "{rows:?}");
    assert!(rows[0].ends_with('›'), "{rows:?}");
}

fn virtual_wide() -> App {
    App::new(|| {
        let columns = (0..8)
            .map(|i| Column::new(&format!("column{i}"), Size::Auto))
            .collect();
        let row = |r: usize| (0..8).map(|c| format!("r{r}c{c}-cell")).collect();
        let options = TableOptions::default().freeze_columns(1);
        virtual_table_with(columns, || 100_000, row, signal(0), options).on_key("q", |cx| cx.quit())
    })
}
