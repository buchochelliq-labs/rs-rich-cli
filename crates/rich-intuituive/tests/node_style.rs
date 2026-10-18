//! A style on any node: `.style`, `.bg` and `.fg` (0.0.19 gap A2).

use std::time::Duration;

use intuituive::interact::{Event, Key};
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

#[test]
fn node_style_fill() {
    let app = App::new(|| {
        column([
            label("one"),
            label("[yellow]two[/] and more"),
            row([label("a").fixed(2), label("b").fixed(2)]).gap(2),
        ])
        .gap(1)
        .bg("blue")
    });
    let driver = shown(app, 14, 6);
    assert_screen_styled!(driver, "node_style_fill");
    // Every cell is blue: text, the gaps between rows and across them, and
    // the rows below.
    for (x, y) in [(0, 0), (13, 0), (0, 1), (2, 4), (0, 5), (13, 5)] {
        let bg = style_at(&driver, x, y).and_then(|s| s.bgcolor().cloned());
        assert_eq!(bg, style("on blue").bgcolor().cloned(), "{x},{y}");
    }
}

#[test]
fn markup_inside_keeps_its_colour() {
    let app = App::new(|| label("[yellow]warn[/] plain").style("white on blue"));
    let driver = shown(app, 12, 1);
    assert_eq!(style_at(&driver, 0, 0), Some(style("yellow on blue")));
    assert_eq!(style_at(&driver, 6, 0), Some(style("white on blue")));
    // A foreground alone, from a theme style's colour.
    let app = App::new(|| label("x").fg("accent"));
    assert_eq!(
        style_at(&shown(app, 2, 1), 0, 0),
        Some(style("bright_cyan"))
    );
}

#[test]
fn code_beats_a_stylesheets_background() {
    let app = App::new(|| label("x").name("n").bg("red"))
        .stylesheet("#n { background: blue; color: green; }");
    // The sheet's colour stays where code set none.
    assert_eq!(
        style_at(&shown(app, 3, 1), 0, 0),
        Some(style("green on red"))
    );
}

#[test]
fn a_theme_switch_recolours_a_theme_style() {
    let app = App::new(|| {
        label("x")
            .style("accent")
            .on_key("t", |cx| cx.set_theme(Theme::light()))
    });
    let mut driver = shown(app, 3, 1);
    assert_eq!(style_at(&driver, 0, 0), Some(style("bright_cyan")));
    driver.press("t");
    assert_eq!(style_at(&driver, 0, 0), Some(style("blue")));
}

#[test]
fn a_tick_that_redraws_one_child_does_not_resend_the_fill() {
    let app = App::new(|| {
        let ticks = signal(0);
        column(
            std::iter::once(text!("tick {ticks}"))
                .chain((0..8).map(|n| label(format!("still {n}")))),
        )
        .bg("#101830")
        .on_key("+", move |_| ticks.update(|t| *t += 1))
    });
    let mut driver = app.driver(30, 9);
    driver.update(Duration::ZERO);
    let first = driver.render().expect("the first frame");
    driver.event(Event::Key(Key::char('+')));
    driver.update(Duration::ZERO);
    let tick = driver.render().expect("the tick draws");
    assert!(tick.contains('1'), "{tick:?}");
    assert!(!tick.contains("still"), "{tick:?}");
    assert!(
        tick.len() * 10 < first.len(),
        "{} of {}",
        tick.len(),
        first.len()
    );
}
