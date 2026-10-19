//! The command line, input completion and history (round 6): the
//! taskwarrior-tui and slumber clones' command bars and query boxes.

use intuituive::command_line::{command_line, command_line_with, CommandLine, Mode};
use intuituive::prelude::*;
use intuituive::{complete_words, history_file, history_session, CommandLineOptions, Completion};

fn rows(driver: &intuituive::Driver) -> Vec<String> {
    driver
        .screen()
        .plain()
        .iter()
        .map(|r| r.trim_end().to_string())
        .collect()
}

#[test]
fn a_filter_follows_each_key_and_esc_puts_it_back() {
    let app = App::new(|| {
        let cmd = CommandLine::new();
        let filter = signal(String::from("old"));
        column([
            text!("filter {filter}").fixed(1),
            list(|| vec!["a".to_string(), "b".to_string()], signal(0usize))
                .name("items")
                .flex(1),
            command_line_with(cmd, CommandLineOptions::new().two_rows()),
        ])
        .on_key("/", move |cx| {
            let was = filter.get_untracked();
            cmd.open(
                cx,
                Mode::new("")
                    .title("Filter Tasks")
                    .initial(was.clone())
                    .on_change(move |text, _| filter.set(text.to_string()))
                    .on_cancel(move |_| filter.set(was.clone())),
            )
        })
    });
    let mut driver = app.driver(30, 5);
    driver.press("tab");
    assert_eq!(driver.focused_name().as_deref(), Some("items"));
    driver.press("/");
    assert_eq!(rows(&driver)[3], "Filter Tasks");
    driver.press("backspace backspace backspace");
    driver.type_text("new");
    assert_eq!(rows(&driver)[0], "filter new");
    assert_eq!(rows(&driver)[4], "new");
    driver.press("esc");
    assert_eq!(rows(&driver)[0], "filter old");
    assert_eq!(rows(&driver)[3], "");
    // The focus is back where it was.
    assert_eq!(driver.focused_name().as_deref(), Some("items"));
}

#[test]
fn completion_and_history_in_the_line() {
    let app = App::new(|| {
        let cmd = CommandLine::new();
        let added = signal(String::new());
        let history = history_session();
        column([text!("added {added}").flex(1), command_line(cmd)]).on_key("a", move |cx| {
            cmd.open(
                cx,
                Mode::new("add ")
                    .complete(complete_words([
                        "project:home",
                        "project:work",
                        "priority:H",
                    ]))
                    .history(history)
                    .on_submit(move |text, _| {
                        added.set(text.to_string());
                        Ok(())
                    }),
            )
        })
    });
    let mut driver = app.driver(40, 6);
    driver.press("a");
    driver.type_text("pro");
    // The ghost, then the pop-up above the bar (no room below).
    assert_eq!(rows(&driver)[5], "add project:home");
    driver.press("tab");
    assert_eq!(rows(&driver)[3], "    project:home");
    assert_eq!(rows(&driver)[4], "    project:work");
    driver.press("tab enter");
    assert_eq!(rows(&driver)[5], "add project:work");
    // The pop-up has gone, and what was under it is back.
    assert_eq!(rows(&driver)[3], "");
    driver.press("enter");
    assert_eq!(rows(&driver)[0], "added project:work");
    // The next time, ↑ brings it back.
    driver.press("a up");
    assert_eq!(rows(&driver)[5], "add project:work");
}

#[test]
fn confirms_in_place_and_errors_in_red() {
    let app = App::new(|| {
        let cmd = CommandLine::new();
        let done = signal(0);
        column([text!("done {done}").flex(1), command_line(cmd)])
            .on_key("x", move |cx| {
                cmd.confirm_keys(cx, "Delete Task 1? Press <x> to confirm", "x", move |_| {
                    done.update(|d| *d += 1)
                })
            })
            .on_key("e", move |_| cmd.error("Cannot locate task id 42"))
    });
    let mut driver = app.driver(40, 3);
    driver.press("x");
    assert_eq!(rows(&driver)[2], "Delete Task 1? Press <x> to confirm");
    // Other keys wait.
    driver.press("y j");
    assert_eq!(rows(&driver)[0], "done 0");
    driver.press("x");
    assert_eq!(rows(&driver)[0], "done 1");
    assert_eq!(rows(&driver)[2], "");
    driver.press("x q");
    assert_eq!(rows(&driver)[0], "done 1");
    driver.press("e");
    assert_eq!(rows(&driver)[2], "Cannot locate task id 42");
    let styled = intuituive::testing::styled(&driver);
    assert!(styled.lines().nth(2).unwrap().contains("red"), "{styled}");
}

#[test]
fn completions_replace_their_range_and_show_details() {
    let app = App::new(|| {
        let text = signal(String::from("due:to"));
        column([
            input(text).complete(|text: &str, caret: usize| {
                let start = text.find(':').map_or(0, |c| c + 1);
                ["today", "tomorrow"]
                    .iter()
                    .filter(|w| w.starts_with(&text[start..caret]))
                    .map(|w| Completion::new(start..caret, *w).detail("date"))
                    .collect()
            }),
            label("below"),
        ])
    });
    let mut driver = app.driver(30, 4);
    // → or End would take the ghost (`day`); Tab opens the list.
    driver.press("tab");
    assert_eq!(rows(&driver)[1], "    today     date");
    assert_eq!(rows(&driver)[2], "bel tomorrow  date");
    driver.press("esc");
    assert!(rows(&driver).iter().all(|r| !r.contains("date")));
    assert!(rows(&driver).iter().any(|r| r == "below"));
    driver.press("tab up enter");
    assert_eq!(rows(&driver)[0], "due:tomorrow");
}

#[test]
fn a_history_file_lasts_between_sessions() {
    let file = std::env::temp_dir().join(format!("intuituive-cmd-history-{}", std::process::id()));
    let _ = std::fs::remove_file(&file);
    let path = file.clone();
    let app = App::new(move || {
        let typed = signal(String::new());
        input(typed)
            .history(history_file(path.clone()))
            .on_submit(move |_, _| typed.set(String::new()))
    });
    let mut driver = app.driver(20, 1);
    driver.type_text("first");
    driver.press("enter");
    driver.type_text("second");
    driver.press("enter");
    let path = file.clone();
    let again = App::new(move || {
        let history = history_file(path.clone());
        let typed = signal(String::new());
        column([
            input(typed).history(history),
            text(move || format!("{:?}", history.status())),
        ])
    });
    let mut driver = again.driver(20, 2);
    driver.press("up up");
    assert_eq!(rows(&driver)[0], "first");
    assert_eq!(rows(&driver)[1], "Some((2, 2))");
    driver.press("left");
    assert_eq!(rows(&driver)[1], "None");
    let _ = std::fs::remove_file(&file);
}
