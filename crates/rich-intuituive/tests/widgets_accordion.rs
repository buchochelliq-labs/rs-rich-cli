//! An accordion: only one section open, opened from the app or a click,
//! and all closed.

use std::cell::Cell;
use std::rc::Rc;

use intuituive::prelude::*;
use intuituive::{Driver, accordion};

fn rows(driver: &Driver) -> Vec<String> {
    driver
        .screen()
        .plain()
        .iter()
        .map(|row| row.trim_end().to_string())
        .filter(|row| !row.is_empty())
        .collect()
}

fn app() -> (Driver, Signal<Option<usize>>) {
    let out: Rc<Cell<Option<Signal<Option<usize>>>>> = Rc::default();
    let set = out.clone();
    let app = App::new(move || {
        let open = signal(None);
        set.set(Some(open));
        accordion(
            open,
            [
                ("One", label("first")),
                ("Two", label("second")),
                ("Three", label("third")),
            ],
        )
    });
    let mut driver = app.driver(20, 6);
    driver.settle();
    (driver, out.get().unwrap())
}

#[test]
fn opening_one_closes_the_other() {
    let (mut driver, open) = app();
    assert_eq!(rows(&driver), ["▸ One", "▸ Two", "▸ Three"]);
    assert!(driver.click_text("Two"));
    assert_eq!(open.get_untracked(), Some(1));
    assert_eq!(rows(&driver), ["▸ One", "▾ Two", "second", "▸ Three"]);
    assert!(driver.click_text("Three"));
    assert_eq!(open.get_untracked(), Some(2));
    assert_eq!(rows(&driver), ["▸ One", "▸ Two", "▾ Three", "third"]);
    // Closing the open one leaves all closed.
    assert!(driver.click_text("Three"));
    assert_eq!(open.get_untracked(), None);
    assert_eq!(rows(&driver), ["▸ One", "▸ Two", "▸ Three"]);
}

#[test]
fn the_app_opens_one_by_its_signal() {
    let (mut driver, open) = app();
    open.set(Some(0));
    driver.settle();
    assert_eq!(rows(&driver), ["▾ One", "first", "▸ Two", "▸ Three"]);
    open.set(Some(2));
    driver.settle();
    assert_eq!(rows(&driver), ["▸ One", "▸ Two", "▾ Three", "third"]);
    // The keys, as a collapsible's: → opens the focused header.
    driver.press("right");
    assert_eq!(open.get_untracked(), Some(0));
    assert_eq!(rows(&driver), ["▾ One", "first", "▸ Two", "▸ Three"]);
}
