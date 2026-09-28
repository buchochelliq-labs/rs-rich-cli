//! Select and MultiSelect, headless.

use rich_interact::headless::{self, Script};
use rich_interact::policy::{Fallback, Reason, ScriptedLineIo};
use rich_interact::{
    degrade, Action, Item, Key, MultiSelect, Outcome, Preview, PreviewLayout, Select,
};

/// The last view before the component collapsed to its answer.
fn before_answer(record: &headless::Record) -> &str {
    &record.frames[record.frames.len() - 2]
}

fn files() -> Vec<Item<&'static str>> {
    [
        "src/lib.rs",
        "src/main.rs",
        "docs/maintenance.md",
        "Cargo.toml",
        "README.md",
    ]
    .into_iter()
    .map(Item::from)
    .collect()
}

#[test]
fn arrows_and_enter_pick() {
    let (outcome, record) = headless::run(
        Select::new("Open", files()),
        Script::new().keys("down down enter"),
        60,
        12,
    );
    assert_eq!(outcome.unwrap(), Outcome::Done("docs/maintenance.md"));
    let first = &record.frames[0];
    assert!(
        first.starts_with("? Open › \n❯ src/lib.rs\n  src/main.rs"),
        "{first}"
    );
    assert!(
        first.contains("5/5 · ↑↓ move · enter pick · esc cancel"),
        "{first}"
    );
}

#[test]
fn typing_filters_and_ranks() {
    let (outcome, record) = headless::run(
        Select::new("Open", files()),
        Script::new().text("main").keys("enter"),
        60,
        12,
    );
    assert_eq!(outcome.unwrap(), Outcome::Done("src/main.rs"));
    let filtered = before_answer(&record);
    assert_eq!(record.last_frame(), "? Open › src/main.rs");
    assert!(
        filtered.starts_with("? Open › main\n❯ src/main.rs\n  docs/maintenance.md\n"),
        "{filtered}"
    );
    assert!(filtered.contains("2/5"), "{filtered}");
}

#[test]
fn highlights_matched_characters() {
    let (_, record) = headless::run(
        Select::new("Open", files()),
        Script::new().text("mr").keys("enter"),
        60,
        12,
    );
    let output = record.output();
    // The focused `src/main.rs`: `m` and `r` bold magenta inside bold.
    assert!(output.contains("\x1b[1;35mm\x1b[0m"), "{output:?}");
}

#[test]
fn nothing_matching_says_so_and_enter_waits() {
    let (outcome, record) = headless::run(
        Select::new("Open", files()),
        Script::new()
            .text("zzz")
            .keys("enter backspace backspace backspace enter"),
        60,
        12,
    );
    assert_eq!(outcome.unwrap(), Outcome::Done("src/lib.rs"));
    assert!(record
        .frames
        .iter()
        .any(|frame| frame.contains("  no matches")));
}

#[test]
fn escape_cancels() {
    let (outcome, record) = headless::run(
        Select::new("Open", files()),
        Script::new().keys("escape"),
        60,
        12,
    );
    assert_eq!(outcome.unwrap(), Outcome::Cancelled);
    assert_eq!(record.last_frame(), "? Open › cancelled");
    // The collapsed answer leaves nothing of the list below it.
    assert!(
        record.output().ends_with("\r\n\x1b[J\x1b[?25h"),
        "{:?}",
        record.output()
    );
}

#[test]
fn scrolls_a_long_list() {
    let items: Vec<Item<usize>> = (1..=50).map(Item::from).collect();
    let script = Script::new().keys("pagedown pagedown down enter");
    let (outcome, record) = headless::run(Select::new("Number", items).height(5), script, 40, 12);
    assert_eq!(outcome.unwrap(), Outcome::Done(12));
    let last = before_answer(&record);
    assert!(last.contains("❯ 12"), "{last}");
    assert_eq!(
        last.lines().count(),
        7,
        "question, five rows, footer:\n{last}"
    );
}

#[test]
fn item_actions_pick_and_say_which() {
    let items = files()
        .into_iter()
        .map(|item| item.action(Action::new("edit", "Edit", Key::ctrl('e'))));
    let mut select = Select::new("Open", items);
    let (outcome, _) = headless::run(&mut select, Script::new().keys("down ctrl+e"), 60, 12);
    assert_eq!(outcome.unwrap(), Outcome::Done("src/main.rs"));
    assert_eq!(select.action(), Some("edit"));
}

#[test]
fn preview_beside_on_wide_terminals_and_below_on_narrow() {
    let items = vec![
        Item::new(1, "alpha").preview(Preview::Text("first\npreview".into())),
        Item::new(2, "beta").preview(Preview::Markup("[bold]second[/]".into())),
    ];
    let (_, record) = headless::run(
        Select::new("Pick", items.clone()),
        Script::new().keys("down enter"),
        80,
        12,
    );
    let wide = before_answer(&record);
    assert!(wide.contains("  alpha"), "{wide}");
    assert!(wide.contains(" │ second"), "{wide}");
    let (_, record) = headless::run(
        Select::new("Pick", items.clone()),
        Script::new().keys("enter"),
        50,
        12,
    );
    let narrow = before_answer(&record);
    let rule = narrow.find('─').expect("a rule above the preview");
    assert!(narrow[rule..].contains("first\npreview"), "{narrow}");
    let (_, record) = headless::run(
        Select::new("Pick", items).preview(PreviewLayout::Hidden),
        Script::new().keys("enter"),
        80,
        12,
    );
    assert!(!before_answer(&record).contains("preview"));
}

#[test]
fn multi_select_marks_with_tab() {
    let script = Script::new().keys("tab down tab enter");
    let (outcome, record) = headless::run(MultiSelect::new("Stage", files()), script, 60, 12);
    // Tab marks and moves down: src/lib.rs, then (down) docs/maintenance.md.
    assert_eq!(
        outcome.unwrap(),
        Outcome::Done(vec!["src/lib.rs", "docs/maintenance.md"])
    );
    assert_eq!(
        record.last_frame(),
        "? Stage › src/lib.rs, docs/maintenance.md"
    );
    let last = before_answer(&record);
    assert!(last.contains("◉ src/lib.rs"), "{last}");
    assert!(last.contains("2 marked"), "{last}");
}

#[test]
fn multi_select_without_marks_picks_the_focused_and_ctrl_a_marks_matches() {
    let (outcome, _) = headless::run(
        MultiSelect::new("Stage", files()),
        Script::new().keys("down enter"),
        60,
        12,
    );
    assert_eq!(outcome.unwrap(), Outcome::Done(vec!["src/main.rs"]));
    let (outcome, _) = headless::run(
        MultiSelect::new("Stage", files()),
        Script::new().text("md").keys("ctrl+a enter"),
        60,
        12,
    );
    let mut picked = outcome.unwrap().value().unwrap();
    picked.sort();
    assert_eq!(picked, ["README.md", "docs/maintenance.md"]);
}

#[test]
fn degrades_to_numbers_or_names() {
    let reason = Reason::StdinNotTerminal;
    let mut io = ScriptedLineIo::new(["2"]);
    let mut select = Select::new("Open", files());
    assert_eq!(
        degrade(&mut select, Fallback::Prompt, reason, &mut io).unwrap(),
        Outcome::Done("src/main.rs")
    );
    assert!(
        io.written
            .starts_with("Open\n  1) src/lib.rs\n  2) src/main.rs\n"),
        "{}",
        io.written
    );
    let mut io = ScriptedLineIo::new(["readme"]);
    assert_eq!(
        degrade(&mut select, Fallback::Prompt, reason, &mut io).unwrap(),
        Outcome::Done("README.md")
    );
    let mut io = ScriptedLineIo::new(["nothing-like-this"]);
    assert!(degrade(&mut select, Fallback::Prompt, reason, &mut io).is_err());
    let mut defaulted = Select::new("Open", files()).default(3);
    assert_eq!(
        degrade(&mut defaulted, Fallback::Default, reason, &mut io).unwrap(),
        Outcome::Done("Cargo.toml")
    );
    let mut multi = MultiSelect::new("Stage", files()).marked([0, 4]);
    assert_eq!(
        degrade(&mut multi, Fallback::Default, reason, &mut io).unwrap(),
        Outcome::Done(vec!["src/lib.rs", "README.md"])
    );
    let mut io = ScriptedLineIo::new(["1, cargo"]);
    assert_eq!(
        degrade(&mut multi, Fallback::Prompt, reason, &mut io).unwrap(),
        Outcome::Done(vec!["src/lib.rs", "Cargo.toml"])
    );
}

#[test]
fn at_the_end_of_input_or_on_an_empty_line_the_default_answers() {
    let reason = Reason::NoTerminal;
    let mut defaulted = Select::new("Open", files()).default(3);
    let mut io = ScriptedLineIo::new(Vec::<String>::new());
    assert_eq!(
        degrade(&mut defaulted, Fallback::Prompt, reason, &mut io).unwrap(),
        Outcome::Done("Cargo.toml")
    );
    let mut io = ScriptedLineIo::new([""]);
    assert_eq!(
        degrade(&mut defaulted, Fallback::Prompt, reason, &mut io).unwrap(),
        Outcome::Done("Cargo.toml")
    );
    let mut multi = MultiSelect::new("Stage", files()).marked([0, 4]);
    let mut io = ScriptedLineIo::new(Vec::<String>::new());
    assert_eq!(
        degrade(&mut multi, Fallback::Prompt, reason, &mut io).unwrap(),
        Outcome::Done(vec!["src/lib.rs", "README.md"])
    );
    // No default: no answer.
    for mut io in [
        ScriptedLineIo::new(Vec::<String>::new()),
        ScriptedLineIo::new([""]),
    ] {
        let result = degrade(
            &mut Select::new("Open", files()),
            Fallback::Prompt,
            reason,
            &mut io,
        );
        assert!(result.is_err(), "{result:?}");
    }
    let mut io = ScriptedLineIo::new(Vec::<String>::new());
    assert!(matches!(
        degrade(
            &mut MultiSelect::new("Stage", files()),
            Fallback::Prompt,
            reason,
            &mut io
        ),
        Err(rich_interact::Error::NotInteractive(
            rich_interact::NotInteractive::NoDefault(Reason::NoTerminal)
        ))
    ));
}

#[test]
fn a_pasted_query_drops_terminal_controls() {
    let script = Script::new()
        .event(rich_interact::Event::Paste("ma\x1b\x07\u{9b}in".into()))
        .keys("enter");
    let (outcome, record) = headless::run(Select::new("Open", files()), script, 60, 10);
    assert_eq!(outcome.unwrap(), Outcome::Done("src/main.rs"));
    assert!(!record.output().contains('\u{9b}'), "{:?}", record.output());
}
