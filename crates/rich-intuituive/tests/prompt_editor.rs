//! A prompt editor on `text_area` (#800): completion after a trigger,
//! history, a box that grows with its rows, and the keys that submit and
//! add a line.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use intuituive::complete::{Completion, history_session};
use intuituive::prelude::*;

fn plain(driver: &intuituive::Driver) -> Vec<String> {
    driver
        .screen()
        .plain()
        .iter()
        .map(|l| l.trim_end().to_string())
        .collect()
}

/// Commands for `/`, from a fixed list.
fn commands(query: &str) -> Vec<Completion> {
    ["help", "history", "model", "quit"]
        .into_iter()
        .filter(|c| c.starts_with(query))
        .map(|c| Completion::new(0..query.len(), c))
        .collect()
}

/// A prompt over a box that grows from one row to `max`, with the text
/// sent and the prompt's signal.
struct Prompt {
    sent: Rc<RefCell<Vec<String>>>,
    prompt: Rc<Cell<Option<Signal<String>>>>,
}

fn prompt_app(max: u16, extra: impl Fn(Node) -> Node + 'static) -> (App, Prompt) {
    let sent = Rc::new(RefCell::new(Vec::new()));
    let out = Rc::new(Cell::new(None));
    let (log, seen) = (sent.clone(), out.clone());
    let app = App::new(move || {
        let prompt = signal(String::new());
        seen.set(Some(prompt));
        let log = log.clone();
        let area = text_area(prompt)
            .grow(1, max)
            .submit_on("enter")
            .placeholder("Ask anything")
            .complete_at('/', commands)
            .on_submit(move |text, _| {
                log.borrow_mut().push(text.to_string());
                prompt.set(String::new());
            });
        column([label("above").auto(), extra(area), label("below").auto()])
    });
    (app, Prompt { sent, prompt: out })
}

#[test]
fn a_trigger_opens_the_popup_and_enter_takes_a_candidate() {
    let (app, _) = prompt_app(5, |area| area);
    let mut driver = app.driver(30, 10);
    driver.settle();
    assert_eq!(plain(&driver)[1], "Ask anything");
    driver.type_text("try /h");
    let rows = plain(&driver);
    assert!(rows[2].ends_with(" help"), "{rows:#?}");
    assert!(rows[3].ends_with(" history"), "{rows:#?}");
    driver.press("down enter");
    // Enter took the candidate, with a space after it: it did not submit.
    driver.type_text("x");
    assert_eq!(plain(&driver)[1], "try /history x");
    // Not at a word's start: nothing opens.
    driver.type_text(" a/h");
    assert_eq!(plain(&driver)[2], "below");
}

#[test]
fn esc_closes_the_popup_until_the_next_trigger() {
    let (app, _) = prompt_app(5, |area| area);
    let mut driver = app.driver(30, 10);
    driver.type_text("/");
    assert!(plain(&driver)[2].ends_with(" help"));
    driver.press("esc");
    assert_eq!(plain(&driver)[2], "below");
    driver.type_text("m");
    assert_eq!(plain(&driver)[2], "below");
    // A new trigger word opens it again.
    driver.type_text(" /m");
    assert!(
        plain(&driver)[2].ends_with(" model"),
        "{:#?}",
        plain(&driver)
    );
    // A space ends the word: it closes.
    driver.type_text("o ");
    assert_eq!(plain(&driver)[2], "below");
}

#[test]
fn enter_submits_and_shift_or_alt_enter_adds_a_line() {
    let (app, prompt) = prompt_app(5, |area| area);
    let mut driver = app.driver(30, 10);
    driver.type_text("one");
    driver.press("shift+enter");
    driver.type_text("two");
    driver.press("alt+enter");
    driver.type_text("three");
    assert_eq!(plain(&driver)[1..5], ["one", "two", "three", "below"]);
    driver.press("enter");
    assert_eq!(*prompt.sent.borrow(), ["one\ntwo\nthree"]);
    assert_eq!(prompt.prompt.get().unwrap().get_untracked(), "");
    assert_eq!(plain(&driver)[1..3], ["Ask anything", "below"]);
}

#[test]
fn the_box_grows_to_its_max_then_scrolls() {
    let (app, _) = prompt_app(3, |area| area);
    let mut driver = app.driver(30, 10);
    driver.settle();
    assert_eq!(plain(&driver)[2], "below");
    driver.type_text("1");
    for n in 2..=5 {
        driver.press("shift+enter");
        driver.type_text(&n.to_string());
        let rows = n.min(3);
        // The row below moves down with it, to three rows at most.
        assert_eq!(
            plain(&driver)[1 + rows],
            "below",
            "{n}: {:#?}",
            plain(&driver)
        );
    }
    // Five lines in three rows: the caret's line is in view.
    let rows = plain(&driver);
    assert!(rows[3].starts_with('5'), "{rows:#?}");
    // Deleting lines shrinks it again.
    driver.press("ctrl+a backspace");
    assert_eq!(plain(&driver)[2], "below");
}

#[test]
fn up_on_the_first_line_recalls_and_down_restores_the_draft() {
    let (app, prompt) = prompt_app(5, |area| area.history("prompt"));
    let mut driver = app.driver(30, 10);
    driver.type_text("first");
    driver.press("enter");
    driver.type_text("second");
    driver.press("shift+enter");
    driver.type_text("line");
    driver.press("enter");
    assert_eq!(prompt.sent.borrow().len(), 2);
    driver.type_text("draft");
    driver.press("up");
    // The newest entry, both of its lines.
    assert_eq!(plain(&driver)[1..3], ["second", "line"]);
    driver.press("up");
    assert_eq!(plain(&driver)[1], "first");
    driver.press("down down");
    assert_eq!(plain(&driver)[1..3], ["draft", "below"]);
    // Up off the first line moves the caret, not through the history.
    driver.press("shift+enter");
    driver.type_text("more");
    driver.press("up");
    assert_eq!(plain(&driver)[1..3], ["draft", "more"]);
    driver.press("up");
    assert_eq!(plain(&driver)[1..3], ["second", "line"]);
}

#[test]
fn a_session_history_keeps_entries_of_more_than_one_line() {
    let app = App::new(|| {
        let history = history_session();
        history.push("a\nb");
        history.push("c");
        let entries = history.entries().len();
        label(format!("{entries}"))
    });
    let mut driver = app.driver(5, 1);
    driver.settle();
    assert_eq!(plain(&driver)[0], "2");
}

#[test]
fn an_input_still_completes_as_it_did() {
    let app = App::new(|| {
        let typed = signal(String::new());
        input(typed).complete_at('/', commands)
    });
    let mut driver = app.driver(20, 4);
    driver.type_text("/q");
    driver.press("enter");
    assert_eq!(plain(&driver)[0], "/quit");
}

#[cfg(feature = "files")]
#[test]
fn at_mentions_complete_paths() {
    use intuituive::files::path_completer;
    use rich_ext::listing::walk;

    let dir = std::env::temp_dir().join(format!("intuituive-mentions-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("src/main.rs"), "").unwrap();
    std::fs::write(dir.join("src/map.rs"), "").unwrap();
    std::fs::write(dir.join("README.md"), "").unwrap();
    let root = dir.clone();
    let app = App::new(move || {
        let prompt = signal(String::new());
        text_area(prompt)
            .grow(1, 4)
            .complete_at('@', path_completer(walk(&root)))
    });
    let mut driver = app.driver(30, 8);
    driver.type_text("@src/ma");
    let rows = plain(&driver);
    assert_eq!(rows[1], " src/map.rs", "{rows:#?}");
    assert_eq!(rows[2], " src/main.rs", "{rows:#?}");
    driver.press("down enter");
    driver.type_text("x");
    assert_eq!(plain(&driver)[0], "@src/main.rs x");
    // Fuzzy: the letters in order.
    driver.press("ctrl+a backspace");
    driver.type_text("@rme");
    assert_eq!(plain(&driver)[1], " README.md");
    std::fs::remove_dir_all(&dir).ok();
}
