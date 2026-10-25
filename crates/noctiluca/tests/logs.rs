//! Live logs on snapshots Chrome made of the fixture pages: one log
//! built from `<ul aria-live>` and from `<div role=log>` projects the
//! same; the newest entries show, and scrolling up holds still while new
//! ones come. No browser needed (`tests/live_logs.rs` runs them live).

use noctiluca::adapt::{Adapt, Rule};
use noctiluca::layout::{log_lines, project, scroll_log, Line, LogScroll, Marks};
use noctiluca::model::{build, Entry, LogKind, Page, Revisions, Run};
use noctiluca::testing::snapshot_file;

fn page(name: &str) -> Page {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(format!("tests/snapshots/{name}.json"));
    build(&snapshot_file(&path).unwrap(), &mut Revisions::new())
}

fn plain(lines: &[Line]) -> Vec<String> {
    lines.iter().map(Line::plain).collect()
}

#[test]
fn a_log_from_a_list_and_from_divs_projects_the_same() {
    let (ul, div) = (page("log-ul"), page("log-div"));
    assert_eq!(ul.logs.len(), 1);
    assert_eq!(div.logs.len(), 1);
    assert_eq!(ul.logs[0].kind, LogKind::Live);
    assert_eq!(div.logs[0].kind, LogKind::Log);
    let texts = |p: &Page| {
        p.logs[0]
            .entries
            .iter()
            .map(Entry::text)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        texts(&ul),
        ["ada: first light", "bea: it glows :glow:", "ada: so blue"]
    );
    assert_eq!(texts(&ul), texts(&div));
    let lines = |p: &Page| project(p, 100, (8.0, 18.0), &Marks::default()).lines;
    // Every cell and every style alike: the author's colour, the emote.
    assert_eq!(lines(&ul), lines(&div));
    let shown = plain(&lines(&ul));
    assert!(
        shown.iter().any(|l| l.trim() == "ada: first light"),
        "{shown:#?}"
    );
    assert!(shown.iter().any(|l| l.trim() == "Under the log."));
}

#[test]
fn authors_keep_their_colour() {
    let page = page("log-div");
    let first = &page.logs[0].entries[0].runs[0];
    assert_eq!(first.text, "ada");
    assert!(first.bold);
    // #ff7f50, light enough to stay as it is.
    assert_eq!(first.color.as_deref(), Some("rgb(255,127,80)"));
    let emote = page.logs[0].entries[1]
        .runs
        .iter()
        .find(|r| r.picture.is_some())
        .unwrap();
    let picture = emote.picture.as_ref().unwrap();
    assert_eq!(
        (picture.alt.as_str(), picture.decorative),
        (":glow:", false)
    );
    let lines = project(&page, 100, (8.0, 18.0), &Marks::default()).lines;
    let author = lines
        .iter()
        .flat_map(|l| &l.spans)
        .find(|s| s.text == "ada")
        .unwrap();
    assert!(author.style.contains("rgb(255,127,80)"), "{author:?}");
}

/// The log of `log-div` with `n` entries, `#1` to `#n`.
fn grown(n: u64) -> Page {
    let mut page = page("log-div");
    page.logs[0].entries = (1..=n)
        .map(|k| Entry {
            key: 1000 + k,
            runs: vec![Run::plain(format!("message #{k}"))],
        })
        .collect();
    page
}

#[test]
fn the_newest_entries_show_and_scrolling_up_holds_still() {
    let page = grown(20);
    let log = &page.logs[0];
    let marks = Marks::default();
    let tail = plain(&log_lines(log, 40, 4, &page, &marks));
    assert_eq!(
        tail,
        ["message #17", "message #18", "message #19", "message #20"]
    );
    // Scrolled up five entries: #15 is the last in view.
    let paused = scroll_log(log, LogScroll::Follow, -5);
    assert_eq!(paused, LogScroll::Paused(1015));
    let mut marks = Marks::default();
    marks.logs.insert(log.node, paused);
    let held = plain(&log_lines(log, 40, 4, &page, &marks));
    assert_eq!(
        held,
        ["message #12", "message #13", "message #14", "message #15"]
    );
    // Ten more come: the paused view stays where it is.
    let more = grown(30);
    assert_eq!(plain(&log_lines(&more.logs[0], 40, 4, &more, &marks)), held);
    // Back down to the end, it follows again, and shows the newest.
    let back = scroll_log(&more.logs[0], paused, 100);
    assert_eq!(back, LogScroll::Follow);
    marks.logs.insert(log.node, back);
    assert_eq!(
        plain(&log_lines(&more.logs[0], 40, 4, &more, &marks))
            .last()
            .unwrap(),
        "message #30"
    );
}

#[test]
fn without_the_live_tail_the_log_shows_what_chrome_shows() {
    let page = grown(20);
    let log = &page.logs[0];
    let marks = Marks {
        adapt: Adapt::default().with(Rule::LiveTail, false),
        ..Marks::default()
    };
    // Chrome's box is scrolled to its top: the oldest entries.
    let mut log = log.clone();
    log.first_shown = Some(1001);
    let lines = plain(&log_lines(&log, 40, 3, &page, &marks));
    assert_eq!(lines, ["message #1", "message #2", "message #3"]);
}
