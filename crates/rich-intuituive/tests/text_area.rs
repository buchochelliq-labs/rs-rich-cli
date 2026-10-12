//! The text area: editing through the shared buffer, the signal both ways,
//! the clipboard, the mouse, and the keys it takes from the app.

mod common;

#[path = "../examples/editor.rs"]
#[allow(dead_code)]
mod editor_example;

use std::time::Duration;

use common::{run, screen};
use intuituive::interact::{Button, Event, Key, Mouse, MouseKind};
use intuituive::prelude::*;
use rich_interact::headless::{Headless, Script};

fn press(key: &str) -> Event {
    Event::Key(Key::parse(key).expect("a key name"))
}

fn typed(text: &str) -> Vec<Event> {
    text.chars().map(|c| Event::Key(Key::char(c))).collect()
}

/// A focused text area of `start` over a line showing the signal's value
/// (its line breaks as `|`).
fn editor(start: &'static str, area: fn(Node) -> Node) -> App {
    App::new(move || {
        let value = signal(start.to_string());
        column([
            area(text_area(value).autofocus()),
            text(move || format!("={}", value.get().replace('\n', "|"))).fixed(1),
        ])
        .on_key("ctrl+q", |cx| cx.quit())
    })
}

fn driven(app: App, width: u16, height: u16, events: Vec<Event>) -> intuituive::Driver {
    let mut driver = app.driver(width, height);
    driver.update(Duration::ZERO);
    let _ = driver.render();
    for event in events {
        driver.event(event);
        driver.update(Duration::ZERO);
        let _ = driver.render();
    }
    driver
}

fn rows(driver: &intuituive::Driver) -> Vec<String> {
    driver
        .screen()
        .plain()
        .iter()
        .map(|row| row.trim_end().to_string())
        .collect()
}

fn value(driver: &intuituive::Driver) -> String {
    rows(driver)
        .into_iter()
        .rev()
        .find_map(|row| row.strip_prefix('=').map(str::to_string))
        .expect("the value line")
}

#[test]
fn typing_sets_the_signal_and_enter_breaks_the_line() {
    let mut events = typed("hello");
    events.push(press("enter"));
    events.extend(typed("world"));
    let driver = driven(editor("", |n| n), 20, 4, events);
    assert_eq!(value(&driver), "hello|world");
    let shown = rows(&driver);
    assert_eq!(shown[0], "hello");
    assert_eq!(shown[1], "world");
}

#[test]
fn text_set_on_the_signal_replaces_what_it_holds() {
    let app = App::new(|| {
        let value = signal("old".to_string());
        column([text_area(value).autofocus(), label("x").fixed(1)])
            .on_key("f1", move |_| value.set("new\ntext".to_string()))
    });
    let driver = driven(app, 20, 4, vec![press("f1")]);
    let shown = rows(&driver);
    assert_eq!(shown[0], "new");
    assert_eq!(shown[1], "text");
}

#[test]
fn ctrl_z_undoes_a_typed_run_at_once_and_ctrl_y_redoes_it() {
    let mut events = typed("one ");
    events.extend(typed("two"));
    events.push(press("ctrl+z"));
    let driver = driven(editor("", |n| n), 20, 3, events.clone());
    assert_eq!(value(&driver), "");
    events.push(press("ctrl+y"));
    let driver = driven(editor("", |n| n), 20, 3, events);
    assert_eq!(value(&driver), "one two");
}

#[test]
fn ctrl_z_reaches_the_area_rather_than_suspending_the_app() {
    let app = editor("", |n| n);
    let script = Script::new().text("ab").keys("ctrl+z ctrl+q");
    let mut backend = Headless::new(script.clone(), 20, 3);
    backend.suspendable = true;
    let record = backend.record();
    app.run_on(&mut backend).expect("the app runs to its end");
    assert_eq!(record.borrow().suspends, 0);

    // Read-only, it has nothing to undo: the app suspends as anywhere.
    let app = editor("ab", |n| n.read_only());
    let mut backend = Headless::new(Script::new().keys("ctrl+z ctrl+q"), 20, 3);
    backend.suspendable = true;
    let record = backend.record();
    app.run_on(&mut backend).expect("the app runs to its end");
    assert_eq!(record.borrow().suspends, 1);
}

#[test]
fn shift_selects_ctrl_c_copies_and_ctrl_x_cuts() {
    let events = vec![
        press("home"),
        press("shift+right"),
        press("shift+right"),
        press("ctrl+c"),
    ];
    let mut driver = driven(editor("abcd", |n| n), 20, 3, events);
    assert!(!driver.is_done(), "with a selection, Ctrl+C copies");
    assert_eq!(driver.take_copies(), ["ab"]);

    driver.event(press("ctrl+x"));
    driver.update(Duration::ZERO);
    let _ = driver.render();
    assert_eq!(driver.take_copies(), ["ab"]);
    assert_eq!(value(&driver), "cd");

    // With nothing selected, Ctrl+C quits as it does anywhere.
    driver.event(press("ctrl+c"));
    assert!(driver.is_done());
}

#[test]
fn pasted_text_is_cleaned_and_inserted_as_one_step() {
    let events = vec![Event::Paste("a\r\nb\x07c".to_string()), press("ctrl+z")];
    let driver = driven(editor("", |n| n), 20, 3, events[..1].to_vec());
    assert_eq!(value(&driver), "a|bc");
    let driver = driven(editor("", |n| n), 20, 3, events);
    assert_eq!(value(&driver), "");
}

#[test]
fn tab_indents_and_shift_tab_outdents_the_selected_lines() {
    let events = vec![press("ctrl+a"), press("tab")];
    let driver = driven(editor("a\nb", |n| n), 20, 4, events.clone());
    assert_eq!(value(&driver), "    a|    b");
    let mut events = events;
    events.push(press("shift+tab"));
    let driver = driven(editor("a\nb", |n| n), 20, 4, events);
    assert_eq!(value(&driver), "a|b");
}

#[test]
fn a_click_places_the_caret_and_a_double_click_selects_a_word() {
    let click = |column, row| {
        [
            Event::Mouse(Mouse::new(MouseKind::Down(Button::Left), column, row)),
            Event::Mouse(Mouse::new(MouseKind::Up(Button::Left), column, row)),
        ]
    };
    let mut events = click(3, 0).to_vec();
    events.extend(typed("X"));
    let driver = driven(editor("one two", |n| n), 20, 3, events);
    assert_eq!(value(&driver), "oneX two");

    let mut events = click(5, 0).to_vec();
    events.extend(click(5, 0));
    events.push(press("ctrl+c"));
    let mut driver = driven(editor("one two", |n| n), 20, 3, events);
    assert_eq!(driver.take_copies(), ["two"]);
}

#[test]
fn a_drag_selects() {
    let events = vec![
        Event::Mouse(Mouse::new(MouseKind::Down(Button::Left), 1, 0)),
        Event::Mouse(Mouse::new(MouseKind::Drag(Button::Left), 2, 0)),
        Event::Mouse(Mouse::new(MouseKind::Drag(Button::Left), 4, 0)),
        Event::Mouse(Mouse::new(MouseKind::Up(Button::Left), 4, 0)),
        press("backspace"),
    ];
    let driver = driven(editor("abcdef", |n| n), 20, 3, events);
    assert_eq!(value(&driver), "aef");
}

#[test]
fn read_only_moves_and_copies_but_does_not_edit() {
    let mut events = typed("zz");
    events.extend([press("ctrl+a"), press("backspace"), press("ctrl+c")]);
    let mut driver = driven(editor("keep", |n| n.read_only()), 20, 3, events);
    assert_eq!(value(&driver), "keep");
    assert_eq!(driver.take_copies(), ["keep"]);
}

#[test]
fn the_placeholder_shows_while_it_is_empty() {
    let driver = driven(editor("", |n| n.placeholder("Write here")), 20, 3, vec![]);
    assert_eq!(rows(&driver)[0], "Write here");
    let driver = driven(
        editor("", |n| n.placeholder("Write here")),
        20,
        3,
        typed("a"),
    );
    assert_eq!(rows(&driver)[0], "a");
}

#[test]
fn line_numbers_take_a_gutter() {
    let driver = driven(editor("a\nb", |n| n.line_numbers()), 20, 4, vec![]);
    let shown = rows(&driver);
    assert!(
        shown[0].starts_with("1 ") && shown[0].ends_with('a'),
        "{shown:?}"
    );
    assert!(
        shown[1].starts_with("2 ") && shown[1].ends_with('b'),
        "{shown:?}"
    );
}

#[test]
fn long_lines_wrap_or_scroll_sideways() {
    let driver = driven(editor("abcdefghijkl", |n| n), 8, 4, vec![]);
    let shown = rows(&driver);
    // Seven a row: one cell is kept for the caret.
    assert_eq!(shown[0], "abcdefg");
    assert_eq!(shown[1], "hijkl");

    let driver = driven(
        editor("abcdefghijkl", |n| n.soft_wrap(false)),
        8,
        4,
        vec![press("end")],
    );
    let shown = rows(&driver);
    assert!(shown[0].ends_with("ijkl"), "{shown:?}");
    assert!(shown[1].is_empty(), "nothing wraps: {shown:?}");
}

#[test]
fn it_runs_under_the_headless_backend() {
    let app = editor("", |n| n);
    let shown = screen(&run(app, Script::new().text("hi").keys("ctrl+q"), 20, 3));
    assert_eq!(shown[0], "hi");
}

#[test]
fn the_accessibility_tree_names_it_a_text_box() {
    let app = editor("hello", |n| n.name("notes"));
    let driver = driven(app, 20, 3, vec![]);
    let tree = format!("{:?}", driver.accessibility());
    assert!(tree.contains("TextBox"), "{tree}");
}

#[test]
fn a_language_highlights_through_the_consoles_highlighter() {
    let driver = driven(
        editor("fn main() {}", |n| n.language("rust")),
        20,
        3,
        vec![],
    );
    let screen = driver.screen();
    assert_eq!(rows(&driver)[0], "fn main() {}");
    let style = |x| screen.style(screen.cell(x, 0).style).cloned();
    // `fn` and `main` are coloured differently.
    assert!(style(0).is_some(), "fn is styled");
    assert_ne!(style(0), style(3));
}

#[test]
fn escape_then_tab_moves_the_focus_on() {
    let app = App::new(|| {
        let value = signal(String::new());
        let left = signal(false);
        column([
            text_area(value).autofocus(),
            label("next")
                .focusable()
                .on_key("x", move |_| left.set(true)),
            text(move || format!("={}|{}", value.get(), left.get())).fixed(1),
        ])
    });
    let events = vec![press("tab"), press("escape"), press("tab"), press("x")];
    let driver = driven(app, 20, 4, events);
    // The first Tab indented; after Escape, Tab left, and x reached `next`.
    assert_eq!(value(&driver), "    |true");
}

#[test]
fn linear_mode_reads_the_caret_line() {
    let app = App::new(|| {
        let value = signal("first line\nsecond line".to_string());
        text_area(value).label("Notes").autofocus()
    })
    .linear(true);
    let mut driver = app.driver(40, 6);
    driver.update(Duration::ZERO);
    let start = driver.render().unwrap_or_default();
    driver.update(Duration::ZERO);
    let start = start + &driver.render().unwrap_or_default();
    assert!(start.contains("first line"), "{start:?}");
    driver.event(press("down"));
    driver.update(Duration::ZERO);
    let moved = driver.render().unwrap_or_default();
    assert!(moved.contains("second line"), "{moved:?}");
    assert!(!moved.contains("first line"), "{moved:?}");
}

/// The plan's benchmark: a 100,000-line file pages through without a stall,
/// because only the lines in view are wrapped, highlighted and drawn.
#[test]
fn a_hundred_thousand_lines_page_without_a_stall() {
    let text: String = (0..100_000)
        .map(|i| format!("let line_{i} = {i}; // a comment on line {i}\n"))
        .collect();
    let app = App::new(move || {
        let value = signal(text.clone());
        text_area(value).language("rust").line_numbers().autofocus()
    });
    let mut driver = app.driver(80, 24);
    driver.update(Duration::ZERO);
    let _ = driver.render();
    let started = std::time::Instant::now();
    for key in ["ctrl+end", "pageup", "pageup", "ctrl+home", "pagedown"] {
        driver.event(press(key));
        driver.update(Duration::ZERO);
        let _ = driver.render();
    }
    let each = started.elapsed() / 5;
    let shown = rows(&driver);
    // A page down from the top: the caret on line 24, the last row.
    assert!(shown[23].contains("line_24 "), "{shown:?}");
    // Generous for a debug build on a busy runner; a frame in release
    // takes well under a millisecond.
    assert!(each < Duration::from_millis(250), "{each:?} a frame");
}

#[test]
fn the_editor_example_edits_and_counts() {
    let mut events = vec![press("ctrl+end")];
    events.extend(typed("// done"));
    events.extend([press("escape"), press("tab")]);
    events.extend(typed("ship it"));
    let driver = driven(editor_example::editor_app(), 72, 30, events);
    let shown = rows(&driver).join("\n");
    assert!(shown.contains("// done"), "{shown}");
    assert!(shown.contains("ship it"), "{shown}");
    assert!(
        shown.contains("count.rs 19 lines · notes 2 words"),
        "{shown}"
    );
}
