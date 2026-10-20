//! The demo loop's seventh round's bugs (0.0.21 workstream 1): each test is
//! the minimal failing case from its clone's report in `docs/design/gaps/`.

use intuituive::command_line::{command_line, CommandLine, Mode};
use intuituive::prelude::*;

/// tasks: a `cx.focus(x)` just before `cx.modal(..)` is where the focus
/// goes back to when the modal closes.
#[test]
fn a_modal_opened_after_a_focus_change_gives_the_focus_back_to_the_new_node() {
    let app = App::new(|| {
        let said = signal(String::new());
        let a = label("a")
            .focusable()
            .on_key("x", move |_| said.set("a".into()));
        let a_id = a.id();
        let b = label("b").focusable().autofocus().on_key("m", move |cx| {
            cx.focus(a_id);
            cx.modal(Size::Fixed(6), Size::Fixed(1), || {
                label("modal").on_any_key(|cx, _| {
                    cx.pop();
                    true
                })
            });
        });
        column([a.fixed(1), b.fixed(1), text!("{said}").fixed(1)])
    });
    let mut d = app.driver(10, 3);
    d.press("m space x");
    assert_eq!(d.screen().plain()[2].trim_end(), "a");
}

/// tasks: a node a watch focuses as the app starts, inside a `switch`'s
/// page, is where a command line gives the focus back to when it closes.
#[test]
fn a_command_line_gives_the_focus_back_to_a_node_focused_as_the_app_starts() {
    let app = App::new(|| {
        let cmd = CommandLine::new();
        let said = signal(String::new());
        let page = signal(0usize);
        // Built first, shown by the switch: focused before it is laid out.
        let report = label("report")
            .focusable()
            .name("report")
            .on_key("x", move |_| said.set("x".into()));
        let id = report.id();
        let pages = std::cell::RefCell::new(Some(report));
        let body = switch(
            move || page.get(),
            move |_| pages.borrow_mut().take().unwrap_or_else(|| label("")),
        );
        watch(move || page.get(), move |_, cx| cx.focus(id));
        column([body.fixed(1), text!("{said}").fixed(1), command_line(cmd)])
            .on_key("a", move |cx| cmd.open(cx, Mode::new("add ")))
    });
    let mut d = app.driver(20, 3);
    d.settle();
    d.press("a");
    d.press("esc");
    assert_eq!(d.focused_name().as_deref(), Some("report"));
}

/// tasks: a focus move made after the modal opens still goes into it.
#[test]
fn a_focus_change_after_a_modal_opens_still_applies_on_the_modal() {
    let app = App::new(|| {
        let said = signal(String::new());
        let b = label("b").focusable().autofocus().on_key("m", move |cx| {
            cx.modal(Size::Fixed(6), Size::Fixed(2), move || {
                column([
                    label("one").focusable().name("one").fixed(1),
                    label("two")
                        .focusable()
                        .name("two")
                        .on_key("x", move |_| said.set("two".into()))
                        .fixed(1),
                ])
            });
            cx.focus_next();
        });
        column([b.fixed(1), text!("{said}").fixed(1)])
    });
    let mut d = app.driver(10, 3);
    d.press("m");
    d.settle();
    assert!(d.focused_name().is_some());
}

/// blocks: a styled `layers` node covers its whole rectangle, the gaps its
/// inset children leave too.
#[test]
fn a_styled_layers_node_covers_the_gaps_its_inset_children_leave() {
    let app = App::new(|| {
        layers([
            fill("x", "red on blue"),
            layers([label("a").inset(0, 0, 0, 1)]).style("on green"),
        ])
    });
    let mut driver = app.driver(5, 1);
    driver.settle();
    assert_eq!(driver.screen().plain()[0], " a   ");
}

/// disk: a thin rest does not take every tile.
#[test]
fn a_thin_rest_does_not_swallow_the_tiles() {
    use intuituive::layout::squarify;
    use intuituive::screen::Rect;
    let weights = [15000.0, 100.0, 100.0, 100.0, 100.0, 100.0, 100.0];
    let map = squarify(Rect::new(0, 0, 79, 20), &weights, (8, 3), 2.5);
    assert!(!map.tiles[0].is_empty(), "{map:?}");
    let (rest, count) = map.rest.expect("the small ones share the rest");
    assert_eq!(count, 6);
    assert!(rest.width >= 8 && rest.height >= 3, "{map:?}");
    // Edge to edge: the tile and the rest cover the rectangle.
    let area = |r: Rect| r.width as u32 * r.height as u32;
    assert_eq!(area(map.tiles[0]) + area(rest), 79 * 20);
}

/// csv: a `previews_keys` container round a `command_line` leaves the line
/// the letters typed into it, and the Enter that submits it.
#[test]
fn a_previewing_container_leaves_a_command_line_its_typed_letters() {
    let app = App::new(|| {
        let cmd = CommandLine::new();
        let went = signal(String::new());
        let opened = signal(0u32);
        let entered = signal(false);
        column([
            text!("went {went} opened {opened} entered {entered}").fixed(1),
            command_line(cmd),
        ])
        .previews_keys()
        .on_key("enter", move |_| entered.set(true))
        .on_key("2", move |cx| {
            opened.update(|n| *n += 1);
            cmd.open(
                cx,
                Mode::new("Go to: ").on_submit(move |text, _| {
                    went.set(text.to_string());
                    Ok(())
                }),
            )
        })
    });
    let mut driver = app.driver(30, 3);
    driver.press("2");
    driver.type_text("42");
    driver.press("enter");
    assert_eq!(
        driver.screen().plain()[0].trim_end(),
        "went 42 opened 1 entered false",
        "{:?}",
        driver.screen().plain()
    );
}
