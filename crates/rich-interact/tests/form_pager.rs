//! Form and Pager, headless.

use rich::Segment;
use rich_interact::headless::{self, Script};
use rich_interact::policy::{Fallback, Reason, ScriptedLineIo};
use rich_interact::{degrade, Form, Input, Outcome, Pager, Value};

fn form() -> Form {
    Form::new("New service")
        .input(
            "name",
            Input::new("Name").validate(|text| {
                if text.is_empty() {
                    Err("required".into())
                } else {
                    Ok(())
                }
            }),
        )
        .input("port", Input::new("Port").default("8080"))
        .choice("env", "Environment", ["dev", "staging", "prod"])
        .toggle("tls", "TLS", false)
}

#[test]
fn fills_every_kind_of_field() {
    let script = Script::new()
        .text("api")
        .keys("tab tab right right tab space enter");
    let (outcome, record) = headless::run(form(), script, 60, 12);
    let answers = outcome.unwrap().value().unwrap();
    assert_eq!(answers.text("name"), Some("api"));
    assert_eq!(
        answers.text("port"),
        Some("8080"),
        "an empty field takes its default"
    );
    assert_eq!(answers.text("env"), Some("prod"));
    assert_eq!(answers.flag("tls"), Some(true));
    assert_eq!(answers.get("missing"), None);
    assert_eq!(
        record.last_frame(),
        "? New service\n  Name         api\n  Port         8080\n  Environment  prod\n  TLS          yes"
    );
    let open = &record.frames[0];
    assert_eq!(
        open.as_str(),
        "? New service\n❯ Name         \n  Port         (8080)\n  Environment  dev\n  TLS          ○ no\n  tab/↑↓ move · enter next · ctrl+s submit · esc cancel"
    );
}

#[test]
fn submitting_shows_each_error_under_its_field() {
    let script = Script::new().keys("ctrl+s").text("api").keys("ctrl+s");
    let (outcome, record) = headless::run(form(), script, 60, 12);
    assert!(matches!(outcome.unwrap(), Outcome::Done(_)));
    let failed = record
        .frames
        .iter()
        .find(|frame| frame.contains('✗'))
        .expect("an inline error");
    assert!(
        failed.contains("❯ Name         \n               ✗ required\n  Port"),
        "{failed}"
    );
}

#[test]
fn enter_moves_on_and_submits_on_the_last_field() {
    let script = Script::new().text("x").keys("enter enter enter enter");
    let (outcome, _) = headless::run(form(), script, 60, 12);
    let answers = outcome.unwrap().value().unwrap();
    assert_eq!(answers.0.len(), 4);
    assert_eq!(answers.get("tls"), Some(&Value::Flag(false)));
}

#[test]
fn escape_cancels_a_form() {
    let (outcome, record) = headless::run(form(), Script::new().keys("escape"), 60, 12);
    assert_eq!(outcome.unwrap(), Outcome::Cancelled);
    assert_eq!(record.last_frame(), "? New service › cancelled");
}

#[test]
fn a_form_degrades_to_a_line_per_field() {
    let mut io = ScriptedLineIo::new(["api", "", "staging", "y"]);
    let mut form = form().password("token", "Token");
    io.answers.push_back("secret".into());
    let answers = degrade(
        &mut form,
        Fallback::Prompt,
        Reason::StdinNotTerminal,
        &mut io,
    )
    .unwrap()
    .value()
    .unwrap();
    assert_eq!(answers.text("port"), Some("8080"));
    assert_eq!(answers.text("env"), Some("staging"));
    assert_eq!(answers.flag("tls"), Some(true));
    assert_eq!(answers.text("token"), Some("secret"));
    assert_eq!(
        io.written,
        "New service\nName: Port [8080]: Environment: dev / staging / prod [dev]: TLS [y/N]: Token: "
    );
}

fn numbered(n: usize) -> Vec<Vec<Segment>> {
    (1..=n)
        .map(|i| {
            vec![Segment::new(
                format!("line {i}: {}", if i % 10 == 0 { "needle" } else { "hay" }),
                None,
            )]
        })
        .collect()
}

#[test]
fn pages_and_quits() {
    let (outcome, record) = headless::run(
        Pager::lines(numbered(50)),
        Script::new().keys("pagedown q"),
        40,
        11,
    );
    assert_eq!(outcome.unwrap(), Outcome::Done(()));
    let last = record.last_frame();
    assert!(last.starts_with("line 11: hay\n"), "{last}");
    assert!(
        last.ends_with("lines 11–20 of 50 · / search · q quit"),
        "{last}"
    );
}

#[test]
fn searches_and_jumps_between_matches() {
    let script = Script::new().text("/needle").keys("enter n n N q");
    let (_, record) = headless::run(Pager::lines(numbered(50)), script, 60, 11);
    let typing = record
        .frames
        .iter()
        .find(|frame| frame.ends_with("/needle"))
        .expect("the search line while typing");
    assert!(typing.starts_with("line 1: hay"));
    let last = record.last_frame();
    // needle is on lines 10, 20, 30, 40, 50: n, n, N ends on the second.
    assert!(last.contains("match 2/5 · n/N"), "{last}");
    assert!(last.contains("line 20: needle"), "{last}");
    // Matches are marked: the current one black on yellow.
    assert!(
        record.output().contains("\x1b[30;43mneedle\x1b[0m"),
        "{:?}",
        record.output()
    );
}

#[test]
fn a_search_with_no_match_says_so() {
    let script = Script::new().text("/zebra").keys("enter q");
    let (_, record) = headless::run(Pager::lines(numbered(5)), script, 60, 11);
    assert!(record
        .last_frame()
        .ends_with("all 5 lines · no match for \"zebra\" · / search · q quit"));
}

#[test]
fn renders_a_renderable_at_the_terminal_width() {
    let text = rich::Text::new("word ".repeat(40));
    let (_, record) = headless::run(
        Pager::new(text),
        Script::new().resize(30, 11).keys("q"),
        60,
        11,
    );
    let first = &record.frames[0];
    assert_eq!(first.lines().next().unwrap().trim_end().len(), 59);
    let last = record.last_frame();
    assert!(
        last.lines().next().unwrap().trim_end().len() <= 30,
        "{last}"
    );
}

#[test]
fn a_pager_degrades_to_printing_everything() {
    let mut pager = Pager::lines(numbered(3));
    let mut io = ScriptedLineIo::default();
    assert_eq!(
        degrade(
            &mut pager,
            Fallback::Prompt,
            Reason::StdoutNotTerminal,
            &mut io
        )
        .unwrap(),
        Outcome::Done(())
    );
    assert_eq!(io.written, "line 1: hay\nline 2: hay\nline 3: hay\n");
    let mut rendered = Pager::new(rich::Text::new("from a renderable"));
    let mut io = ScriptedLineIo::default();
    degrade(
        &mut rendered,
        Fallback::Prompt,
        Reason::StdoutNotTerminal,
        &mut io,
    )
    .unwrap();
    assert_eq!(io.written.trim_end(), "from a renderable");
}
