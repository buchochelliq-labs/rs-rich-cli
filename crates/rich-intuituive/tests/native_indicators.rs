//! A dial, a rating and a carousel: generic widgets for a value in a
//! range, a score in stars and a paged strip of items, driven as a person
//! would.

use std::cell::RefCell;
use std::rc::Rc;

use intuituive::interact::{Button, Event, Modifiers, Mouse, MouseKind};
use intuituive::pixels::PixelMode;
use intuituive::prelude::*;
use intuituive::{Driver, assert_screen, carousel, dial, rating, rating_input};

fn mouse(driver: &mut Driver, kind: MouseKind, column: u16, row: u16) {
    driver.event(Event::Mouse(Mouse::new(kind, column, row)));
    driver.settle();
}

fn click(driver: &mut Driver, column: u16, row: u16) {
    mouse(driver, MouseKind::Down(Button::Left), column, row);
    mouse(driver, MouseKind::Up(Button::Left), column, row);
}

fn rows(driver: &Driver) -> Vec<String> {
    driver
        .screen()
        .plain()
        .into_iter()
        .map(|r| r.trim_end().to_string())
        .collect()
}

/// The definition of the style at a cell.
fn style_at(driver: &Driver, column: u16, row: u16) -> String {
    let screen = driver.screen();
    screen
        .style(screen.cell(column, row).style)
        .map(|s| s.definition())
        .unwrap_or_default()
}

fn braille(c: char) -> bool {
    ('\u{2801}'..='\u{28ff}').contains(&c)
}

// Dial.

/// A download dial over a signal, the way a speed test shows one.
fn download(speed: Signal<f64>) -> Node {
    dial(move || speed.get(), 0.0..=1000.0)
        .unit("Mbps")
        .caption("Download")
}

#[test]
fn a_dial_draws_an_arc_with_its_reading_unit_and_ends() {
    let app = App::new(|| download(signal(287.4)));
    let mut driver = app.driver(30, 13);
    driver.settle();
    assert_screen!(driver, "dial_download");
    let shown = rows(&driver);
    assert!(shown.iter().any(|r| r.chars().any(braille)), "{shown:?}");
    // The range's ends under the arc's ends, the caption at the bottom.
    let ends = &shown[11];
    assert!(
        ends.trim_start().starts_with('0') && ends.ends_with("1000"),
        "{ends:?}"
    );
    assert_eq!(shown[12].trim(), "Download");
    let tree = driver.accessibility();
    assert_eq!(tree[0].describe(), "Download, meter: 287 Mbps");
    assert!(
        tree[0]
            .aria_attributes()
            .contains(&("aria-valuetext", "287 Mbps".to_string()))
    );
}

#[test]
fn a_dial_follows_its_signal() {
    let speed = Rc::new(RefCell::new(None));
    let held = speed.clone();
    let app = App::new(move || {
        let speed = signal(0.0);
        *held.borrow_mut() = Some(speed);
        download(speed)
    });
    let mut driver = app.driver(30, 13);
    driver.settle();
    let at_zero = rows(&driver);
    let speed = speed.borrow().expect("built");
    speed.set(940.0);
    driver.settle();
    let at_top = rows(&driver);
    assert_ne!(at_zero, at_top);
    assert_eq!(
        driver.accessibility()[0].describe(),
        "Download, meter: 940 Mbps"
    );
    // The digits changed with it: 9 in big digits.
    assert!(at_top.join("\n").contains("└─┤"), "{at_top:?}");
}

#[test]
fn a_dial_fills_in_the_style_of_the_zone_its_value_is_in() {
    let level = Rc::new(RefCell::new(None));
    let held = level.clone();
    let app = App::new(move || {
        let level = signal(30.0);
        *held.borrow_mut() = Some(level);
        dial(move || level.get(), 0.0..=100.0)
            .zones([(50.0, "green"), (80.0, "yellow"), (100.0, "red")])
            .range_labels(false)
    });
    let mut driver = app.driver(20, 9);
    driver.settle();
    // The reading is drawn in the zone's colour.
    let reading_at = |driver: &Driver| {
        let shown = rows(driver);
        let y = shown
            .iter()
            .position(|r| r.contains("╶─┐") || r.contains("╷ ╷") || r.contains("┌─"))
            .expect("big digits");
        let x = shown[y]
            .chars()
            .position(|c| "┌╶╷".contains(c))
            .expect("a digit");
        style_at(driver, x as u16, y as u16)
    };
    assert!(
        reading_at(&driver).contains("green"),
        "{}",
        reading_at(&driver)
    );
    level.borrow().expect("built").set(95.0);
    driver.settle();
    assert!(
        reading_at(&driver).contains("red"),
        "{}",
        reading_at(&driver)
    );
}

#[test]
fn bands_colour_a_dial_as_a_browser_colours_a_meter() {
    // The optimum is high: a low value is in the far region, bad.
    let app = App::new(|| dial(|| 10.0, 0.0..=100.0).bands(25.0, 75.0, 90.0));
    let mut driver = app.driver(20, 1);
    driver.settle();
    // One row: a bar and the reading, in the bad style.
    let shown = rows(&driver)[0].clone();
    assert!(shown.ends_with(" 10"), "{shown:?}");
    assert!(
        style_at(&driver, 0, 0).contains("red"),
        "{}",
        style_at(&driver, 0, 0)
    );
}

#[test]
fn a_dial_in_half_blocks() {
    let app = App::new(|| {
        dial(|| 62.0, 0.0..=100.0)
            .unit("%")
            .arc_mode(PixelMode::HalfBlocks)
    });
    let mut driver = app.driver(20, 9);
    driver.settle();
    assert_screen!(driver, "dial_half_blocks");
    assert!(rows(&driver).iter().any(|r| r.contains('█')));
}

#[test]
fn a_small_dial_is_a_bar_with_its_reading() {
    let app = App::new(|| dial(|| 42.0, 0.0..=100.0).unit("%").caption("CPU"));
    let mut driver = app.driver(24, 1);
    driver.settle();
    assert_eq!(rows(&driver)[0], "CPU ██████▎░░░░░░░░ 42 %");
    // Narrower, the caption goes first.
    driver.resize(10, 1);
    driver.settle();
    assert_eq!(rows(&driver)[0], "██▏░░ 42 %");
}

#[test]
fn a_half_circle_dial_and_a_readout_of_its_own() {
    let app = App::new(|| {
        dial(|| 0.5, 0.0..=1.0)
            .sweep(180.0)
            .readout(|| "half".into())
    });
    let mut driver = app.driver(24, 8);
    driver.settle();
    assert_screen!(driver, "dial_half_circle");
    assert_eq!(driver.accessibility()[0].describe(), "meter: half");
}

#[test]
fn in_text_mode_a_dial_is_a_bar_of_hashes() {
    let app = App::new(|| dial(|| 50.0, 0.0..=100.0).unit("%")).accessible(true);
    let mut driver = app.driver(20, 6);
    driver.settle();
    let shown = rows(&driver);
    let bar = shown.iter().find(|r| !r.is_empty()).expect("a row");
    assert!(
        bar.starts_with("#####") && bar.ends_with("50 %"),
        "{shown:?}"
    );
}

// Rating.

#[test]
fn a_rating_draws_full_half_and_empty_stars_and_its_number() {
    let app = App::new(|| {
        column([
            rating(|| 4.5, 5).fixed(1),
            rating(|| 3.2, 10).fixed(1),
            rating(|| 4.0, 5).label("Rating").fixed(1),
            rating(|| 9.0, 5).fixed(1),
        ])
    });
    let mut driver = app.driver(20, 4);
    driver.settle();
    assert_eq!(
        rows(&driver),
        ["★★★★½ 4.5", "★★★☆☆☆☆☆☆☆ 3.2", "★★★★☆ 4", "★★★★★ 5"]
    );
    let said: Vec<String> = driver
        .accessibility()
        .iter()
        .map(|n| n.describe())
        .collect();
    assert_eq!(
        said,
        [
            "4.5 out of 5, image",
            "3.2 out of 10, image",
            "Rating: 4 out of 5, image",
            "5 out of 5, image"
        ]
    );
}

#[test]
fn a_rating_input_is_set_by_keys_digits_clicks_and_the_wheel() {
    let held = Rc::new(RefCell::new(None));
    let keep = held.clone();
    let app = App::new(move || {
        let stars = signal(2.0);
        *keep.borrow_mut() = Some(stars);
        rating_input(stars, 5).label("Yours")
    });
    let mut driver = app.driver(10, 1);
    driver.settle();
    let stars = || held.borrow().expect("built").get_untracked();
    driver.press("right");
    assert_eq!(stars(), 3.0);
    driver.press("left left left left");
    assert_eq!(stars(), 0.0);
    driver.press("end");
    assert_eq!(stars(), 5.0);
    driver.press("home 4");
    assert_eq!(stars(), 4.0);
    // 9 is more than five stars: not taken.
    driver.press("9");
    assert_eq!(stars(), 4.0);
    // A click sets up to the star; again, half of it.
    click(&mut driver, 1, 0);
    assert_eq!(stars(), 2.0);
    click(&mut driver, 1, 0);
    assert_eq!(stars(), 1.5);
    driver.settle();
    assert_eq!(rows(&driver)[0], "★½☆☆☆ 1.5");
    mouse(&mut driver, MouseKind::ScrollUp, 0, 0);
    assert_eq!(stars(), 2.0);
    driver.settle();
    let tree = driver.accessibility();
    assert_eq!(tree[0].describe(), "Yours, slider: 2 out of 5");
    assert!(
        tree[0]
            .aria_attributes()
            .contains(&("aria-valuetext", "2 out of 5".to_string()))
    );
}

#[test]
fn in_text_mode_a_rating_is_its_number() {
    let app = App::new(|| rating(|| 3.5, 5)).accessible(true);
    let mut driver = app.driver(10, 1);
    driver.settle();
    assert_eq!(rows(&driver)[0], "3.5/5");
}

// Carousel.

/// A shelf of `len` items, each a focusable panel named `item<n>`, its
/// first item in `first`; the pages turned to are logged in `turned`.
fn shelf(len: usize, first: Signal<usize>, turned: Rc<RefCell<Vec<usize>>>) -> Node {
    carousel(
        move || len,
        first,
        |i| {
            label(format!("item {}", i + 1))
                .panel("")
                .focusable()
                .name(&format!("item{}", i + 1))
        },
    )
    .label("Shelf")
    .on_page(move |to| turned.borrow_mut().push(to))
}

#[test]
fn a_carousel_shows_a_page_of_items_with_arrows_and_a_counter() {
    let app = App::new(|| shelf(12, signal(0), Rc::default()));
    let mut driver = app.driver(50, 5);
    driver.settle();
    assert_screen!(driver, "carousel_first_page");
    assert_eq!(
        driver.accessibility()[0].describe(),
        "Shelf, region: 1–2 of 12"
    );
    // At the first page, ‹ is dimmed and › is not.
    let (back, next) = (style_at(&driver, 0, 1), style_at(&driver, 49, 1));
    assert_ne!(back, next);
}

#[test]
fn a_carousel_pages_by_keys_and_keeps_the_last_page_full() {
    let turned = Rc::new(RefCell::new(Vec::new()));
    let log = turned.clone();
    let app = App::new(move || shelf(7, signal(0), log.clone()).per_page(3));
    let mut driver = app.driver(50, 5);
    driver.settle();
    driver.press("]");
    assert_eq!(rows(&driver)[4].trim(), "4–6 of 7");
    driver.press("]");
    assert_eq!(rows(&driver)[4].trim(), "5–7 of 7");
    // At the end, ] does nothing.
    driver.press("]");
    assert_eq!(rows(&driver)[4].trim(), "5–7 of 7");
    driver.press("[ [");
    assert_eq!(rows(&driver)[4].trim(), "1–3 of 7");
    assert_eq!(*turned.borrow(), [3, 4, 1, 0]);
}

#[test]
fn paging_from_inside_an_item_moves_the_focus_into_the_new_page() {
    let app = App::new(|| shelf(12, signal(0), Rc::default()).per_page(2));
    let mut driver = app.driver(40, 5);
    driver.settle();
    driver.press("tab tab");
    driver.assert_focused("item2");
    driver.press("]");
    driver.settle();
    driver.assert_focused("item3");
    assert!(rows(&driver)[1].contains("item 3"));
}

#[test]
fn the_strip_itself_pages_with_arrows_home_and_end() {
    let first = Rc::new(RefCell::new(None));
    let held = first.clone();
    let app = App::new(move || {
        let first = signal(0usize);
        *held.borrow_mut() = Some(first);
        // Items that do not take the focus: the strip has it.
        carousel(|| 9, first, |i| label(format!("item {}", i + 1))).per_page(3)
    });
    let mut driver = app.driver(40, 2);
    driver.settle();
    let first = || first.borrow().expect("built").get_untracked();
    driver.press("right");
    assert_eq!(first(), 3);
    driver.press("end");
    assert_eq!(first(), 6);
    driver.press("left home");
    assert_eq!(first(), 0);
    assert_eq!(rows(&driver)[1].trim(), "1–3 of 9");
}

#[test]
fn clicks_on_the_arrows_and_the_wheel_with_shift_turn_pages() {
    let app = App::new(|| shelf(12, signal(0), Rc::default()).per_page(2));
    let mut driver = app.driver(40, 5);
    driver.settle();
    click(&mut driver, 39, 1);
    assert_eq!(rows(&driver)[4].trim(), "3–4 of 12");
    click(&mut driver, 0, 1);
    assert_eq!(rows(&driver)[4].trim(), "1–2 of 12");
    // The wheel alone scrolls what the strip is in; with Shift, it pages.
    mouse(&mut driver, MouseKind::ScrollDown, 10, 1);
    assert_eq!(rows(&driver)[4].trim(), "1–2 of 12");
    let shifted = Mouse {
        modifiers: Modifiers {
            shift: true,
            ..Modifiers::default()
        },
        ..Mouse::new(MouseKind::ScrollDown, 10, 1)
    };
    driver.event(Event::Mouse(shifted));
    driver.settle();
    assert_eq!(rows(&driver)[4].trim(), "3–4 of 12");
}

#[test]
fn a_carousel_follows_its_width_and_the_app_sets_its_page() {
    let first = Rc::new(RefCell::new(None));
    let held = first.clone();
    let app = App::new(move || {
        let first = signal(0usize);
        *held.borrow_mut() = Some(first);
        shelf(12, first, Rc::default()).item_width(10)
    });
    let mut driver = app.driver(40, 5);
    driver.settle();
    // 36 cells inside the arrows: three items of 10 and the gaps.
    assert_eq!(rows(&driver)[4].trim(), "1–3 of 12");
    driver.resize(60, 5);
    driver.settle();
    assert_eq!(rows(&driver)[4].trim(), "1–5 of 12");
    first.borrow().expect("built").set(10);
    driver.settle();
    assert_eq!(rows(&driver)[4].trim(), "11–12 of 12");
    assert!(rows(&driver)[1].contains("item 12"));
}

// The example.

#[path = "../examples/indicators.rs"]
#[allow(dead_code)]
mod indicators;

#[test]
fn the_indicators_example_shows_two_dials_a_rating_and_a_shelf() {
    let mut driver = indicators::indicators_app(true).driver(72, 26);
    driver.settle();
    assert_screen!(driver, "indicators_example");
    let said: Vec<String> = driver
        .accessibility()
        .iter()
        .map(|n| n.describe())
        .collect();
    assert!(
        said.contains(&"Download, meter: 287 Mbps".to_string()),
        "{said:?}"
    );
    assert!(
        said.contains(&"Upload, meter: 41.8 Mbps".to_string()),
        "{said:?}"
    );
    assert!(
        said.contains(&"Your rating, slider: 4 out of 5".to_string()),
        "{said:?}"
    );
    // Tab to the shelf, and on a page.
    driver.press("tab ]");
    let shown = rows(&driver).join("\n");
    assert!(shown.contains("Stove") || shown.contains("Rope"), "{shown}");
}
