//! Round 4's generic widget pieces: list options through a wrapper, a tab
//! set, fitted tabs, a multi-select with a visual range, find over a list,
//! a cursor kept by key, a context handler, a gauge segment, a description
//! list, a streaming fuzzy list, digits, and the Miller columns' table of
//! moves and hooks. Each is driven here with nothing about files in it.

use std::time::{Duration, Instant};

use intuituive::finder::{fuzzy_list_with, FuzzyOptions, FuzzySource};
use intuituive::interact::{Button, Event, Mouse, MouseKind};
use intuituive::miller::{MillerOptions, MillerSource, Move};
use intuituive::prelude::*;
use intuituive::widgets::{seg, seg_gauge, status_bar, tabs, HighlightLayer};
use intuituive::{
    digits, facts, list_search, miller_of, multi_select, tab_set, tabs_for, Driver, Mark,
};

fn rows(driver: &Driver) -> Vec<String> {
    driver
        .screen()
        .plain()
        .iter()
        .map(|r| r.trim_end().to_string())
        .collect()
}

fn styled(driver: &Driver) -> String {
    intuituive::testing::styled(driver)
}

fn names(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

fn mouse(driver: &mut Driver, kind: MouseKind, column: u16, row: u16) {
    driver.event(Event::Mouse(Mouse::new(kind, column, row)));
    driver.settle();
}

// 1. List options on a wrapper reach the list it holds.

/// A list behind a switch, with "No items" in its place when empty, as a
/// file list is.
fn wrapped(items: Signal<Vec<String>>, cursor: Signal<usize>) -> Node {
    switch(
        move || items.with(Vec::is_empty),
        move |empty| {
            if empty {
                label("No items")
            } else {
                list(move || items.get(), cursor)
            }
        },
    )
}

#[test]
fn list_options_given_a_switch_reach_its_list_now_and_later() {
    let app = App::new(|| {
        let items = signal(Vec::<String>::new());
        let cursor = signal(0usize);
        column([
            wrapped(items, cursor)
                .row_style(|_| Some("blue".into()))
                .highlight_layer(HighlightLayer::Under)
                .selected_style_for(|_| Some("underline".into()))
                .wheel_step(1)
                .fixed(3),
            text!("at {cursor}"),
        ])
        .on_key("f", move |_| {
            items.set(names(&["[not underline red]<[/]a", "b", "c", "d", "e"]))
        })
    });
    let mut driver = app.driver(10, 4);
    driver.settle();
    assert_eq!(rows(&driver)[0], "No items");
    // The list is built after the options were given: it still has them.
    driver.press("f");
    let shown = styled(&driver);
    assert!(
        shown.contains("[not underline red]<[/][underline blue]a"),
        "the highlight goes under the markup: {shown}"
    );
    mouse(&mut driver, MouseKind::ScrollDown, 1, 1);
    assert_eq!(rows(&driver)[3], "at 1", "one item a turn");
}

#[test]
fn list_options_reach_a_list_inside_a_panel() {
    let app = App::new(|| {
        list(|| names(&["a", "b"]), signal(1))
            .panel("box")
            .symbol("> ")
    });
    let mut driver = app.driver(10, 4);
    driver.settle();
    assert_eq!(rows(&driver)[2], "│> b     │");
}

// 2. A tab set.

#[test]
fn a_tab_set_opens_closes_swaps_and_switches_through_its_strip() {
    let app = App::new(|| {
        let set = tab_set(|| 0u32);
        let made = signal(0u32);
        column([
            tabs_for(&set, |n| format!("t{n}")).fixed(1),
            text(move || set.with_active(|n| format!("on t{n} of {}", set.len()))),
        ])
        .on_key("t", move |_| {
            made.update(|m| *m += 1);
            set.open_with(made.get_untracked());
        })
        .on_key("x", move |_| {
            set.close_active();
        })
        .on_key("s", move |_| set.swap(1))
    });
    let mut driver = app.driver(24, 2);
    driver.press("t");
    driver.press("t");
    assert_eq!(rows(&driver)[0], " t0 │ t1 │ t2");
    assert_eq!(rows(&driver)[1], "on t2 of 3");
    // A click on the strip switches.
    mouse(&mut driver, MouseKind::Down(Button::Left), 1, 0);
    assert_eq!(rows(&driver)[1], "on t0 of 3");
    driver.press("s");
    assert_eq!(rows(&driver)[0], " t1 │ t0 │ t2");
    driver.press("x");
    assert_eq!(rows(&driver)[0], " t1 │ t2");
    assert_eq!(rows(&driver)[1], "on t2 of 2");
}

// 3. Fitted tabs and the unselected style.

#[test]
fn fitted_tabs_share_the_width_and_unselected_titles_take_their_style() {
    let app = App::new(|| {
        let titles = || names(&["~", "projects", "downloads"]);
        tabs(titles, signal(0))
            .fit()
            .theme_styles([("tabs.unselected", "dim")])
    });
    let mut driver = app.driver(18, 1);
    driver.settle();
    assert_eq!(rows(&driver)[0], " ~ │ pro… │ down…");
    assert!(
        styled(&driver).contains("[dim] pro…"),
        "{}",
        styled(&driver)
    );
    // With room, nothing is cut.
    let app = App::new(|| tabs(|| names(&["~", "src"]), signal(0)).fit());
    let mut driver = app.driver(18, 1);
    driver.settle();
    assert_eq!(rows(&driver)[0], " ~ │ src");
}

// 4. A multi-select.

#[test]
fn a_multi_select_toggles_selects_all_inverts_and_takes_a_visual_range() {
    let app = App::new(|| {
        let items = signal(names(&["a", "b", "c", "d", "e"]));
        let cursor = signal(0usize);
        let picked = multi_select::<String>()
            .over(move || items.get())
            .range_from(cursor);
        let hovered = move || items.with(|i| i.get(cursor.get_untracked()).cloned());
        column([
            list(move || items.get(), cursor)
                .selection_markers(picked, move |i| items.with(|n| n[i].clone()))
                .fixed(5),
            text(move || picked.selected().join(",")),
            text(move || format!("{:?}", picked.targets(hovered()))),
        ])
        .on_key("space", move |_| picked.toggle(hovered().unwrap()))
        .on_key("ctrl+a", move |_| picked.select_all())
        .on_key("ctrl+r", move |_| picked.invert())
        .on_key("v", move |_| picked.visual(false))
        .on_key("V", move |_| picked.visual(true))
        .on_key("escape", move |_| picked.end_visual())
        .on_key("c", move |_| picked.clear())
    });
    let mut driver = app.driver(30, 7);
    driver.settle();
    // Nothing selected: the hovered item is the target.
    assert_eq!(rows(&driver)[6], r#"["a"]"#);
    driver.press("space");
    driver.press("j");
    driver.press("j");
    driver.press("v");
    driver.press("j");
    // In the range, c and d are selected already.
    assert_eq!(rows(&driver)[5], "a,c,d");
    let shown = styled(&driver);
    assert!(shown.contains("[on yellow] [/]"), "{shown}");
    assert!(shown.contains("[on cyan] [/]"), "{shown}");
    driver.press("escape");
    assert_eq!(rows(&driver)[5], "a,c,d");
    // V unselects what it covers.
    driver.press("k");
    driver.press("V");
    driver.press("j");
    assert!(
        styled(&driver).contains("[on red] [/]"),
        "{}",
        styled(&driver)
    );
    driver.press("escape");
    assert_eq!(rows(&driver)[5], "a");
    driver.press("ctrl+r");
    assert_eq!(rows(&driver)[5], "b,c,d,e");
    driver.press("ctrl+a");
    assert_eq!(rows(&driver)[5], "a,b,c,d,e");
    assert_eq!(rows(&driver)[6], r#"["a", "b", "c", "d", "e"]"#);
    driver.press("c");
    assert_eq!(rows(&driver)[5], "");
}

#[test]
fn a_selection_outlives_a_re_sort_and_marks_say_how_keys_stand() {
    let app = App::new(|| {
        let items = signal(names(&["b", "a"]));
        let cursor = signal(0usize);
        let picked = multi_select::<String>()
            .over(move || items.get())
            .range_from(cursor);
        column([
            text(move || format!("{:?}", picked.mark(&"b".to_string()))),
            text(move || picked.selected().join(",")),
        ])
        .on_key("s", move |_| picked.select("b".into()))
        .on_key("o", move |_| items.update(|i| i.sort()))
    });
    let mut driver = app.driver(20, 2);
    driver.press("s");
    driver.press("o");
    assert_eq!(rows(&driver)[0], format!("{:?}", Mark::Selected));
    assert_eq!(rows(&driver)[1], "b");
}

// 5. Find over a list.

#[test]
fn list_search_moves_between_matches_wrapping_in_smart_case() {
    let app = App::new(|| {
        let items = || names(&["Notes.md", "main.rs", "[b]notes[/].txt", "lib.rs"]);
        let query = signal(String::from("notes"));
        let cursor = signal(0usize);
        let find = list_search(move || query.get(), items, cursor);
        let place = find.clone();
        let (next, previous) = (find.clone(), find);
        column([
            text(move || format!("{cursor} {:?}", place.place())),
            label(""),
        ])
        .on_key("n", move |_| {
            next.next();
        })
        .on_key("N", move |_| {
            previous.previous();
        })
        .on_key("u", move |_| query.set("Notes".into()))
    });
    let mut driver = app.driver(20, 2);
    driver.settle();
    assert_eq!(rows(&driver)[0], "0 (1, 2)");
    driver.press("n");
    assert_eq!(rows(&driver)[0], "2 (2, 2)", "markup is matched as text");
    driver.press("n");
    assert_eq!(rows(&driver)[0], "0 (1, 2)", "wrapped");
    driver.press("N");
    assert_eq!(rows(&driver)[0], "2 (2, 2)", "wrapped back");
    // A capital matches exactly.
    driver.press("u");
    assert_eq!(rows(&driver)[0], "2 (0, 1)");
    driver.press("n");
    assert_eq!(rows(&driver)[0], "0 (1, 1)");
}

// 6. A cursor kept by key.

#[test]
fn the_cursor_follows_its_item_through_a_sort_and_a_filter() {
    let app = App::new(|| {
        let all = names(&["delta", "alpha", "charlie", "bravo"]);
        let items = signal(all.clone());
        let cursor = signal(0usize);
        column([
            list(move || items.get(), cursor)
                .cursor_by(move |i| items.with(|n| n.get(i).cloned()))
                .fixed(4),
            text!("at {cursor}"),
        ])
        .on_key("s", move |_| items.update(|i| i.sort()))
        .on_key("f", move |_| {
            items.update(|i| i.retain(|n| n.contains('l') || n.contains('h')))
        })
        .on_key("r", move |_| items.update(|i| i.reverse()))
    });
    let mut driver = app.driver(10, 5);
    driver.press("j");
    driver.press("j");
    assert_eq!(rows(&driver)[4], "at 2", "on charlie");
    driver.press("s");
    assert_eq!(rows(&driver)[4], "at 2", "alpha bravo charlie delta");
    driver.press("r");
    assert_eq!(rows(&driver)[4], "at 1", "delta charlie bravo alpha");
    driver.press("f");
    // delta, charlie, alpha: charlie stays hovered.
    assert_eq!(rows(&driver)[4], "at 1");
    assert!(styled(&driver).contains("[reverse]charlie"));
}

// 7. A context handler.

#[test]
fn a_right_press_or_the_menu_key_runs_the_context_handler() {
    let app = App::new(|| {
        let menu = signal(String::new());
        column([
            list(|| names(&["a", "b", "c"]), signal(0))
                .on_context(move |i, cx| {
                    menu.set(format!("menu {i} at {:?}", cx.pointer()));
                })
                .fixed(3),
            text!("{menu}"),
        ])
    });
    let mut driver = app.driver(30, 4);
    driver.settle();
    mouse(&mut driver, MouseKind::Down(Button::Right), 0, 2);
    assert!(
        rows(&driver)[3].starts_with("menu 2"),
        "{:?}",
        rows(&driver)
    );
    assert!(
        styled(&driver).contains("[reverse]c"),
        "the item is selected"
    );
    driver.press("up");
    driver.press("shift+f10");
    assert_eq!(rows(&driver)[3], "menu 1 at None");
    // The menu key itself, as a kitty-protocol terminal reports it.
    driver.press("up");
    driver.press("menu");
    assert_eq!(rows(&driver)[3], "menu 0 at None");
}

// 8. A gauge segment.

#[test]
fn a_gauge_segment_fills_its_share() {
    let app = App::new(|| {
        let done = signal(0.0f64);
        status_bar(
            [seg(|| "x".into())],
            [seg_gauge(move || done.get(), || "half".into()).width(6)],
        )
        .on_key("h", move |_| done.set(0.5))
        .on_key("f", move |_| done.set(2.0))
    });
    let mut driver = app.driver(10, 1);
    driver.settle();
    assert_eq!(rows(&driver)[0], "x    half");
    assert!(styled(&driver).contains("[white on bright_black] half [/]"));
    driver.press("h");
    assert!(styled(&driver).contains("[black on green] ha[/][white on bright_black]lf [/]"));
    // Past 1 is full.
    driver.press("f");
    assert!(styled(&driver).contains("[black on green] half [/]"));
}

// 9. A description list.

#[test]
fn facts_line_up_their_keys() {
    let app = App::new(|| {
        let size = signal(12);
        facts_of_size(size).on_key("+", move |_| size.update(|s| *s += 1))
    });
    let mut driver = app.driver(16, 2);
    driver.press("+");
    assert_eq!(rows(&driver), ["Name notes.txt", "Size 13 B"]);
    assert!(styled(&driver).contains("[bold]Name"));
}

fn facts_of_size(size: Signal<i32>) -> Node {
    intuituive::facts_of(move || {
        vec![
            ("Name".into(), "notes.txt".into()),
            ("Size".into(), format!("{} B", size.get())),
        ]
    })
}

#[test]
fn facts_from_an_array() {
    let app = App::new(|| facts([("A", "1"), ("Long key", "2")]));
    let mut driver = app.driver(16, 2);
    driver.settle();
    assert_eq!(rows(&driver), ["A        1", "Long key 2"]);
}

// 10. A streaming fuzzy list.

/// Settle until `done` holds of the screen, or panic after a few seconds.
fn until(driver: &mut Driver, done: impl Fn(&[String]) -> bool) -> Vec<String> {
    let start = Instant::now();
    loop {
        driver.settle();
        let screen = rows(driver);
        if done(&screen) {
            return screen;
        }
        assert!(start.elapsed() < Duration::from_secs(10), "{screen:#?}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn a_fuzzy_list_fed_from_a_thread_closes_its_modal_on_esc() {
    let app = App::new(|| {
        let picked = signal(String::from("none"));
        text!("picked {picked}").on_key("z", move |cx| {
            cx.modal(Size::Fixed(24), Size::Fixed(8), move || {
                let source = FuzzySource::feed(|feed| {
                    for n in 0..200 {
                        if !feed.push(format!("match {n}")) {
                            return;
                        }
                    }
                });
                fuzzy_list_with(
                    source,
                    String::clone,
                    move |m, cx| {
                        picked.set(m);
                        cx.pop();
                    },
                    FuzzyOptions::new().title("Find"),
                )
            })
        })
    })
    .wait_for_tasks(true);
    let mut driver = app.driver(30, 10);
    driver.press("z");
    until(&mut driver, |s| s.iter().any(|r| r.contains("200/200")));
    assert!(rows(&driver).iter().any(|r| r.contains("Find")));
    driver.type_text("h 42");
    driver.press("enter");
    assert_eq!(rows(&driver)[0], "picked match 42");
    driver.press("z");
    driver.press("esc");
    assert!(!rows(&driver).iter().any(|r| r.contains("Find")), "closed");
}

// 11. Digits.

#[test]
fn digits_draw_a_clock_in_three_rows() {
    let app = App::new(|| {
        let n = signal(7);
        digits(move || format!("{}0", n.get())).on_key("+", move |_| n.set(8))
    });
    let mut driver = app.driver(8, 3);
    driver.settle();
    assert_eq!(rows(&driver), ["╶─┐┌─┐", "  ││ │", "  ╵└─┘"]);
    driver.press("+");
    assert_eq!(rows(&driver)[1], "├─┤│ │");
    assert_eq!(driver.accessibility()[0].name, "80");
}

// 12. Miller columns.

/// A two-level menu.
struct Menu;

impl MillerSource for Menu {
    type Key = String;

    fn items(&self, at: &String) -> Vec<String> {
        match at.as_str() {
            "" => names(&["fruit", "nuts", "veg"]),
            "fruit" => names(&["apple", "pear"]),
            "nuts" => names(&["almond"]),
            _ => names(&["leek"]),
        }
    }

    fn child(&self, at: &String, name: &str) -> Option<String> {
        at.is_empty().then(|| name.to_string())
    }

    fn parent(&self, at: &String) -> Option<(String, String)> {
        (!at.is_empty()).then(|| (String::new(), at.clone()))
    }
}

#[test]
fn miller_moves_from_a_table_and_hooks_reach_its_lists() {
    let app = App::new(|| {
        let m = miller_of(Menu, String::new());
        let opened = signal(String::new());
        let options = MillerOptions::new().current(move |list| {
            list.symbol("> ")
                .on_context(move |i, _| opened.set(i.to_string()))
        });
        column([
            m.navigation_with(
                m.view_with(options).flex(1),
                &[
                    (Move::Down, "n", "Next item"),
                    (Move::Enter, "o", "Open"),
                    (Move::Leave, "u", "Up"),
                    (Move::Bottom, "e", "Last"),
                ],
            ),
            text(move || {
                format!(
                    "{} {} {:?} {opened}",
                    m.at().get(),
                    m.cursor().get(),
                    m.visited().get()
                )
            })
            .fixed(1),
        ])
    });
    let mut driver = app.driver(48, 4);
    driver.settle();
    assert!(rows(&driver)[0].contains("> fruit"), "{:?}", rows(&driver));
    driver.press("n");
    driver.press("o");
    driver.press("e");
    assert_eq!(rows(&driver)[3], r#"nuts 0 ["", "nuts"]"#);
    driver.press("u");
    assert_eq!(rows(&driver)[3], r#" 1 ["nuts", ""]"#);
    // The hook's context handler, on the current column's second row.
    let x = driver.screen().plain()[1]
        .find("nuts")
        .expect("nuts is shown") as u16;
    mouse(&mut driver, MouseKind::Down(Button::Right), x, 1);
    assert!(rows(&driver)[3].ends_with(" 1"), "{:?}", rows(&driver));
    // Remembered levels, by method.
    assert_eq!(m_remembered(), Some("almond".to_string()));
}

/// What a Miller remembers of a level it showed, read by method.
fn m_remembered() -> Option<String> {
    let found = std::rc::Rc::new(std::cell::RefCell::new(None));
    let out = found.clone();
    let app = App::new(move || {
        let m = miller_of(Menu, String::new());
        let found = found.clone();
        m.view()
            .on_key("x", move |_| {
                m.go(Move::Down);
                m.go(Move::Enter);
            })
            .on_key("y", move |_| m.go(Move::Leave))
            .on_key("z", move |_| {
                *found.borrow_mut() = m
                    .remembered_all()
                    .with_untracked(|r| r.get("nuts").cloned());
            })
    });
    let mut driver = app.driver(40, 3);
    driver.press("x");
    driver.press("y");
    driver.press("z");
    let value = out.borrow().clone();
    value
}

/// A fuzzy list on the first screen leaves Esc to the app: there is no
/// screen of its own to close.
#[test]
fn a_fuzzy_list_on_the_first_screen_leaves_esc_to_the_app() {
    let app = App::new(|| {
        let said = signal(String::new());
        column([
            fuzzy_list_with(
                FuzzySource::iter(vec!["alpha", "beta"]),
                |s| s.to_string(),
                |_, _| {},
                FuzzyOptions::new(),
            )
            .flex(1),
            text!("{said}").fixed(1),
        ])
        .on_key("esc", move |cx| {
            said.set(format!("app esc, modal {}", cx.is_modal()))
        })
    })
    .wait_for_tasks(true);
    let mut driver = app.driver(20, 5);
    driver.settle();
    driver.press("esc");
    assert_eq!(rows(&driver)[4], "app esc, modal false");
}
