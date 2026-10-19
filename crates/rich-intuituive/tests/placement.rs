//! Placing things on the screen: `Ctx::popup_at` from a point of the
//! screen, clamped to it, and `Node::rect_handle`.

use intuituive::prelude::*;
use intuituive::screen::Rect;
use intuituive::{Offset, Origin, RectHandle};

/// An app whose `o` opens a 6 x 3 box placed by `origin` and `offset`.
fn opener(origin: Origin, offset: Offset) -> App {
    App::new(move || {
        label("").on_key("o", move |cx| {
            cx.popup_at(origin, offset, || label("hi").panel(""))
        })
    })
}

/// Where the pop-up's top left corner is.
fn corner(app: App, width: u16, height: u16) -> (usize, usize) {
    let mut driver = app.driver(width, height);
    driver.press("o");
    let rows = driver.screen().plain();
    let y = rows.iter().position(|r| r.contains('╭')).expect("a box");
    let x = rows[y].chars().position(|c| c == '╭').unwrap();
    (x, y)
}

#[test]
fn origins_are_corners_edges_and_the_centre() {
    let size = Offset::new(0, 0, 6, 3);
    let cases = [
        (Origin::TopLeft, (0, 0)),
        (Origin::TopCenter, (7, 0)),
        (Origin::TopRight, (14, 0)),
        (Origin::Center, (7, 3)),
        (Origin::BottomLeft, (0, 7)),
        (Origin::BottomCenter, (7, 7)),
        (Origin::BottomRight, (14, 7)),
    ];
    for (origin, at) in cases {
        assert_eq!(corner(opener(origin, size), 20, 10), at, "{origin:?}");
    }
}

#[test]
fn the_offset_moves_it_and_the_screen_keeps_it() {
    assert_eq!(
        corner(opener(Origin::TopCenter, Offset::new(2, 1, 6, 3)), 20, 10),
        (9, 1)
    );
    assert_eq!(
        corner(
            opener(Origin::BottomRight, Offset::new(-1, -1, 6, 3)),
            20,
            10
        ),
        (13, 6)
    );
    // Pushed off the screen, it is moved back inside.
    assert_eq!(
        corner(opener(Origin::TopRight, Offset::new(30, -5, 6, 3)), 20, 10),
        (14, 0)
    );
    // Larger than the screen, it is made smaller.
    let mut driver = opener(Origin::Center, Offset::new(0, 0, 50, 3)).driver(20, 5);
    driver.press("o");
    assert_eq!(driver.screen().plain()[1], "╭──────────────────╮");
}

#[test]
fn at_a_rect_it_goes_below_or_above_when_there_is_no_room() {
    let below = Origin::At(Rect::new(3, 2, 10, 1));
    assert_eq!(
        corner(opener(below, Offset::new(0, 0, 6, 3)), 20, 10),
        (3, 3)
    );
    let low = Origin::At(Rect::new(3, 8, 10, 1));
    assert_eq!(corner(opener(low, Offset::new(0, 0, 6, 3)), 20, 10), (3, 5));
}

#[test]
fn it_closes_on_esc_and_on_a_press_outside_like_any_pop_up() {
    let mut driver = opener(Origin::TopLeft, Offset::new(0, 0, 6, 3)).driver(20, 6);
    driver.press("o");
    assert!(driver.screen().plain()[0].contains('╭'));
    driver.press("esc");
    assert!(!driver.screen().plain()[0].contains('╭'));
}

#[test]
fn a_size_can_be_the_contents() {
    let app = App::new(|| {
        label("").on_key("o", |cx| {
            let offset = Offset::sized(Size::Auto, Size::Auto);
            cx.popup_at(Origin::Center, offset, || label("hello").panel(""))
        })
    });
    let mut driver = app.driver(21, 7);
    driver.press("o");
    assert_eq!(driver.screen().plain()[3], "       │hello│       ");
}

#[test]
fn a_rect_handle_follows_the_node_on_the_screen() {
    let mut handle: Option<RectHandle> = None;
    let app = App::new(|| {
        let side = label("side");
        handle = Some(side.rect_handle());
        row([label("main").flex(1), side.fixed(4).hide_below(15)])
    });
    let handle = handle.expect("made while building");
    let mut driver = app.driver(20, 2);
    driver.settle();
    assert_eq!(handle.get_untracked(), Some(Rect::new(16, 0, 4, 2)));
    // Hidden, it has none; back, it has its new place.
    driver.resize(12, 2);
    driver.settle();
    assert_eq!(handle.get_untracked(), None);
    driver.resize(30, 2);
    driver.settle();
    assert_eq!(handle.get_untracked(), Some(Rect::new(26, 0, 4, 2)));
}

#[test]
fn a_rect_handle_under_a_pushed_screen_has_none() {
    let mut handle: Option<RectHandle> = None;
    let app = App::new(|| {
        let node = label("here");
        handle = Some(node.rect_handle());
        column([node.fixed(1)]).on_key("p", |cx| cx.push(|| label("over")))
    });
    let handle = handle.expect("made while building");
    let mut driver = app.driver(20, 3);
    driver.settle();
    assert!(handle.get_untracked().is_some());
    // A full screen covers it: it is nowhere on the screen.
    driver.press("p");
    assert_eq!(handle.get_untracked(), None);
}

#[test]
fn a_rect_handle_inside_a_scroll_is_on_the_screen() {
    let mut handle: Option<RectHandle> = None;
    let app = App::new(|| {
        let rows: Vec<Node> = (0..20)
            .map(|i| {
                let node = label(format!("row {i}")).fixed(1);
                if i == 5 {
                    handle = Some(node.rect_handle());
                }
                node
            })
            .collect();
        column([label("title").fixed(1), scroll(column(rows))])
    });
    let handle = handle.expect("made while building");
    let mut driver = app.driver(20, 4);
    driver.settle();
    // Row 5 is below the view.
    assert_eq!(handle.get_untracked(), None);
    driver.press("down down down");
    assert_eq!(handle.get_untracked(), Some(Rect::new(0, 3, 19, 1)));
}

#[test]
fn reading_a_rect_handle_redraws_when_the_node_moves() {
    let app = App::new(|| {
        let side = label("x");
        let handle = side.rect_handle();
        column([
            row([label("pad").fixed(5).hide_below(15), side]).fixed(1),
            text(move || format!("{:?}", handle.get().map(|r| r.x))),
        ])
    });
    let mut driver = app.driver(20, 2);
    driver.settle();
    assert_eq!(driver.screen().plain()[1].trim_end(), "Some(5)");
    driver.resize(12, 2);
    driver.settle();
    assert_eq!(driver.screen().plain()[1].trim_end(), "Some(0)");
}

#[test]
fn a_handle_made_before_its_node_is_set_once_it_is_given_one() {
    let app = App::new(|| {
        // App state holds the handle before the view is built, in a signal.
        let status: Signal<Option<RectHandle>> = signal(None);
        let handle = RectHandle::new();
        status.set(Some(handle));
        assert_eq!(handle.get_untracked(), None);
        column([
            text(move || {
                let rect = status.get().and_then(|h| h.get());
                format!("{:?}", rect.map(|r| (r.y, r.width)))
            }),
            label("status").fixed(1).rect_into(&handle),
        ])
    });
    let mut driver = app.driver(30, 4);
    driver.settle();
    assert_eq!(driver.screen().plain()[0].trim_end(), "Some((3, 30))");
}
