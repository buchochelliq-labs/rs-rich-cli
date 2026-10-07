//! More widgets on the public trait: tree, split panes, calendar and the
//! virtual list.

mod common;

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use common::{run, screen};
use intuituive::interact::headless::Script;
use intuituive::interact::{Button, MouseKind};
use intuituive::node::Axis;
use intuituive::prelude::*;
use intuituive::widgets::{
    calendar, calendar_with, split, split_with, tree, tree_with, virtual_list, vsplit, Date,
    TreeItem,
};

fn items() -> Vec<TreeItem> {
    vec![
        TreeItem::new("src").children([
            TreeItem::new("main.rs"),
            TreeItem::new("ui").child(TreeItem::new("view.rs")),
        ]),
        TreeItem::new("Cargo.toml"),
    ]
}

/// A tree five rows tall over a line showing the selected path.
fn tree_app(height: u16) -> App {
    App::new(move || {
        let selected = signal(vec![0usize]);
        column([
            tree(items, selected).fixed(height),
            text(move || format!("at {:?}", selected.get())).fixed(1),
        ])
        .on_key("q", |cx| cx.quit())
    })
}

#[test]
fn a_tree_expands_moves_in_and_out_with_the_arrows() {
    let rows = screen(&run(tree_app(5), Script::new().keys("q"), 20, 6));
    assert_eq!(rows[..3], ["▸ src", "  Cargo.toml", ""]);
    // Expand src, into main.rs, down to ui, expand it, into view.rs.
    let keys = "right right down right right q";
    let rows = screen(&run(tree_app(5), Script::new().keys(keys), 20, 6));
    assert_eq!(
        rows,
        [
            "▾ src",
            "    main.rs",
            "  ▾ ui",
            "      view.rs",
            "  Cargo.toml",
            "at [0, 1, 0]"
        ]
    );
    // Left goes to the parent, then collapses it, then to its parent.
    let keys = "right right down right right left left q";
    let rows = screen(&run(tree_app(5), Script::new().keys(keys), 20, 6));
    assert_eq!(rows[2], "  ▸ ui");
    assert_eq!(rows[5], "at [0, 1]");
    let keys = "right right down right right left left left q";
    let rows = screen(&run(tree_app(5), Script::new().keys(keys), 20, 6));
    assert_eq!(rows[5], "at [0]");
}

#[test]
fn enter_toggles_and_home_and_end_jump() {
    let rows = screen(&run(tree_app(5), Script::new().keys("enter end q"), 20, 6));
    assert_eq!(rows[1], "    main.rs");
    assert_eq!(rows[5], "at [1]");
    // Collapsing the item that holds the selection selects the item.
    let keys = "space down space home space up q";
    let rows = screen(&run(tree_app(5), Script::new().keys(keys), 20, 6));
    assert_eq!(rows[..2], ["▸ src", "  Cargo.toml"]);
    assert_eq!(rows[5], "at [0]");
    let rows = screen(&run(tree_app(5), Script::new().keys("j k G g q"), 20, 6));
    assert_eq!(rows[5], "at [0]");
}

#[test]
fn a_tree_click_selects_and_a_click_on_the_arrow_toggles() {
    // The arrow of src is at columns 0 and 1.
    let script = Script::new().click(0, 0).click(6, 1).keys("q");
    let rows = screen(&run(tree_app(5), script, 20, 6));
    assert_eq!(rows[0], "▾ src");
    assert_eq!(rows[5], "at [0, 0]");
    // Clicking the arrow again collapses it and selects src.
    let script = Script::new().click(0, 0).click(6, 1).click(1, 0).keys("q");
    let rows = screen(&run(tree_app(5), script, 20, 6));
    assert_eq!(rows[..2], ["▸ src", "  Cargo.toml"]);
    assert_eq!(rows[5], "at [0]");
    // The wheel moves the selection.
    let script = Script::new().click(0, 0).scroll(true, 3, 1).keys("q");
    let rows = screen(&run(tree_app(5), script, 20, 6));
    assert_eq!(rows[5], "at [1]");
}

#[test]
fn a_tree_scrolls_to_keep_the_selection_in_view() {
    let app = App::new(|| {
        let selected = signal(vec![0usize]);
        let expanded = signal(HashSet::from([vec![0usize]]));
        let items = || {
            vec![TreeItem::new("root").children((0..20).map(|i| TreeItem::new(format!("n{i}"))))]
        };
        column([
            tree_with(items, selected, expanded).fixed(4),
            text(move || format!("open {}", expanded.with(|e| e.len()))).fixed(1),
        ])
        .on_key("q", |cx| cx.quit())
    });
    let rows = screen(&run(app, Script::new().keys("end q"), 20, 5));
    assert_eq!(rows[..4], ["    n16", "    n17", "    n18", "    n19"]);
    assert_eq!(rows[4], "open 1");
}

#[test]
fn a_tree_measures_its_rows() {
    let app = App::new(|| {
        let selected = signal(vec![0usize]);
        column([tree(items, selected).auto(), label("below").fixed(1)]).on_key("q", |cx| cx.quit())
    });
    let rows = screen(&run(app, Script::new().keys("right q"), 20, 8));
    assert_eq!(rows[4], "below");
}

/// A split of a list (which takes the focus) and a label, over the ratio.
fn split_app(axis: Axis) -> App {
    App::new(move || {
        let ratio = signal(0.5f64);
        let left = list(|| vec!["a".into(), "b".into()], signal(0usize));
        column([
            split(axis, left, label("right"), ratio).fixed(5),
            text(move || format!("ratio {:.2}", ratio.get())).fixed(1),
        ])
        .on_key("q", |cx| cx.quit())
    })
}

#[test]
fn a_split_lays_out_two_panes_around_a_divider() {
    let rows = screen(&run(
        split_app(Axis::Horizontal),
        Script::new().keys("q"),
        21,
        6,
    ));
    assert_eq!(rows[0], "a         │right");
    assert_eq!(rows[4], "          │");
    let rows = screen(&run(
        split_app(Axis::Vertical),
        Script::new().keys("q"),
        8,
        6,
    ));
    assert_eq!(rows[..5], ["a", "b", "────────", "right", ""]);
}

#[test]
fn dragging_the_divider_resizes_the_panes() {
    let script = Script::new()
        .mouse(MouseKind::Down(Button::Left), 10, 2)
        .mouse(MouseKind::Drag(Button::Left), 7, 2)
        // Captured: a drag outside the split still moves the divider.
        .mouse(MouseKind::Drag(Button::Left), 5, 5)
        .mouse(MouseKind::Up(Button::Left), 5, 5)
        // Released: this drag does nothing.
        .mouse(MouseKind::Drag(Button::Left), 12, 2)
        .keys("q");
    let rows = screen(&run(split_app(Axis::Horizontal), script, 21, 6));
    assert_eq!(rows[0], "a    │right");
    assert_eq!(rows[5], "ratio 0.25");
    // A press off the divider does not drag.
    let script = Script::new()
        .mouse(MouseKind::Down(Button::Left), 15, 2)
        .mouse(MouseKind::Drag(Button::Left), 3, 2)
        .mouse(MouseKind::Up(Button::Left), 3, 2)
        .keys("q");
    // Without text selection, whose toast would cover the last row.
    let app = split_app(Axis::Horizontal).selectable(false);
    let rows = screen(&run(app, script, 21, 6));
    assert_eq!(rows[5], "ratio 0.50");
}

#[test]
fn a_drag_keeps_each_pane_its_minimum() {
    let script = Script::new().drag((10, 0), (0, 0)).keys("q");
    let rows = screen(&run(split_app(Axis::Horizontal), script, 21, 6));
    assert_eq!(rows[0], "a  │right");
    let script = Script::new()
        .mouse(MouseKind::Down(Button::Left), 4, 2)
        .mouse(MouseKind::Drag(Button::Left), 4, 0)
        .mouse(MouseKind::Up(Button::Left), 4, 0)
        .keys("q");
    let rows = screen(&run(split_app(Axis::Vertical), script, 8, 6));
    // Stacked in five rows, each pane keeps two: the divider stays.
    assert_eq!(rows[2], "────────");
}

#[test]
fn alt_arrows_nudge_the_ratio_from_inside() {
    let rows = screen(&run(
        split_app(Axis::Horizontal),
        Script::new().keys("alt+right alt+right q"),
        21,
        6,
    ));
    assert_eq!(rows[5], "ratio 0.60");
    assert_eq!(rows[0], "a           │right");
    let rows = screen(&run(
        split_app(Axis::Horizontal),
        Script::new().keys("alt+left alt+up q"),
        21,
        6,
    ));
    assert_eq!(rows[5], "ratio 0.45");
}

#[test]
fn split_with_takes_a_minimum() {
    let app = App::new(|| {
        let ratio = signal(1.0f64);
        split_with(Axis::Horizontal, label("l"), label("r"), ratio, 2).on_key("q", |cx| cx.quit())
    });
    let rows = screen(&run(app, Script::new().keys("q"), 10, 1));
    assert_eq!(rows[0], "l      │r");
    let app = App::new(|| vsplit(label("t"), label("b"), signal(0.0)).on_key("q", |cx| cx.quit()));
    let rows = screen(&run(app, Script::new().keys("q"), 3, 9));
    assert_eq!(rows[3], "───");
}

fn calendar_app() -> App {
    App::new(|| {
        let day = signal(Date::new(2026, 10, 7));
        column([
            calendar_with(day, Some(Date::new(2026, 10, 7))).fixed(8),
            text!("on {day}").fixed(1),
        ])
        .on_key("q", |cx| cx.quit())
    })
}

#[test]
fn a_calendar_draws_a_month() {
    let rows = screen(&run(calendar_app(), Script::new().keys("q"), 20, 9));
    assert_eq!(
        rows,
        [
            "    October 2026",
            "Mo Tu We Th Fr Sa Su",
            "          1  2  3  4",
            " 5  6  7  8  9 10 11",
            "12 13 14 15 16 17 18",
            "19 20 21 22 23 24 25",
            "26 27 28 29 30 31",
            "",
            "on 2026-10-07",
        ]
    );
    // February 2027 starts on a Monday and has four weeks.
    let rows = screen(&run(
        calendar_app(),
        Script::new().keys("pagedown pagedown pagedown pagedown q"),
        20,
        9,
    ));
    assert_eq!(rows[0], "   February 2027");
    assert_eq!(rows[2], " 1  2  3  4  5  6  7");
    assert_eq!(rows[5], "22 23 24 25 26 27 28");
    assert_eq!(rows[6], "");
    assert_eq!(rows[8], "on 2027-02-07");
}

#[test]
fn calendar_keys_move_by_day_week_month_and_to_the_ends() {
    let on = |keys: &str| screen(&run(calendar_app(), Script::new().keys(keys), 20, 9))[8].clone();
    assert_eq!(on("right right q"), "on 2026-10-09");
    assert_eq!(on("left q"), "on 2026-10-06");
    assert_eq!(on("down down down down q"), "on 2026-11-04");
    assert_eq!(on("up q"), "on 2026-09-30");
    assert_eq!(on("pageup q"), "on 2026-09-07");
    assert_eq!(on("home q"), "on 2026-10-01");
    assert_eq!(on("end q"), "on 2026-10-31");
    assert_eq!(on("end pagedown q"), "on 2026-11-30");
}

#[test]
fn a_calendar_click_selects_a_day_and_the_wheel_turns_the_month() {
    let at = |script: Script| screen(&run(calendar_app(), script.keys("q"), 20, 9));
    // The 15th: Thursday (column 9), the third week (row 4).
    assert_eq!(at(Script::new().click(10, 4))[8], "on 2026-10-15");
    // Before the 1st: nothing.
    assert_eq!(at(Script::new().click(1, 2))[8], "on 2026-10-07");
    // The header: nothing.
    assert_eq!(at(Script::new().click(1, 1))[8], "on 2026-10-07");
    let rows = at(Script::new().scroll(true, 5, 5).scroll(true, 5, 5));
    assert_eq!(rows[0], "   December 2026");
    assert_eq!(rows[8], "on 2026-12-07");
    let rows = at(Script::new().scroll(false, 5, 5));
    assert_eq!(rows[8], "on 2026-09-07");
}

#[test]
fn a_calendar_measures_twenty_by_eight() {
    let app = App::new(|| {
        let day = signal(Date::new(2026, 10, 7));
        column([
            row([calendar(day).auto(), label("|").fixed(1)]).auto(),
            label("below").fixed(1),
        ])
        .on_key("q", |cx| cx.quit())
    });
    let rows = screen(&run(app, Script::new().keys("q"), 30, 12));
    assert_eq!(rows[0], "    October 2026    |");
    assert_eq!(rows[8], "below");
}

fn vlist_app(asked: Rc<RefCell<Vec<usize>>>) -> App {
    App::new(move || {
        let selected = signal(0usize);
        let log = asked.clone();
        column([
            virtual_list(
                || 1_000_000,
                move |i| {
                    log.borrow_mut().push(i);
                    format!("row {i}")
                },
                selected,
            )
            .fixed(4),
            text!("picked {selected}").fixed(1),
        ])
        .on_key("q", |cx| cx.quit())
    })
}

#[test]
fn a_virtual_list_asks_only_for_the_rows_in_view() {
    let asked = Rc::new(RefCell::new(Vec::new()));
    let rows = screen(&run(
        vlist_app(asked.clone()),
        Script::new().keys("G q"),
        14,
        5,
    ));
    assert_eq!(
        rows,
        [
            "row 999996",
            "row 999997",
            "row 999998",
            "row 999999",
            "picked 999999"
        ]
    );
    assert!(asked.borrow().iter().all(|i| *i < 4 || *i >= 999_996));
    assert!(asked.borrow().len() <= 16, "{}", asked.borrow().len());
}

#[test]
fn a_virtual_list_moves_with_keys_clicks_and_the_wheel() {
    let picked = |script: Script| {
        let asked = Rc::new(RefCell::new(Vec::new()));
        screen(&run(vlist_app(asked), script.keys("q"), 14, 5))[4].clone()
    };
    assert_eq!(picked(Script::new().keys("j j k")), "picked 1");
    assert_eq!(picked(Script::new().keys("pagedown pagedown")), "picked 8");
    assert_eq!(picked(Script::new().keys("end home")), "picked 0");
    assert_eq!(picked(Script::new().keys("pagedown g")), "picked 0");
    // No header: the first row is row 0.
    assert_eq!(picked(Script::new().click(2, 0)), "picked 0");
    assert_eq!(picked(Script::new().click(2, 3)), "picked 3");
    assert_eq!(picked(Script::new().scroll(true, 2, 2)), "picked 3");
    assert_eq!(
        picked(Script::new().scroll(true, 2, 2).scroll(false, 2, 2)),
        "picked 0"
    );
}
