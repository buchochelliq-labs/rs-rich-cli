//! Tables that follow state (round 6): options and columns from signals,
//! a marker column and per-row symbols, a keyed cursor, the scroll across
//! read and set, widths in percent, set from keys, and dropped by
//! priority, and per-row heights in a virtual table.

use std::cell::Cell;
use std::rc::Rc;

use intuituive::multi_select;
use intuituive::prelude::*;
use intuituive::widgets::{
    table_with, virtual_table_with, Column, HighlightLayer, Order, TableLayout, TableOptions,
};

fn plain(driver: &intuituive::Driver) -> Vec<String> {
    driver
        .screen()
        .plain()
        .iter()
        .map(|row| row.trim_end().to_string())
        .collect()
}

fn wide_columns(n: usize) -> Vec<Column> {
    (0..n)
        .map(|i| Column::new(&format!("col{i}"), Size::Auto))
        .collect()
}

fn wide_row(r: usize) -> Vec<String> {
    (0..6).map(|c| format!("r{r}c{c}")).collect()
}

#[test]
fn new_columns_keep_the_scroll_across_and_the_cursor() {
    let app = App::new(|| {
        let upper = signal(false);
        let (cursor, first) = (signal(0usize), signal(0usize));
        let columns = move || {
            (0..6)
                .map(|i| {
                    let title = if upper.get() { "COL" } else { "col" };
                    Column::new(&format!("{title}{i}"), Size::Auto)
                })
                .collect()
        };
        let options = TableOptions::default()
            .columns_with(columns)
            .cells(cursor)
            .freeze_columns(1)
            .offset_x(first);
        let table = virtual_table_with(vec![], || 10, wide_row, signal(0), options);
        column([table.fixed(2), text!("{cursor} {first}")]).on_key("u", move |_| upper.set(true))
    });
    let mut driver = app.driver(20, 3);
    driver.press("right right right right");
    let before = plain(&driver);
    assert_eq!(before[2], "4 2");
    driver.press("u");
    let after = plain(&driver);
    // The same window across, the new titles, the cursor where it was.
    assert_eq!(after[0], before[0].to_uppercase());
    assert_eq!(after[1], before[1]);
    assert_eq!(after[2], "4 2");
}

#[test]
fn shown_columns_hide_and_move_while_the_cursor_keeps_its_column() {
    let app = App::new(|| {
        let shown = signal(vec![0, 1, 2, 3]);
        let cursor = signal(0usize);
        let columns = ["a", "b", "c", "d"]
            .map(|t| Column::new(t, Size::Auto))
            .to_vec();
        let rows = || vec![vec!["1".into(), "2".into(), "3".into(), "4".into()]];
        let options = TableOptions::default()
            .cells(cursor)
            .columns_shown(move || shown.get());
        let table = table_with(columns, rows, signal(0), options);
        column([table.fixed(2), text!("{cursor}")]).on_key("x", move |_| shown.set(vec![3, 1, 0]))
    });
    let mut driver = app.driver(12, 3);
    driver.press("x");
    assert_eq!(plain(&driver)[0], "d b a");
    // The cursor is on `a`, shown last: ← goes to `b`, then `d`.
    assert_eq!(plain(&driver)[2], "0");
    driver.press("left");
    assert_eq!(plain(&driver)[2], "1");
    driver.press("left left");
    assert_eq!(plain(&driver)[2], "3");
}

#[test]
fn the_frozen_count_and_wrapping_follow_signals() {
    let app = App::new(|| {
        let (frozen, wrap) = (signal(0usize), signal(false));
        let options = TableOptions::default()
            .freeze_columns_with(move || frozen.get())
            .multiline_when(move || wrap.get());
        let row = |r: usize| wide_row(r);
        virtual_table_with(wide_columns(6), || 10, row, signal(0), options)
            .on_key("f", move |_| frozen.set(1))
            .on_key("w", move |_| wrap.set(true))
    });
    let mut driver = app.driver(20, 3);
    driver.press("right right");
    assert_eq!(plain(&driver)[0], "‹col2 col3 col4 col5");
    driver.press("f");
    assert!(
        plain(&driver)[0].starts_with("col0 ‹"),
        "{:?}",
        plain(&driver)
    );
}

#[test]
fn widths_from_a_signal_resize_and_drags_write_them() {
    use intuituive::interact::{Button, Event, Mouse, MouseKind};
    let seen = Rc::new(Cell::new(None));
    let read = seen.clone();
    let app = App::new(move || {
        let widths = signal(vec![None, None]);
        let columns = vec![
            Column::new("Name", Size::Auto),
            Column::new("N", Size::Auto),
        ];
        let rows = || vec![vec!["notes".into(), "1".into()]];
        let options = TableOptions::default().widths(widths).resizable();
        let read = read.clone();
        column([
            table_with(columns, rows, signal(0), options).fixed(2),
            text(move || {
                read.set(widths.get()[0]);
                String::new()
            }),
        ])
        .on_key(">", move |_| widths.update(|w| w[0] = Some(9)))
    });
    let mut driver = app.driver(20, 3);
    driver.press(">");
    assert_eq!(plain(&driver)[1], "notes     1");
    assert_eq!(seen.get(), Some(9));
    // Drag the gap after `Name` (at 9) to 7.
    driver.event(Event::Mouse(Mouse::new(
        MouseKind::Down(Button::Left),
        9,
        0,
    )));
    driver.event(Event::Mouse(Mouse::new(
        MouseKind::Drag(Button::Left),
        7,
        0,
    )));
    driver.event(Event::Mouse(Mouse::new(MouseKind::Up(Button::Left), 7, 0)));
    driver.settle();
    assert_eq!(seen.get(), Some(7));
    assert_eq!(plain(&driver)[1], "notes   1");
}

#[test]
fn a_column_capped_in_percent_holds_while_scrolling_across() {
    let app = App::new(|| {
        let mut columns = wide_columns(3);
        columns[0] = Column::new("text", Size::Auto).max_percent(30);
        let row = |_| vec!["a very long cell indeed".into(), "b".into(), "c".into()];
        virtual_table_with(
            columns,
            || 3,
            row,
            signal(0),
            TableOptions::default().scroll_x(),
        )
    });
    let mut driver = app.driver(20, 2);
    driver.settle();
    // 30% of 20 cells is six.
    assert_eq!(plain(&driver)[1], "a ver… b    c");
}

#[test]
fn priorities_drop_whole_columns_and_keep_a_flexible_one() {
    let draw = |width: u16| {
        let app = App::new(|| {
            let columns = vec![
                Column::new("PID", Size::Fixed(4)),
                Column::new("Name", Size::Flex(1)).min(8).priority(9),
                Column::new("CPU%", Size::Fixed(5)).priority(5),
                Column::new("R/s", Size::Fixed(4)).priority(1),
                Column::new("W/s", Size::Fixed(4)).priority(1),
            ];
            let rows = || {
                vec![["1", "systemd", "0.3", "1K", "2K"]
                    .map(String::from)
                    .to_vec()]
            };
            table_with(columns, rows, signal(0), TableOptions::default())
        });
        let mut driver = app.driver(width, 2);
        driver.settle();
        plain(&driver)
    };
    assert_eq!(draw(40)[0], "PID  Name                CPU%  R/s  W/s");
    // W/s (the rightmost of the lowest) goes first, then R/s; Name keeps
    // its eight cells.
    assert_eq!(draw(25)[0], "PID  Name      CPU%  R/s");
    assert_eq!(draw(19)[1], "1    systemd  0.3");
    // Name has a priority too: it goes last, before PID would squeeze.
    assert_eq!(draw(12), ["PID", "1"]);
}

#[test]
fn selection_markers_and_a_keyed_cursor_on_a_table() {
    let app = App::new(|| {
        let names = signal(vec!["cherry", "apple", "banana"]);
        let cursor = signal(0usize);
        let picked = multi_select::<String>()
            .over(move || names.get().iter().map(|n| n.to_string()).collect())
            .range_from(cursor);
        picked.select("cherry".to_string());
        let rows = move || names.get().iter().map(|n| vec![n.to_string()]).collect();
        let columns = vec![Column::new("Name", Size::Auto)];
        table_with(columns, rows, cursor, TableOptions::default())
            .selection_markers(picked, move |i| names.with(|n| n[i].to_string()))
            .cursor_by(move |i| names.with(|n| n.get(i).copied()))
            .on_key("s", move |_| names.update(|n| n.sort()))
    });
    let mut driver = app.driver(10, 4);
    driver.settle();
    assert_eq!(plain(&driver)[0], " Name");
    driver.press("s");
    // `cherry`, sorted last, keeps both the cursor and its mark.
    let styled = intuituive::testing::styled(&driver);
    let last = styled.lines().nth(3).unwrap();
    assert!(
        last.starts_with("[on yellow] [/][reverse]cherry"),
        "{styled}"
    );
}

#[test]
fn a_symbol_per_row_and_the_highlight_under_the_cells() {
    let app = App::new(|| {
        let columns = vec![Column::new("Track", Size::Auto)];
        let rows = || vec![vec!["[red]one[/]".into()], vec!["two".into()]];
        let options = TableOptions::default()
            .symbol_for(2, |i, selected| match (i, selected) {
                (_, true) => Some("> ".into()),
                (1, false) => Some("* ".into()),
                _ => None,
            })
            .highlight_layer(HighlightLayer::Under);
        table_with(columns, rows, signal(0), options).theme_styles([("selected", "on blue")])
    });
    let mut driver = app.driver(10, 3);
    driver.settle();
    assert_eq!(plain(&driver), ["  Track", "> one", "* two"]);
    let styled = intuituive::testing::styled(&driver);
    assert!(styled.contains("[red on blue]one"), "{styled}");
}

#[test]
fn rows_of_their_own_heights_in_a_virtual_table() {
    let asked = Rc::new(Cell::new(0usize));
    let count = asked.clone();
    let app = App::new(move || {
        let count = count.clone();
        let row = move |i: usize| {
            count.set(count.get() + 1);
            vec![format!("row {i} is long enough to wrap")]
        };
        let options =
            TableOptions::default()
                .multiline()
                .row_height_for(|i| if i % 2 == 0 { 1 } else { 2 });
        let columns = vec![Column::new("Text", Size::Flex(1))];
        virtual_table_with(columns, || 1_000_000, row, signal(0), options)
            .on_key("q", |cx| cx.quit())
    });
    let mut driver = app.driver(16, 6);
    driver.settle();
    assert_eq!(
        plain(&driver),
        [
            "Text",
            "row 0 is long",
            "row 1 is long",
            "enough to wrap",
            "row 2 is long",
            ""
        ]
    );
    // Only the rows near the view were asked for, not a million.
    assert!(asked.get() < 100, "{}", asked.get());
}

#[test]
fn the_layout_says_what_was_drawn() {
    let layout = TableLayout::new();
    let shown = layout.clone();
    let app = App::new(move || {
        let options = TableOptions::default().scroll_x().layout(&shown);
        virtual_table_with(wide_columns(6), || 50, wide_row, signal(0), options)
    });
    let mut driver = app.driver(20, 5);
    driver.press("right");
    assert_eq!(layout.columns_in_view(), [1, 2, 3, 4]);
    assert_eq!(layout.width_of(1), Some(4));
    assert_eq!(layout.width_of(0), None);
    assert_eq!(layout.rows_in_view(), 4);
}

#[test]
fn a_sort_indicator_on_the_sorted_column() {
    let app = App::new(|| {
        let sort = signal(Some((1, Order::Ascending)));
        let options = TableOptions::default()
            .sort_rows(sort)
            .sort_indicator(|title, _| format!("{title}*"));
        let columns = vec![Column::new("a", Size::Auto), Column::new("b", Size::Auto)];
        let rows = || vec![vec!["x".into(), "2".into()], vec!["y".into(), "1".into()]];
        table_with(columns, rows, signal(0), options)
    });
    let mut driver = app.driver(10, 3);
    driver.settle();
    assert_eq!(plain(&driver), ["a b*", "y 1", "x 2"]);
}
