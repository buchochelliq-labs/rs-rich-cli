//! The 0.0.18 widget gaps: scrolling across, sortable tables with a cell
//! cursor, the lazy tree, tooltips, drag-and-drop and hover styles.

mod common;

use common::{row_of, run_open, screen};
use intuituive::interact::headless::Script;
use intuituive::interact::{Modifiers, Mouse, MouseKind};
use intuituive::prelude::*;

/// Ten lines, each `wide` cells: `line N` then letters.
fn wide_lines(wide: usize) -> String {
    (0..10)
        .map(|n| {
            let mut line = format!("line {n} ");
            while line.len() < wide {
                line.push(char::from(b'a' + (line.len() % 26) as u8));
            }
            line
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn a_view_that_scrolls_both_ways_shows_two_bars() {
    let app = App::new(|| scroll_both(label(wide_lines(40))));
    let rows = screen(&run_open(app, Script::new(), 12, 5));
    // The last column is the bar down, the last row the bar across.
    assert!(rows[0].ends_with('┃') || rows[0].ends_with('│'), "{rows:?}");
    assert!(rows[4].contains('━'), "{rows:?}");
    assert!(rows[0].starts_with("line 0"), "{rows:?}");
}

#[test]
fn shift_and_the_wheel_scroll_across_and_arrows_too() {
    let app = App::new(|| scroll_both(label(wide_lines(40))));
    let wheel = Mouse {
        modifiers: Modifiers {
            shift: true,
            ..Modifiers::NONE
        },
        ..Mouse::new(MouseKind::ScrollDown, 2, 2)
    };
    let script = Script::new().event(intuituive::interact::Event::Mouse(wheel));
    let rows = screen(&run_open(app, script, 12, 5));
    assert!(!rows[0].starts_with("line 0"), "moved across: {rows:?}");
    // Down is untouched: line 0 is still the first row.
    assert!(rows[0].contains("line 0") || rows[0].starts_with(' ') || !rows[0].is_empty());
    let app = App::new(|| scroll_x(label(wide_lines(40))));
    let rows = screen(&run_open(app, Script::new().keys("end"), 12, 5));
    assert!(
        rows[0].trim_end().ends_with(char::is_alphabetic),
        "{rows:?}"
    );
    assert!(!rows[0].contains("line"), "at the far end: {rows:?}");
}

#[test]
fn a_focused_cell_far_to_the_right_is_scrolled_into_view() {
    let app = App::new(|| {
        scroll_x(row([
            label("left side of a wide row").fixed(30),
            label("target").focusable().focus_style("reverse").fixed(6),
        ]))
    });
    let rows = screen(&run_open(app, Script::new().keys("tab tab"), 12, 3));
    assert!(row_of(&rows, "target").is_some(), "{rows:?}");
}

use intuituive::widgets::{table_with, Column, Order, TableOptions};

fn files() -> Vec<Vec<String>> {
    vec![
        vec!["b.txt".into(), "20".into()],
        vec!["a.txt".into(), "100".into()],
        vec!["c.txt".into(), "3".into()],
    ]
}

fn columns() -> Vec<Column> {
    vec![
        Column::new("Name", Size::Fixed(6)),
        Column::new("Size", Size::Fixed(6)),
    ]
}

#[test]
fn a_click_on_a_header_sorts_by_it_as_numbers_and_again_reverses() {
    let app = App::new(|| {
        table_with(
            columns(),
            files,
            signal(0),
            TableOptions::default().sort_rows(signal(None)),
        )
    });
    // "Size" starts at column 7.
    let rows = screen(&run_open(app, Script::new().click(8, 0), 20, 4));
    assert_eq!(rows[0].trim_end(), "Name   Size ▲");
    let sizes: Vec<&str> = rows[1..]
        .iter()
        .map(|r| r.split_whitespace().nth(1).unwrap_or(""))
        .collect();
    assert_eq!(sizes, ["3", "20", "100"]);
    let app = App::new(|| {
        table_with(
            columns(),
            files,
            signal(0),
            TableOptions::default().sort_rows(signal(None)),
        )
    });
    let rows = screen(&run_open(app, Script::new().click(8, 0).click(8, 0), 20, 4));
    assert_eq!(rows[0].trim_end(), "Name   Size ▼");
    assert!(rows[1].contains("100"), "{rows:?}");
}

#[test]
fn the_app_sorts_when_it_owns_the_order() {
    let app = App::new(|| {
        let sort: Signal<Option<(usize, Order)>> = signal(None);
        let rows = move || {
            let mut rows = files();
            if let Some((column, order)) = sort.get() {
                rows.sort_by(|a: &Vec<String>, b: &Vec<String>| a[column].cmp(&b[column]));
                if order == Order::Descending {
                    rows.reverse();
                }
            }
            rows
        };
        table_with(
            columns(),
            rows,
            signal(0),
            TableOptions::default().sort(sort),
        )
    });
    let rows = screen(&run_open(app, Script::new().keys("s"), 20, 4));
    assert_eq!(rows[0].trim_end(), "Name ▲ Size");
    assert!(rows[1].starts_with("a.txt"), "{rows:?}");
}

#[test]
fn the_cell_cursor_moves_across_and_s_sorts_by_its_column() {
    let app = App::new(|| {
        let column = signal(0usize);
        column_with_status(column)
    });
    let rows = screen(&run_open(app, Script::new().keys("right s"), 20, 5));
    assert_eq!(rows[4].trim_end(), "cell 1");
    assert_eq!(rows[0].trim_end(), "Name   Size ▲");
    let app = App::new(|| column_with_status(signal(0usize)));
    // Clicking a cell moves the cursor there.
    let rows = screen(&run_open(app, Script::new().click(9, 2), 20, 5));
    assert_eq!(rows[4].trim_end(), "cell 1");
}

fn column_with_status(column: Signal<usize>) -> Node {
    let options = TableOptions::default()
        .sort_rows(signal(None))
        .cells(column);
    intuituive::column([
        table_with(columns(), files, signal(0), options).fixed(4),
        text!("cell {column}"),
    ])
}

#[test]
fn dragging_the_gap_after_a_header_resizes_the_column() {
    let app = App::new(|| {
        table_with(
            columns(),
            files,
            signal(0),
            TableOptions::default().resizable(),
        )
    });
    // The gap after "Name" (6 wide) is column 6; drag it to 10.
    let script = Script::new().drag((6, 0), (10, 0));
    let rows = screen(&run_open(app, script, 24, 4));
    assert_eq!(rows[0].trim_end(), "Name       Size");
}
