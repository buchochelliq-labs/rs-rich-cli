//! A navigation that fails, on the tab's thread over the scripted engine:
//! Chrome shows its own error page at `chrome-error://chromewebdata/`; the
//! address bar keeps the address that was asked for, and a notice says
//! what failed and how to try again.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use noctiluca::engine::{HistoryStep, PageSnapshot, Rect};
use noctiluca::model::Page;
use noctiluca::tab::{Command, ERROR_NOTICE, Tab, Update};
use noctiluca::testing::{FakeEngine, PageBuilder};

/// Chrome's error page as it reads after a redirect to a bot check the
/// network refused (the YouTube `/watch` case, reduced).
fn error_page() -> PageSnapshot {
    let mut p = PageBuilder::new(
        "chrome-error://chromewebdata/",
        "www.google.com",
        800.0,
        600.0,
    );
    let body = p.body();
    let h1 = p.element(
        body,
        "H1",
        &[],
        &[],
        Some(Rect::new(100.0, 220.0, 295.0, 29.0)),
    );
    p.text(
        h1,
        "This site can’t be reached",
        Rect::new(100.0, 220.0, 295.0, 29.0),
    );
    let para = p.element(
        body,
        "P",
        &[],
        &[],
        Some(Rect::new(100.0, 268.0, 600.0, 40.0)),
    );
    p.text(
        para,
        "The webpage at https://www.google.com/sorry/index might be temporarily down.",
        Rect::new(100.0, 268.0, 600.0, 40.0),
    );
    let code = p.element(
        body,
        "DIV",
        &[],
        &[],
        Some(Rect::new(100.0, 426.0, 208.0, 15.0)),
    );
    p.text(
        code,
        "ERR_TUNNEL_CONNECTION_FAILED",
        Rect::new(100.0, 426.0, 208.0, 15.0),
    );
    p.snapshot()
}

fn ok_page() -> PageSnapshot {
    let mut p = PageBuilder::new("https://example.test/home", "Home", 800.0, 600.0);
    let body = p.body();
    let h1 = p.element(body, "H1", &[], &[], Some(Rect::new(8.0, 8.0, 200.0, 30.0)));
    p.text(h1, "Home", Rect::new(8.0, 8.0, 200.0, 30.0));
    p.snapshot()
}

/// The first page read whose address satisfies `want`, and the last state
/// seen by then.
fn wait_for(
    updates: &mpsc::Receiver<Update>,
    want: impl Fn(&Page) -> bool,
) -> (std::sync::Arc<Page>, Option<String>) {
    let start = Instant::now();
    let mut state = None;
    while start.elapsed() < Duration::from_secs(5) {
        match updates.recv_timeout(Duration::from_millis(20)) {
            Ok(Update::Page(page)) if want(&page) => {
                // Let the state that follows the reading come too.
                while let Ok(update) = updates.recv_timeout(Duration::from_millis(100)) {
                    if let Update::State(s) = update {
                        state = Some(s.url);
                    }
                }
                return (page, state);
            }
            Ok(Update::State(s)) => state = Some(s.url),
            _ => {}
        }
    }
    panic!("no such reading");
}

#[test]
fn a_failed_navigation_keeps_the_asked_address_and_says_what_failed() {
    let dir = std::env::temp_dir().join(format!("noctiluca-naverror-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let engine = FakeEngine::from_dir(&dir).unwrap();
    let fake = engine.state.clone();
    {
        let mut fake = fake.lock().unwrap();
        fake.pages.insert("home".into(), ok_page());
        // The fake names a page by the address's last segment: `/watch`
        // lands on Chrome's error page, as the redirect did.
        fake.pages.insert("watch".into(), error_page());
    }
    let (send, updates) = mpsc::channel();
    let tab = Tab::spawn(Box::new(engine), move |u| {
        let _ = send.send(u);
    });

    tab.send(Command::Open("https://example.test/home".into()));
    let (home, _) = wait_for(&updates, |p| p.url.ends_with("/home"));
    assert!(home.blocks.iter().all(|b| b.node != ERROR_NOTICE));

    let asked = "https://www.youtube.com/watch?v=abc";
    tab.send(Command::Open(asked.into()));
    let (page, state) = wait_for(&updates, |p| p.title == "www.google.com");
    // The address bar reads the address asked for, not Chrome's.
    assert_eq!(page.url, asked);
    assert_eq!(state.as_deref(), Some(asked));
    // The notice comes first: what, where, the code and the key.
    let notice = &page.blocks[0];
    assert_eq!(notice.node, ERROR_NOTICE);
    let text = notice.text();
    assert!(
        text.starts_with("Could not open https://www.youtube.com/watch?v=abc"),
        "{text}"
    );
    assert!(text.contains("ERR_TUNNEL_CONNECTION_FAILED"), "{text}");
    assert!(text.contains("Reload (R)"), "{text}");
    // Above Chrome's own text, which stays.
    assert!(notice.rect.y + notice.rect.height <= page.blocks[1].rect.y);
    assert!(
        page.blocks
            .iter()
            .any(|b| b.text() == "This site can’t be reached")
    );

    // Back on a page that loads, its own address shows again.
    tab.send(Command::History(HistoryStep::Back));
    let (back, state) = wait_for(&updates, |p| p.url.ends_with("/home"));
    assert_eq!(back.url, "https://example.test/home");
    assert_eq!(state.as_deref(), Some("https://example.test/home"));
    let _ = std::fs::remove_dir_all(&dir);
}
