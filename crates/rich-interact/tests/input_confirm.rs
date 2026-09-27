//! Input and Confirm, headless.

use std::time::Duration;

use rich_interact::headless::{self, Script};
use rich_interact::policy::{Fallback, Reason, ScriptedLineIo};
use rich_interact::{degrade, Choice, Confirm, Input, Outcome, Suggestion};

fn before_answer(record: &headless::Record) -> &str {
    &record.frames[record.frames.len() - 2]
}

#[test]
fn types_edits_and_submits() {
    let script = Script::new()
        .text("helo")
        .keys("left")
        .text("l")
        .keys("end")
        .text("!")
        .keys("enter");
    let (outcome, record) = headless::run(Input::new("Greeting"), script, 40, 6);
    assert_eq!(outcome.unwrap(), Outcome::Done("hello!".into()));
    assert_eq!(record.last_frame(), "? Greeting › hello!");
    // The caret sits after what is typed: column 13 + 6.
    assert!(
        record.output().contains("\x1b[19C"),
        "{:?}",
        record.output()
    );
}

#[test]
fn line_editing_keys() {
    let script = Script::new()
        .text("one two three")
        .keys("ctrl+w")
        .keys("home delete")
        .keys("ctrl+e backspace")
        .keys("enter");
    let (outcome, _) = headless::run(Input::new("Words"), script, 40, 6);
    assert_eq!(outcome.unwrap(), Outcome::Done("ne two".into()));
    let (outcome, _) = headless::run(
        Input::new("Words"),
        Script::new().text("abc def").keys("left left ctrl+u enter"),
        40,
        6,
    );
    assert_eq!(outcome.unwrap(), Outcome::Done("ef".into()));
}

#[test]
fn validation_shows_inline_and_waits() {
    let input = Input::new("Port").validate(|text| {
        text.parse::<u16>()
            .map(|_| ())
            .map_err(|_| "a number from 0 to 65535".to_string())
    });
    let script = Script::new().text("80a").keys("enter backspace enter");
    let (outcome, record) = headless::run(input, script, 40, 6);
    assert_eq!(outcome.unwrap(), Outcome::Done("80".into()));
    assert!(
        record
            .frames
            .iter()
            .any(|frame| frame == "? Port › 80a\n  ✗ a number from 0 to 65535"),
        "{:#?}",
        record.frames
    );
}

#[test]
fn defaults_placeholders_and_masks() {
    let (outcome, record) = headless::run(
        Input::new("Name").default("world"),
        Script::new().keys("enter"),
        40,
        6,
    );
    assert_eq!(outcome.unwrap(), Outcome::Done("world".into()));
    assert_eq!(record.frames[0], "? Name › (world)");
    let (outcome, record) = headless::run(
        Input::password("Token"),
        Script::new().text("s3cret").keys("enter"),
        40,
        6,
    );
    assert_eq!(outcome.unwrap(), Outcome::Done("s3cret".into()));
    assert!(
        !record.output().contains("s3cret"),
        "a password is never painted"
    );
    assert_eq!(record.last_frame(), "? Token › ••••••");
}

#[test]
fn history_walks_up_and_back_down() {
    let input = Input::new("Command").history(["ls", "cargo test"]);
    let script = Script::new().text("ech").keys("up up down down enter");
    let (outcome, record) = headless::run(input, script, 40, 6);
    assert_eq!(outcome.unwrap(), Outcome::Done("ech".into()));
    let shown: Vec<&str> = record.frames.iter().map(String::as_str).collect();
    assert!(shown.contains(&"? Command › cargo test"));
    assert!(shown.contains(&"? Command › ls"));
}

#[test]
fn fixed_suggestions_filter_and_tab_accepts() {
    let input = Input::new("Branch").suggestions([
        Suggestion::new("main").description("default"),
        Suggestion::new("feature/login"),
        Suggestion::new("fix/logout"),
    ]);
    let script = Script::new().text("log").keys("down down tab enter");
    let (outcome, record) = headless::run(input, script, 50, 8);
    // Equal scores: the shorter first.
    assert_eq!(outcome.unwrap(), Outcome::Done("feature/login".into()));
    let shown = record
        .frames
        .iter()
        .find(|frame| frame.contains("❯"))
        .expect("a selected suggestion");
    assert!(
        shown.starts_with("? Branch › log\n  ❯ fix/logout\n    feature/login"),
        "{shown}"
    );
}

#[test]
fn a_provider_runs_off_thread_and_stale_results_are_dropped() {
    let input = Input::new("Package").provider(|query| {
        ["serde", "serde_json", "tokio"]
            .into_iter()
            .filter(|name| name.starts_with(query))
            .map(Suggestion::from)
            .collect()
    });
    let script = Script::new()
        .text("se")
        .wait(Duration::from_millis(200))
        .text("rde_")
        .wait(Duration::from_millis(200))
        .keys("tab enter");
    let (outcome, record) = headless::run(input, script, 40, 8);
    assert_eq!(outcome.unwrap(), Outcome::Done("serde_json".into()));
    // After Tab, a lookup for the accepted text may still be pending.
    let frame = before_answer(&record);
    assert!(frame.starts_with("? Package › serde_json"), "{frame}");
    assert!(!frame.contains("tokio"), "{frame}");
    assert!(
        record
            .frames
            .iter()
            .any(|frame| frame == "? Package › se\n    serde\n    serde_json"),
        "{:#?}",
        record.frames
    );
}

#[test]
fn input_degrades_to_a_line() {
    let reason = Reason::StdoutNotTerminal;
    let mut input = Input::new("Name").default("world");
    let mut io = ScriptedLineIo::new([""]);
    assert_eq!(
        degrade(&mut input, Fallback::Prompt, reason, &mut io).unwrap(),
        Outcome::Done("world".into())
    );
    assert_eq!(io.written, "Name [world]: ");
    let mut strict = Input::new("Port")
        .validate(|text| text.parse::<u16>().map(|_| ()).map_err(|e| e.to_string()));
    let mut io = ScriptedLineIo::new(["http"]);
    assert!(degrade(&mut strict, Fallback::Prompt, reason, &mut io).is_err());
}

#[test]
fn confirm_yes_no_by_key_or_arrows() {
    let (outcome, record) = headless::run(
        Confirm::new("Delete 3 files?"),
        Script::new().keys("y"),
        50,
        8,
    );
    assert_eq!(outcome.unwrap(), Outcome::Done("yes".into()));
    assert_eq!(
        record.frames[0],
        "? Delete 3 files?\n   Yes   No  \n  y/n · ←→ move · enter choose · esc cancel"
    );
    assert_eq!(record.last_frame(), "? Delete 3 files? › Yes");
    let (outcome, _) = headless::run(
        Confirm::new("Delete?").default("no"),
        Script::new().keys("enter"),
        50,
        8,
    );
    assert_eq!(outcome.unwrap(), Outcome::Done("no".into()));
    let (outcome, _) = headless::run(
        Confirm::new("Delete?"),
        Script::new().keys("right left right enter"),
        50,
        8,
    );
    assert_eq!(outcome.unwrap(), Outcome::Done("no".into()));
    let (outcome, _) = headless::run(Confirm::new("Delete?"), Script::new().keys("escape"), 50, 8);
    assert_eq!(outcome.unwrap(), Outcome::Cancelled);
}

#[test]
fn confirm_sheet_shows_a_body_warnings_and_choices() {
    let diff = rich::Text::new("- replicas: 2\n+ replicas: 5");
    let sheet = Confirm::new("Apply this change?")
        .body(diff)
        .warning("3 pods will restart")
        .choices([
            Choice::new("apply", "Apply", 'a'),
            Choice::new("all", "Apply all", 'l'),
            Choice::new("skip", "Skip", 's'),
        ]);
    let (outcome, record) = headless::run(sheet, Script::new().keys("tab tab enter"), 60, 10);
    assert_eq!(outcome.unwrap(), Outcome::Done("skip".into()));
    assert_eq!(
        record.frames[0],
        "? Apply this change?\n  - replicas: 2\n  + replicas: 5\n  ⚠ 3 pods will restart\n   Apply   Apply all   Skip  \n  a/l/s · ←→ move · enter choose · esc cancel"
    );
}

#[test]
fn a_long_body_scrolls() {
    let body = rich::Text::new(
        (1..=30)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n"),
    );
    let sheet = Confirm::new("Proceed?").body(body).body_height(5);
    let (_, record) = headless::run(sheet, Script::new().keys("down down y"), 90, 20);
    let frame = before_answer(&record);
    assert!(frame.starts_with("? Proceed?\n  line 3\n"), "{frame}");
    assert!(frame.contains("↑↓ scroll (lines 3–7 of 30)"), "{frame}");
}

#[test]
fn confirm_degrades_to_a_line() {
    let reason = Reason::Ci;
    let mut sheet = Confirm::new("Continue?").warning("slow").default("yes");
    let mut io = ScriptedLineIo::new(["N"]);
    assert_eq!(
        degrade(&mut sheet, Fallback::Prompt, reason, &mut io).unwrap(),
        Outcome::Done("no".into())
    );
    assert_eq!(io.written, "warning: slow\nContinue? [y=Yes, n=No]: ");
    let mut io = ScriptedLineIo::new([""]);
    assert_eq!(
        degrade(&mut sheet, Fallback::Prompt, reason, &mut io).unwrap(),
        Outcome::Done("yes".into())
    );
    assert_eq!(
        degrade(&mut sheet, Fallback::Default, reason, &mut io).unwrap(),
        Outcome::Done("yes".into())
    );
}
