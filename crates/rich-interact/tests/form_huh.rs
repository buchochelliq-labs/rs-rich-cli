//! Form's Huh-style fields (0.0.21): multi-select, notes, descriptions,
//! pages, a file field and a dynamic choice, headless; and the Paginator.

use rich_interact::headless::{self, Script};
use rich_interact::policy::{Fallback, Reason, ScriptedLineIo};
use rich_interact::{
    degrade, Answers, Component, FilePicker, Form, Input, Outcome, Paginator, PaginatorStyle, Value,
};

/// The last view before the form collapsed to its answers.
fn before_answer(record: &headless::Record) -> &str {
    &record.frames[record.frames.len() - 2]
}

fn required(label: &str) -> Input {
    Input::new(label).validate(|text| {
        if text.is_empty() {
            Err("required".into())
        } else {
            Ok(())
        }
    })
}

// ---- Multi-select ----

#[test]
fn a_multi_select_chooses_several_options_in_their_order() {
    let form = Form::new("Pizza").multi("toppings", "Toppings", ["cheese", "ham", "olives"]);
    // Choose olives first, then cheese: the answer keeps the options' order.
    let script = Script::new().keys("left space left left space enter");
    let (outcome, record) = headless::run(form, script, 60, 8);
    let answers = outcome.unwrap().value().unwrap();
    assert_eq!(
        answers.get("toppings"),
        Some(&Value::List(vec!["cheese".into(), "olives".into()]))
    );
    assert_eq!(answers.list("toppings").unwrap(), ["cheese", "olives"]);
    assert_eq!(
        before_answer(&record),
        "? Pizza\n❯ Toppings  ◉ cheese  ○ ham  ◉ olives\n  tab/↑↓ move · enter submit · ctrl+s submit · esc cancel"
    );
    assert_eq!(record.last_frame(), "? Pizza\n  Toppings  cheese, olives");
}

#[test]
fn a_multi_select_keeps_to_its_limit_and_starts_with_its_chosen() {
    let form = Form::new("Pick")
        .multi("n", "Numbers", ["one", "two", "three"])
        .chosen(["two"])
        .limit(2);
    // `two` is chosen; choosing one and three as well stops at two.
    let script = Script::new().keys("space right right space enter");
    let (outcome, _) = headless::run(form, script, 60, 8);
    let answers = outcome.unwrap().value().unwrap();
    assert_eq!(answers.list("n").unwrap(), ["one", "two"]);
}

#[test]
fn a_click_on_an_option_chooses_it() {
    let form = Form::new("Pick")
        .multi("n", "N", ["aa", "bb"])
        .with_mouse(true);
    // Row 1: `❯ N  ○ aa  ○ bb`: `bb`'s mark is at column 11.
    let script = Script::new().click(12, 1).keys("enter");
    let (outcome, _) = headless::run(form, script, 60, 8);
    assert_eq!(outcome.unwrap().value().unwrap().list("n").unwrap(), ["bb"]);
}

// ---- Notes and descriptions ----

#[test]
fn notes_are_read_only_and_descriptions_show_under_their_field() {
    let form = Form::new("Setup")
        .note("Welcome", "This sets up the service.")
        .text("name", "Name")
        .describe("lower case")
        .toggle("tls", "TLS", true);
    let script = Script::new().text("api").keys("enter enter");
    let (outcome, record) = headless::run(form, script, 60, 10);
    let answers = outcome.unwrap().value().unwrap();
    // The note has no answer; focus started on the first real field.
    assert_eq!(answers.0.len(), 2);
    assert_eq!(answers.text("name"), Some("api"));
    assert_eq!(
        record.frames[0],
        "? Setup\n  Welcome\n    This sets up the service.\n❯ Name  \n        lower case\n  TLS   ◉ yes\n  tab/↑↓ move · enter next · ctrl+s submit · esc cancel"
    );
    // The summary leaves the note out.
    assert_eq!(record.last_frame(), "? Setup\n  Name  api\n  TLS   yes");
}

// ---- Pages ----

fn paged() -> Form {
    Form::new("Account")
        .group("You")
        .input("name", required("Name"))
        .text("mail", "Mail")
        .group("Plan")
        .choice("plan", "Plan", ["free", "pro"])
        .input("card", required("Card"))
}

#[test]
fn groups_show_one_page_at_a_time_with_an_indicator() {
    let script = Script::new()
        .text("ann")
        .keys("enter enter")
        .keys("right enter")
        .text("4242")
        .keys("enter");
    let (outcome, record) = headless::run(paged(), script, 80, 10);
    let answers = outcome.unwrap().value().unwrap();
    assert_eq!(answers.text("name"), Some("ann"));
    assert_eq!(answers.text("plan"), Some("pro"));
    assert_eq!(answers.text("card"), Some("4242"));
    assert_eq!(
        record.frames[0],
        "? Account\n  You\n❯ Name  \n  Mail  \n  ● ○\n  tab/↑↓ move · enter next · pgup/pgdn page · ctrl+s submit · esc cancel"
    );
    let second = record
        .frames
        .iter()
        .find(|frame| frame.contains("  Plan\n"))
        .expect("the second page");
    assert!(second.contains("  ○ ●\n"), "{second}");
    assert!(!second.contains("Name"), "{second}");
    // Every page's answers are in the summary.
    assert_eq!(
        record.last_frame(),
        "? Account\n  Name  ann\n  Mail  \n  Plan  pro\n  Card  4242"
    );
}

#[test]
fn a_page_must_pass_before_the_next_shows() {
    // Page Down with an empty required name stays, with the error.
    let script = Script::new()
        .keys("pagedown")
        .text("ann")
        .keys("pagedown pageup escape");
    let mut form = paged();
    let (outcome, record) = headless::run(&mut form, script, 60, 10);
    assert_eq!(outcome.unwrap(), Outcome::Cancelled);
    let failed = record
        .frames
        .iter()
        .find(|frame| frame.contains("✗ required"))
        .expect("the error");
    assert!(failed.contains("  You\n❯ Name"), "{failed}");
    // Then it passed, moved to page two, and Page Up went back.
    assert!(record.frames.iter().any(|frame| frame.contains("  Plan\n")));
    assert_eq!(form.page(), 0);
}

#[test]
fn submitting_from_the_first_page_shows_the_page_that_fails() {
    let script = Script::new().text("ann").keys("ctrl+s");
    let mut form = paged();
    let (_, record) = headless::run(&mut form, script, 60, 10);
    assert_eq!(form.page(), 1);
    let last = record.last_frame();
    assert!(last.contains("❯ Card  \n        ✗ required"), "{last}");
}

#[test]
fn a_click_on_a_page_dot_moves_through_the_checks() {
    let form = || paged().with_mouse(true);
    // Rows: title, group, two fields, the indicator at row 4; page two's
    // dot at column 4.
    let (_, record) = headless::run(form(), Script::new().click(4, 4), 60, 10);
    assert!(record.last_frame().contains("✗ required"));
    let script = Script::new().text("ann").click(4, 4);
    let (_, record) = headless::run(form(), script, 60, 10);
    assert!(
        record.last_frame().contains("  Plan\n"),
        "{}",
        record.last_frame()
    );
}

#[test]
fn the_page_indicator_can_be_a_fraction() {
    let form = paged().page_style(PaginatorStyle::Fraction);
    let (_, record) = headless::run(form, Script::new().keys("escape"), 60, 10);
    assert!(
        record.frames[0].contains("\n  1/2\n"),
        "{}",
        record.frames[0]
    );
}

// ---- File field ----

fn tree() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.rs"), "fn main() {}\n").unwrap();
    std::fs::write(dir.path().join("b.txt"), "hello\n").unwrap();
    dir
}

#[test]
fn a_file_field_picks_with_the_file_picker() {
    let dir = tree();
    let form = Form::new("Upload")
        .file("source", "Source", FilePicker::new("Source", dir.path()))
        .toggle("go", "Go", true);
    // Enter with nothing picked fails; Space opens the picker under the
    // field, a filter and Enter pick, and Enter moves on and submits.
    let script = Script::new()
        .keys("ctrl+s space")
        .text("b.t")
        .keys("enter enter enter");
    let (outcome, record) = headless::run(form, script, 100, 20);
    let answers = outcome.unwrap().value().unwrap();
    assert_eq!(
        answers.path("source"),
        Some(dir.path().join("b.txt").as_path())
    );
    assert!(record.frames[0].contains("❯ Source  (space to browse)"));
    assert!(record
        .frames
        .iter()
        .any(|frame| frame.contains("✗ nothing picked yet")));
    let browsing = record
        .frames
        .iter()
        .find(|frame| frame.contains("esc close the picker"))
        .expect("the open picker");
    assert!(browsing.contains("a.rs"), "{browsing}");
    assert!(browsing.starts_with("? Upload\n❯ Source"), "{browsing}");
}

#[test]
fn escape_closes_the_picker_without_cancelling_the_form() {
    let dir = tree();
    let picker = FilePicker::new("Source", dir.path()).default(dir.path().join("a.rs"));
    let form = Form::new("Upload").file("source", "Source", picker);
    let script = Script::new().keys("space escape enter");
    let (outcome, _) = headless::run(form, script, 100, 20);
    // Nothing picked: the picker's default answers.
    let answers = outcome.unwrap().value().unwrap();
    assert_eq!(
        answers.path("source"),
        Some(dir.path().join("a.rs").as_path())
    );
}

// ---- Dynamic choice ----

fn regions(answers: &Answers) -> Vec<String> {
    match answers.text("cloud") {
        Some("gcp") => vec!["us-central1".into(), "europe-west1".into()],
        _ => vec!["us-east-1".into(), "eu-west-1".into()],
    }
}

#[test]
fn a_dynamic_choice_follows_the_answers_above_it() {
    let form = || {
        Form::new("Deploy")
            .choice("cloud", "Cloud", ["aws", "gcp"])
            .dynamic_choice("region", "Region", regions)
    };
    let (outcome, record) = headless::run(form(), Script::new().keys("enter enter"), 60, 8);
    assert_eq!(
        outcome.unwrap().value().unwrap().text("region"),
        Some("us-east-1")
    );
    assert!(record.frames[0].contains("Region  us-east-1"));
    // Changing the cloud changes the region's options.
    let script = Script::new().keys("right tab right enter");
    let (outcome, _) = headless::run(form(), script, 60, 8);
    let answers = outcome.unwrap().value().unwrap();
    assert_eq!(answers.text("region"), Some("europe-west1"));
}

#[test]
fn a_dynamic_choice_keeps_its_option_while_it_is_offered() {
    let form =
        Form::new("Deploy")
            .text("prefix", "Prefix")
            .dynamic_choice("name", "Name", |answers| {
                let prefix = answers.text("prefix").unwrap_or_default();
                vec![format!("{prefix}-a"), "fixed".into()]
            });
    // Choose `fixed`, then type: the options change, `fixed` stays chosen.
    let script = Script::new()
        .keys("tab right shift+tab")
        .text("x")
        .keys("ctrl+s");
    let (outcome, _) = headless::run(form, script, 60, 8);
    assert_eq!(
        outcome.unwrap().value().unwrap().text("name"),
        Some("fixed")
    );
}

// ---- Without a terminal ----

#[test]
fn every_new_field_degrades_to_lines() {
    let dir = tree();
    let mut form = Form::new("All")
        .note("Hi", "Read me.")
        .multi("tags", "Tags", ["a", "b", "c"])
        .group("Second")
        .choice("cloud", "Cloud", ["aws", "gcp"])
        .dynamic_choice("region", "Region", regions)
        .file(
            "file",
            "File",
            FilePicker::new("File", dir.path()).default("x.txt"),
        );
    let mut io = ScriptedLineIo::new(["c, A", "gcp", "", ""]);
    let answers = degrade(
        &mut form,
        Fallback::Prompt,
        Reason::StdinNotTerminal,
        &mut io,
    )
    .unwrap()
    .value()
    .unwrap();
    assert_eq!(answers.list("tags").unwrap(), ["a", "c"]);
    assert_eq!(answers.text("region"), Some("us-central1"));
    assert_eq!(answers.path("file"), Some(std::path::Path::new("x.txt")));
    assert_eq!(
        io.written,
        "All\nHi\nRead me.\nTags: a / b / c (comma-separated) []: Second\n\
         Cloud: aws / gcp [aws]: Region: us-central1 / europe-west1 [us-central1]: File [x.txt]: "
    );
    // An option that is not offered is an error.
    let mut form = Form::new("X").multi("tags", "Tags", ["a"]);
    let mut io = ScriptedLineIo::new(["z"]);
    assert!(degrade(
        &mut form,
        Fallback::Prompt,
        Reason::StdinNotTerminal,
        &mut io
    )
    .is_err());
}

#[test]
fn the_keymap_follows_the_focused_field() {
    let dir = tree();
    let form = Form::new("K").multi("m", "M", ["a"]).group("Two").file(
        "f",
        "F",
        FilePicker::new("F", dir.path()),
    );
    let actions: Vec<String> = form
        .keymap()
        .bindings()
        .iter()
        .map(|binding| binding.action.clone())
        .collect();
    for action in ["next-page", "previous-page", "choose", "next-choice"] {
        assert!(actions.iter().any(|a| a == action), "{action}: {actions:?}");
    }
}

// ---- Paginator ----

#[test]
fn a_paginator_pages_through_items() {
    let paginator = Paginator::over("Letters", ["a", "b", "c", "d", "e"], 2);
    let script = Script::new().keys("right right right left enter");
    let (outcome, record) = headless::run(paginator, script, 40, 8);
    assert_eq!(outcome.unwrap(), Outcome::Done(1));
    assert_eq!(
        record.frames[0],
        "? Letters\n  a\n  b\n  ● ○ ○\n  ←/→ page · enter choose · esc cancel"
    );
    // The last page has one item.
    assert!(record
        .frames
        .iter()
        .any(|frame| frame.starts_with("? Letters\n  e\n  ○ ○ ●\n")));
    assert_eq!(record.last_frame(), "? Letters › page 2 of 3");
}

#[test]
fn a_paginator_moves_and_slices_as_state() {
    let mut pages = Paginator::new(4);
    assert!(!pages.previous_page());
    assert!(pages.on_first());
    assert_eq!(pages.bounds(7, 2), 0..2);
    pages.set_page(9);
    assert!(pages.on_last());
    assert!(!pages.next_page());
    assert_eq!(pages.bounds(7, 2), 6..7);
    assert!(pages.handle_key(rich_interact::Key::parse("home").unwrap()));
    assert_eq!(pages.page(), 0);
    assert!(!pages.handle_key(rich_interact::Key::parse("enter").unwrap()));
    pages.set_pages(2);
    assert_eq!(pages.pages(), 2);
    // Dots that do not fit become a fraction.
    let many = Paginator::new(30).start_at(2);
    assert_eq!(many.indicator_text(80), format!("○ ○ ●{}", " ○".repeat(27)));
    assert_eq!(many.indicator_text(20), "3/30");
    let styled = Paginator::new(3).style(PaginatorStyle::Fraction);
    assert_eq!(styled.indicator_text(80), "1/3");
    let custom = Paginator::new(3).dots("■", "□");
    assert_eq!(custom.indicator_text(80), "■ □ □");
}

#[test]
fn a_paginator_takes_the_mouse_and_rebinding() {
    let paginator = || Paginator::over("P", ["a", "b", "c"], 1).with_mouse(true);
    // Row 2 is the indicator; the third dot is at column 2 + 4.
    let (outcome, _) = headless::run(paginator(), Script::new().click(6, 2).keys("enter"), 40, 8);
    assert_eq!(outcome.unwrap(), Outcome::Done(2));
    let script = Script::new()
        .scroll(true, 0, 0)
        .scroll(true, 0, 0)
        .scroll(false, 0, 0);
    let (outcome, _) = headless::run(paginator(), script.keys("enter"), 40, 8);
    assert_eq!(outcome.unwrap(), Outcome::Done(1));
    let rebound =
        Paginator::over("P", ["a", "b"], 1).rebind("next-page", rich_interact::keymap::keys("n"));
    let (outcome, _) = headless::run(rebound, Script::new().keys("right n enter"), 40, 8);
    assert_eq!(outcome.unwrap(), Outcome::Done(1));
    let (outcome, record) = headless::run(paginator(), Script::new().keys("q"), 40, 8);
    assert_eq!(outcome.unwrap(), Outcome::Cancelled);
    assert_eq!(record.last_frame(), "? P › cancelled");
}

#[test]
fn a_paginator_degrades_to_a_page_number() {
    let mut paginator = Paginator::over("P", ["a", "b", "c"], 1);
    let mut io = ScriptedLineIo::new(["3"]);
    let page = degrade(
        &mut paginator,
        Fallback::Prompt,
        Reason::StdinNotTerminal,
        &mut io,
    )
    .unwrap()
    .value();
    assert_eq!(page, Some(2));
    assert_eq!(io.written, "P\n  a\n  b\n  c\nPage (1-3) [1]: ");
    let mut io = ScriptedLineIo::new(["9"]);
    assert!(degrade(
        &mut paginator,
        Fallback::Prompt,
        Reason::StdinNotTerminal,
        &mut io
    )
    .is_err());
    assert_eq!(paginator.default_value(), Some(2));
}
