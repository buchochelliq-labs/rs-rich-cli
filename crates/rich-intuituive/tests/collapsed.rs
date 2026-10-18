//! Collapsed borders: one frame for a container's panels (0.0.19 gap A6).

use intuituive::prelude::*;
use intuituive::{assert_screen, Driver};

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

#[test]
fn collapsed_row() {
    let app = App::new(|| {
        row([
            label("one").panel("One"),
            label("two").panel("Two").fixed(10),
            label("three").panel("Three").title_bottom("3"),
        ])
        .collapse_borders()
    });
    let driver = shown(app, 32, 4);
    assert_screen!(driver, "collapsed_row");
    let rows = driver.screen().plain();
    // One frame: the panels share the lines between them.
    assert!(
        rows[0].starts_with("╭─ One ─") && rows[0].ends_with("╮"),
        "{rows:?}"
    );
    assert_eq!(rows[0].matches('┬').count(), 2, "{rows:?}");
    assert_eq!(rows[3].matches('┴').count(), 2, "{rows:?}");
    // Each content sits between the lines.
    assert!(rows[1].starts_with("│one"), "{rows:?}");
}

#[test]
fn collapsed_grid() {
    let app = App::new(|| {
        grid(
            [Size::Flex(1), Size::Flex(1)],
            [
                label("cpu").panel("CPU"),
                label("memory").panel("Memory"),
                label("disk").panel("Disk"),
                label("network").panel("Net"),
            ],
        )
        .collapse_borders()
        .border(Border::Plain)
    });
    let driver = shown(app, 25, 7);
    assert_screen!(driver, "collapsed_grid");
    let rows = driver.screen().plain();
    assert!(
        rows[3].starts_with("├─ Disk ") && rows[3].ends_with("┤"),
        "{rows:?}"
    );
    assert_eq!(rows[3].matches('┼').count(), 1, "{rows:?}");
}

/// ratatui's `collapsed-borders` example: four panes sharing their lines,
/// the one with the focus thick and lit.
#[test]
fn collapsed_focus() {
    let pane = |name: &'static str| label(name).focusable().panel(name);
    let app = App::new(move || {
        column([
            pane("Top"),
            // A container that collapses its own panes' borders shares the
            // column's lines, and takes its look.
            row([pane("Left"), pane("Right")]).collapse_borders(),
            pane("Bottom"),
        ])
        .collapse_borders()
        .border(Border::Plain)
        .focus_border(Border::Thick)
    })
    .theme(Theme {
        border: style("blue"),
        border_focused: style("yellow"),
        ..Theme::default()
    });
    let mut driver = shown(app, 24, 9);
    assert_screen!(driver, "collapsed_focus_top");
    // The top pane has the focus: its edges are thick and lit.
    assert_eq!(style_at(&driver, 0, 0), Some(style("yellow")));
    assert!(driver.screen().plain()[0].starts_with("┏━ Top ━"));
    driver.press("tab");
    assert_screen!(driver, "collapsed_focus_left");
    assert_eq!(style_at(&driver, 0, 0), Some(style("blue")));
    driver.press("tab");
    assert_screen!(driver, "collapsed_focus_right");
    driver.press("tab");
    assert_screen!(driver, "collapsed_focus_bottom");
    let rows = driver.screen().plain();
    assert!(rows[8].starts_with("┗━━"), "{rows:?}");
    // One grid with spans draws the same.
    let grid = App::new(move || {
        grid(
            [Size::Flex(1), Size::Flex(1)],
            [
                pane("Top").span(2, 1),
                pane("Left"),
                pane("Right"),
                pane("Bottom").span(2, 1),
            ],
        )
        .collapse_borders()
        .border(Border::Plain)
        .focus_border(Border::Thick)
    });
    let mut grid = shown(grid, 24, 9);
    grid.press("tab tab tab");
    assert_eq!(grid.screen().plain(), driver.screen().plain());
}

#[test]
fn a_hidden_child_reflows_the_lines() {
    let app = App::new(|| {
        row([
            label("a").panel("A"),
            label("b").panel("B").name("b"),
            label("c").panel("C"),
        ])
        .collapse_borders()
    });
    let all = shown(app, 19, 3).screen().plain();
    assert_eq!(all[0].matches('┬').count(), 2, "{all:?}");
    let app = App::new(|| {
        row([
            label("a").panel("A"),
            label("b").panel("B").name("b"),
            label("c").panel("C"),
        ])
        .collapse_borders()
    })
    .stylesheet("#b { display: none; }");
    let rows = shown(app, 19, 3).screen().plain();
    assert_eq!(
        rows,
        [
            "╭─ A ────┬─ C ────╮",
            "│a       │c       │",
            "╰────────┴────────╯"
        ]
    );
}

#[test]
fn a_stylesheet_collapses_and_colours_the_lines() {
    let code = App::new(|| {
        row([label("a").panel("A"), label("b").panel("B")])
            .collapse_borders()
            .border(Border::Double)
    });
    let sheet = App::new(|| row([label("a").panel("A"), label("b").panel("B")]))
        .stylesheet("row { border-collapse: collapse; border: double green; }");
    let (code, sheet) = (shown(code, 13, 3), shown(sheet, 13, 3));
    assert_eq!(code.screen().plain(), sheet.screen().plain());
    assert_eq!(code.screen().plain()[0], "╔═ A ═╦═ B ═╗");
    // The sheet's colour, and no box of its own round the row.
    assert_eq!(style_at(&sheet, 0, 0), Some(style("green")));
}

#[test]
fn clicks_on_a_collapsed_panel_land_inside_it() {
    use intuituive::interact::{Button, Event, Mouse, MouseKind};
    let app = App::new(|| {
        let picked = signal("");
        column([
            row([
                label("left")
                    .on_click(move |_| picked.set("left"))
                    .panel("L"),
                label("right")
                    .on_click(move |_| picked.set("right"))
                    .panel("R"),
            ])
            .collapse_borders()
            .fixed(3),
            text!("{picked}"),
        ])
    });
    let mut driver = shown(app, 20, 4);
    driver.event(Event::Mouse(Mouse::new(
        MouseKind::Down(Button::Left),
        12,
        1,
    )));
    driver.event(Event::Mouse(Mouse::new(MouseKind::Up(Button::Left), 12, 1)));
    driver.settle();
    assert_eq!(driver.screen().plain()[3].trim_end(), "right");
}
