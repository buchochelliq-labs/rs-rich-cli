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

// Titles and borders that follow state.

#[test]
fn a_panels_titles_follow_signals_and_name_it() {
    let app = App::new(|| {
        let (file, at) = (signal("lib.rs"), signal(1));
        label("+ one line")
            .panel_with(move || format!("Diff: {file}"))
            .add_title_with(Edge::Bottom, Align::End, move || format!("{at}/3"))
            .on_key("n", move |_| {
                file.set("main.rs");
                at.set(2);
            })
    });
    let mut driver = app.driver(22, 3);
    driver.settle();
    assert!(rows(&driver)[0].contains("Diff: lib.rs"));
    assert!(rows(&driver)[2].contains("1/3"));
    driver.press("n");
    assert!(rows(&driver)[0].contains("Diff: main.rs"));
    assert!(rows(&driver)[2].contains("2/3"));
}

#[test]
fn a_panels_box_follows_the_focus_and_state() {
    let app = App::new(|| {
        let alert = signal(false);
        column([
            label("a")
                .focusable()
                .panel("A")
                .border(Border::Plain)
                .focus_border(Border::Double),
            label("b")
                .focusable()
                .panel("B")
                .border_with(move || match alert.get() {
                    true => Border::Thick,
                    false => Border::Plain,
                }),
        ])
        .on_key("x", move |_| alert.set(true))
    });
    let mut driver = app.driver(8, 6);
    driver.settle();
    assert!(rows(&driver)[0].starts_with('╔'), "{:?}", rows(&driver));
    assert!(rows(&driver)[3].starts_with('┌'));
    driver.press("tab");
    assert!(rows(&driver)[0].starts_with('┌'));
    driver.press("x");
    assert!(rows(&driver)[3].starts_with('┏'), "{:?}", rows(&driver));
}

// Nodes shown and sized by state.

#[test]
fn shown_when_hides_a_node_and_lets_its_focus_go() {
    let app = App::new(|| {
        let details = signal(true);
        row([
            label("list").focusable().name("list").fixed(5),
            label("details")
                .focusable()
                .name("details")
                .shown_when(move || details.get()),
        ])
        .on_key("d", move |_| details.update(|on| *on = !*on))
    });
    let mut driver = app.driver(14, 1);
    driver.press("tab");
    driver.assert_focused("details");
    driver.press("d");
    assert_eq!(rows(&driver)[0], "list");
    driver.assert_focused("list");
    driver.press("d");
    assert_eq!(rows(&driver)[0], "list details");
}

#[test]
fn sizes_follow_the_focus() {
    let app = App::new(|| {
        let right = label("right").focusable().name("right");
        let id = right.id();
        let grows = move || match intuituive::focus::has_focus(id) {
            true => Size::Percent(70),
            false => Size::Percent(30),
        };
        row([
            label("left").focusable().name("left").flex(1),
            right.size_with(grows),
        ])
    });
    let mut driver = app.driver(20, 1);
    driver.settle();
    assert_eq!(rows(&driver)[0].find("right"), Some(14));
    driver.press("tab");
    assert_eq!(rows(&driver)[0].find("right"), Some(6));
}

// Focus moved into containers, by scope and by direction, and heard.

#[test]
fn focus_within_waits_for_a_node_shown_later() {
    let app = App::new(|| {
        let searching = signal(false);
        let search = column([label("Search:"), label("box").focusable().name("box")])
            .shown_when(move || searching.get());
        let id = search.id();
        column([label("table").focusable().name("table"), search]).on_key("/", move |cx| {
            searching.set(true);
            cx.focus_within(id);
        })
    });
    let mut driver = app.driver(10, 3);
    driver.settle();
    driver.assert_focused("table");
    driver.press("/");
    driver.assert_focused("box");
}

#[test]
fn a_focus_move_drops_a_wait_for_a_container() {
    let app = App::new(|| {
        let shown = signal(false);
        let later = label("later")
            .focusable()
            .name("later")
            .shown_when(move || shown.get());
        let id = later.id();
        column([
            label("a").focusable().name("a"),
            label("b").focusable().name("b"),
            later,
        ])
        .on_key("w", move |cx| cx.focus_within(id))
        .on_key("s", move |_| shown.set(true))
    });
    let mut driver = app.driver(10, 3);
    driver.press("w tab s");
    driver.assert_focused("b");
}

#[test]
fn scopes_take_the_focus_back_where_it_was() {
    let app = App::new(|| {
        row([
            column([
                label("a1").focusable().name("a1"),
                label("a2").focusable().name("a2"),
            ])
            .focus_scope(),
            column([
                label("b1").focusable().name("b1"),
                label("b2").focusable().name("b2"),
            ])
            .focus_scope(),
        ])
        .on_key("w", |cx| cx.focus_next_scope())
        .on_key("W", |cx| cx.focus_previous_scope())
    });
    let mut driver = app.driver(10, 2);
    driver.press("tab");
    driver.assert_focused("a2");
    driver.press("w");
    driver.assert_focused("b1");
    driver.press("tab");
    driver.assert_focused("b2");
    driver.press("W");
    driver.assert_focused("a2");
    driver.press("W");
    driver.assert_focused("b2");
}

#[test]
fn on_focus_and_on_blur_hear_the_focus_come_and_go_including_for_a_modal() {
    let app = App::new(|| {
        let said = signal(String::new());
        column([
            column([label("a").focusable(), label("b").focusable()])
                .on_focus(move |_| said.update(|s| s.push('+')))
                .on_blur(move |_| said.update(|s| s.push('-'))),
            label("c").focusable().fixed(1),
            text!("{said}").fixed(1),
        ])
        .on_key("m", |cx| {
            cx.modal(Size::Fixed(6), Size::Fixed(3), || {
                label("hi")
                    .focusable()
                    .panel("")
                    .on_key("esc", |cx| cx.pop())
            })
        })
    });
    let mut driver = app.driver(12, 4);
    driver.settle();
    // Focused at the start, then within, then out and back.
    assert_eq!(rows(&driver)[3], "+");
    driver.press("tab tab tab");
    assert_eq!(rows(&driver)[3], "+-+");
    driver.press("m");
    driver.press("esc");
    assert_eq!(rows(&driver)[3], "+-+-+");
}

#[test]
fn focused_follows_the_focus() {
    let app = App::new(|| {
        let b = label("b").focusable();
        let id = b.id();
        column([
            label("a").focusable(),
            b,
            text(move || format!("{}", intuituive::focus::focused() == Some(id))),
        ])
    });
    let mut driver = app.driver(8, 3);
    driver.settle();
    assert_eq!(rows(&driver)[2], "false");
    driver.press("tab");
    assert_eq!(rows(&driver)[2], "true");
}

#[test]
fn focus_moves_to_the_neighbour_on_a_side() {
    let app = App::new(|| {
        let cell = |name: &str| label(name).focusable().name(name);
        column([
            row([cell("nw"), cell("ne")]),
            row([cell("sw"), cell("se")]),
            cell("bottom"),
        ])
        .bind_directional_focus("left", "right", "up", "down")
    });
    let mut driver = app.driver(10, 3);
    driver.settle();
    driver.assert_focused("nw");
    driver.press("right");
    driver.assert_focused("ne");
    driver.press("down");
    driver.assert_focused("se");
    driver.press("left");
    driver.assert_focused("sw");
    driver.press("down");
    driver.assert_focused("bottom");
    // Nothing further down: it stays.
    driver.press("down");
    driver.assert_focused("bottom");
    driver.press("up");
    driver.assert_focused("sw");
}

#[test]
fn focus_moves_between_scopes_by_direction_keeping_their_place() {
    let app = App::new(|| {
        let pane = |a: &str, b: &str| {
            column([label(a).focusable().name(a), label(b).focusable().name(b)])
                .panel("")
                .focus_scope()
        };
        row([pane("l1", "l2"), pane("r1", "r2")]).bind_directional_focus("h", "l", "k", "j")
    });
    let mut driver = app.driver(12, 4);
    driver.press("tab");
    driver.assert_focused("l2");
    driver.press("l");
    driver.assert_focused("r1");
    driver.press("h");
    driver.assert_focused("l2");
}

// A treemap.

#[test]
fn squarify_covers_the_rectangle_and_keeps_tiles_near_square() {
    use intuituive::layout::squarify;
    use intuituive::screen::Rect;
    let area = Rect::new(2, 1, 60, 20);
    let weights = [40.0, 25.0, 15.0, 10.0, 6.0, 4.0];
    let map = squarify(area, &weights, (1, 1), 2.0);
    let cells: u32 = map
        .tiles
        .iter()
        .map(|t| t.width as u32 * t.height as u32)
        .sum();
    assert_eq!(cells, 60 * 20);
    for (i, a) in map.tiles.iter().enumerate() {
        assert!(area.intersection(*a) == *a, "{a:?} outside");
        for b in &map.tiles[i + 1..] {
            assert!(!a.intersects(*b), "{a:?} overlaps {b:?}");
        }
    }
    // The largest takes about its share, and looks squarer than a strip.
    let first = map.tiles[0];
    let share = first.width as f64 * first.height as f64 / 1200.0;
    assert!((share - 0.4).abs() < 0.05, "{share}");
    assert!(first.width < 60 && first.height < 20 || first.height == 20);
}

#[test]
fn squarify_groups_what_is_too_small_into_the_rest() {
    use intuituive::layout::squarify;
    use intuituive::screen::Rect;
    let weights: Vec<f64> = (1..=30).rev().map(|n| n as f64).collect();
    let map = squarify(Rect::new(0, 0, 40, 12), &weights, (8, 3), 2.0);
    let (rest, count) = map.rest.expect("a rest");
    assert!(count > 0 && rest.width >= 8 && rest.height >= 3);
    for tile in map.tiles.iter().filter(|t| !t.is_empty()) {
        assert!(tile.width >= 8 && tile.height >= 3, "{tile:?}");
    }
    // The ones kept are the heaviest.
    let kept = map.tiles.iter().filter(|t| !t.is_empty()).count();
    assert!(map.tiles[..kept].iter().all(|t| !t.is_empty()));
    assert_eq!(kept + count, 30);
}

#[test]
fn a_treemap_builds_a_tile_per_key_and_rebuilds_only_what_changed() {
    use intuituive::{treemap_with, TreemapOptions};
    let built = Rc::new(Cell::new(0));
    let b = built.clone();
    let app = App::new(move || {
        let items = signal(vec![("a", 2.0), ("b", 1.0), ("c", 1.0)]);
        let b = b.clone();
        treemap_with(
            move || items.get(),
            move |name, rect| {
                b.set(b.get() + 1);
                label(format!("{name}{}", rect.width)).panel("")
            },
            TreemapOptions::new().aspect(1.0),
        )
        .on_key("x", move |_| items.update(|v| v[2] = ("d", 1.0)))
    });
    let mut driver = app.driver(20, 6);
    driver.settle();
    assert_eq!(built.get(), 3);
    let shown = rows(&driver).join("\n");
    assert!(shown.contains("a10") && shown.contains("c5"), "{shown}");
    driver.press("x");
    let shown = rows(&driver).join("\n");
    assert!(shown.contains("d5") && !shown.contains("c5"), "{shown}");
    assert_eq!(built.get(), 4);
}

// A too-small screen.

#[test]
fn a_too_small_screen_holds_the_apps_keys_and_quits_with_q() {
    let app = App::new(|| {
        let n = signal(0);
        text!("count {n}").on_key("+", move |_| n.update(|n| *n += 1))
    })
    .min_size(30, 4, "Too small");
    let mut driver = app.driver(40, 4);
    driver.press("+");
    assert_eq!(rows(&driver)[0], "count 1");
    driver.resize(20, 4);
    driver.press("+");
    assert!(rows(&driver).iter().any(|r| r.trim() == "Too small"));
    driver.resize(40, 4);
    driver.settle();
    assert_eq!(rows(&driver)[0], "count 1");
    driver.resize(20, 4);
    driver.press("q");
    assert!(driver.is_done());
}

// Fitted text and middle truncation.

#[test]
fn fit_text_follows_its_width_and_state() {
    let app = App::new(|| {
        let name = signal("docs");
        row([
            intuituive::fit_text(move || {
                vec![
                    format!("SELECTED: {name} (29.3K, 1 files)"),
                    format!("SELECTED: {name} (29.3K)"),
                    format!("{name}"),
                ]
            })
            .auto(),
            label("|"),
        ])
        .on_key("n", move |_| name.set("src"))
    });
    let mut driver = app.driver(26, 1);
    driver.settle();
    assert_eq!(rows(&driver)[0], "SELECTED: docs (29.3K)|");
    driver.press("n");
    assert_eq!(rows(&driver)[0], "SELECTED: src (29.3K)|");
    driver.resize(8, 1);
    driver.settle();
    assert_eq!(rows(&driver)[0], "src|");
}

#[test]
fn truncate_middle_keeps_both_ends_of_wide_text() {
    let app = App::new(|| label("日本語のファイル名.txt").truncate_middle());
    let mut driver = app.driver(12, 1);
    driver.settle();
    let shown = rows(&driver)[0].clone();
    assert!(
        shown.starts_with("日本") && shown.ends_with(".txt"),
        "{shown}"
    );
    assert!(shown.contains('…'));
}
