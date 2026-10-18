//! Layers: children stacked on one rectangle (0.0.19 gap A8).

use std::time::Duration;

use intuituive::interact::{Button, Event, Mouse, MouseKind};
use intuituive::prelude::*;
use intuituive::{assert_screen, Driver};

fn shown(app: App, width: u16, height: u16) -> Driver {
    let mut driver = app.driver(width, height);
    driver.settle();
    driver
}

fn click(driver: &mut Driver, x: u16, y: u16) {
    driver.event(Event::Mouse(Mouse::new(
        MouseKind::Down(Button::Left),
        x,
        y,
    )));
    driver.event(Event::Mouse(Mouse::new(MouseKind::Up(Button::Left), x, y)));
    driver.settle();
}

#[test]
fn layers_badge() {
    let app = App::new(|| {
        let ticks = signal(0u32);
        every(Duration::from_millis(100), move |_| {
            ticks.update(|t| *t += 1)
        });
        layers([
            // A board that redraws every tick, all over its rectangle.
            text(move || {
                let t = ticks.get();
                (0..6)
                    .map(|row| format!("{:-<24}", format!("row {row} tick {t} ")))
                    .collect::<Vec<_>>()
                    .join("\n")
            }),
            label("on top").panel("Badge").inset(1, 2, 1, 10),
        ])
    });
    let mut driver = app.driver(24, 6);
    for step in 1..=3 {
        driver.update(Duration::from_millis(100 * step));
        let _ = driver.render();
    }
    driver.settle();
    assert_screen!(driver, "layers_badge");
    let rows = driver.screen().plain();
    // The base ticked three times, and the panel is still whole over it.
    assert!(rows[0].contains("tick 3"), "{rows:?}");
    assert_eq!(&rows[2][..10], "row 2 tick", "{rows:?}");
    assert!(rows[2].contains("│on top"), "{rows:?}");
}

#[test]
fn a_click_goes_to_the_top_layer() {
    let app = App::new(|| {
        let hit = signal("nothing");
        column([
            layers([
                // Deeper under the point, but under the top layer.
                column([row([label("base").on_click(move |_| hit.set("base"))])]),
                label("top")
                    .on_click(move |_| hit.set("top"))
                    .place(Align::Start, Align::Start),
            ])
            .fixed(2),
            text!("{hit}"),
        ])
    });
    let mut driver = shown(app, 10, 3);
    click(&mut driver, 1, 0);
    assert_eq!(driver.screen().plain()[2].trim_end(), "top");
    // Where the top layer is not, the one under it takes the click.
    click(&mut driver, 6, 0);
    assert_eq!(driver.screen().plain()[2].trim_end(), "base");
}

#[test]
fn the_tab_order_and_accessibility_follow_the_children() {
    let app = App::new(|| {
        layers([
            label("under").focusable().name("under"),
            label("over")
                .focusable()
                .name("over")
                .place(Align::End, Align::End),
        ])
    });
    let mut driver = shown(app, 12, 2);
    driver.assert_focused("under");
    driver.press("tab");
    driver.assert_focused("over");
    let names: Vec<String> = driver
        .accessibility()
        .iter()
        .map(|node| node.name.clone())
        .collect();
    // Each named by what it shows: the lower one without the upper one's
    // text over it.
    assert_eq!(names, ["under", "over"]);
    assert_eq!(driver.screen().plain(), ["under       ", "        over"]);
}

#[test]
fn a_layer_placed_by_its_content_sits_where_it_says() {
    let app = App::new(|| {
        layers([
            fill("·", ""),
            label("tl").place(Align::Start, Align::Start),
            label("c").place(Align::Center, Align::Center),
            label("br").place(Align::End, Align::End),
        ])
    });
    let rows = shown(app, 7, 3).screen().plain();
    assert_eq!(rows, ["tl·····", "···c···", "·····br"]);
}
