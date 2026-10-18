//! The inspector's selection, picking, outline, timings, event trace and
//! redraw flash (#714), driven headless.

use std::time::Duration;

use intuituive::inspect::{TraceKind, TRACE_LIMIT};
use intuituive::interact::{Button, Event, Key, Mouse, MouseKind};
use intuituive::prelude::*;
use intuituive::Driver;

/// 100 columns: the app lays out in the left 60, the panel takes 40.
const WIDTH: u16 = 100;
const HEIGHT: u16 = 20;
const PANEL: usize = 60;

fn build() -> Node {
    let count = signal(0);
    let clicks = signal(0);
    column([
        text!("count {count}").fixed(1).name("count"),
        text!("clicks {clicks}")
            .fixed(1)
            .name("button")
            .on_click(move |_| clicks.update(|c| *c += 1)),
        row([
            label("left").panel("A").name("a"),
            label("right").panel("B").name("b").hide_below(70),
        ])
        .name("cards"),
        label("help").fixed(1).name("help").class("muted"),
    ])
    .name("root")
    .on_key("+ ctrl+up", move |_| count.update(|c| *c += 1))
    .on_key("q", |cx| cx.quit())
}

fn inspected() -> Driver {
    let mut driver = App::new(build).inspector(true).driver(WIDTH, HEIGHT);
    driver.settle();
    driver
}

/// The panel's text: the right of each row.
fn panel(driver: &Driver) -> String {
    driver
        .screen()
        .plain()
        .iter()
        .map(|row| {
            row.chars()
                .skip(PANEL)
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The app's rows: the left of each row.
fn app_rows(driver: &Driver) -> Vec<String> {
    driver
        .screen()
        .plain()
        .iter()
        .map(|row| {
            row.chars()
                .take(PANEL)
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect()
}

fn mouse(driver: &mut Driver, kind: MouseKind, column: u16, row: u16) {
    driver.event(Event::Mouse(Mouse::new(kind, column, row)));
    driver.settle();
}

/// Whether the cell's style is reversed (the outline, the cursor).
fn reversed(driver: &Driver, x: u16, y: u16) -> bool {
    let screen = driver.screen();
    screen
        .style(screen.cell(x, y).style)
        .is_some_and(|style| style.attr(6) == Some(true))
}

// --8<-- [start:trace]
#[test]
fn a_test_reads_what_the_inspector_saw() {
    let app = App::new(|| {
        let count = signal(0);
        text!("count {count}")
            .name("count")
            .on_key("+", move |_| count.update(|n| *n += 1))
    })
    .inspector(true);
    let mut driver = app.driver(80, 10);
    driver.press("+");
    let trace = driver.inspector_trace();
    assert_eq!(trace[0].text, "key + → count on_key");
    assert!(trace[1].text.contains("→ count"), "the write made it dirty");
    let timing = &driver.inspector_timings()[0];
    // The first frame, and the one after `+`.
    assert_eq!((timing.name.as_str(), timing.draws), ("count", 2));
}
// --8<-- [end:trace]

#[test]
fn the_arrows_select_nodes_and_the_node_pane_shows_their_layout() {
    let mut driver = inspected();
    // Shift+F12: the inspector has the keys; the first arrow selects the
    // focused node (the clickable `button`).
    driver.press("shift+f12 down");
    let text = panel(&driver);
    assert!(text.contains("keys"), "{text}");
    assert!(text.contains("│›   button (text)"), "{text}");
    driver.press("enter");
    let text = panel(&driver);
    assert!(text.contains("button (text) #"), "{text}");
    assert!(text.contains("at 0,1 · 60x1"), "{text}");
    assert!(text.contains("size fixed 1 (code)"), "{text}");
    assert!(text.contains("focusable · focused"), "{text}");
    assert!(text.contains("states :focus"), "{text}");
    assert!(text.contains("has on_click"), "{text}");
    // Down to the row of cards, into the first panel, to its label, and
    // back out to the panel.
    driver.press("down");
    let text = panel(&driver);
    assert!(text.contains("cards (row) #"), "{text}");
    assert!(text.contains("at 0,2 · 60x17"), "{text}");
    assert!(text.contains("not focusable"), "{text}");
    driver.press("right right");
    assert!(panel(&driver).contains("label #"), "{}", panel(&driver));
    driver.press("left");
    let text = panel(&driver);
    assert!(text.contains("a (panel) \"A\" #"), "{text}");
    assert!(text.contains("role region"), "{text}");
    // The hidden panel: its rule, and the viewport that hides it.
    driver.press("down down");
    let text = panel(&driver);
    assert!(text.contains("b (panel) \"B\" #"), "{text}");
    assert!(
        text.contains("hidden by hide_below 70 (app 60x20)"),
        "{text}"
    );
    driver.press("end");
    let text = panel(&driver);
    assert!(text.contains("help (label.muted) #"), "{text}");
    assert!(text.contains("classes .muted"), "{text}");
    // The tree pane marks the selection.
    driver.press("enter");
    let text = panel(&driver);
    let line = text
        .lines()
        .find(|line| line.contains("help (label.muted)"))
        .expect("the tree shows it");
    assert!(line.trim_start_matches('│').starts_with('›'), "{text}");
}

#[test]
fn keys_do_not_reach_the_app_while_the_inspector_has_them() {
    let mut driver = inspected();
    driver.press("shift+f12 + ctrl+up q space x");
    assert_eq!(app_rows(&driver)[0], "count 0");
    assert!(!driver.is_done(), "q was the inspector's");
    // Pasting goes nowhere either.
    driver.event(Event::Paste("hello".into()));
    driver.settle();
    // Esc gives the keys back.
    driver.press("esc +");
    assert_eq!(app_rows(&driver)[0], "count 1");
    // Shift+F12 too, both ways.
    driver.press("shift+f12 + shift+f12 +");
    assert_eq!(app_rows(&driver)[0], "count 2");
    // F12 hides the panel and gives the keys back.
    driver.press("shift+f12 f12 +");
    assert_eq!(driver.screen().plain()[0].trim_end(), "count 3");
}

#[test]
fn the_selected_node_is_outlined_without_moving_anything() {
    let mut driver = inspected();
    let before = app_rows(&driver);
    let styled = driver.screen().lines();
    // The row of cards: rows 2 to 18, columns 0 to 59.
    driver.press("shift+f12 down down");
    assert_eq!(app_rows(&driver), before, "the layout is the same");
    for (x, y) in [
        (0, 2),
        (59, 2),
        (0, 18),
        (59, 18),
        (0, 10),
        (59, 10),
        (30, 2),
    ] {
        assert!(reversed(&driver, x, y), "edge cell {x},{y}");
    }
    // Inside it, the panel's own cells are as they were.
    assert!(!reversed(&driver, 5, 5));
    assert!(!reversed(&driver, 30, 10));
    // Outside it too.
    assert!(!reversed(&driver, 0, 1) && !reversed(&driver, 0, 19));
    // The keys back to the app: the outline goes, every cell as it was.
    driver.press("esc");
    for (x, y) in [(0, 2), (59, 18), (30, 2)] {
        assert!(!reversed(&driver, x, y), "{x},{y} put back");
    }
    let app = |lines: &[Vec<intuituive::rich::Segment>]| {
        lines
            .iter()
            .map(|line| {
                let mut column = 0;
                line.iter()
                    .take_while(|segment| {
                        column += segment.cell_length();
                        column <= PANEL
                    })
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(app(&driver.screen().lines()), app(&styled));
}

#[test]
fn picking_follows_the_pointer_and_a_click_keeps_what_is_under_it() {
    let mut driver = inspected();
    driver.press("shift+f12 p");
    assert!(panel(&driver).contains("pick"), "{}", panel(&driver));
    // The pointer over the button selects it, and outlines it; the node
    // pane shows it.
    mouse(&mut driver, MouseKind::Moved, 3, 1);
    assert!(
        panel(&driver).contains("button (text) #"),
        "{}",
        panel(&driver)
    );
    assert!(reversed(&driver, 0, 1) && reversed(&driver, 59, 1));
    // Over the first panel's label.
    mouse(&mut driver, MouseKind::Moved, 4, 4);
    assert!(panel(&driver).contains("label #"), "{}", panel(&driver));
    // A click on the button keeps it, and the button does not see it.
    mouse(&mut driver, MouseKind::Down(Button::Left), 3, 1);
    mouse(&mut driver, MouseKind::Up(Button::Left), 3, 1);
    let text = panel(&driver);
    assert!(text.contains("button (text)"), "{text}");
    assert!(text.contains("keys  · esc"), "picking is over: {text}");
    assert_eq!(app_rows(&driver)[1], "clicks 0");
    // Picking with the arrows: from the button's corner down a row, onto
    // the first panel's border; Enter keeps it.
    driver.press("p down enter");
    assert!(
        panel(&driver).contains("a (panel) \"A\""),
        "{}",
        panel(&driver)
    );
    // Esc puts the selection back as it was.
    driver.press("p down down esc");
    assert!(
        panel(&driver).contains("a (panel) \"A\""),
        "{}",
        panel(&driver)
    );
    // Without the inspector's keys, a click is the app's again.
    driver.press("esc");
    mouse(&mut driver, MouseKind::Down(Button::Left), 3, 1);
    assert_eq!(app_rows(&driver)[1], "clicks 1");
}

#[test]
fn a_click_on_the_panel_selects_a_tree_row_and_takes_the_keys() {
    let mut driver = inspected();
    let rows = driver.screen().plain();
    let y = rows
        .iter()
        .position(|row| row.contains("cards (row)"))
        .expect("the tree shows the row") as u16;
    mouse(&mut driver, MouseKind::Down(Button::Left), 70, y);
    driver.press("2");
    let text = panel(&driver);
    assert!(text.contains("cards (row) #"), "{text}");
    // The panel's keys: `+` is not the app's.
    driver.press("+");
    assert_eq!(app_rows(&driver)[0], "count 0");
    // A click on a pane's name shows it.
    let rows = driver.screen().plain();
    let tabs = rows
        .iter()
        .position(|row| row.contains("tree node timings trace"))
        .expect("the panes' names");
    let row = &rows[tabs];
    let at = row[..row.find("trace").expect("trace")].chars().count() as u16;
    mouse(
        &mut driver,
        MouseKind::Down(Button::Left),
        at + 1,
        tabs as u16,
    );
    assert!(panel(&driver).contains("events"), "{}", panel(&driver));
}

#[test]
fn the_timings_pane_measures_each_node_while_the_inspector_is_open() {
    let mut driver = inspected();
    driver.press("+ + +");
    let timings = driver.inspector_timings();
    let count = timings
        .iter()
        .find(|t| t.name == "count")
        .expect("the count node was measured");
    // The first frame, and one for each `+`.
    assert_eq!(count.draws, 4);
    assert!(count.total >= count.last);
    let help = timings.iter().find(|t| t.name == "help").expect("help");
    assert_eq!(help.draws, 1, "it drew once, in the first frame");
    driver.press("shift+f12 3");
    let text = panel(&driver);
    assert!(text.contains("last frame"), "{text}");
    assert!(text.contains("avg"), "{text}");
    assert!(text.contains("4  count"), "{text}");
    // `s` sorts by draws, `c` starts again.
    driver.press("s s");
    assert!(panel(&driver).contains("by draws"), "{}", panel(&driver));
    driver.press("c");
    assert!(driver.inspector_timings().is_empty());
    assert!(panel(&driver).contains("nothing drew yet"));
}

fn busy() -> App {
    App::new(|| {
        let count = signal(0);
        let ticks = signal(0);
        let answer = signal(0);
        every(Duration::from_secs(1), move |_| ticks.update(|t| *t += 1));
        column([
            text!("count {count} ticks {ticks} answer {answer}")
                .fixed(1)
                .name("status"),
            label("click me")
                .fixed(1)
                .name("button")
                .on_click(move |_| count.update(|c| *c += 1)),
            label("rest").name("rest"),
        ])
        .name("root")
        .on_key("ctrl+up", move |_| count.update(|c| *c += 1))
        .on_key("t", move |_| {
            spawn(|| 6 * 7, move |n, _| answer.set(n));
        })
        .on_key("o", |cx| {
            cx.push(|| label("second").name("second").on_key("esc", |cx| cx.pop()))
        })
    })
    .inspector(true)
    .wait_for_tasks(true)
}

fn has(trace: &[intuituive::inspect::TraceEntry], kind: TraceKind, text: &str) -> bool {
    trace
        .iter()
        .any(|e| e.kind == kind && e.text.contains(text))
}

#[test]
fn the_trace_says_what_happened_and_what_took_it() {
    let mut driver = busy().driver(WIDTH, 12);
    driver.settle();
    // A key with a modifier, the binding that took it, and the write.
    driver.press("ctrl+up");
    let trace = driver.inspector_trace();
    assert!(
        has(&trace, TraceKind::Key, "key ctrl+up → root on_key"),
        "{trace:#?}"
    );
    let write = trace
        .iter()
        .find(|e| e.kind == TraceKind::Signal)
        .expect("the write");
    assert!(write.text.starts_with("signal inspector.rs:"), "{write:?}");
    assert!(
        write.text.contains("→ status (i32, by inspector.rs:"),
        "{write:?}"
    );
    // A click: the node under it and the handler that ran.
    mouse(&mut driver, MouseKind::Down(Button::Left), 1, 1);
    mouse(&mut driver, MouseKind::Up(Button::Left), 1, 1);
    let trace = driver.inspector_trace();
    assert!(
        has(
            &trace,
            TraceKind::Mouse,
            "press left 1,1 on button → button on_click"
        ),
        "{trace:#?}"
    );
    assert!(has(&trace, TraceKind::Mouse, "release left 1,1 on button"));
    // The wheel and a drag that nothing takes.
    mouse(&mut driver, MouseKind::ScrollDown, 2, 4);
    driver.event(Event::Mouse(Mouse {
        modifiers: intuituive::interact::Modifiers::CTRL,
        ..Mouse::new(MouseKind::Drag(Button::Left), 3, 5)
    }));
    driver.settle();
    let trace = driver.inspector_trace();
    assert!(
        has(&trace, TraceKind::Mouse, "wheel down 2,4 on rest (unused)"),
        "{trace:#?}"
    );
    assert!(
        has(&trace, TraceKind::Mouse, "ctrl+drag left 3,5 on rest"),
        "{trace:#?}"
    );
    // A resize, and a paste the focused button does not take.
    driver.event(Event::Resize {
        columns: 90,
        rows: 12,
    });
    driver.event(Event::Paste("abc".into()));
    driver.settle();
    let trace = driver.inspector_trace();
    assert!(has(&trace, TraceKind::Resize, "resize 90x12"));
    assert!(has(&trace, TraceKind::Paste, "paste 3 characters (unused)"));
    // A task: its key, its result back, and what that wrote.
    driver.press("t");
    driver.update(Duration::from_millis(10));
    driver.settle();
    let trace = driver.inspector_trace();
    assert!(
        has(&trace, TraceKind::Task, "task from inspector.rs:"),
        "{trace:#?}"
    );
    let done = trace
        .iter()
        .position(|e| e.kind == TraceKind::Task)
        .unwrap();
    assert!(trace[done + 1..]
        .iter()
        .any(|e| e.kind == TraceKind::Signal && e.text.contains("→ status")));
    // A timer.
    driver.update(Duration::from_millis(1000));
    driver.settle();
    let trace = driver.inspector_trace();
    assert!(
        has(&trace, TraceKind::Timer, "timer every 1s (inspector.rs:"),
        "{trace:#?}"
    );
    // A screen opening and closing, and the focus moving with it.
    driver.press("o");
    driver.press("esc");
    let trace = driver.inspector_trace();
    assert!(has(&trace, TraceKind::Screen, "screen opened"));
    assert!(has(&trace, TraceKind::Focus, "focus → nothing"));
    assert!(
        has(&trace, TraceKind::Key, "key escape → second on_key"),
        "{trace:#?}"
    );
    assert!(has(&trace, TraceKind::Screen, "screen closed"));
    assert!(has(&trace, TraceKind::Focus, "focus → button"));
    // In order: each event before what it caused.
    let at = |text: &str| trace.iter().position(|e| e.text.contains(text)).unwrap();
    assert!(has(&trace, TraceKind::Key, "key o → root on_key"));
    assert!(at("key o") < at("screen opened"));
    assert!(at("screen opened") < at("focus → nothing"));
    // The trace pane shows the newest, with their frames.
    driver.press("shift+f12 4");
    let text = panel_of(&driver, 50);
    assert!(text.contains("events · newest last"), "{text}");
    assert!(text.contains("focus → button"), "{text}");
}

/// The panel's text on a screen whose app takes `columns`.
fn panel_of(driver: &Driver, columns: usize) -> String {
    driver
        .screen()
        .plain()
        .iter()
        .map(|row| row.chars().skip(columns).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn the_trace_keeps_its_newest_events_up_to_its_limit() {
    let mut driver = inspected();
    let a = vec!["a"; 600].join(" ");
    let b = vec!["b"; 600].join(" ");
    driver.press(&a);
    driver.press(&b);
    // A frame, to take the last ones in.
    driver.press("+");
    let trace = driver.inspector_trace();
    assert_eq!(trace.len(), TRACE_LIMIT);
    let keys = |name: &str| trace.iter().filter(|e| e.text == name).count();
    // 1,202 events (the `+` wrote a signal): the oldest 202 went.
    assert_eq!(keys("key b (unused)"), 600);
    assert_eq!(keys("key a (unused)"), 398);
    assert_eq!(trace[0].text, "key a (unused)");
    assert_eq!(trace.last().unwrap().kind, TraceKind::Signal);
}

#[test]
fn the_trace_pauses_and_clears() {
    let mut driver = inspected();
    driver.press("+");
    // The focus going to the button at the start, the key and the write it
    // made.
    let before = driver.inspector_trace();
    assert_eq!(before.len(), 3, "{before:#?}");
    assert_eq!(before[0].text, "focus → button");
    // Paused: what happens is not kept.
    driver.press("shift+f12 4 space esc + ctrl+up");
    assert_eq!(driver.inspector_trace(), before);
    assert_eq!(app_rows(&driver)[0], "count 3", "the app ran meanwhile");
    assert!(panel(&driver).contains("paused"), "{}", panel(&driver));
    // Resumed and cleared: only what came after.
    driver.press("shift+f12 space c esc +");
    let trace = driver.inspector_trace();
    assert_eq!(trace.len(), 2, "{trace:#?}");
    assert_eq!(trace[0].text, "key + → root on_key");
    assert_eq!(trace[1].kind, TraceKind::Signal);
}

#[test]
fn a_terminal_too_narrow_for_the_panel_gives_the_app_the_keys_and_waits() {
    let mut driver = inspected();
    // The trace pane, with the inspector's keys; then too narrow for it.
    driver.press("shift+f12 4");
    driver.resize(30, 10);
    driver.settle();
    // The app has the whole width and the keys, and no frame waits.
    driver.press("+");
    assert_eq!(driver.screen().plain()[0].trim_end(), "count 1");
    driver.update(Duration::ZERO);
    let _ = driver.render();
    assert!(!driver.needs_render(), "nothing waits to be drawn");
    // What happened meanwhile is in the trace.
    let trace = driver.inspector_trace();
    assert!(
        has(&trace, TraceKind::Key, "key + → root on_key"),
        "{trace:#?}"
    );
    // Wide enough again: the panel is back, on its trace.
    driver.resize(WIDTH, HEIGHT);
    driver.settle();
    assert!(panel(&driver).contains("events"), "{}", panel(&driver));
}

#[test]
fn the_redraw_flash_marks_what_drew_for_a_moment() {
    let mut driver = inspected();
    driver.press("shift+f12 f esc");
    assert!(panel(&driver).contains("flash"), "{}", panel(&driver));
    let lit = |driver: &Driver, x: u16, y: u16| {
        let screen = driver.screen();
        screen
            .style(screen.cell(x, y).style)
            .is_some_and(|style| style.bgcolor().is_some())
    };
    // `+` draws the count again: its row is marked, nothing else is.
    driver.press("+");
    assert!(lit(&driver, 0, 0) && lit(&driver, 59, 0));
    assert!(!lit(&driver, 0, 1));
    assert_eq!(app_rows(&driver)[0], "count 1", "the text is the same");
    // The loop wakes up when the flash is due to go.
    assert!(driver.timeout(Duration::ZERO) <= Duration::from_millis(300));
    driver.update(Duration::from_millis(400));
    driver.settle();
    assert!(!lit(&driver, 0, 0), "the flash went");
    // Off again: nothing is marked.
    driver.press("shift+f12 f esc +");
    assert!(!lit(&driver, 0, 0));
}

/// Each step's bytes.
fn steps(driver: &mut Driver, events: &[Event]) -> Vec<Option<String>> {
    let mut out = Vec::new();
    for (i, event) in events.iter().enumerate() {
        driver.event(event.clone());
        driver.update(Duration::from_millis(100 * (i as u64 + 1)));
        out.push(driver.render());
    }
    out
}

#[test]
fn the_app_draws_the_same_while_the_inspector_is_closed() {
    let events = [
        Event::Key(Key::parse("+").unwrap()),
        Event::Key(Key::parse("ctrl+up").unwrap()),
        Event::Mouse(Mouse::new(MouseKind::Down(Button::Left), 2, 1)),
        Event::Mouse(Mouse::new(MouseKind::Up(Button::Left), 2, 1)),
        Event::Mouse(Mouse::new(MouseKind::Moved, 5, 5)),
        Event::Resize {
            columns: 90,
            rows: 18,
        },
        Event::Key(Key::parse("+").unwrap()),
    ];
    let mut plain = App::new(build).driver(WIDTH, HEIGHT);
    plain.settle();
    // Opened, then closed with F12 (which draws everything once).
    let mut closed = App::new(build).inspector(true).driver(WIDTH, HEIGHT);
    closed.settle();
    closed.press("+ shift+f12 down f esc f12");
    plain.press("+");
    assert_eq!(closed.screen().lines(), plain.screen().lines());
    let a = steps(&mut plain, &events);
    let b = steps(&mut closed, &events);
    assert_eq!(a, b, "the same bytes, frame for frame");
    assert_eq!(closed.screen().lines(), plain.screen().lines());
    // Nothing was traced or measured meanwhile.
    let traced = closed.inspector_trace().len();
    let measured = closed.inspector_timings();
    closed.press("+ +");
    assert_eq!(closed.inspector_trace().len(), traced);
    assert_eq!(closed.inspector_timings(), measured);
}

#[test]
fn linear_mode_is_the_same_with_the_inspector() {
    let build = || {
        let pressed = signal(0);
        column([
            text!("pressed {pressed}").name("status").live(),
            label("Save").name("save").focusable(),
        ])
        .on_key("f12", move |_| pressed.update(|p| *p += 1))
        .on_key("tab", |cx| cx.focus_next())
    };
    let run = |app: App| {
        let mut driver = app.linear(true).driver(40, 5);
        let mut out = Vec::new();
        driver.update(Duration::ZERO);
        out.push(driver.render());
        for key in ["f12", "tab", "shift+f12", "f12"] {
            driver.event(Event::Key(Key::parse(key).unwrap()));
            driver.update(Duration::ZERO);
            out.push(driver.render());
        }
        assert!(driver.inspector_trace().is_empty());
        out
    };
    let plain = run(App::new(build));
    let inspected = run(App::new(build).inspector(true));
    assert_eq!(plain, inspected);
    // F12 reached the app's binding.
    assert!(
        plain.iter().flatten().any(|out| out.contains("pressed 2")),
        "{plain:?}"
    );
}
