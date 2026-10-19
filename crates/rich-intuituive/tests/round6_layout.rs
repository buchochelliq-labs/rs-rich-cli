//! Round 6's state-driven layout, focus scopes and keyed nodes: an
//! `.auto()` switch sized by its page, layers that let the one below show
//! where they draw nothing, switches that drop the pages they no longer
//! show, `keyed` rebuilds, titles and borders that follow state, nodes
//! shown and sized by signals, focus moved into containers, by scope and
//! by direction, a treemap, a too-small screen, fitted text and footers
//! and helps in other formats. Nothing about one app is in them.

use std::cell::Cell;
use std::rc::Rc;

use intuituive::prelude::*;
use intuituive::Driver;

fn rows(driver: &Driver) -> Vec<String> {
    driver
        .screen()
        .plain()
        .iter()
        .map(|r| r.trim_end().to_string())
        .collect()
}

// Bug: an `.auto()` switch measured its page's content, ignoring the
// page's own `.fixed(n)` and `.min_size(n)`.

#[test]
fn an_auto_switch_takes_its_pages_fixed_size() {
    let app = App::new(|| {
        let page = signal(0usize);
        row([
            switch(
                move || page.get(),
                |page| match page {
                    0 => label("ab").fixed(11),
                    _ => label("ab").min_size(6),
                },
            )
            .auto(),
            label("|rest"),
        ])
        .on_key("n", move |_| page.set(1))
    });
    let mut driver = app.driver(30, 1);
    driver.settle();
    assert_eq!(rows(&driver)[0], "ab         |rest");
    driver.press("n");
    assert_eq!(rows(&driver)[0], "ab    |rest");
}

#[test]
fn an_auto_switch_in_a_column_takes_its_pages_fixed_height() {
    let app = App::new(|| {
        column([
            switch(|| 0, |_| label("menu").fixed(3)).auto(),
            label("below"),
        ])
    });
    let mut driver = app.driver(10, 5);
    driver.settle();
    assert_eq!(rows(&driver)[3], "below");
}

// Bug: a full-size layer painted over a `fill` beneath it, blanks and
// all, so a backdrop needed a ring of fills round the content.

#[test]
fn a_layer_lets_the_one_below_show_where_it_draws_nothing() {
    let app = App::new(|| {
        let word = signal("hi");
        layers([
            fill("·", ""),
            column([row([text!("{word}").auto()])
                .justify(Justify::Center)
                .fixed(1)])
            .justify(Justify::Center),
        ])
        .on_key("x", move |_| word.set("hello"))
    });
    let mut driver = app.driver(7, 3);
    driver.settle();
    assert_eq!(rows(&driver), ["·······", "··hi···", "·······"]);
    // The top layer lays out again: what it left still shows the fill.
    driver.press("x");
    assert_eq!(rows(&driver), ["·······", "·hello·", "·······"]);
}

#[test]
fn a_layer_with_a_background_or_a_panel_still_covers_what_is_below() {
    let app = App::new(|| {
        layers([
            fill("·", ""),
            label("").bg("blue").inset(0, 4, 2, 0),
            // A panel is opaque inside its frame, where its child is not.
            column([label("x").fixed(1)]).panel("").inset(0, 0, 0, 3),
        ])
    });
    let mut driver = app.driver(7, 4);
    driver.settle();
    assert_eq!(rows(&driver), ["   ╭──╮", "   │x │", "···│  │", "···╰──╯"]);
}

// Bug: a `switch` kept every page it ever built, so one keyed by live
// state grew without end.

#[test]
fn a_switch_keeping_its_last_page_drops_the_others() {
    let gone = Rc::new(Cell::new(0));
    let built = Rc::new(Cell::new(0));
    let (g, b) = (gone.clone(), built.clone());
    let app = App::new(move || {
        let key = signal(0u32);
        switch(
            move || key.get(),
            move |k| {
                b.set(b.get() + 1);
                let g = g.clone();
                text!("page {k}").on_unmount(move || g.set(g.get() + 1))
            },
        )
        .keep_last(1)
        .on_key("n", move |_| key.update(|k| *k += 1))
        .on_key("b", move |_| key.update(|k| *k -= 1))
    });
    let mut driver = app.driver(10, 1);
    driver.settle();
    driver.press("n n n");
    assert_eq!(rows(&driver)[0], "page 3");
    assert_eq!((built.get(), gone.get()), (4, 3));
    // Going back builds the page again.
    driver.press("b");
    assert_eq!(rows(&driver)[0], "page 2");
    assert_eq!((built.get(), gone.get()), (5, 4));
}

#[test]
fn a_switch_keeps_every_page_by_default_and_the_last_n_when_asked() {
    let built = Rc::new(Cell::new(0));
    let b = built.clone();
    let app = App::new(move || {
        let key = signal(0u32);
        switch(
            move || key.get() % 3,
            move |k| {
                b.set(b.get() + 1);
                text!("page {k}")
            },
        )
        .keep_last(3)
        .on_key("n", move |_| key.update(|k| *k += 1))
    });
    let mut driver = app.driver(10, 1);
    driver.settle();
    driver.press("n n n n n n");
    assert_eq!(rows(&driver)[0], "page 0");
    assert_eq!(built.get(), 3);
}

// `keyed`: one child, built again when its key changes.

#[test]
fn keyed_rebuilds_its_child_when_the_key_changes_and_keeps_it_otherwise() {
    let built = Rc::new(Cell::new(0));
    let b = built.clone();
    let app = App::new(move || {
        let (key, other) = (signal(0u32), signal(0u32));
        column([
            keyed(
                move || key.get(),
                move |k| {
                    b.set(b.get() + 1);
                    text!("{k} {other}")
                },
            ),
            label("end").fixed(1),
        ])
        .on_key("k", move |_| key.update(|k| *k += 1))
        .on_key("o", move |_| other.update(|o| *o += 1))
    });
    let mut driver = app.driver(10, 3);
    driver.settle();
    driver.press("o o");
    assert_eq!(rows(&driver)[0], "0 2");
    assert_eq!(built.get(), 1);
    driver.press("k");
    assert_eq!(rows(&driver)[0], "1 2");
    assert_eq!(built.get(), 2);
    // It is a flexible child: the label stays at the bottom.
    assert_eq!(rows(&driver)[2], "end");
}

#[test]
fn keyed_moves_the_focus_into_the_new_child_when_the_old_one_had_it() {
    let app = App::new(|| {
        let key = signal(0u32);
        column([
            label("before").focusable().name("before"),
            keyed(
                move || key.get(),
                |k| {
                    label(format!("item {k}"))
                        .focusable()
                        .name(&format!("item{k}"))
                },
            ),
        ])
        .on_key("k", move |_| key.update(|k| *k += 1))
    });
    let mut driver = app.driver(10, 2);
    driver.settle();
    driver.press("tab");
    driver.assert_focused("item0");
    driver.press("k");
    driver.assert_focused("item1");
}
