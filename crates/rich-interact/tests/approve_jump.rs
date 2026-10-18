//! The approval queue (#471) and the jump navigator (#484), headless:
//! keys, bulk decisions, reasons, the mouse, theme keys and the line
//! prompts without a terminal.

use rich_interact::components::{
    ApprovalQueue, ApprovalReport, Jump, JumpIndex, JumpSource, Jumped, Verdict,
};
use rich_interact::headless::{self, Script};
use rich_interact::policy::{Reason, ScriptedLineIo};
use rich_interact::{degrade, Component, Context, Fallback, Item, Outcome, Preview};

fn queue() -> ApprovalQueue<&'static str> {
    ApprovalQueue::new(
        "Deploys",
        [
            Item::new("api", "api")
                .description("v2.1")
                .meta("owner", "ada"),
            Item::new("web", "web"),
            Item::new("db", "db").preview(Preview::Text("ALTER TABLE users".into())),
            Item::new("cache", "cache"),
        ],
    )
}

fn report(queue: ApprovalQueue<&'static str>, script: Script) -> ApprovalReport<&'static str> {
    let (outcome, record) = headless::run(queue, script, 90, 14);
    match outcome.unwrap() {
        Outcome::Done(report) => report,
        other => panic!("{other:?}\n{}", record.last_frame()),
    }
}

#[test]
fn one_by_one_each_decision_moves_on_to_the_next_pending() {
    let report = report(queue(), Script::new().keys("a r s enter"));
    use Verdict::*;
    assert_eq!(report.verdicts(), [Approved, Rejected, Skipped, Pending]);
    assert_eq!(report.approved(), [&"api"]);
    assert_eq!(report.rejected(), [&"web"]);
    assert_eq!(
        report.summary(),
        "1 approved · 1 rejected · 1 skipped · 1 pending"
    );
    assert_eq!(report.decisions[3].index, 3);
}

#[test]
fn uppercase_asks_for_a_reason_and_escape_backs_out() {
    let script = Script::new()
        .keys("R")
        .text("needs review")
        .keys("enter")
        // Back out of a reason: nothing decided.
        .keys("A escape")
        .keys("enter");
    let report = report(queue(), script);
    assert_eq!(report.decisions[0].verdict, Verdict::Rejected);
    assert_eq!(report.decisions[0].reason.as_deref(), Some("needs review"));
    assert_eq!(report.decisions[1].verdict, Verdict::Pending);
}

#[test]
fn a_required_reason_is_asked_for_and_an_empty_one_refused() {
    let queue = queue().require_reason(Verdict::Rejected);
    let script = Script::new().keys("r enter");
    let (outcome, record) = headless::run(queue, script, 90, 14);
    assert!(outcome.is_err(), "the script ran out while it waited");
    assert!(
        record.last_frame().contains("a reason is needed to reject"),
        "{}",
        record.last_frame()
    );
    let queue = self::queue().require_reason(Verdict::Rejected);
    let report = report(
        queue,
        Script::new().keys("r").text("no").keys("enter enter"),
    );
    assert_eq!(report.decisions[0].reason.as_deref(), Some("no"));
}

#[test]
fn marked_items_are_decided_together() {
    // Mark api and db, approve both; then mark every pending one and skip.
    let report = report(
        queue(),
        Script::new().keys("space down space a ctrl+a s enter"),
    );
    use Verdict::*;
    assert_eq!(report.verdicts(), [Approved, Skipped, Approved, Skipped]);
    // With a reason for all of them at once.
    let report = self::report(
        queue(),
        Script::new()
            .keys("ctrl+a R")
            .text("freeze")
            .keys("enter enter"),
    );
    assert!(report
        .decisions
        .iter()
        .all(|d| d.verdict == Rejected && d.reason.as_deref() == Some("freeze")));
}

#[test]
fn undo_takes_a_decision_back_and_escape_cancels() {
    let report = report(queue(), Script::new().keys("a up u enter"));
    assert_eq!(report.count(Verdict::Pending), 4);
    let (outcome, record) = headless::run(queue(), Script::new().keys("a escape"), 90, 14);
    assert_eq!(outcome.unwrap(), Outcome::Cancelled);
    assert!(record.last_frame().contains("cancelled"));
}

#[test]
fn auto_submit_finishes_once_nothing_is_pending() {
    let queue = queue().auto_submit(true);
    let report = report(queue, Script::new().keys("a a s r"));
    assert_eq!(report.count(Verdict::Pending), 0);
}

#[test]
fn the_detail_pane_shows_the_focused_item() {
    let (_, record) = headless::run(queue(), Script::new().keys("a a escape"), 90, 14);
    let frame = &record.frames[0];
    assert!(frame.contains("owner: ada"), "{frame}");
    assert!(frame.contains("· pending"), "{frame}");
    assert!(frame.contains("0/4 decided"), "{frame}");
    // db has a preview, under its details.
    let frame = &record.frames[2];
    assert!(frame.contains("ALTER TABLE users"), "{frame}");
    assert!(frame.contains("✓ api"), "{frame}");
    // Narrow: the details go below the list.
    let (_, record) = headless::run(queue(), Script::new().keys("escape"), 50, 20);
    assert!(record.frames[0].contains("owner: ada"));
    assert!(record.frames[0].contains('─'));
    // `p` hides them.
    let (_, record) = headless::run(queue(), Script::new().keys("p escape"), 90, 14);
    assert!(!record.frames[1].contains("owner: ada"));
}

#[test]
fn the_mouse_focuses_marks_and_presses_the_buttons() {
    let queue = queue().with_mouse(true);
    // Row 3 is db; its mark column is cells 2 and 3. The buttons are on
    // the row under the four items: [a] approve at cell 2.
    let script = Script::new()
        .click(10, 3)
        .click(2, 4)
        .click(2, 5)
        .scroll(true, 10, 2);
    let (outcome, record) = headless::run(queue, script.keys("enter"), 90, 14);
    let Outcome::Done(report) = outcome.unwrap() else {
        panic!("{}", record.last_frame())
    };
    use Verdict::*;
    // db focused, cache marked and approved with the button.
    assert_eq!(report.verdicts(), [Pending, Pending, Pending, Approved]);
}

#[test]
fn decision_styles_come_from_the_theme() {
    let mut theme = rich::Theme::new();
    theme.insert("approve.approved", rich::Style::parse("bold blue").unwrap());
    let mut console = rich::Console::builder()
        .force_terminal(true)
        .width(80)
        .build();
    console.push_theme(theme, true);
    let mut queue = queue().decided(0, Verdict::Approved, None);
    let context = Context {
        console: &console,
        width: 80,
        height: 10,
    };
    let _ = queue.start(&context);
    let view = queue.render(&context);
    let mark = view.lines[1]
        .iter()
        .find(|segment| segment.text.starts_with('✓'))
        .expect("the approved mark");
    assert_eq!(
        mark.style,
        Some(rich::Style::parse("bold blue").unwrap()),
        "{:?}",
        view.lines[1]
    );
}

#[test]
fn without_a_terminal_it_asks_item_by_item() {
    let mut queue = queue().require_reason(Verdict::Rejected);
    let mut io = ScriptedLineIo::new(["a", "r", "r: too big", "maybe", "", "q"]);
    let outcome = degrade(
        &mut queue,
        Fallback::Prompt,
        Reason::StdinNotTerminal,
        &mut io,
    )
    .unwrap();
    let Outcome::Done(report) = outcome else {
        panic!()
    };
    use Verdict::*;
    assert_eq!(report.verdicts(), [Approved, Rejected, Skipped, Pending]);
    assert_eq!(report.decisions[1].reason.as_deref(), Some("too big"));
    assert!(io.written.contains("A reason is needed to reject"));
    assert!(io.written.contains("\"maybe\" is not a, r, s or q"));
    assert!(io.written.contains("owner: ada"));
    // Asked for defaults: nothing is decided.
    let mut queue = self::queue();
    let mut io = ScriptedLineIo::new(Vec::<String>::new());
    let outcome = degrade(
        &mut queue,
        Fallback::Default,
        Reason::StdinNotTerminal,
        &mut io,
    );
    let Ok(Outcome::Done(report)) = outcome else {
        panic!()
    };
    assert_eq!(report.count(Verdict::Pending), 4);
}

// ---------------------------------------------------------------------------
// Jump

fn jump() -> Jump<String> {
    let s = |text: &str| text.to_string();
    Jump::new("Jump")
        .source(JumpSource::new("recent", "Recent").items([s("src/lib.rs")]))
        .source(
            JumpSource::new("files", "Files")
                .items([
                    s("src/lib.rs"),
                    s("src/main.rs"),
                    s("README.md"),
                    s("docs/guide.md"),
                    s("docs/cli.md"),
                    s("Cargo.toml"),
                ])
                .limit(3),
        )
        .source(
            JumpSource::new("headings", "Headings").prefix('#').item(
                Item::new(s("README.md:12"), "Installation")
                    .description("README.md:12")
                    .preview(Preview::Text("## Installation\ncargo install".into())),
            ),
        )
        .source(
            JumpSource::new("commands", "Commands")
                .prefix('>')
                .items([s("cargo build"), s("cargo test")]),
        )
}

fn jumped(jump: Jump<String>, script: Script) -> Jumped<String> {
    let (outcome, record) = headless::run(jump, script, 90, 20);
    match outcome.unwrap() {
        Outcome::Done(jumped) => jumped,
        other => panic!("{other:?}\n{}", record.last_frame()),
    }
}

#[test]
fn results_are_grouped_by_source_with_the_best_focused() {
    let (_, record) = headless::run(jump(), Script::new().text("inst").keys("escape"), 90, 20);
    let frame = record.frames.iter().rev().nth(1).unwrap();
    assert!(frame.contains("Headings #"), "{frame}");
    assert!(frame.contains("❯ Installation"), "{frame}");
    assert!(frame.contains("cargo install"), "the preview: {frame}");
    let found = jumped(jump(), Script::new().text("inst").keys("enter"));
    assert_eq!(found.source, "headings");
    assert_eq!(found.value, "README.md:12");
    assert_eq!(found.label, "Installation");
}

#[test]
fn groups_show_their_best_few_until_narrowed() {
    let (_, record) = headless::run(jump(), Script::new().keys("escape"), 90, 20);
    let frame = &record.frames[0];
    assert!(frame.contains("Files  +3 more · tab"), "{frame}");
    assert!(!frame.contains("Cargo.toml"), "{frame}");
    // Tab twice: Recent, then Files alone, all of them.
    let (_, record) = headless::run(jump(), Script::new().keys("tab tab escape"), 90, 20);
    let frame = record.frames.iter().rev().nth(1).unwrap();
    assert!(frame.contains("[Files]"), "{frame}");
    assert!(frame.contains("Cargo.toml"), "{frame}");
    assert!(!frame.contains("Recent"), "{frame}");
    // Shift+Tab widens again.
    let found = jumped(
        jump(),
        Script::new().keys("tab tab shift+tab shift+tab end enter"),
    );
    assert_eq!(found.source, "commands");
}

#[test]
fn a_prefix_searches_one_source() {
    let found = jumped(jump(), Script::new().text(">te").keys("enter"));
    assert_eq!(
        (found.source.as_str(), found.value.as_str()),
        ("commands", "cargo test")
    );
    let found = jumped(jump(), Script::new().text("#").keys("enter"));
    assert_eq!(found.source, "headings");
}

#[test]
fn arrows_cross_groups_and_alt_arrows_jump_between_them() {
    // The first result is Recent's; down goes into Files.
    let found = jumped(jump(), Script::new().keys("down down enter"));
    assert_eq!((found.source.as_str(), found.index), ("files", 1));
    let found = jumped(jump(), Script::new().keys("alt+down alt+down enter"));
    assert_eq!(found.source, "headings");
    let found = jumped(jump(), Script::new().keys("alt+down alt+down alt+up enter"));
    assert_eq!((found.source.as_str(), found.index), ("files", 0));
    let found = jumped(jump(), Script::new().keys("up up enter"));
    assert_eq!(found.source, "recent");
}

#[test]
fn the_mouse_picks_results_and_narrows_by_title() {
    // Row 1 is Recent's title, row 2 its result, row 3 Files' title.
    let script = Script::new().click(4, 4).click(4, 4);
    let found = jumped(jump().with_mouse(true), script);
    assert_eq!((found.source.as_str(), found.index), ("files", 0));
    let script = Script::new().click(2, 3).keys("end enter");
    let found = jumped(jump().with_mouse(true), script);
    assert_eq!(
        (found.source.as_str(), found.value.as_str()),
        ("files", "Cargo.toml")
    );
}

#[test]
fn no_matches_says_so_and_picks_nothing() {
    let (outcome, record) = headless::run(
        jump(),
        Script::new().text("zzzz").keys("enter escape"),
        90,
        20,
    );
    assert_eq!(outcome.unwrap(), Outcome::Cancelled);
    assert!(record
        .frames
        .iter()
        .any(|frame| frame.contains("no matches")));
}

#[test]
fn without_a_terminal_the_best_match_answers() {
    let mut jump = self::jump();
    let mut io = ScriptedLineIo::new(["guide"]);
    let outcome = degrade(
        &mut jump,
        Fallback::Prompt,
        Reason::StdinNotTerminal,
        &mut io,
    );
    let Ok(Outcome::Done(found)) = outcome else {
        panic!("{outcome:?}")
    };
    assert_eq!(found.value, "docs/guide.md");
    let mut jump = self::jump().query("cargo b");
    let outcome = degrade(
        &mut jump,
        Fallback::Default,
        Reason::StdinNotTerminal,
        &mut ScriptedLineIo::default(),
    );
    let Ok(Outcome::Done(found)) = outcome else {
        panic!("{outcome:?}")
    };
    assert_eq!(found.value, "cargo build");
    // Nothing typed is no default.
    let mut jump = self::jump();
    assert!(degrade(
        &mut jump,
        Fallback::Default,
        Reason::StdinNotTerminal,
        &mut ScriptedLineIo::default()
    )
    .is_err());
}

#[test]
fn the_index_searches_without_the_component() {
    let index = JumpIndex::new()
        .source(JumpSource::new("a", "A").items(["alpha", "beta"]))
        .source(JumpSource::new("b", "B").prefix('@').items(["gamma"]));
    assert_eq!(index.scope("@ga"), (Some(1), "ga"));
    let groups = index.search("a", None);
    assert_eq!(groups.len(), 2);
    assert_eq!(index.search("@", None)[0].source, 1);
    assert_eq!(index.best("gam"), Some((1, 0)));
}
