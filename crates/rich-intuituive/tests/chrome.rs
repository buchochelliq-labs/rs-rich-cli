//! Chrome: a rule joined to its panel's border, `.borders` on a node that
//! is not a panel.

use intuituive::prelude::*;
use intuituive::rule;

fn rows(driver: &intuituive::Driver) -> Vec<String> {
    driver.screen().plain()
}

#[test]
fn a_rule_joins_the_border_of_the_panel_it_spans() {
    let app = App::new(|| column([label("Keys"), rule(), label("q quit")]).panel("Help"));
    let mut driver = app.driver(12, 5);
    driver.settle();
    assert_eq!(
        rows(&driver),
        ["╭─ Help ───╮", "│Keys      │", "├──────────┤", "│q quit    │", "╰──────────╯"]
    );
}

#[test]
fn a_rule_takes_the_panels_kind_and_its_focus_colour() {
    let app = App::new(|| {
        column([label("a").focusable(), rule(), label("b")])
            .panel("")
            .border(Border::Double)
    });
    let mut driver = app.driver(6, 5);
    driver.settle();
    assert_eq!(rows(&driver)[2], "╠════╣");
    // The focus is inside: the joined line is lit as the border is.
    let corner = driver.screen().cell(0, 0).style;
    let tee = driver.screen().cell(0, 2).style;
    let line = driver.screen().cell(2, 2).style;
    assert_eq!(driver.screen().style(corner), driver.screen().style(tee));
    assert_eq!(driver.screen().style(corner), driver.screen().style(line));
}

#[test]
fn a_rule_inside_padding_does_not_reach_the_border() {
    let app = App::new(|| column([label("a"), rule(), label("b")]).padding(0, 1).panel(""));
    let mut driver = app.driver(7, 5);
    driver.settle();
    assert_eq!(rows(&driver)[2], "│ ─── │");
}

#[test]
fn a_rule_on_its_own_is_a_line() {
    let app = App::new(|| column([label("a"), rule().border(Border::Thick), label("b")]));
    let mut driver = app.driver(4, 3);
    driver.settle();
    assert_eq!(rows(&driver), ["a   ", "━━━━", "b   "]);
    // It is decoration: left out of the accessibility tree.
    assert_eq!(driver.accessibility().len(), 2);
}

#[test]
fn borders_on_a_node_without_a_panel_draw_its_sides() {
    let app = App::new(|| {
        column([
            label("Header").borders(Sides::BOTTOM).fixed(2),
            label("body"),
            label("left").borders(Sides::LEFT).fixed(1),
        ])
    });
    let mut driver = app.driver(8, 4);
    driver.settle();
    assert_eq!(rows(&driver), ["Header  ", "────────", "body    ", "│left   "]);
}
