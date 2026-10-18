//! A list's options (gap batch B1): a highlight symbol and its spacing,
//! the direction, scroll padding, multi-line items, row styles, the
//! offset, and an empty selection.

use intuituive::interact::{Button, Event, Mouse, MouseKind};
use intuituive::prelude::*;
use intuituive::widgets::{HighlightSpacing, ListDirection, Move};
use intuituive::{assert_screen, assert_screen_styled};

fn fruit() -> Vec<String> {
    ["apple", "banana", "cherry", "damson", "elder"]
        .into_iter()
        .map(String::from)
        .collect()
}

fn rows(driver: &intuituive::Driver) -> Vec<String> {
    driver
        .screen()
        .plain()
        .iter()
        .map(|row| row.trim_end().to_string())
        .collect()
}

#[test]
fn list_symbol() {
    let app = App::new(|| list(fruit, signal(1)).symbol(">> "));
    let mut driver = app.driver(12, 5);
    driver.settle();
    assert_screen!(driver, "list_symbol");
    let screen = rows(&driver);
    assert_eq!(screen[0], "   apple");
    assert_eq!(screen[1], ">> banana");
}

#[test]
fn the_symbol_keeps_its_column_by_the_spacing() {
    let draw = |spacing: HighlightSpacing, selected: Option<usize>| {
        let app = App::new(move || {
            list_optional(fruit, signal(selected))
                .symbol("> ")
                .highlight_spacing(spacing)
        });
        let mut driver = app.driver(10, 2);
        driver.settle();
        rows(&driver)
    };
    // Nothing selected: only `Always` keeps the column.
    assert_eq!(draw(HighlightSpacing::Always, None)[0], "  apple");
    assert_eq!(draw(HighlightSpacing::WhenSelected, None)[0], "apple");
    assert_eq!(draw(HighlightSpacing::Never, None)[0], "apple");
    // Selected: `Never` draws no symbol.
    assert_eq!(draw(HighlightSpacing::WhenSelected, Some(0))[0], "> apple");
    assert_eq!(draw(HighlightSpacing::Never, Some(0))[0], "apple");
}

#[test]
fn list_bottom_to_top() {
    let app = App::new(|| {
        let selected = signal(0usize);
        column([
            list(fruit, selected).direction(ListDirection::BottomToTop),
            text!("{selected}").fixed(1),
        ])
    });
    let mut driver = app.driver(10, 7);
    driver.settle();
    assert_screen!(driver, "list_bottom_to_top");
    let screen = rows(&driver);
    // The first item at the bottom, a blank row above the last.
    assert_eq!(screen[0], "");
    assert_eq!(screen[1], "elder");
    assert_eq!(screen[5], "apple");
    // ↑ goes on up the screen: to the next item.
    driver.press("up");
    assert_eq!(rows(&driver)[6], "1");
    driver.press("down down");
    assert_eq!(rows(&driver)[6], "0");
}

#[test]
fn list_multiline() {
    let items = || {
        vec![
            "one line".to_string(),
            "two\nlines".into(),
            "a long item that wraps".into(),
        ]
    };
    let app = App::new(move || {
        list(items, signal(1))
            .multiline()
            .symbol("» ")
            .repeat_symbol()
    });
    let mut driver = app.driver(14, 6);
    driver.settle();
    assert_screen!(driver, "list_multiline");
    let screen = rows(&driver);
    assert_eq!(screen[1], "» two");
    assert_eq!(screen[2], "» lines");
    assert_eq!(screen[3], "  a long item");
    // Without the repeat, the second line is blank in the symbol's column.
    let app = App::new(move || list(items, signal(1)).multiline().symbol("» "));
    let mut driver = app.driver(14, 6);
    driver.settle();
    assert_eq!(rows(&driver)[2], "  lines");
}

#[test]
fn multi_line_items_scroll_by_their_heights() {
    let items = || (0..6).map(|n| format!("item {n}\nmore {n}")).collect();
    let app = App::new(move || list(items, signal(0)).multiline());
    let mut driver = app.driver(10, 4);
    driver.press("down down");
    let screen = rows(&driver);
    // Two items of two lines fit: item 1 and item 2 show.
    assert_eq!(screen[0], "item 1");
    assert_eq!(screen[2], "item 2");
}

#[test]
fn list_scroll_padding() {
    let items = || (0..20).map(|n| format!("item {n}")).collect();
    let app = App::new(move || list(items, signal(0)).scroll_padding(1));
    let mut driver = app.driver(10, 5);
    driver.press("down down down down");
    assert_screen!(driver, "list_scroll_padding");
    // Item 4 is selected, with item 5 still in view below it.
    let screen = rows(&driver);
    assert_eq!(screen[4], "item 5");
    driver.press("up up up");
    // Going back, item 0 is kept in view above item 1.
    assert_eq!(rows(&driver)[0], "item 0");
}

#[test]
fn list_zebra() {
    let app =
        App::new(|| list(fruit, signal(0)).row_style(|i| (i % 2 == 1).then(|| "on blue".into())));
    let mut driver = app.driver(10, 4);
    driver.settle();
    assert_screen_styled!(driver, "list_zebra");
    let styled = intuituive::testing::styled(&driver);
    assert!(
        styled.lines().nth(1).unwrap().contains("on blue"),
        "{styled}"
    );
}

#[test]
fn list_none() {
    let app = App::new(|| {
        let selected = signal(None);
        column([
            list_optional(fruit, selected),
            text(move || format!("{:?}", selected.get())).fixed(1),
        ])
    });
    let mut driver = app.driver(10, 4);
    driver.settle();
    assert_screen_styled!(driver, "list_none");
    // No row is highlighted.
    let styled = intuituive::testing::styled(&driver);
    assert!(!styled.contains("reverse"), "{styled}");
    assert_eq!(rows(&driver)[3], "None");
    driver.press("down");
    assert_eq!(rows(&driver)[3], "Some(0)");
    // Setting the signal to `None` clears it again.
    let app = App::new(|| {
        let selected = signal(Some(2));
        list_optional(fruit, selected).on_key("x", move |_| selected.set(None))
    });
    let mut driver = app.driver(10, 3);
    driver.press("x");
    assert!(!intuituive::testing::styled(&driver).contains("reverse"));
    // From none, ↑ selects the last item.
    driver.press("up");
    let tree = driver.accessibility();
    assert_eq!(tree[0].state.position, Some((5, 5)));
}

#[test]
fn the_offset_signal_follows_and_moves_the_view() {
    let items = || (0..30).map(|n| format!("item {n}")).collect();
    let app = App::new(move || {
        let first = signal(0usize);
        let selected = signal(0usize);
        column([
            list(items, selected).offset(first),
            text!("{first}").fixed(1),
        ])
        .on_key("x", move |_| {
            selected.set(20);
            first.set(18);
        })
    });
    let mut driver = app.driver(10, 5);
    driver.press("end");
    // The view followed the selection to the end: the first of the last
    // four rows.
    assert_eq!(rows(&driver)[4], "26");
    driver.press("x");
    assert_eq!(rows(&driver)[0], "item 18");
}

#[test]
fn a_reorderable_list_keeps_its_options() {
    let app = App::new(|| {
        let items = signal(fruit());
        list(move || items.get(), signal(0))
            .reorderable(move |step: Move, _| items.update(|items| step.apply(items)))
            .symbol("> ")
    });
    let mut driver = app.driver(12, 5);
    driver.press("J");
    let screen = rows(&driver);
    assert_eq!(screen[0], "  banana");
    assert_eq!(screen[1], "> apple");
}

#[test]
fn accessibility_positions_are_unchanged() {
    let app = App::new(|| list(fruit, signal(2)).symbol("> ").multiline());
    let mut driver = app.driver(12, 5);
    driver.settle();
    let tree = driver.accessibility();
    assert_eq!(tree[0].role, intuituive::a11y::Role::List);
    assert_eq!(tree[0].state.position, Some((3, 5)));
    // The value is the item, not its symbol.
    assert_eq!(tree[0].value.as_deref().map(str::trim_end), Some("cherry"));
}

#[test]
fn a_press_selects_an_item_and_the_wheel_moves_the_selection() {
    // A list that never takes the focus still takes the mouse (the demo
    // port's task list), as a table does.
    let app = App::new(|| {
        let selected = signal(None);
        column([
            list_optional(fruit, selected)
                .symbol("> ")
                .no_focus()
                .fixed(5),
            text(move || format!("{:?}", selected.get())).fixed(1),
        ])
    });
    let mut driver = app.driver(12, 6);
    driver.settle();
    let mouse = |kind, row| Event::Mouse(Mouse::new(kind, 3, row));
    driver.event(mouse(MouseKind::Down(Button::Left), 1));
    driver.settle();
    assert_eq!(rows(&driver)[1], "> banana");
    assert_eq!(rows(&driver)[5], "Some(1)");
    // The wheel: three items a turn, kept in the list.
    driver.event(mouse(MouseKind::ScrollDown, 1));
    driver.settle();
    assert_eq!(rows(&driver)[5], "Some(4)");
    driver.event(mouse(MouseKind::ScrollUp, 1));
    driver.settle();
    assert_eq!(rows(&driver)[5], "Some(1)");
    // A right press selects too, and is left to the node's own handler.
    driver.event(mouse(MouseKind::Down(Button::Right), 2));
    driver.settle();
    assert_eq!(rows(&driver)[5], "Some(2)");
}

#[test]
fn a_reorderable_list_still_drags_with_a_press() {
    let app = App::new(|| {
        let items = signal(fruit());
        list(move || items.get(), signal(0))
            .reorderable(move |step: Move, _| items.update(|items| step.apply(items)))
    });
    let mut driver = app.driver(12, 5);
    driver.settle();
    let mouse = |kind, row| Event::Mouse(Mouse::new(kind, 3, row));
    driver.event(mouse(MouseKind::Down(Button::Left), 0));
    driver.event(mouse(MouseKind::Drag(Button::Left), 2));
    driver.event(mouse(MouseKind::Up(Button::Left), 2));
    driver.settle();
    assert_eq!(rows(&driver)[2], "apple", "{:?}", rows(&driver));
}
