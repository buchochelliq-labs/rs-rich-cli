//! A child a `switch` keeps while hidden misses no update: shown again, it
//! draws what its signals say now (plan 0.0.23, item 6). Noctiluca's page
//! area, a switch between a reader view and a projection, showed the page
//! the hidden view last drew after another page was opened from a prompt.

use intuituive::prelude::*;

/// What each view shows: the page, in a plain label or in a scroll box
/// (whose offscreen buffer is what kept the old page).
fn app(scrolled: bool) -> App {
    App::new(move || {
        let page = signal(String::from("first page"));
        let view = signal(0usize);
        let body = switch(
            move || view.get(),
            move |view| match view {
                0 => label("reader view"),
                _ if scrolled => scroll(column([
                    text!("projection of {page}").fixed(1),
                    label("more").fixed(1),
                ])),
                _ => text!("projection of {page}"),
            },
        );
        column([body.fixed(3), label("status").fixed(1)])
            .on_key("r", move |_| view.update(|v| *v = 1 - *v))
            .on_key("o", move |cx| {
                cx.prompt("Open:", "", move |text, _| page.set(text));
            })
    })
}

fn first_row(driver: &intuituive::Driver) -> String {
    driver.screen().plain()[0].trim_end().to_string()
}

fn check(scrolled: bool) {
    let mut driver = app(scrolled).driver(40, 8);
    driver.settle();
    // The projection is built and shown, then hidden behind the reader view.
    driver.press("r");
    assert_eq!(first_row(&driver), "projection of first page");
    driver.press("r");
    assert_eq!(first_row(&driver), "reader view");
    // Another page, opened from the prompt while the projection is hidden.
    driver.press("o");
    driver.press("n e w enter");
    assert_eq!(first_row(&driver), "reader view");
    driver.press("r");
    assert_eq!(
        first_row(&driver),
        "projection of new",
        "scrolled {scrolled}"
    );
}

#[test]
fn a_hidden_label_shows_the_page_opened_from_a_prompt() {
    check(false);
}

#[test]
fn a_hidden_scroll_box_shows_the_page_opened_from_a_prompt() {
    check(true);
}

/// The same without a modal: the signal set by a key while the view is
/// hidden.
#[test]
fn a_hidden_scroll_box_shows_a_signal_set_while_hidden() {
    let app = App::new(|| {
        let page = signal(String::from("first page"));
        let view = signal(1usize);
        let body = switch(
            move || view.get(),
            move |view| match view {
                0 => label("reader view"),
                _ => scroll(column([text!("projection of {page}").fixed(1)])),
            },
        );
        column([body.fixed(3)])
            .on_key("r", move |_| view.update(|v| *v = 1 - *v))
            .on_key("n", move |_| page.set("new".into()))
    });
    let mut driver = app.driver(40, 4);
    driver.settle();
    assert_eq!(first_row(&driver), "projection of first page");
    driver.press("r n r");
    assert_eq!(first_row(&driver), "projection of new");
}
