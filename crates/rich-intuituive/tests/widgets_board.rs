//! The board: cards moved within and between columns by keys, by grab
//! and carry, and by the mouse, each move handed to the app; paging on a
//! narrow terminal; what a screen reader is told.

use std::cell::Cell;
use std::rc::Rc;

use intuituive::interact::{Button, Event, Mouse, MouseKind};
use intuituive::prelude::*;
use intuituive::{BoardMove, Driver, board};

type Cards = Signal<Vec<Vec<String>>>;

/// A board of Todo, Doing and Done over `cards`, `width` wide; and the
/// cards' signal.
fn app(cards: &[&[&str]], width: u16) -> (Driver, Cards) {
    let out: Rc<Cell<Option<Cards>>> = Rc::default();
    let set = out.clone();
    let start: Vec<Vec<String>> = cards
        .iter()
        .map(|column| column.iter().map(|c| c.to_string()).collect())
        .collect();
    let app = App::new(move || {
        let cards = signal(start.clone());
        set.set(Some(cards));
        board(
            || vec!["Todo".into(), "Doing".into(), "Done".into()],
            move || cards.get(),
        )
        .on_board_move(move |step: BoardMove, _| cards.update(|c| step.apply(c)))
    });
    let mut driver = app.driver(width, 12);
    driver.settle();
    (driver, out.get().unwrap())
}

fn columns(cards: Cards) -> Vec<Vec<String>> {
    cards.get_untracked()
}

#[test]
fn columns_side_by_side_each_a_stack_of_boxed_cards() {
    let (driver, _) = app(&[&["Write", "Test"], &["Review"], &[]], 60);
    let rows: Vec<String> = driver
        .screen()
        .plain()
        .iter()
        .map(|r| r.trim_end().to_string())
        .collect();
    assert_eq!(rows[0], "Todo (2)            Doing (1)           Done (0)");
    assert_eq!(rows[1], "╭─────────────────╮ ╭─────────────────╮");
    assert_eq!(rows[2], "│ Write           │ │ Review          │");
    assert_eq!(rows[3], "╰─────────────────╯ ╰─────────────────╯");
    assert_eq!(rows[5].trim_end(), "│ Test            │");
}

#[test]
fn alt_arrows_and_h_j_k_l_move_a_card_within_and_between_columns() {
    let (mut driver, cards) = app(&[&["Write", "Test"], &["Review"], &[]], 60);
    driver.take_announcements();
    // Write to Doing, at its place there (the top).
    driver.press("alt+right");
    assert_eq!(
        columns(cards),
        [vec!["Test"], vec!["Write", "Review"], vec![]]
    );
    let said: Vec<String> = driver
        .take_announcements()
        .into_iter()
        .map(|a| a.text)
        .collect();
    assert_eq!(said, ["Moved to Doing, 1 of 2"]);
    // Down a place in Doing, then on to Done.
    driver.press("J L");
    assert_eq!(
        columns(cards),
        [vec!["Test"], vec!["Review"], vec!["Write"]]
    );
    // The selection followed it.
    assert_eq!(
        driver.accessibility()[0].describe(),
        "Board, grid: Done, card 1 of 1: Write"
    );
    // ← and → only select.
    driver.press("left left");
    assert_eq!(
        driver.accessibility()[0].describe(),
        "Board, grid: Todo, card 1 of 1: Test"
    );
}

#[test]
fn space_grabs_the_arrows_carry_and_esc_puts_it_back() {
    let (mut driver, cards) = app(&[&["Write", "Test"], &["Review"], &[]], 60);
    driver.press("space right right");
    assert_eq!(
        columns(cards),
        [vec!["Test"], vec!["Review"], vec!["Write"]]
    );
    assert!(driver.screen().plain()[1].contains("╭↕"));
    driver.press("escape");
    assert_eq!(
        columns(cards),
        [vec!["Write", "Test"], vec!["Review"], vec![]]
    );
    // Carried and dropped with Space.
    driver.press("space down space");
    assert_eq!(
        columns(cards),
        [vec!["Test", "Write"], vec!["Review"], vec![]]
    );
    assert!(!driver.screen().plain().join("\n").contains('↕'));
}

#[test]
fn a_drag_carries_a_card_to_another_column() {
    let (mut driver, cards) = app(&[&["Write", "Test"], &["Review"], &[]], 60);
    let mouse = |kind, x, y| Event::Mouse(Mouse::new(kind, x, y));
    driver.send(mouse(MouseKind::Down(Button::Left), 3, 5));
    driver.send(mouse(MouseKind::Drag(Button::Left), 44, 2));
    driver.send(mouse(MouseKind::Up(Button::Left), 44, 2));
    assert_eq!(
        columns(cards),
        [vec!["Write"], vec!["Review"], vec!["Test"]]
    );
}

#[test]
fn a_narrow_board_pages_its_columns() {
    let (mut driver, cards) = app(&[&["Write"], &["Review"], &["Ship"]], 30);
    let row0 = |driver: &Driver| driver.screen().plain()[0].trim_end().to_string();
    // One column a page: Todo, and a `›` for the rest.
    assert!(row0(&driver).starts_with("Todo (1)"));
    assert!(row0(&driver).ends_with('›'));
    driver.press("right right");
    assert!(row0(&driver).starts_with("‹"));
    assert!(row0(&driver).contains("Done (1)"));
    // Ship back a column, at its place there (the top); the page follows.
    driver.press("H");
    assert!(row0(&driver).contains("Doing (2)"));
    assert_eq!(
        columns(cards),
        [vec!["Write"], vec!["Ship", "Review"], vec![]]
    );
}
