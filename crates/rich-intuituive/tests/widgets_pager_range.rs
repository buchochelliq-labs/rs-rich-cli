//! A pager strip over a list, its dots, and a slider with two handles.

use std::cell::Cell;
use std::rc::Rc;

use intuituive::prelude::*;
use intuituive::{Driver, pager, range_slider};

fn row(driver: &Driver, at: usize) -> String {
    driver.screen().plain()[at].trim_end().to_string()
}

#[test]
fn a_pager_moves_with_keys_and_clicks_and_stops_at_the_ends() {
    let out: Rc<Cell<Option<Signal<usize>>>> = Rc::default();
    let set = out.clone();
    let app = App::new(move || {
        let page = signal(0usize);
        set.set(Some(page));
        column([
            text!("page {page}").fixed(1),
            pager(page, || 4).fixed(1),
            pager(page, || 4).pager_dots().fixed(1),
        ])
    });
    let mut driver = app.driver(20, 3);
    driver.settle();
    let page = out.get().unwrap();
    assert_eq!(row(&driver, 1), "‹ 1 of 4 ›");
    assert_eq!(row(&driver, 2), "● ○ ○ ○");
    // At the start, back does nothing; the focus is on the first strip.
    driver.press("left");
    assert_eq!(page.get_untracked(), 0);
    driver.press("right pagedown");
    assert_eq!(page.get_untracked(), 2);
    driver.press("end right");
    assert_eq!(page.get_untracked(), 3);
    assert_eq!(row(&driver, 1), "‹ 4 of 4 ›");
    assert_eq!(row(&driver, 2), "○ ○ ○ ●");
    // A click on a dot, and on the back arrow.
    driver.click(2, 2);
    assert_eq!(page.get_untracked(), 1);
    assert!(driver.click_text("‹"));
    assert_eq!(page.get_untracked(), 0);
    // The arrow at an end is greyed.
    let styled = intuituive::testing::styled(&driver);
    assert!(styled.contains("[dim]‹"), "{styled}");
    assert_eq!(driver.accessibility()[1].describe(), "slider: page 1 of 4");
}

#[test]
fn a_range_sliders_handles_never_cross() {
    let out: Rc<Cell<Option<(Signal<f64>, Signal<f64>)>>> = Rc::default();
    let set = out.clone();
    let app = App::new(move || {
        let (low, high) = (signal(30.0), signal(60.0));
        set.set(Some((low, high)));
        range_slider(low, high, 0.0..=100.0, 10.0).fixed(1)
    });
    let mut driver = app.driver(11, 1);
    driver.settle();
    let (low, high) = out.get().unwrap();
    assert_eq!(row(&driver, 0), "───●━━●────");
    // The low handle stops at the high one.
    driver.press("right right right right right");
    assert_eq!((low.get_untracked(), high.get_untracked()), (60.0, 60.0));
    // The high one, then, moves on alone.
    driver.press("space end");
    assert_eq!((low.get_untracked(), high.get_untracked()), (60.0, 100.0));
    // A click near the low end takes the low handle there.
    driver.click(1, 0);
    assert_eq!((low.get_untracked(), high.get_untracked()), (10.0, 100.0));
    // A drag from the high end takes the high one.
    use intuituive::interact::{Button, Event, Mouse, MouseKind};
    driver.send(Event::Mouse(Mouse::new(
        MouseKind::Down(Button::Left),
        10,
        0,
    )));
    driver.send(Event::Mouse(Mouse::new(
        MouseKind::Drag(Button::Left),
        7,
        0,
    )));
    driver.send(Event::Mouse(Mouse::new(MouseKind::Up(Button::Left), 7, 0)));
    driver.settle();
    assert_eq!((low.get_untracked(), high.get_untracked()), (10.0, 70.0));
    assert_eq!(driver.accessibility()[0].describe(), "slider: 10 to 70");
}
