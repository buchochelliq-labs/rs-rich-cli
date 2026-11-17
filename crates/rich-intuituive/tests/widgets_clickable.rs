//! Clickable markup spans (`[@click=…]`, links), actions run by name,
//! status-bar segments and breadcrumb parts that take clicks.

use std::cell::RefCell;
use std::rc::Rc;

use intuituive::prelude::*;
use intuituive::widgets::{breadcrumb, seg, status_bar};
use intuituive::{Driver, SpanTarget};

fn shown(app: App, width: u16, height: u16) -> Driver {
    let mut driver = app.driver(width, height);
    driver.settle();
    driver
}

fn row(driver: &Driver, at: usize) -> String {
    driver.screen().plain()[at].trim_end().to_string()
}

#[test]
fn a_click_on_an_action_span_runs_the_binding_by_description_or_id() {
    let app = App::new(|| {
        let n = signal(0);
        column([
            label("[@click=add]add[/] [@click=twice]twice[/] plain"),
            text!("{n}"),
        ])
        .bind("+", "add", move |_| n.update(|n| *n += 1))
        .bind("*", "Double", move |_| n.update(|n| *n *= 2))
        .binding_id("twice")
    });
    let mut driver = shown(app, 30, 2);
    assert!(driver.click_text("add"));
    assert_eq!(row(&driver, 1), "1");
    assert!(driver.click_text("add"));
    assert!(driver.click_text("twice"));
    assert_eq!(row(&driver, 1), "4");
    // Text that is not a span does nothing.
    assert!(driver.click_text("plain"));
    assert_eq!(row(&driver, 1), "4");
}

#[test]
fn app_actions_by_name_quit_and_ring_the_bell() {
    let app = App::new(|| label("[@click=app.bell]ring[/] [@click=quit]bye[/]"));
    let mut driver = shown(app, 20, 1);
    assert_eq!(driver.take_bells(), 0);
    assert!(driver.click_text("ring"));
    assert_eq!(driver.take_bells(), 1);
    assert_eq!(driver.take_bells(), 0);
    assert!(!driver.is_done());
    assert!(driver.click_text("bye"));
    assert!(driver.is_done());
}

#[test]
fn a_span_handler_gets_actions_with_their_arguments_and_links() {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let log = seen.clone();
    let app = App::new(move || {
        let log = log.clone();
        label("[@click=open('a.txt', 3)]open[/] [link=https://example.com]site[/]")
            .on_span(move |_, target| log.borrow_mut().push(target.clone()))
    });
    let mut driver = shown(app, 20, 1);
    assert!(driver.click_text("open"));
    assert!(driver.click_text("site"));
    assert_eq!(
        *seen.borrow(),
        vec![
            SpanTarget::Action {
                name: "open".into(),
                args: vec!["a.txt".into(), "3".into()],
            },
            SpanTarget::Link("https://example.com".into()),
        ]
    );
}

#[test]
fn a_link_in_plain_text_is_not_clickable_without_a_handler() {
    // No handler and no focus_spans: a click on a link leaves it to the
    // terminal, and selecting text still starts there.
    let app = App::new(|| label("[link=https://example.com]site[/]"));
    let mut driver = shown(app, 10, 1);
    assert!(driver.click_text("site"));
    assert!(!driver.is_done());
}

#[test]
fn focused_spans_are_picked_with_the_arrows_and_run_with_enter() {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let log = seen.clone();
    let app = App::new(move || {
        let log = log.clone();
        column([
            label("first").focusable(),
            label("[@click=a]one[/] and [@click=b]two[/]")
                .focus_spans()
                .on_span(move |_, target| {
                    if let SpanTarget::Action { name, .. } = target {
                        log.borrow_mut().push(name.clone());
                    }
                }),
        ])
    });
    let mut driver = shown(app, 20, 2);
    driver.press("tab");
    // The first span is picked: reversed.
    let styled = intuituive::testing::styled(&driver);
    assert!(styled.contains("[reverse]one[/]"), "{styled}");
    driver.press("enter right enter left space");
    assert_eq!(*seen.borrow(), vec!["a", "b", "a"]);
    let styled = intuituive::testing::styled(&driver);
    assert!(styled.contains("[reverse]one[/]"), "{styled}");
}

#[test]
fn status_bar_segments_take_clicks() {
    let app = App::new(|| {
        let clicks = signal(0);
        status_bar(
            [
                seg(move || format!(" {} ", clicks.get())).style("white on blue"),
                seg(|| " still ".into()),
            ],
            [seg(|| " reset ".into()).on_click(move |_| clicks.set(0))],
        )
        .on_key("x", move |_| clicks.set(7))
    });
    let mut driver = shown(app, 30, 1);
    driver.press("x");
    assert!(driver.screen().plain()[0].starts_with(" 7 "));
    // A segment with no action: nothing.
    assert!(driver.click_text("still"));
    assert!(driver.screen().plain()[0].starts_with(" 7 "));
    assert!(driver.click_text("reset"));
    assert!(driver.screen().plain()[0].starts_with(" 0 "));
}

#[test]
fn breadcrumb_parts_report_their_index_even_when_cut() {
    let picked = Rc::new(RefCell::new(Vec::new()));
    let log = picked.clone();
    let app = App::new(move || {
        let log = log.clone();
        breadcrumb(|| vec!["alpha".into(), "beta".into(), "gamma".into()])
            .on_select(move |i, _| log.borrow_mut().push(i))
    });
    let mut driver = shown(app, 40, 1);
    assert!(driver.click_text("gamma"));
    assert!(driver.click_text("alpha"));
    // The separator: nothing.
    assert!(driver.click_text("›"));
    assert_eq!(*picked.borrow(), vec![2, 0]);
    // Cut from its start: `…ta › gamma`, the parts still found.
    driver.resize(11, 1);
    driver.settle();
    assert_eq!(row(&driver, 0), "…ta › gamma");
    assert!(driver.click_text("ta"));
    assert!(driver.click_text("gamma"));
    assert_eq!(*picked.borrow(), vec![2, 0, 1, 2]);
}
