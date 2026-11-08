//! Layers: children stacked on one rectangle (0.0.19 gap A8).

use std::time::Duration;

use intuituive::interact::{Button, Event, Mouse, MouseKind};
use intuituive::prelude::*;
use intuituive::{Driver, assert_screen, button};

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

#[test]
fn dynamic_layer_wrapper_can_leave_outside_pointer_hits_to_underlay() {
    let hits = std::rc::Rc::new(std::cell::Cell::new(0));
    let h = hits.clone();
    let app = App::new(move || {
        let h = h.clone();
        layers([
            label("under").on_click(move |_| h.set(h.get() + 1)),
            keyed(
                || true,
                |_| {
                    layers([label("dialog").on_click(|_| {}).inset(1, 4, 1, 4)])
                        .children_cover_only()
                },
            )
            .children_cover_only(),
        ])
    });
    let mut d = shown(app, 20, 5);
    let names = d
        .accessibility()
        .iter()
        .map(|node| node.name.clone())
        .collect::<Vec<_>>();
    assert!(
        names.iter().any(|name| name.contains("under")),
        "outside content lost from accessibility: {names:?}"
    );
    click(&mut d, 0, 0);
    assert_eq!(hits.get(), 1, "the wrapper blocked a hit outside its child");
}

#[test]
fn owner_controlled_modal_scope_traps_focus_and_restores_it() {
    use std::{cell::Cell, rc::Rc};
    let state = Rc::new(Cell::new(None));
    let handle = state.clone();
    let hits = Rc::new(Cell::new(0));
    let h = hits.clone();
    let app = App::new(move || {
        let h = h.clone();
        let open = signal(false);
        handle.set(Some(open));
        layers([
            label("under")
                .focusable()
                .name("under")
                .on_click(move |_| h.set(h.get() + 1)),
            column([
                label("first").focusable().name("first"),
                label("second").focusable().name("second"),
            ])
            .controlled_modal()
            .inset(1, 2, 1, 2)
            .shown_when(move || open.get()),
        ])
    });
    let mut driver = shown(app, 20, 5);
    driver.assert_focused("under");
    state.get().unwrap().set(true);
    driver.settle();
    driver.assert_focused("first");
    click(&mut driver, 0, 0);
    assert_eq!(hits.get(), 0, "modal allowed an outside pointer action");
    driver.press("tab");
    driver.assert_focused("second");
    driver.press("tab");
    driver.assert_focused("first");
    driver.press("escape");
    assert!(driver.screen().plain().join("\n").contains("first"));
    state.get().unwrap().set(false);
    driver.settle();
    driver.assert_focused("under");
    click(&mut driver, 0, 0);
    assert_eq!(hits.get(), 1);
}

#[test]
fn nested_owner_modals_restore_each_focus_and_preserve_a_live_draft() {
    use std::{cell::Cell, rc::Rc};
    let states = Rc::new(Cell::new(None));
    let handles = states.clone();
    let app = App::new(move || {
        let outer = signal(false);
        let inner = signal(false);
        let draft = signal(String::new());
        let caption = signal(String::from("before"));
        handles.set(Some((outer, inner, draft, caption)));
        layers([
            label("base").focusable().name("base"),
            layers([
                input(draft).name("draft"),
                text(move || caption.get()),
                label("inner")
                    .focusable()
                    .name("inner")
                    .controlled_modal()
                    .inset(1, 2, 1, 2)
                    .shown_when(move || inner.get()),
            ])
            .controlled_modal()
            .inset(1, 60, 1, 60)
            .shown_when(move || outer.get()),
        ])
    });
    let mut driver = shown(app, 160, 12);
    let (outer, inner, draft, caption) = states.get().unwrap();
    driver.assert_focused("base");
    outer.set(true);
    driver.settle();
    driver.assert_focused("draft");
    driver.press("x");
    assert_eq!(draft.get_untracked(), "x");
    caption.set("updated".into());
    driver.settle();
    driver.assert_focused("draft");
    assert_eq!(draft.get_untracked(), "x");
    inner.set(true);
    driver.settle();
    driver.assert_focused("inner");
    driver.press("tab");
    driver.assert_focused("inner");
    inner.set(false);
    driver.settle();
    driver.assert_focused("draft");
    assert_eq!(draft.get_untracked(), "x");
    outer.set(false);
    driver.settle();
    driver.assert_focused("base");
}

#[test]
fn controlled_modal_releases_stock_button_capture_outside() {
    use std::{cell::Cell, rc::Rc};
    let hits = Rc::new(Cell::new(0));
    let under = Rc::new(Cell::new(0));
    let (h, u) = (hits.clone(), under.clone());
    let app = App::new(move || {
        let (h, u) = (h.clone(), u.clone());
        layers([
            label("under").on_click(move |_| u.set(u.get() + 1)),
            button("Save", move |_| h.set(h.get() + 1))
                .controlled_modal()
                .inset(1, 4, 1, 4),
        ])
    });
    let mut driver = shown(app, 20, 5);
    driver.event(Event::Mouse(Mouse::new(
        MouseKind::Down(Button::Left),
        6,
        2,
    )));
    driver.event(Event::Mouse(Mouse::new(
        MouseKind::Drag(Button::Left),
        0,
        0,
    )));
    driver.event(Event::Mouse(Mouse::new(MouseKind::Up(Button::Left), 0, 0)));
    driver.event(Event::Mouse(Mouse::new(MouseKind::Up(Button::Left), 6, 2)));
    driver.settle();
    assert_eq!(hits.get(), 0, "stale capture accepted a later release");
    assert_eq!(under.get(), 0, "modal leaked a pointer event");
    click(&mut driver, 6, 2);
    assert_eq!(hits.get(), 1, "subsequent normal click failed");
}

#[test]
fn text_only_controlled_modal_routes_owner_escape_and_restores_focus() {
    use std::{cell::Cell, rc::Rc};
    let state = Rc::new(Cell::new(None));
    let handle = state.clone();
    let app = App::new(move || {
        let open = signal(false);
        handle.set(Some(open));
        layers([
            label("under").focusable().name("under"),
            label("Information")
                .controlled_modal()
                .on_key("esc", move |_| open.set(false))
                .inset(1, 2, 1, 2)
                .shown_when(move || open.get()),
        ])
    });
    let mut driver = shown(app, 20, 5);
    driver.assert_focused("under");
    driver.event(Event::Mouse(Mouse::new(MouseKind::Moved, 0, 0)));
    state.get().unwrap().set(true);
    driver.settle();
    driver.press("esc");
    driver.settle();
    assert!(
        !state.get().unwrap().get_untracked(),
        "owner did not receive Escape"
    );
    driver.assert_focused("under");
}

#[test]
fn controlled_modal_does_not_route_old_underlay_capture_into_its_scope() {
    use std::{cell::Cell, rc::Rc};
    let open = Rc::new(Cell::new(None));
    let handle = open.clone();
    let hits = Rc::new(Cell::new(0));
    let h = hits.clone();
    let routed = Rc::new(Cell::new(0));
    let routed_handle = routed.clone();
    let app = App::new(move || {
        let shown = signal(false);
        handle.set(Some(shown));
        let h = h.clone();
        let routed_handle = routed_handle.clone();
        layers([
            button("under", move |_| h.set(h.get() + 1)).on_mouse(move |_, mouse| {
                if matches!(mouse.kind, MouseKind::Up(_)) {
                    routed_handle.set(routed_handle.get() + 1);
                }
                false
            }),
            label("Information")
                .controlled_modal()
                .inset(1, 2, 1, 2)
                .shown_when(move || shown.get()),
        ])
    });
    let mut d = shown(app, 20, 5);
    d.event(Event::Mouse(Mouse::new(
        MouseKind::Down(Button::Left),
        6,
        2,
    )));
    open.get().unwrap().set(true);
    d.settle();
    d.event(Event::Mouse(Mouse::new(MouseKind::Up(Button::Left), 6, 2)));
    d.settle();
    assert_eq!(
        hits.get(),
        0,
        "capture belonging to underlay received a modal event"
    );
    assert_eq!(routed.get(), 0, "underlay received the modal release");
}

#[test]
fn controlled_modal_slider_drag_left_keeps_direction_and_releases() {
    use intuituive::slider;
    use std::{cell::Cell, rc::Rc};
    let value = Rc::new(Cell::new(None));
    let handle = value.clone();
    let app = App::new(move || {
        let volume = signal(50.0);
        handle.set(Some(volume));
        layers([
            label("under"),
            slider(volume, 0.0..=100.0, 1.0)
                .controlled_modal()
                .inset(1, 4, 1, 4),
        ])
    });
    let mut d = shown(app, 20, 5);
    d.event(Event::Mouse(Mouse::new(
        MouseKind::Down(Button::Left),
        12,
        2,
    )));
    d.event(Event::Mouse(Mouse::new(
        MouseKind::Drag(Button::Left),
        0,
        2,
    )));
    d.settle();
    assert_eq!(
        value.get().unwrap().get_untracked(),
        0.0,
        "left drag went to the maximum"
    );
    d.event(Event::Mouse(Mouse::new(MouseKind::Up(Button::Left), 0, 2)));
    d.event(Event::Mouse(Mouse::new(MouseKind::Moved, 14, 2)));
    d.settle();
    assert_eq!(
        value.get().unwrap().get_untracked(),
        0.0,
        "released slider continued dragging"
    );
}

#[test]
fn a_dynamically_created_controlled_scope_is_discovered_and_removed() {
    use std::{cell::Cell, rc::Rc};
    let state = Rc::new(Cell::new(None));
    let handle = state.clone();
    let app = App::new(move || {
        let open = signal(false);
        handle.set(Some(open));
        layers([
            label("base").focusable().name("base"),
            keyed(
                move || open.get(),
                move |visible| {
                    if visible {
                        label("dynamic modal")
                            .controlled_modal()
                            .on_key("esc", move |_| open.set(false))
                            .inset(1, 2, 1, 2)
                    } else {
                        label("")
                    }
                },
            )
            .children_cover_only(),
        ])
    });
    let mut d = shown(app, 20, 5);
    d.assert_focused("base");
    state.get().unwrap().set(true);
    d.settle();
    assert!(d.find_text("dynamic modal").is_some());
    d.press("esc");
    d.settle();
    assert!(!state.get().unwrap().get_untracked());
    d.assert_focused("base");
}
