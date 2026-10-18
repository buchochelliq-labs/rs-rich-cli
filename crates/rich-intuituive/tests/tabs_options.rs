//! A tab strip's options (gap batch B4): its divider, the padding round
//! each title, and where the titles sit.

use intuituive::assert_screen;
use intuituive::interact::{Button, Event, Mouse, MouseKind};
use intuituive::prelude::*;
use intuituive::widgets::tabs;

fn titles() -> Vec<String> {
    vec!["Recipe".into(), "Email".into(), "Traceroute".into()]
}

fn strip(build: impl Fn(Signal<usize>) -> Node + 'static, width: u16) -> intuituive::Driver {
    let app = App::new(move || {
        let tab = signal(0usize);
        column([build(tab).fixed(1), text!("{tab}").fixed(1)])
    });
    let mut driver = app.driver(width, 2);
    driver.settle();
    driver
}

fn first_row(driver: &intuituive::Driver) -> String {
    driver.screen().plain()[0].trim_end().to_string()
}

#[test]
fn tabs_dot_divider() {
    let driver = strip(|tab| tabs(titles, tab).divider(" • "), 40);
    assert_screen!(driver, "tabs_dot_divider");
    assert_eq!(first_row(&driver), " Recipe  •  Email  •  Traceroute");
}

#[test]
fn tabs_no_padding() {
    // demo2's strip: no padding, a space for a divider.
    let driver = strip(|tab| tabs(titles, tab).divider(" ").tab_padding("", ""), 40);
    assert_screen!(driver, "tabs_no_padding");
    assert_eq!(first_row(&driver), "Recipe Email Traceroute");
}

#[test]
fn titles_sit_at_the_centre_or_the_end() {
    // 29 cells of titles in 33: four to spare.
    let driver = strip(|tab| tabs(titles, tab).titles_align(Align::End), 33);
    assert_eq!(first_row(&driver), "     Recipe │ Email │ Traceroute");
    let driver = strip(|tab| tabs(titles, tab).titles_align(Align::Center), 33);
    assert_eq!(first_row(&driver), "   Recipe │ Email │ Traceroute");
}

#[test]
fn a_click_hits_the_right_title_with_any_divider_and_padding() {
    let mut driver = strip(
        |tab| tabs(titles, tab).divider(" :: ").tab_padding("[", "]"),
        40,
    );
    // `[Recipe] :: [Email] :: [Traceroute]`: column 14 is in "Email".
    assert_eq!(first_row(&driver), "[Recipe] :: [Email] :: [Traceroute]");
    let press = |column| Event::Mouse(Mouse::new(MouseKind::Down(Button::Left), column, 0));
    driver.event(press(14));
    driver.settle();
    assert_eq!(driver.screen().plain()[1].trim_end(), "1");
    // On the divider, nothing.
    driver.event(press(9));
    driver.settle();
    assert_eq!(driver.screen().plain()[1].trim_end(), "1");
    driver.event(press(30));
    driver.settle();
    assert_eq!(driver.screen().plain()[1].trim_end(), "2");
}

#[test]
fn measure_counts_the_divider() {
    // As wide as it measures, in a row of its own size: nothing is cut.
    let app = App::new(|| {
        let tab = signal(0usize);
        row([
            tabs(titles, tab).divider(" • ").tab_padding("", "").auto(),
            label("|"),
        ])
    });
    let mut driver = app.driver(40, 1);
    driver.settle();
    assert_eq!(first_row(&driver), "Recipe • Email • Traceroute|");
}
