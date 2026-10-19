//! The sprites example (examples/sprites.rs): a click selects the sprite
//! under the pointer, the arrows steer it, and the cast animates.

#[path = "../examples/sprites.rs"]
#[allow(dead_code)]
mod sprites;

use std::time::Duration;

use intuituive::interact::{Button, Event, Mouse, MouseKind};

#[test]
fn click_a_sprite_to_select_it_and_steer_it_with_the_arrows() {
    let mut driver = sprites::app().driver(80, 24);
    driver.settle();
    let rows = |driver: &intuituive::Driver| driver.screen().plain();
    // `n` cells of row `y` from column `x`.
    let at = |driver: &intuituive::Driver, x: usize, y: usize, n: usize| -> String {
        rows(driver)[y].chars().skip(x).take(n).collect()
    };
    assert!(rows(&driver)[23].contains("nothing selected"));
    // The ship at 4, 3, its flame below it; the ghost's top left at 30, 8.
    assert_eq!(at(&driver, 4, 3, 4), "·/\\·");
    assert_eq!(at(&driver, 4, 4, 4), "<##>");
    assert_eq!(at(&driver, 30, 8, 4), ".--.");
    driver.event(Event::Mouse(Mouse::new(
        MouseKind::Down(Button::Left),
        31,
        8,
    )));
    driver.settle();
    assert!(rows(&driver)[23].contains("ghost selected"));
    // Its marker is over it.
    assert_eq!(rows(&driver)[7].chars().nth(31), Some('▼'));
    driver.press("right right down");
    assert_eq!(at(&driver, 32, 9, 4), ".--.");
    // Steered, it stays put while the others drift.
    let before = rows(&driver)[9].clone();
    driver.advance(Duration::from_millis(500));
    assert_eq!(at(&driver, 32, 9, 4), ".--.");
    assert_ne!(rows(&driver), vec![before; 24]);
}

#[test]
fn the_ball_bounces_in_the_pixel_panel() {
    let mut driver = sprites::app().driver(80, 24);
    driver.settle();
    // The panel's cells' styles: every cell is a half block, its colours
    // are the picture.
    let panel = |driver: &intuituive::Driver| -> Vec<Option<intuituive::rich::Style>> {
        let screen = driver.screen();
        (1..16)
            .flat_map(|y| (57..79).map(move |x| (x, y)))
            .map(|(x, y)| screen.style(screen.cell(x, y).style).cloned())
            .collect()
    };
    assert!(driver.screen().plain()[0].contains("pixels"));
    let first = panel(&driver);
    driver.advance(Duration::from_millis(200));
    assert_ne!(panel(&driver), first);
}
