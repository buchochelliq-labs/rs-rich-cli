//! A new document's layout settling, on the tab's thread over the
//! scripted engine: the first reading can come while the layout still
//! moves (web fonts, a header a script puts in place, all with no DOM
//! mutation to ping), so the tab reads a new document again until its
//! boxes hold still, and publishes what it finds.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use noctiluca::engine::{PageSnapshot, Rect};
use noctiluca::model::{BlockKind, Page};
use noctiluca::tab::{Command, SETTLE_MAX, SETTLE_STEP, Tab, Update};
use noctiluca::testing::{FakeEngine, PageBuilder};

/// `tests/fixtures/settle.html` as Chrome lays it out at 800 CSS pixels:
/// before its script runs, the header over the code (`settled` false),
/// and after, the header in its own 60 pixels and the code under it.
fn settle(settled: bool) -> PageSnapshot {
    let mut p = PageBuilder::new(
        "file:///noctiluca/fixtures/settle.html",
        "Settle fixture",
        800.0,
        200.0,
    );
    let body = p.body();
    let (header_y, code_y) = if settled { (0.0, 80.0) } else { (60.0, 60.0) };
    let header = p.element(
        body,
        "HEADER",
        &[],
        &[("position", if settled { "static" } else { "absolute" })],
        Some(Rect::new(0.0, header_y, 800.0, 60.0)),
    );
    let h1 = p.element(
        header,
        "H1",
        &[],
        &[("font-weight", "700")],
        Some(Rect::new(0.0, header_y + 11.0, 800.0, 37.0)),
    );
    p.text(
        h1,
        "Array.prototype.map()",
        Rect::new(0.0, header_y + 11.0, 330.0, 37.0),
    );
    let pre = p.element(
        body,
        "PRE",
        &[],
        &[("white-space", "pre")],
        Some(Rect::new(8.0, code_y, 784.0, 18.0)),
    );
    p.text(
        pre,
        "const doubled = [1, 2, 3].map((x) => x * 2);",
        Rect::new(8.0, code_y, 380.0, 18.0),
    );
    p.snapshot()
}

fn heading_y(page: &Page) -> Option<f64> {
    page.blocks
        .iter()
        .find(|b| matches!(b.kind, BlockKind::Heading(_)))
        .map(|b| b.rect.y)
}

#[test]
fn a_new_documents_layout_is_read_again_until_it_holds_still() {
    let dir = std::env::temp_dir().join(format!("noctiluca-settle-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let engine = FakeEngine::from_dir(&dir).unwrap();
    let state = engine.state.clone();
    state
        .lock()
        .unwrap()
        .pages
        .insert("settle".into(), settle(false));
    let (send, updates) = mpsc::channel();
    let tab = Tab::spawn(Box::new(engine), move |u| {
        let _ = send.send(u);
    });
    tab.send(Command::Open("settle.html".into()));
    let next_page = |deadline: Duration| {
        let start = Instant::now();
        while start.elapsed() < deadline {
            if let Ok(Update::Page(page)) = updates.recv_timeout(Duration::from_millis(20)) {
                return Some(page);
            }
        }
        None
    };
    // The first reading: the header over the code.
    let first = next_page(Duration::from_secs(5)).expect("a first reading");
    assert_eq!(heading_y(&first), Some(71.0));
    // The page's script moves the header with no DOM change, so no ping.
    state
        .lock()
        .unwrap()
        .pages
        .insert("settle".into(), settle(true));
    let settled = next_page(SETTLE_MAX).expect("the settled layout was never read");
    assert_eq!(heading_y(&settled), Some(11.0));
    // Once still, nothing new is published.
    assert!(next_page(Duration::from_millis(800)).is_none());
}

#[test]
fn a_move_after_a_quiet_first_step_is_still_read() {
    let dir = std::env::temp_dir().join(format!("noctiluca-settle-late-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let engine = FakeEngine::from_dir(&dir).unwrap();
    let state = engine.state.clone();
    state
        .lock()
        .unwrap()
        .pages
        .insert("settle".into(), settle(false));
    let (send, updates) = mpsc::channel();
    let tab = Tab::spawn(Box::new(engine), move |u| {
        let _ = send.send(u);
    });
    tab.send(Command::Open("settle.html".into()));
    let start = Instant::now();
    let first = loop {
        assert!(start.elapsed() < Duration::from_secs(5), "no first reading");
        if let Ok(Update::Page(page)) = updates.recv_timeout(Duration::from_millis(20)) {
            break page;
        }
    };
    assert_eq!(heading_y(&first), Some(71.0));
    // The layout holds still for longer than a step, then moves: a script
    // that ran a little after the first reading.
    std::thread::sleep(SETTLE_STEP * 2);
    state
        .lock()
        .unwrap()
        .pages
        .insert("settle".into(), settle(true));
    let start = Instant::now();
    let settled = loop {
        assert!(start.elapsed() < SETTLE_MAX, "the late move was never read");
        if let Ok(Update::Page(page)) = updates.recv_timeout(Duration::from_millis(20)) {
            break page;
        }
    };
    assert_eq!(heading_y(&settled), Some(11.0));
}
