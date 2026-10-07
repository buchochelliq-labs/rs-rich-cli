//! Screens and modals: the stack, where input goes, and what is drawn.

mod common;

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use common::{row_of, run, run_open, screen};
use intuituive::prelude::*;
use rich_interact::headless::Script;

fn detail() -> Node {
    let count = signal(0u32);
    text!("detail {count}")
        .focusable()
        .on_key("+", move |_| count.update(|c| *c += 1))
        .on_key("esc", |cx| cx.pop())
}

#[test]
fn a_pushed_screen_takes_the_keys_and_popping_goes_back() {
    let app = App::new(|| {
        let opened = signal(0u32);
        column([text!("home, opened {opened} times")])
            .focusable()
            .on_key("enter", move |cx| {
                opened.update(|n| *n += 1);
                cx.push(detail);
            })
            .on_key("+", |_| panic!("the home screen's keys are not reachable"))
            .on_key("q", |cx| cx.quit())
    });
    let record = run_open(app, Script::new().keys("enter + +"), 30, 3);
    assert_eq!(screen(&record)[0], "detail 2");

    let app = App::new(|| {
        let opened = signal(0u32);
        text!("home, opened {opened} times")
            .on_key("enter", move |cx| {
                opened.update(|n| *n += 1);
                cx.push(detail);
            })
            .on_key("q", |cx| cx.quit())
    });
    let record = run(app, Script::new().keys("enter esc enter esc q"), 30, 3);
    let rows = screen(&record);
    assert_eq!(rows[0], "home, opened 2 times");
    assert!(rows[1..].iter().all(String::is_empty), "{rows:?}");
}

#[test]
fn a_modal_draws_over_the_screen_below_and_closes() {
    let app = App::new(|| {
        column([label("background ".repeat(40))])
            .on_key("o", |cx| {
                cx.modal(Size::Fixed(14), Size::Auto, || {
                    label("Sure?").panel("Quit").on_key("esc", |cx| cx.pop())
                })
            })
            .on_key("q", |cx| cx.quit())
    });
    let record = run_open(app, Script::new().keys("o"), 30, 7);
    let rows = screen(&record);
    // A 3-row box (the label and its border) centred: rows 2-4, columns
    // 8-21; the background is still drawn around it.
    assert_eq!(rows[1], "background background");
    assert_eq!(rows[2], "backgrou╭─ Quit ─────╮");
    assert_eq!(rows[3], "backgrou│Sure?       │");
    assert_eq!(rows[4], "backgrou╰────────────╯");

    let app = App::new(|| {
        column([label("background ".repeat(40))])
            .on_key("o", |cx| {
                cx.modal(Size::Fixed(14), Size::Auto, || {
                    label("Sure?").panel("Quit").on_key("esc", |cx| cx.pop())
                })
            })
            .on_key("q", |cx| cx.quit())
    });
    let rows = screen(&run(app, Script::new().keys("o esc q"), 30, 7));
    assert!(
        rows.iter().all(|r| r == "background background"),
        "{rows:?}"
    );
}

#[test]
fn the_screen_below_a_modal_keeps_drawing_without_overwriting_it() {
    let app = App::new(|| {
        let ticks = signal(0u32);
        every(Duration::from_secs(1), move |_| ticks.update(|t| *t += 1));
        column([text(move || format!("tick {}\n", ticks.get()).repeat(5))]).on_key("o", |cx| {
            cx.modal(Size::Percent(50), Size::Fixed(3), || {
                label("modal").panel("").on_key("q", |cx| cx.quit())
            })
        })
    });
    let rows = screen(&run(
        app,
        Script::new()
            .keys("o")
            .wait(Duration::from_secs(2))
            .keys("q"),
        20,
        5,
    ));
    assert_eq!(rows[0], "tick 2");
    assert!(rows[2].contains("│modal"), "{rows:?}");
    assert!(rows[1].contains("╭"), "{rows:?}");
}

#[test]
fn each_screen_keeps_its_focus() {
    let app = App::new(|| {
        let picked = signal(String::new());
        column([
            label("a")
                .focusable()
                .on_key("enter", move |_| picked.set("a".into())),
            label("b")
                .focusable()
                .on_key("enter", move |_| picked.set("b".into())),
            text!("picked {picked}"),
        ])
        .on_key("o", |cx| cx.push(detail))
        .on_key("q", |cx| cx.quit())
    });
    // Focus b, open and close a screen, then Enter: b still has the focus.
    let rows = screen(&run(app, Script::new().keys("tab o esc enter q"), 20, 3));
    assert_eq!(row_of(&rows, "picked b"), Some(2), "{rows:?}");
}

#[test]
fn a_screens_timers_stop_when_it_closes() {
    let ticks = Rc::new(Cell::new(0));
    let counted = ticks.clone();
    let app = App::new(move || {
        let counted = counted.clone();
        label("home")
            .on_key("o", move |cx| {
                let counted = counted.clone();
                cx.push(move || {
                    every(Duration::from_millis(100), move |_| {
                        counted.set(counted.get() + 1)
                    });
                    label("timed").on_key("esc", |cx| cx.pop())
                })
            })
            .on_key("q", |cx| cx.quit())
    });
    run(
        app,
        Script::new()
            .keys("o")
            .wait(Duration::from_millis(350))
            .keys("esc")
            .wait(Duration::from_secs(1))
            .keys("q"),
        20,
        2,
    );
    assert_eq!(ticks.get(), 3);
}

#[test]
fn replace_swaps_the_top_screen() {
    let app = App::new(|| {
        label("step 1")
            .on_key("n", |cx| {
                cx.replace(|| label("step 2").on_key("q", |cx| cx.quit()))
            })
            .on_key("q", |_| panic!("step 1 is gone"))
    });
    let rows = screen(&run(app, Script::new().keys("n q"), 20, 2));
    assert_eq!(rows[0], "step 2");
}

#[test]
fn named_theme_styles_work_in_markup_and_switch_at_run_time() {
    let app = App::new(|| {
        label("[accent]hi[/]")
            .on_key("l", |cx| cx.set_theme(Theme::light()))
            .on_key("q", |cx| cx.quit())
    });
    let record = run(app, Script::new().keys("l q"), 10, 1);
    let out = record.output();
    // Dark accent is bright cyan (96); light accent is blue (34).
    let dark = out.find("\x1b[0;96mhi").expect("drawn in the dark accent");
    let light = out
        .find("\x1b[0;34mhi")
        .expect("drawn again in the light accent");
    assert!(dark < light);
}

#[test]
fn a_modal_that_shrinks_leaves_nothing_behind() {
    let app = App::new(|| {
        let lines = signal(3usize);
        label("")
            .on_key("o", move |cx| {
                cx.modal(Size::Fixed(12), Size::Auto, move || {
                    text(move || vec!["modal"; lines.get()].join("\n"))
                        .panel("")
                        .on_key("-", move |_| lines.set(1))
                })
            })
            .on_key("q", |cx| cx.quit())
    });
    let rows = screen(&run_open(app, Script::new().keys("o -"), 20, 7));
    // A 3-row box (one line and the border) in rows 2-4; rows 1 and 5,
    // where the taller box was, are empty again.
    assert_eq!(rows[1], "");
    assert!(rows[3].contains("│modal"), "{rows:?}");
    assert_eq!(rows[5], "");
}

#[test]
fn moving_the_focus_redraws_only_panel_edges() {
    let app = App::new(|| {
        row([
            label("left content").focusable().panel("A"),
            label("right content").focusable().panel("B"),
        ])
        .on_key("q", |cx| cx.quit())
    })
    .inspector(true);
    let rows = screen(&run(app, Script::new().keys("tab q"), 100, 12));
    // Both panels' borders change colour; their labels do not draw again.
    let text = rows.join("\n");
    assert!(text.contains("drew 2 of 2 dirty"), "{text}");
}

#[test]
fn a_screens_watches_stop_when_it_closes() {
    let calls = Rc::new(Cell::new(0));
    let seen = calls.clone();
    let app = App::new(move || {
        let n = signal(0u32);
        let seen = seen.clone();
        label("home")
            .on_key("o", move |cx| {
                let seen = seen.clone();
                cx.push(move || {
                    watch(move || n.get(), move |_, _| seen.set(seen.get() + 1));
                    label("watching").on_key("esc", |cx| cx.pop())
                })
            })
            .on_key("+", move |_| n.update(|v| *v += 1))
            .on_key("q", |cx| cx.quit())
    });
    // Open (first run), close, then change n: the watch does not run again.
    run(app, Script::new().keys("o esc + + q"), 20, 2);
    assert_eq!(calls.get(), 1);
}

#[test]
fn a_panel_title_keeps_its_emoji_when_the_focus_moves() {
    let app = App::new(|| {
        row([
            label("a").focusable().panel("👨‍👩‍👧 team"),
            label("b").focusable().panel("other"),
        ])
        .on_key("q", |cx| cx.quit())
    });
    // The first panel takes the focus, then gives it up: its edges are
    // redrawn twice without its contents.
    let rows = screen(&run(app, Script::new().keys("tab q"), 40, 4));
    assert!(rows[0].contains("👨‍👩‍👧 team"), "{rows:?}");
    assert!(rows[0].contains("╮╭"), "the border is whole: {rows:?}");
}
