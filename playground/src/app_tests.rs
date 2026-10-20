//! Tier-3 sessions, driven natively as the page drives them in the browser.

use crate::app::Session;

const DOWN: &str = "\x1b[B";
const ENTER: &str = "\r";

fn start(app: &str, input: &str) -> (Session, String) {
    let mut session = Session::new(app, input, 40, 12).unwrap();
    let first = session.output(0.0);
    (session, first)
}

#[test]
fn choose_picks_a_line() {
    let (mut session, first) = start("choose", "red\ngreen\nblue\n");
    assert!(first.contains("red") && first.contains("blue"), "{first:?}");
    assert!(first.contains("\x1b[?1000h"), "mouse reporting on");
    session.input(DOWN, 10.0);
    assert!(!session.done());
    let last = session.input(ENTER, 20.0);
    assert!(session.done());
    assert_eq!(session.result(), "green");
    assert!(last.contains("\x1b[?1000l"), "mouse reporting off");
    assert_eq!(session.input(DOWN, 30.0), "", "nothing after the end");
}

#[test]
fn filter_narrows_as_typed() {
    let (mut session, _) = start("filter", "red\ngreen\nblue\n");
    let typed = session.input("bl", 10.0);
    assert!(typed.contains('b'));
    session.input(ENTER, 20.0);
    assert_eq!(session.result(), "blue");
}

#[test]
fn choose_multi_marks_lines() {
    let (mut session, _) = start("choose-multi", "red\ngreen\nblue\n");
    // Tab marks the line and moves down.
    session.input("\t", 10.0);
    session.input(DOWN, 20.0);
    session.input("\t", 30.0);
    session.input(ENTER, 40.0);
    assert_eq!(session.result(), "red\nblue");
}

#[test]
fn escape_cancels_and_ctrl_c_interrupts() {
    let (mut session, _) = start("choose", "a\nb\n");
    session.input("\x1b", 10.0);
    assert_eq!(session.result(), "cancelled");
    let (mut session, _) = start("choose", "a\nb\n");
    session.input("\x03", 10.0);
    assert_eq!(session.result(), "interrupted");
}

#[test]
fn a_click_chooses() {
    let (mut session, first) = start("choose", "red\ngreen\nblue\n");
    // The row "green" is drawn on: find it in the first frame's lines.
    let row = first
        .split("\r\n")
        .position(|line| line.contains("green"))
        .expect("green drawn") as u16
        + 1;
    session.input(&format!("\x1b[<0;6;{row}M\x1b[<0;6;{row}m"), 10.0);
    if !session.done() {
        session.input(ENTER, 20.0);
    }
    assert_eq!(session.result(), "green");
}

#[test]
fn the_pager_scrolls_and_quits() {
    let text: String = (1..=100).map(|n| format!("line {n}\n")).collect();
    let (mut session, first) = start("pager", &text);
    assert!(first.contains("line 1"));
    // Frames after the first are what changed: the end's last number.
    let scrolled = session.input("G", 10.0);
    assert!(
        scrolled.contains("of 100") || scrolled.contains("01 of"),
        "{scrolled:?}"
    );
    session.input("q", 20.0);
    assert!(session.done());
}

#[test]
fn explore_reads_json_and_yaml() {
    let (mut session, first) = start(
        "explore",
        r#"{"server": {"host": "localhost"}, "debug": true}"#,
    );
    assert!(first.contains("server"), "{first:?}");
    session.input(DOWN, 10.0);
    session.input(ENTER, 20.0);
    assert!(session.done());
    // The first row is the root; one down is `server`.
    assert_eq!(session.result(), "server");
    let (_, first) = start("explore", "server:\n  host: localhost\n");
    assert!(first.contains("server"));
    assert!(Session::new("explore", "{not json: [", 40, 12).is_err());
}

#[test]
fn the_todo_app_runs_on_its_lines() {
    let (mut session, first) = start("todo", "Water the plants\nCall Ada\n");
    assert!(first.contains("\x1b[?1049h"), "the alternate screen");
    assert!(first.contains("Water the plants") && first.contains("Call Ada"));
    session.input("\x03", 10.0);
    assert!(session.done());
    let (_, first) = start("todo", "");
    assert!(first.contains("Read the intuiTUIve tutorial"));
}

#[test]
fn animations_ask_for_ticks() {
    for app in ["meters", "sprites"] {
        let (mut session, first) = start(app, "");
        assert!(!first.is_empty(), "{app}");
        let wait = session.timeout(0.0);
        assert!(wait >= 0.0, "{app} animates");
        let later = session.tick(wait + 1000.0);
        assert!(!later.is_empty(), "{app} draws again");
        session.input("\x03", 2000.0);
        assert!(session.done(), "{app}");
    }
    let (session, _) = start("form", "");
    assert!(!session.done());
}

#[test]
fn a_resize_redraws() {
    let (mut session, _) = start("choose", "red\ngreen\n");
    assert!(!session.resize(60, 8, 10.0).is_empty());
    let (mut session, _) = start("todo", "");
    assert!(!session.resize(60, 8, 10.0).is_empty());
}

#[test]
fn unknown_apps_and_empty_lists_are_refused() {
    assert!(Session::new("nope", "", 40, 12).is_err());
    assert!(Session::new("choose", "\n\n", 40, 12).is_err());
    let apps: serde_json::Value = serde_json::from_str(&Session::apps()).unwrap();
    assert_eq!(apps.as_array().unwrap().len(), crate::app::APPS.len());
}
