//! A table's options (gap batch B2): a footer, aligned columns, column
//! spacing, a highlight symbol, row styles, multi-line rows, a column
//! highlight and an empty selection.

use intuituive::prelude::*;
use intuituive::widgets::{
    table_optional, table_with, virtual_table_with, Column, HighlightSpacing, TableOptions,
};
use intuituive::{assert_screen, assert_screen_styled};

fn food() -> Vec<Vec<String>> {
    [
        ["Apple", "1", "95 kcal"],
        ["Bread", "2", "160 kcal"],
        ["Cheese", "12", "113 kcal"],
    ]
    .iter()
    .map(|row| row.iter().map(|cell| cell.to_string()).collect())
    .collect()
}

fn columns() -> Vec<Column> {
    vec![
        Column::new("Food", Size::Auto),
        Column::new("Qty", Size::Fixed(4)).align(Align::End),
        Column::new("Energy", Size::Auto).align(Align::End),
    ]
}

fn rows(driver: &intuituive::Driver) -> Vec<String> {
    driver
        .screen()
        .plain()
        .iter()
        .map(|row| row.trim_end().to_string())
        .collect()
}

fn draw(options: TableOptions, width: u16, height: u16) -> intuituive::Driver {
    draw_with(move || options, width, height)
}

/// A table with options made while the app is built (they may make signals).
fn draw_with(
    options: impl FnOnce() -> TableOptions + 'static,
    width: u16,
    height: u16,
) -> intuituive::Driver {
    let app = App::new(move || table_with(columns(), food, signal(0), options()));
    let mut driver = app.driver(width, height);
    driver.settle();
    driver
}

#[test]
fn table_footer() {
    let options =
        TableOptions::default().footer(|| vec!["Total".into(), "15".into(), "368 kcal".into()]);
    let driver = draw(options, 24, 7);
    assert_screen!(driver, "table_footer");
    let screen = rows(&driver);
    // The footer stays at the bottom, the rows above it.
    assert_eq!(screen[6], "Total    15 368 kcal");
    assert_eq!(screen[3], "Cheese   12 113 kcal");
    assert_eq!(screen[4], "");
}

#[test]
fn table_aligned() {
    let driver = draw(TableOptions::default(), 24, 4);
    assert_screen!(driver, "table_aligned");
    let screen = rows(&driver);
    assert_eq!(screen[0], "Food    Qty   Energy");
    assert_eq!(screen[1], "Apple     1  95 kcal");
    assert_eq!(screen[2], "Bread     2 160 kcal");
}

#[test]
fn a_column_keeps_to_its_least_and_most() {
    let columns = vec![
        Column::new("A", Size::Flex(1)).max(3),
        Column::new("B", Size::Fixed(1)).min(5),
    ];
    let app = App::new(move || {
        table_with(
            columns.clone(),
            || vec![vec!["xxxxxx".into(), "y".into()]],
            signal(0),
            TableOptions::default(),
        )
    });
    let mut driver = app.driver(20, 2);
    driver.settle();
    assert_eq!(rows(&driver)[1], "xx… y");
}

#[test]
fn table_spacing() {
    let driver = draw(TableOptions::default().column_spacing(3), 30, 4);
    assert_screen!(driver, "table_spacing");
    assert_eq!(rows(&driver)[1], "Apple       1    95 kcal");
}

#[test]
fn table_symbol() {
    let options = TableOptions::default()
        .symbol("» ")
        .highlight_spacing(HighlightSpacing::Always);
    let driver = draw(options, 26, 4);
    assert_screen!(driver, "table_symbol");
    let screen = rows(&driver);
    assert_eq!(screen[0], "  Food    Qty   Energy");
    assert_eq!(screen[1], "» Apple     1  95 kcal");
    assert_eq!(screen[2], "  Bread     2 160 kcal");
}

#[test]
fn table_zebra() {
    let options = TableOptions::default().row_style(|i| (i % 2 == 1).then(|| "on blue".into()));
    let driver = draw(options, 22, 4);
    assert_screen_styled!(driver, "table_zebra");
    let styled = intuituive::testing::styled(&driver);
    // The whole row, gaps and all.
    assert!(
        styled
            .lines()
            .nth(2)
            .unwrap()
            .starts_with("[on blue]Bread     2 160 kcal  [/]"),
        "{styled}"
    );
}

#[test]
fn table_multiline() {
    let rows_of = || {
        vec![
            vec![
                "Ratatui".into(),
                "A library\nfor terminal\nuser\ninterfaces".into(),
            ],
            vec!["intuiTUIve".into(), "A framework".into()],
        ]
    };
    let columns = vec![
        Column::new("Name", Size::Fixed(10)),
        Column::new("About", Size::Flex(1)),
    ];
    let app = App::new(move || {
        table_with(
            columns.clone(),
            rows_of,
            signal(0),
            TableOptions::default().multiline().header_gap(1),
        )
    });
    let mut driver = app.driver(26, 8);
    driver.settle();
    assert_screen!(driver, "table_multiline");
    let screen = rows(&driver);
    assert_eq!(screen[0], "Name       About");
    assert_eq!(screen[1], "");
    assert_eq!(screen[2], "Ratatui    A library");
    assert_eq!(screen[5], "           interfaces");
    assert_eq!(screen[6], "intuiTUIve A framework");
}

#[test]
fn multi_line_rows_move_a_row_at_a_time() {
    let tall = || {
        (0..10)
            .map(|n| vec![format!("row {n}"), format!("a\nb\nc{n}")])
            .collect()
    };
    let columns = vec![
        Column::new("Name", Size::Fixed(6)),
        Column::new("Lines", Size::Flex(1)),
    ];
    let app = App::new(move || {
        let selected = signal(0usize);
        column([
            table_with(
                columns.clone(),
                tall,
                selected,
                TableOptions::default().multiline(),
            ),
            text!("{selected}").fixed(1),
        ])
    });
    let mut driver = app.driver(16, 8);
    // Two rows of three lines fit under the header: ↓ moves a row.
    driver.press("down");
    assert_eq!(rows(&driver)[7], "1");
    // PgDn moves a screenful of rows: two.
    driver.press("pagedown");
    assert_eq!(rows(&driver)[7], "3");
    let screen = rows(&driver);
    assert_eq!(screen[1], "row 2  a");
    assert_eq!(screen[4], "row 3  a");
}

#[test]
fn a_virtual_table_takes_a_row_height() {
    let columns = vec![Column::new("N", Size::Flex(1))];
    let app = App::new(move || {
        virtual_table_with(
            columns.clone(),
            || 1_000_000,
            |i| vec![format!("row {i}")],
            signal(0),
            TableOptions::default().row_height(2),
        )
    });
    let mut driver = app.driver(12, 5);
    driver.press("end");
    let screen = rows(&driver);
    assert_eq!(screen[1], "row 999998");
    assert_eq!(screen[3], "row 999999");
}

#[test]
fn table_column_highlight() {
    let options = || TableOptions::default().cells(signal(1)).column_highlight();
    let driver = draw_with(options, 22, 4);
    assert_screen_styled!(driver, "table_column_highlight");
    let styled = intuituive::testing::styled(&driver);
    // The column is bold down the rows, the cell cursor over it.
    assert!(
        styled.lines().nth(2).unwrap().contains("[bold]   2[/]"),
        "{styled}"
    );
}

#[test]
fn table_none() {
    let app = App::new(|| {
        let selected = signal(None);
        column([
            table_optional(columns(), food, selected, TableOptions::default()),
            text(move || format!("{:?}", selected.get())).fixed(1),
        ])
    });
    let mut driver = app.driver(22, 5);
    driver.settle();
    assert_screen!(driver, "table_none");
    assert!(!intuituive::testing::styled(&driver).contains("reverse"));
    assert_eq!(rows(&driver)[4], "None");
    driver.press("down");
    assert_eq!(rows(&driver)[4], "Some(0)");
    assert!(intuituive::testing::styled(&driver).contains("reverse"));
}

#[test]
fn clicks_select_the_row_drawn_there() {
    use intuituive::interact::{Button, Event, Mouse, MouseKind};
    let app = App::new(|| {
        let selected = signal(0usize);
        column([
            table_with(
                columns(),
                food,
                selected,
                TableOptions::default().header_gap(1).multiline(),
            ),
            text!("{selected}").fixed(1),
        ])
    });
    let mut driver = app.driver(22, 6);
    driver.settle();
    // The header, a gap, then the rows: line 3 is the second row.
    driver.event(Event::Mouse(Mouse::new(
        MouseKind::Down(Button::Left),
        2,
        3,
    )));
    driver.settle();
    assert_eq!(rows(&driver)[5], "1");
}
