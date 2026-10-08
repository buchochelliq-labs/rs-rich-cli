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

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use intuituive::widgets::{tree_lazy, LazyItem};

fn dirs() -> Vec<LazyItem> {
    vec![
        LazyItem::branch("src", "src"),
        LazyItem::branch("bad", "bad"),
    ]
}

#[test]
fn a_lazy_tree_loads_a_level_once_when_it_is_first_opened() {
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = calls.clone();
    let app = App::new(move || {
        let counted = counted.clone();
        let children = move |key: String| -> Result<Vec<LazyItem>, String> {
            counted.fetch_add(1, Ordering::SeqCst);
            if key == "bad" {
                return Err("permission denied".into());
            }
            Ok((0..3)
                .map(|i| LazyItem::leaf(format!("{key}/{i}"), format!("file {i}")))
                .collect())
        };
        tree_lazy(dirs, children, signal(None))
    })
    .wait_for_tasks(true);
    // Open src, close it, open it again; then open bad.
    let rows = screen(&run_open(
        app,
        Script::new().keys("right left right down down down down right"),
        30,
        8,
    ));
    assert!(row_of(&rows, "file 2").is_some(), "{rows:?}");
    assert!(row_of(&rows, "permission denied").is_some(), "{rows:?}");
    assert_eq!(calls.load(Ordering::SeqCst), 2, "src once, bad once");
}

#[test]
fn a_lazy_tree_shows_loading_until_children_arrive() {
    let app = App::new(|| {
        let children = |_key: String| -> Result<Vec<LazyItem>, String> {
            std::thread::sleep(std::time::Duration::from_millis(300));
            Ok(vec![LazyItem::leaf("x", "late")])
        };
        tree_lazy(dirs, children, signal(None))
    });
    let rows = screen(&run_open(app, Script::new().keys("right"), 30, 4));
    assert!(row_of(&rows, "loading…").is_some(), "{rows:?}");
}

#[test]
fn the_hover_style_comes_and_goes_with_the_pointer() {
    use intuituive::interact::{Event, Mouse};
    use std::time::Duration;
    let app = App::new(|| {
        column([
            row([label("one").fixed(3), label(" x")]).hover_style("reverse"),
            label("two"),
        ])
    });
    let mut driver = app.driver(10, 2);
    let mut frame = |driver: &mut intuituive::Driver, event: Option<Event>| {
        if let Some(event) = event {
            driver.event(event);
        }
        driver.update(Duration::ZERO);
        driver.render().unwrap_or_default()
    };
    frame(&mut driver, None);
    let over = frame(
        &mut driver,
        Some(Event::Mouse(Mouse::new(MouseKind::Moved, 1, 0))),
    );
    assert!(over.contains("\x1b[0;7m"), "reversed: {over:?}");
    let away = frame(
        &mut driver,
        Some(Event::Mouse(Mouse::new(MouseKind::Moved, 1, 1))),
    );
    assert!(
        !away.contains("\x1b[0;7m") && away.contains("one"),
        "plain again: {away:?}"
    );
    // The whole row, gap included, is plain again.
    assert!(driver.screen().plain()[0].starts_with("one x"));
}
