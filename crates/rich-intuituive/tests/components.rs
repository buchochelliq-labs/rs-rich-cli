//! The components built on the widget layer: scroll, table, tabs and
//! anchored pop-ups.

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{run, run_open, screen};
use intuituive::interact::headless::Script;
use intuituive::interact::{Button, MouseKind};
use intuituive::prelude::*;
use intuituive::widgets::{table, tabs, virtual_table, Column};
use intuituive::Placement;

fn numbered(n: usize) -> Node {
    column((0..n).map(|i| label(format!("line {i}")).fixed(1)))
}

#[test]
fn a_scroll_shows_a_window_and_a_scrollbar() {
    let app = App::new(|| scroll(numbered(20)).on_key("q", |cx| cx.quit()));
    let rows = screen(&run(app, Script::new().keys("q"), 12, 4));
    assert_eq!(rows[0], "line 0     ┃");
    assert_eq!(rows[3], "line 3     │");
    let app = App::new(|| scroll(numbered(20)).on_key("q", |cx| cx.quit()));
    let rows = screen(&run(app, Script::new().keys("down down pagedown q"), 12, 4));
    // Two rows, then a page of three (four rows, less one kept).
    assert_eq!(rows[0], "line 5     │");
    let app = App::new(|| scroll(numbered(20)).on_key("q", |cx| cx.quit()));
    let rows = screen(&run(app, Script::new().keys("end q"), 12, 4));
    assert_eq!(rows[3], "line 19    ┃");
}

#[test]
fn the_wheel_scrolls_and_a_short_child_gets_no_scrollbar() {
    let app = App::new(|| scroll(numbered(20)).on_key("q", |cx| cx.quit()));
    let script = Script::new()
        .scroll(true, 2, 1)
        .scroll(true, 2, 1)
        .keys("q");
    let rows = screen(&run(app, script, 12, 4));
    assert_eq!(rows[0], "line 6     │");
    let app = App::new(|| scroll(numbered(2)).on_key("q", |cx| cx.quit()));
    let rows = screen(&run(app, Script::new().keys("q"), 12, 4));
    assert_eq!(rows, ["line 0", "line 1", "", ""]);
}

#[test]
fn the_focus_moving_inside_a_scroll_brings_its_node_into_view() {
    let app = App::new(|| {
        let rows = (0..20).map(|i| label(format!("item {i}")).focus_style("reverse").fixed(1));
        scroll(column(rows)).on_key("q", |cx| cx.quit())
    });
    // The scroll is the first stop, then each item.
    let keys = "tab ".repeat(11) + "q";
    let rows = screen(&run(app, Script::new().keys(&keys), 12, 4));
    assert!(rows.iter().any(|r| r.starts_with("item 9")), "{rows:?}");
    assert!(!rows.iter().any(|r| r.starts_with("item 0")), "{rows:?}");
}

#[test]
fn a_click_inside_a_scroll_reaches_the_node_drawn_there() {
    let clicked = Rc::new(RefCell::new(None));
    let seen = clicked.clone();
    let app = App::new(move || {
        let rows = (0..20).map(|i| {
            let seen = seen.clone();
            label(format!("row {i}"))
                .fixed(1)
                .on_click(move |_| *seen.borrow_mut() = Some(i))
        });
        column([label("title").fixed(1), scroll(column(rows))]).on_key("q", |cx| cx.quit())
    });
    // Scroll three rows, then click the second row of the viewport (screen
    // row 2): row 4 of the list.
    let script = Script::new().scroll(true, 1, 2).click(1, 2).keys("q");
    run(app, script, 12, 5);
    assert_eq!(*clicked.borrow(), Some(4));
}

#[test]
fn a_table_moves_its_selection_with_keys_clicks_and_the_wheel() {
    let rows = || {
        (0..30)
            .map(|i| vec![format!("file{i}"), format!("{}K", i * 2)])
            .collect()
    };
    let make = move |selected: Signal<usize>| {
        table(
            vec![
                Column::new("Name", Size::Auto),
                Column::new("Size", Size::Flex(1)),
            ],
            rows,
            selected,
        )
    };
    let app = App::new(move || {
        let selected = signal(0usize);
        column([make(selected).fixed(5), text!("picked {selected}").fixed(1)])
            .on_key("q", |cx| cx.quit())
    });
    let rows_shown = screen(&run(app, Script::new().keys("j j pagedown q"), 20, 6));
    // Two down, then a page of four body rows: row 6, scrolled into view.
    assert_eq!(rows_shown[5], "picked 6");
    // The name column fits the names in view.
    assert_eq!(rows_shown[0], "Name  Size");
    assert_eq!(rows_shown[4], "file6 12K");

    let app = App::new(move || {
        let selected = signal(0usize);
        column([make(selected).fixed(5), text!("picked {selected}").fixed(1)])
            .on_key("q", |cx| cx.quit())
    });
    let script = Script::new().click(2, 3).keys("q");
    assert_eq!(screen(&run(app, script, 20, 6))[5], "picked 2");

    let app = App::new(move || {
        let selected = signal(0usize);
        column([make(selected).fixed(5), text!("picked {selected}").fixed(1)])
            .on_key("q", |cx| cx.quit())
    });
    let script = Script::new().scroll(true, 2, 2).keys("q");
    assert_eq!(screen(&run(app, script, 20, 6))[5], "picked 3");
}

#[test]
fn a_virtual_table_asks_only_for_the_rows_in_view() {
    let asked = Rc::new(RefCell::new(Vec::new()));
    let log = asked.clone();
    let app = App::new(move || {
        let selected = signal(0usize);
        virtual_table(
            vec![Column::new("n", Size::Flex(1))],
            || 1_000_000,
            move |i| {
                log.borrow_mut().push(i);
                vec![i.to_string()]
            },
            selected,
        )
        .on_key("q", |cx| cx.quit())
    });
    let rows = screen(&run(app, Script::new().keys("end q"), 10, 4));
    assert_eq!(rows[3], "999999");
    assert!(asked.borrow().iter().all(|i| *i < 3 || *i >= 999_997));
    assert!(asked.borrow().len() <= 12, "{}", asked.borrow().len());
}

#[test]
fn tabs_change_with_number_keys_and_clicks() {
    let make = || {
        let tab = signal(0usize);
        column([
            tabs(vec!["One".into(), "Two".into(), "Three".into()], tab).fixed(1),
            text!("tab {tab}").fixed(1),
        ])
        .on_key("q", |cx| cx.quit())
    };
    let app = App::new(make);
    assert_eq!(
        screen(&run(app, Script::new().keys("3 q"), 30, 2))[1],
        "tab 2"
    );
    // " One │ Two │ Three": "Two" spans columns 6 to 10.
    let app = App::new(make);
    assert_eq!(
        screen(&run(app, Script::new().click(7, 0).keys("q"), 30, 2))[1],
        "tab 1"
    );
}

fn popup_app(anchor_row: u16) -> App {
    App::new(move || {
        let field = label("field:").fixed(1);
        let id = field.id();
        let mut rows: Vec<Node> = (0..anchor_row).map(|_| label("").fixed(1)).collect();
        rows.push(field);
        column(rows)
            .on_key("o", move |cx| {
                cx.popup(id, Placement::Below, Size::Fixed(6), Size::Fixed(2), || {
                    label("menu\nitems").on_key("x", |cx| cx.pop())
                })
            })
            .on_key("q", |cx| cx.quit())
    })
}

#[test]
fn a_popup_opens_below_its_anchor_and_flips_when_there_is_no_room() {
    let rows = screen(&run_open(popup_app(1), Script::new().keys("o"), 12, 6));
    assert_eq!(rows[1], "field:");
    assert_eq!(&rows[2..4], ["menu", "items"]);
    let rows = screen(&run_open(popup_app(5), Script::new().keys("o"), 12, 6));
    assert_eq!(rows[5], "field:");
    assert_eq!(&rows[3..5], ["menu", "items"]);
}

#[test]
fn esc_or_a_press_outside_closes_a_popup() {
    let rows = screen(&run(popup_app(1), Script::new().keys("o esc q"), 12, 6));
    assert_eq!(rows[2], "");
    let script = Script::new()
        .keys("o")
        .mouse(MouseKind::Down(Button::Left), 10, 5)
        .keys("q");
    let rows = screen(&run(popup_app(1), script, 12, 6));
    assert_eq!(rows[2], "");
}
