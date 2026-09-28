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
fn caret_counts_the_prompts_shown_controls() {
    // ESC and the tab measure no cells as text, but are painted as `␛` and
    // a space: the caret has to move past them.
    let script = Script::new().text("ab").keys("enter");
    let (outcome, record) = headless::run(Input::new("x\x1b\tz"), script, 40, 6);
    assert_eq!(outcome.unwrap(), Outcome::Done("ab".into()));
    assert_eq!(before_answer(&record), "? x␛ z › ab");
    // "? x␛ z › " is 9 cells: the caret starts at column 9 and, after
    // "ab" is written, is moved back to column 11.
    let output = record.output();
    assert!(output.contains("\r\x1b[9C\x1b[?25h"), "{output:?}");
    assert!(output.contains("b\r\x1b[11C"), "{output:?}");
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
        Input::masked("Token"),
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

#[test]
fn a_slow_provider_runs_one_lookup_at_a_time_for_the_latest_text() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    let calls = Arc::new(AtomicUsize::new(0));
    let running = Arc::new(AtomicUsize::new(0));
    let most = Arc::new(AtomicUsize::new(0));
    let (c, r, m) = (Arc::clone(&calls), Arc::clone(&running), Arc::clone(&most));
    let input = Input::new("Package").provider(move |query| {
        c.fetch_add(1, Ordering::SeqCst);
        let now = r.fetch_add(1, Ordering::SeqCst) + 1;
        m.fetch_max(now, Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(40));
        r.fetch_sub(1, Ordering::SeqCst);
        vec![Suggestion::from(format!("{query}-crate").as_str())]
    });
    // Ten keys at once, then enough time for the lookups to settle.
    let script = Script::new()
        .text("abcdefghij")
        .wait(Duration::from_millis(3000))
        .keys("escape");
    let (_, record) = headless::run(input, script, 40, 8);
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(most.load(Ordering::SeqCst), 1, "lookups overlapped");
    let calls = calls.load(Ordering::SeqCst);
    assert!(calls < 10, "{calls} lookups for ten keys");
    // The latest text's results are what shows.
    assert!(
        record
            .frames
            .iter()
            .any(|frame| frame.contains("abcdefghij-crate")),
        "{:#?}",
        record.frames
    );
}

#[test]
fn a_password_default_is_never_shown_or_written() {
    // On a terminal: the hint says there is a default, not what it is.
    let (outcome, record) = headless::run(
        Input::masked("Token").default("s3cr3t"),
        Script::new().keys("enter"),
        40,
        6,
    );
    assert_eq!(outcome.unwrap(), Outcome::Done("s3cr3t".into()));
    assert_eq!(record.frames[0], "? Token › (default set)");
    assert!(!record.output().contains("s3cr3t"), "{:?}", record.output());
    // Without one: the line prompt does not write it either.
    let mut input = Input::masked("Token").default("s3cr3t");
    let mut io = ScriptedLineIo::new([""]);
    assert_eq!(
        degrade(
            &mut input,
            Fallback::Prompt,
            Reason::StdoutNotTerminal,
            &mut io
        )
        .unwrap(),
        Outcome::Done("s3cr3t".into())
    );
    assert_eq!(io.written, "Token [default set]: ");
}

#[test]
fn a_masked_answer_is_read_as_a_secret() {
    let reason = Reason::StdoutNotTerminal;
    let mut masked = Input::masked("Token");
    let mut io = ScriptedLineIo::new(["hunter2"]);
    assert_eq!(
        degrade(&mut masked, Fallback::Prompt, reason, &mut io).unwrap(),
        Outcome::Done("hunter2".into())
    );
    assert_eq!(io.secrets, 1, "read without echo");
    let mut plain = Input::new("Name");
    let mut io = ScriptedLineIo::new(["ada"]);
    degrade(&mut plain, Fallback::Prompt, reason, &mut io).unwrap();
    assert_eq!(io.secrets, 0, "an ordinary answer is echoed as usual");
}

#[test]
fn the_caret_moves_and_deletes_by_grapheme() {
    // `e` and a combining acute are one grapheme: Left steps over both,
    // Backspace deletes both.
    let script = Script::new()
        .text("ae\u{301}")
        .keys("left")
        .text("x")
        .keys("end backspace")
        .keys("enter");
    let (outcome, _) = headless::run(Input::new("Name"), script, 40, 6);
    assert_eq!(outcome.unwrap(), Outcome::Done("ax".into()));
    let script = Script::new().text("e\u{301}b").keys("home delete enter");
    let (outcome, _) = headless::run(Input::new("Name"), script, 40, 6);
    assert_eq!(outcome.unwrap(), Outcome::Done("b".into()));
}

#[test]
fn a_long_line_scrolls_to_keep_the_caret_in_view() {
    let script = Script::new()
        .text(&"x".repeat(50))
        .text("END")
        .keys("enter");
    let (_, record) = headless::run(Input::new("Input"), script, 40, 6);
    // `? Input › ` takes 10 columns: the end of the line shows, with the
    // caret after it on the last column.
    let frame = before_answer(&record);
    assert!(frame.ends_with("xxxEND"), "{frame:?}");
    assert_eq!(frame.chars().count(), 39, "{frame:?}");
    assert!(
        record.output().contains("\x1b[39C"),
        "{:?}",
        record.output()
    );
    // Home scrolls back to the start.
    let script = Script::new()
        .text("START")
        .text(&"x".repeat(50))
        .keys("home")
        .keys("enter");
    let (_, record) = headless::run(Input::new("Input"), script, 40, 6);
    assert!(
        before_answer(&record).starts_with("? Input › STARTxxx"),
        "{:?}",
        before_answer(&record)
    );
}

#[test]
fn pasted_controls_do_not_reach_the_answer() {
    let script = Script::new()
        .event(rich_interact::Event::Paste(
            "X\x1bcY\u{9b}2JZ\x07\x08W".into(),
        ))
        .keys("enter");
    let (outcome, record) = headless::run(Input::new("Input"), script, 40, 6);
    assert_eq!(outcome.unwrap(), Outcome::Done("XcY2JZW".into()));
    assert!(!record.output().contains("\x1bc"), "{:?}", record.output());
}

#[test]
fn at_the_end_of_input_the_default_answers() {
    let reason = Reason::NoTerminal;
    let none = || ScriptedLineIo::new(Vec::<String>::new());
    let mut input = Input::new("Name").default("d");
    assert_eq!(
        degrade(&mut input, Fallback::Prompt, reason, &mut none()).unwrap(),
        Outcome::Done("d".into())
    );
    let mut masked = Input::masked("Token").default("s3cr3t");
    assert_eq!(
        degrade(&mut masked, Fallback::Prompt, reason, &mut none()).unwrap(),
        Outcome::Done("s3cr3t".into())
    );
    let mut sheet = Confirm::new("Go?").default("yes");
    assert_eq!(
        degrade(&mut sheet, Fallback::Prompt, reason, &mut none()).unwrap(),
        Outcome::Done("yes".into())
    );
    // Without a default, the end of input is no answer, not a refusal.
    for result in [
        degrade(
            &mut Input::new("Name"),
            Fallback::Prompt,
            reason,
            &mut none(),
        )
        .map(|_| ()),
        degrade(
            &mut Confirm::new("Go?"),
            Fallback::Prompt,
            reason,
            &mut none(),
        )
        .map(|_| ()),
        degrade(
            &mut Input::masked("Token"),
            Fallback::Prompt,
            reason,
            &mut none(),
        )
        .map(|_| ()),
    ] {
        assert!(
            matches!(
                result,
                Err(rich_interact::Error::NotInteractive(
                    rich_interact::NotInteractive::NoDefault(Reason::NoTerminal)
                ))
            ),
            "{result:?}"
        );
    }
}
