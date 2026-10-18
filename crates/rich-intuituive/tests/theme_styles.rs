//! Theme styles for a subtree (0.0.19 gap A9).

use intuituive::prelude::*;
use intuituive::{assert_screen_styled, Driver};

fn shown(app: App, width: u16, height: u16) -> Driver {
    let mut driver = app.driver(width, height);
    driver.settle();
    driver
}

fn style(definition: &str) -> rich::Style {
    rich::Style::parse(definition).unwrap()
}

fn style_at(driver: &Driver, x: u16, y: u16) -> Option<rich::Style> {
    let screen = driver.screen();
    screen.style(screen.cell(x, y).style).cloned()
}

fn items() -> Vec<String> {
    vec!["first".into(), "second".into()]
}

#[test]
fn theme_styles_list() {
    let app = App::new(|| {
        let (a, b) = (signal(0usize), signal(1usize));
        row([
            list(items, a),
            list(items, b).theme_styles([("selected", "underline")]),
        ])
    });
    let driver = shown(app, 16, 2);
    assert_screen_styled!(driver, "theme_styles_list");
    // The app's `selected` on the left, the override on the right.
    assert_eq!(style_at(&driver, 0, 0), Some(style("reverse")));
    assert_eq!(style_at(&driver, 8, 1), Some(style("underline")));
}

#[test]
fn markup_inside_picks_the_override() {
    let app = App::new(|| {
        column([
            label("[selected]x[/] [accent]y[/]"),
            column([label("[selected]x[/] [accent]y[/]")])
                .theme_styles([("selected", "bold red"), ("accent", "green")]),
        ])
    });
    let driver = shown(app, 4, 2);
    assert_eq!(style_at(&driver, 0, 0), Some(style("reverse")));
    assert_eq!(style_at(&driver, 2, 0), Some(style("bright_cyan")));
    assert_eq!(style_at(&driver, 0, 1), Some(style("bold red")));
    assert_eq!(style_at(&driver, 2, 1), Some(style("green")));
}

#[test]
fn a_theme_switch_keeps_the_override_and_names_follow_it() {
    let app = App::new(|| {
        column([
            // An override that names another theme style takes the new
            // theme's.
            label("[selected]x[/]").theme_styles([("selected", "accent")]),
            label("[warn]w[/]").theme_styles([("warn", "bold magenta")]),
        ])
        .on_key("t", |cx| cx.set_theme(Theme::light()))
    });
    let mut driver = shown(app, 2, 2);
    assert_eq!(style_at(&driver, 0, 0), Some(style("bright_cyan")));
    driver.press("t");
    assert_eq!(style_at(&driver, 0, 0), Some(style("blue")));
    assert_eq!(style_at(&driver, 0, 1), Some(style("bold magenta")));
}

#[test]
fn nested_overrides_and_the_frameworks_own_parts() {
    let app = App::new(|| {
        column([label("[a]x[/]")
            .theme_styles([("a", "red")])
            .panel("P")
            .theme_styles([("a", "green"), ("border", "magenta"), ("title", "italic")])])
    });
    let driver = shown(app, 8, 3);
    // The inner override wins inside; the panel draws with its own.
    assert_eq!(style_at(&driver, 1, 1), Some(style("red")));
    assert_eq!(style_at(&driver, 0, 0), Some(style("magenta")));
    assert_eq!(style_at(&driver, 3, 0), Some(style("magenta italic")));
}

#[test]
fn widgets_inside_take_their_styles_from_the_override() {
    use intuituive::widgets::{table, Column};
    let app = App::new(|| {
        let selected = signal(0usize);
        table(
            vec![Column::new("Name", Size::Flex(1))],
            || vec![vec!["ada".to_string()]],
            selected,
        )
        .theme_styles([("table.header", "bold underline green")])
    });
    let driver = shown(app, 8, 2);
    assert_eq!(style_at(&driver, 0, 0), Some(style("bold underline green")));
}
