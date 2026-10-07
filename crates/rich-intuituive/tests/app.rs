//! Whole apps under rich-interact's headless driver: what draws, what is
//! sent, and where input goes.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use intuituive::prelude::*;
use rich_interact::headless::{Headless, Record, Script};

fn run(app: App, script: Script, width: u16, height: u16) -> Record {
    let mut backend = Headless::new(script, width, height);
    let record = backend.record();
    app.run_on(&mut backend)
        .expect("the app runs to the end of its script");
    let record = record.borrow().clone();
    record
}

fn screen(record: &Record) -> Vec<String> {
    record
        .last_frame()
        .lines()
        .map(|l| l.trim_end().to_string())
        .collect()
}

#[test]
fn a_change_sends_only_the_cells_that_changed() {
    let app = App::new(|| {
        let tick = signal(0u32);
        column([
            label("[bold]ops[/] · 20 services").fixed(1),
            row([
                label("left pane\nmore text").panel("Services"),
                label("right pane").panel("Detail"),
            ]),
            text!("tick {tick}").fixed(1),
        ])
        .on_key("+", move |_| tick.update(|t| *t += 1))
        .on_key("q", |cx| cx.quit())
    });
    let record = run(app, Script::new().keys("+ q"), 60, 12);
    assert_eq!(screen(&record)[11], "tick 1");
    // The first paint writes the screen; the tick writes one cell.
    let paints: Vec<&String> = record
        .writes
        .iter()
        .filter(|w| w.contains("\x1b["))
        .collect();
    assert!(
        paints[1].len() > 500,
        "the first paint: {:?}",
        paints[1].len()
    );
    let tick = paints[2];
    assert_eq!(tick, "\x1b[12;6H\x1b[0m1", "{tick:?}");
}

#[test]
fn only_the_nodes_that_read_a_signal_draw_again() {
    let draws = Rc::new(RefCell::new(Vec::<&'static str>::new()));
    let log = |name: &'static str, draws: &Rc<RefCell<Vec<&'static str>>>| {
        let draws = draws.clone();
        move |_: &rich::Console, _: u16, _: u16| {
            draws.borrow_mut().push(name);
            Vec::new()
        }
    };
    let (a, b) = (log("a", &draws), log("b", &draws));
    let app = App::new(move || {
        let count = signal(0);
        column([
            leaf(move |c, w, h| {
                count.get();
                a(c, w, h)
            }),
            leaf(b),
        ])
        .on_key("+", move |_| count.update(|c| *c += 1))
        .on_key("q", |cx| cx.quit())
    });
    run(app, Script::new().keys("+ + q"), 20, 4);
    assert_eq!(*draws.borrow(), ["a", "b", "a", "a"]);
}

#[test]
fn keys_bubble_from_the_focused_node_and_tab_moves_the_focus() {
    let pressed = Rc::new(RefCell::new(Vec::<String>::new()));
    let p = pressed.clone();
    let app = App::new(move || {
        let (p1, p2, p3) = (p.clone(), p.clone(), p.clone());
        column([
            label("one")
                .focusable()
                .on_key("x", move |_| p1.borrow_mut().push("one".into()))
                .panel("One"),
            label("two")
                .focusable()
                .on_key("y", move |_| p2.borrow_mut().push("two".into()))
                .panel("Two"),
        ])
        .on_key("x y", move |_| p3.borrow_mut().push("root".into()))
        .on_key("q", |cx| cx.quit())
    });
    // Focus starts on "one": x is its own; y bubbles to the root. After Tab,
    // "two" has it: y is its own, x bubbles.
    run(app, Script::new().keys("x y tab y x q"), 30, 8);
    assert_eq!(*pressed.borrow(), ["one", "root", "two", "root"]);
}

#[test]
fn the_focused_panel_is_highlighted_and_moving_focus_redraws_only_borders() {
    let app = App::new(|| {
        column([
            label("one").focusable().panel("One"),
            label("two").focusable().panel("Two"),
        ])
        .on_key("q", |cx| cx.quit())
    })
    .theme(Theme {
        border: rich::Style::parse("blue").unwrap(),
        border_focused: rich::Style::parse("red").unwrap(),
        ..Theme::default()
    });
    let record = run(app, Script::new().keys("tab q"), 20, 6);
    let paints: Vec<&String> = record
        .writes
        .iter()
        .filter(|w| w.contains("\x1b["))
        .collect();
    let first = paints[1];
    // Truecolor red for the focused panel, blue for the other.
    assert!(first.contains("\x1b[0;31m╭─"), "{first:?}");
    assert!(first.contains("\x1b[0;34m╭─"), "{first:?}");
    // After Tab both borders swap colour; the labels inside are not sent.
    let tab = paints[2];
    assert!(tab.contains("31m") && tab.contains("34m"), "{tab:?}");
    assert!(!tab.contains("one") && !tab.contains("two"), "{tab:?}");
}

#[test]
fn clicks_go_to_the_node_under_the_pointer() {
    let clicked = Rc::new(RefCell::new(Vec::<&str>::new()));
    let c = clicked.clone();
    let app = App::new(move || {
        let (c1, c2) = (c.clone(), c.clone());
        row([
            label("left").on_click(move |_| c1.borrow_mut().push("left")),
            label("right").on_click(move |_| c2.borrow_mut().push("right")),
        ])
        .on_key("q", |cx| cx.quit())
    });
    run(
        app,
        Script::new().click(2, 0).click(15, 1).click(9, 0).keys("q"),
        20,
        2,
    );
    assert_eq!(*clicked.borrow(), ["left", "right", "left"]);
}

#[test]
fn keyed_children_keep_their_nodes_when_the_list_is_reordered() {
    let built = Rc::new(std::cell::Cell::new(0));
    let b = built.clone();
    let app = App::new(move || {
        let order = signal(vec!["a", "b", "c"]);
        let b = b.clone();
        each(
            move || order.get(),
            move |key| {
                b.set(b.get() + 1);
                label(key)
            },
        )
        .on_key("r", move |_| order.update(|o| o.reverse()))
        .on_key("d", move |_| order.update(|o| o.retain(|k| *k != "b")))
        .on_key("n", move |_| order.update(|o| o.push("n")))
        .on_key("q", |cx| cx.quit())
    });
    let record = run(app, Script::new().keys("r d n q"), 10, 4);
    assert_eq!(screen(&record), ["c", "a", "n", ""]);
    // Three built at first, one more for "n"; reordering and removing
    // built nothing.
    assert_eq!(built.get(), 4);
}

#[test]
fn timers_run_on_the_backends_clock() {
    let app = App::new(|| {
        let seconds = signal(0u32);
        every(Duration::from_secs(1), move |_| seconds.update(|s| *s += 1));
        text!("up {seconds}s").on_key("q", |cx| cx.quit())
    });
    let record = run(
        app,
        Script::new().wait(Duration::from_millis(3500)).keys("q"),
        10,
        1,
    );
    assert_eq!(screen(&record), ["up 3s"]);
}

#[test]
fn a_proxy_updates_the_screen_from_another_thread() {
    let app = App::new(|| {
        let status = signal(String::from("loading"));
        text!("{status}")
            .on_key("f", move |cx| {
                let proxy = cx.proxy();
                std::thread::spawn(move || {
                    let body = "fetched".to_string(); // slow work
                    proxy.run(move || status.set(body));
                })
                .join()
                .unwrap();
            })
            .on_key("q", |cx| cx.quit())
    });
    let record = run(
        app,
        Script::new()
            .keys("f")
            .wait(Duration::from_millis(100))
            .keys("q"),
        20,
        1,
    );
    assert_eq!(screen(&record), ["fetched"]);
}

#[test]
fn a_resize_lays_everything_out_again() {
    let app =
        App::new(|| row([label("left").percent(50), label("right")]).on_key("q", |cx| cx.quit()));
    let record = run(app, Script::new().resize(30, 1).keys("q"), 20, 1);
    assert_eq!(screen(&record), ["left           right"]);
}

#[test]
fn hyperlinks_reach_the_terminal() {
    let app =
        App::new(|| label("[link=https://example.com]docs[/link]").on_key("q", |cx| cx.quit()));
    let record = run(app, Script::new().keys("q"), 20, 1);
    let output = record.output();
    // Open the link, (colours), the text, close it.
    assert!(
        output.contains("\x1b]8;;https://example.com\x1b\\\x1b[0mdocs\x1b]8;;\x1b\\"),
        "{output:?}"
    );
}

#[test]
fn rich_renderables_are_nodes() {
    let app = App::new(|| {
        renderable(|| {
            let mut table = rich::Table::new();
            table.add_column("name");
            table.add_row(&["api"]);
            table
        })
        .on_key("q", |cx| cx.quit())
    });
    let record = run(app, Script::new().keys("q"), 20, 5);
    let screen = screen(&record).join("\n");
    assert!(
        screen.contains("name") && screen.contains("api"),
        "{screen}"
    );
}

#[test]
fn a_log_appends_under_its_lines_then_scrolls() {
    let app = App::new(|| {
        let log = Log::new(100);
        let n = signal(0u32);
        log.view()
            .on_key("a", move |_| {
                n.update(|n| *n += 1);
                log.push(format!("line {}", n.get_untracked()));
            })
            .on_key("q", |cx| cx.quit())
    });
    let mut backend = Headless::new(Script::new().keys("a a a a a q"), 10, 3);
    let record = backend.record();
    app.run_on(&mut backend).unwrap();
    let record = record.borrow();
    let frames: Vec<Vec<String>> = record
        .frames
        .iter()
        .map(|f| f.lines().map(|l| l.trim_end().to_string()).collect())
        .collect();
    assert_eq!(frames[1], ["line 1", "", ""]);
    assert_eq!(frames[3], ["line 1", "line 2", "line 3"]);
    assert_eq!(frames[5], ["line 3", "line 4", "line 5"]);
}

#[test]
fn a_log_keeps_only_its_capacity() {
    let app = App::new(|| {
        let log = Log::new(2);
        for i in 0..5 {
            log.push(format!("{i}"));
        }
        log.view().on_key("q", |cx| cx.quit())
    });
    assert_eq!(
        app.render_with(&["q"], 4, 3).unwrap(),
        ["3   ", "4   ", "    "]
    );
}

#[test]
fn a_focused_component_takes_keys_and_shows_its_caret() {
    use rich_interact::Input;
    let app = App::new(|| {
        let name = signal(String::new());
        let cancelled = signal(false);
        column([
            component(Input::new("Name"), move |value, _| name.set(value))
                .on_cancel(move |_| cancelled.set(true))
                .fixed(1),
            text!("name={name} cancelled={cancelled}"),
        ])
        .on_key("ctrl+q", |cx| cx.quit())
    });
    let record = run(app, Script::new().keys("A d a enter esc ctrl+q"), 30, 3);
    assert_eq!(screen(&record)[1], "name=Ada cancelled=true");
    // While typing, the terminal cursor is shown at the caret.
    assert!(
        record.output().contains("\x1b[?25h"),
        "{:?}",
        record.output()
    );
}

#[test]
fn clicking_a_component_focuses_it() {
    use rich_interact::Input;
    let app = App::new(|| {
        let a = signal(String::new());
        let b = signal(String::new());
        column([
            component(Input::new("A"), move |v, _| a.set(v)).fixed(1),
            component(Input::new("B"), move |v, _| b.set(v)).fixed(1),
            text!("a={a} b={b}"),
        ])
        .on_key("ctrl+q", |cx| cx.quit())
    });
    let record = run(app, Script::new().click(5, 1).keys("x enter ctrl+q"), 30, 3);
    assert_eq!(screen(&record)[2], "a= b=x");
}

/// A border two or three columns wide underflowed its width.
#[test]
fn narrow_panels_draw() {
    for width in 1..6 {
        let app = App::new(|| label("x").panel("Title").on_key("q", |cx| cx.quit()));
        let screen = app.render_with(&["q"], width, 3).unwrap();
        assert_eq!(screen.len(), 3, "{width}: {screen:?}");
    }
}

/// Rows below a keyed list's viewport that change while hidden draw their
/// new value when they come into view.
#[test]
fn hidden_keyed_rows_redraw_when_shown() {
    let app = App::new(|| {
        let order = signal(vec![0usize, 1, 2]);
        let labels = signal(vec!["a".to_string(), "b".into(), "c".into()]);
        each(
            move || order.get(),
            move |k| text(move || labels.get()[k].clone()),
        )
        .on_key("x", move |_| labels.update(|l| l[2] = "C".into()))
        .on_key("r", move |_| order.set(vec![2, 0, 1]))
        .on_key("q", |cx| cx.quit())
    });
    let screen = app.render_with(&["x", "r", "q"], 4, 2).unwrap();
    assert_eq!(screen[0].trim_end(), "C", "{screen:?}");
}

/// Review findings on #672, each failing before its fix.
mod review {
    use super::*;
    use rich_interact::{Button, Event, Mouse, MouseKind};

    /// A right click is not a left click: `on_click` takes the left button
    /// only.
    #[test]
    fn only_the_left_button_clicks() {
        let clicks = Rc::new(RefCell::new(0));
        let c = clicks.clone();
        let app = App::new(move || {
            let c = c.clone();
            label("button")
                .on_click(move |_| *c.borrow_mut() += 1)
                .on_key("q", |cx| cx.quit())
        });
        let right = Event::Mouse(Mouse::new(MouseKind::Down(Button::Right), 1, 0));
        run(app, Script::new().event(right).click(1, 0).keys("q"), 10, 1);
        assert_eq!(*clicks.borrow(), 1);
    }

    /// Removing the focused row of a keyed list must not strand the keys:
    /// app-wide bindings keep working.
    #[test]
    fn keys_still_reach_the_app_after_the_focused_node_is_removed() {
        let app = App::new(|| {
            let rows = signal(vec![1, 2]);
            each(
                move || rows.get(),
                |k| label(format!("row {k}")).focusable(),
            )
            .on_key("d", move |_| rows.set(vec![2]))
            .on_key("q", |cx| cx.quit())
        });
        // Focus starts on row 1; "d" removes it; "q" must still quit.
        let record = run(app, Script::new().keys("d q"), 10, 2);
        assert_eq!(screen(&record)[0], "row 2");
    }

    /// A zero interval spun the loop forever; it runs once a millisecond.
    #[test]
    fn a_zero_interval_timer_does_not_hang() {
        let app = App::new(|| {
            let n = signal(0u32);
            every(Duration::ZERO, move |_| n.update(|n| *n += 1));
            text!("{n}").on_key("q", |cx| cx.quit())
        });
        let record = run(
            app,
            Script::new().wait(Duration::from_millis(5)).keys("q"),
            10,
            1,
        );
        assert!(!screen(&record)[0].is_empty());
    }

    /// A log smaller than its view that drops its oldest line shows the
    /// lines it keeps.
    #[test]
    fn a_log_that_evicts_shows_what_it_keeps() {
        let app = App::new(|| {
            let log = Log::new(2);
            log.push("0");
            log.push("1");
            log.view()
                .on_key("a", move |_| log.push("2"))
                .on_key("q", |cx| cx.quit())
        });
        let record = run(app, Script::new().keys("a q"), 4, 3);
        assert_eq!(screen(&record), ["1", "2", ""]);
    }
}
