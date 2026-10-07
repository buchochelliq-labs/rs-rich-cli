//! Layout: content sizes, clamps, gaps, padding, grids and switches.

mod common;

use std::cell::Cell;
use std::rc::Rc;

use common::{row_of, run, screen};
use intuituive::prelude::*;
use rich_interact::headless::Script;

#[test]
fn an_auto_node_grows_with_its_content_and_moves_what_follows() {
    let app = App::new(|| {
        let lines = signal(1usize);
        column([
            text(move || vec!["line"; lines.get()].join("\n")).auto(),
            label("below").auto(),
            label("rest"),
        ])
        .on_key("+", move |_| lines.update(|n| *n += 1))
        .on_key("-", move |_| lines.update(|n| *n -= 1))
        .on_key("q", |cx| cx.quit())
    });
    let record = run(app, Script::new().keys("+ + q"), 20, 8);
    let rows = screen(&record);
    assert_eq!(&rows[..5], ["line", "line", "line", "below", "rest"]);

    // Shrinking leaves nothing behind where the content was.
    let app = App::new(|| {
        let lines = signal(3usize);
        column([
            text(move || vec!["line"; lines.get()].join("\n")).auto(),
            label("below").auto(),
            label("rest"),
        ])
        .on_key("-", move |_| lines.update(|n| *n -= 1))
        .on_key("q", |cx| cx.quit())
    });
    let record = run(app, Script::new().keys("- - q"), 20, 8);
    let rows = screen(&record);
    assert_eq!(&rows[..3], ["line", "below", "rest"]);
    assert!(rows[3..].iter().all(String::is_empty), "{rows:?}");
}

#[test]
fn clamps_bound_flexible_children() {
    let app = App::new(|| {
        row([label("a").max_size(5), label("b"), label("c").min_size(12)])
            .on_key("q", |cx| cx.quit())
    });
    // 30 columns: a is capped at 5; c's even share (12.5) already meets
    // its minimum; b takes the rest.
    let rows = screen(&run(app, Script::new().keys("q"), 30, 1));
    assert_eq!(rows[0].find('a'), Some(0));
    assert_eq!(rows[0].find('b'), Some(5));
    assert_eq!(rows[0].find('c'), Some(17));

    let app = App::new(|| row([label("a"), label("b").min_size(16)]).on_key("q", |cx| cx.quit()));
    let rows = screen(&run(app, Script::new().keys("q"), 20, 1));
    assert_eq!(rows[0].find('b'), Some(4));
}

#[test]
fn gaps_and_padding_leave_space() {
    let app = App::new(|| {
        column([
            label("one").fixed(1),
            label("two").fixed(1),
            label("three").fixed(1),
        ])
        .gap(1)
        .on_key("q", |cx| cx.quit())
    });
    let rows = screen(&run(app, Script::new().keys("q"), 20, 10));
    assert_eq!(row_of(&rows, "one"), Some(0));
    assert_eq!(row_of(&rows, "two"), Some(2));
    assert_eq!(row_of(&rows, "three"), Some(4));

    let app = App::new(|| {
        column([label("two").padding(1, 2).fixed(3), label("after").fixed(1)])
            .on_key("q", |cx| cx.quit())
    });
    let rows = screen(&run(app, Script::new().keys("q"), 20, 5));
    assert_eq!(rows[0], "");
    assert_eq!(rows[1], "  two");
    assert_eq!(rows[2], "");
    assert_eq!(rows[3], "after");
}

#[test]
fn a_grid_places_spans_and_sizes_rows_to_their_content() {
    let app = App::new(|| {
        grid(
            [Size::Fixed(6), Size::Flex(1), Size::Flex(1)],
            [
                label("a\nb\nc").span(1, 2),
                label("wide").span(2, 1),
                label("x"),
                label("y"),
                label("tall\nrow"),
            ],
        )
        .rows([Size::Auto])
        .gap(1)
        .on_key("q", |cx| cx.quit())
    });
    let rows = screen(&run(app, Script::new().keys("q"), 27, 8));
    // Columns: 6, then (27 - 6 - 2 gaps) shared: 9 and 10.
    assert_eq!(rows[0], "a      wide");
    // Row 1 starts after row 0 (1 row) and a gap: a's span covers rows 0-1.
    assert_eq!(rows[2], "c      x         y");
    // The third row is as tall as its content.
    assert_eq!(rows[4], "tall");
    assert_eq!(rows[5], "row");
}

#[test]
fn each_rows_can_size_to_their_content() {
    let app = App::new(|| {
        let items = signal(vec![1usize, 2, 3]);
        column([
            each(
                move || items.get(),
                |n| label(vec![format!("item {n}"); n].join("\n")).auto(),
            )
            .auto(),
            label("end"),
        ])
        .on_key("x", move |_| items.update(|v| v.retain(|n| *n != 2)))
        .on_key("q", |cx| cx.quit())
    });
    let rows = screen(&run(app, Script::new().keys("x q"), 20, 8));
    assert_eq!(&rows[..5], ["item 1", "item 3", "item 3", "item 3", "end"]);
}

#[test]
fn a_switch_keeps_each_child_and_routes_only_to_the_shown_one() {
    let builds = Rc::new(Cell::new(0));
    let counted = builds.clone();
    let app = App::new(move || {
        let tab = signal(0usize);
        let counts = [signal(0u32), signal(0u32)];
        column([switch(
            move || tab.get(),
            move |tab| {
                counted.set(counted.get() + 1);
                let count = counts[tab];
                text(move || format!("tab {tab}: {}", count.get()))
                    .focusable()
                    .on_key("+", move |_| count.update(|c| *c += 1))
            },
        )])
        .on_key("tab", move |_| tab.update(|t| *t = 1 - *t))
        .on_key("q", |cx| cx.quit())
    });
    let record = run(app, Script::new().keys("+ tab + + tab q"), 20, 2);
    assert_eq!(screen(&record)[0], "tab 0: 1");
    assert_eq!(builds.get(), 2, "each tab is built once and kept");
}

#[test]
fn a_child_that_does_not_fit_takes_no_clicks() {
    let clicked = Rc::new(Cell::new(false));
    let seen = clicked.clone();
    let app = App::new(move || {
        let seen = seen.clone();
        column([
            label("top").fixed(3),
            label("hidden").fixed(3).on_click(move |_| seen.set(true)),
        ])
        .on_key("q", |cx| cx.quit())
    });
    // Two rows: the second child gets none, so a click on row 0 is the
    // first's, and the second has no rectangle to be clicked in.
    run(app, Script::new().click(0, 0).click(0, 1).keys("q"), 10, 2);
    assert!(!clicked.get());
}

#[test]
fn a_measured_switch_still_redraws_a_tab_it_showed_before() {
    // The column measures the switch (auto) before drawing it: switching
    // back to tab 0, at the same rectangle, must still draw tab 0.
    let app = App::new(|| {
        let tab = signal(0usize);
        column([
            switch(move || tab.get(), |tab| label(format!("tab {tab}"))).auto(),
            label("end"),
        ])
        .on_key("tab", move |_| tab.update(|t| *t = 1 - *t))
        .on_key("q", |cx| cx.quit())
    });
    let rows = screen(&run(app, Script::new().keys("tab tab q"), 20, 3));
    assert_eq!(&rows[..2], ["tab 0", "end"]);
}

#[test]
fn spanning_children_size_the_content_tracks_they_cover() {
    let app = App::new(|| {
        column([
            grid([Size::Auto], [label("one\ntwo\nthree").span(1, 2)])
                .rows([Size::Auto])
                .fixed(3),
            label("end"),
        ])
        .on_key("q", |cx| cx.quit())
    });
    let rows = screen(&run(app, Script::new().keys("q"), 20, 4));
    assert_eq!(&rows[..4], ["one", "two", "three", "end"]);
}
