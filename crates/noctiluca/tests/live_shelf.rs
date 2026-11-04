//! A shelf paged in a real browser: `product-grid.html`'s "Customers also
//! viewed" scrolls sideways when paged, and the page read again shows its
//! later items. Runs only when `RICH_EMBED_CHROME` names a Chrome or
//! Chromium binary, as `tests/live.rs` does; without it the test says it
//! skipped and passes.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use noctiluca::cdp::{ChromeBackend, ChromeOptions};
use noctiluca::file_url;
use noctiluca::model::Page;
use noctiluca::shelf::page_by;
use noctiluca::tab::{Command, Tab, Update};

fn chrome() -> Option<PathBuf> {
    let binary = std::env::var_os("RICH_EMBED_CHROME");
    if binary.is_none() {
        eprintln!("skipped: set RICH_EMBED_CHROME to a Chrome or Chromium binary to run");
    }
    binary.map(PathBuf::from)
}

/// Wait up to 30 s for a page that `ok` accepts.
fn wait(updates: &Receiver<Update>, what: &str, ok: impl Fn(&Page) -> bool) -> Arc<Page> {
    let start = Instant::now();
    loop {
        if let Ok(Update::Page(page)) = updates.recv_timeout(Duration::from_millis(50))
            && ok(&page)
        {
            return page;
        }
        assert!(
            start.elapsed() < Duration::from_secs(30),
            "timed out waiting for {what}"
        );
    }
}

#[test]
fn a_shelf_paged_scrolls_its_box_and_shows_its_later_items() {
    let Some(binary) = chrome() else { return };
    let backend = ChromeBackend::start(ChromeOptions {
        binary: Some(binary),
        columns: 100,
        rows: 40,
        ..ChromeOptions::default()
    })
    .unwrap();
    let (send, updates) = mpsc::channel();
    let tab = Tab::spawn(Box::new(backend), move |u| {
        let _ = send.send(u);
    });
    let url = file_url(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join("product-grid.html"),
    )
    .unwrap();
    tab.send(Command::Open(url.clone()));
    let page = wait(&updates, "the shelf", |p| {
        p.url == url && !p.shelves.is_empty()
    });
    let shelf = page.shelves[0].clip_to_screen();
    assert_eq!(shelf.shown().0, 0);
    assert_eq!(shelf.can_page(), (false, true));
    let by = page_by(&shelf, true);
    tab.send(Command::ScrollBox {
        document: page.document,
        node: shelf.node,
        by,
    });
    let paged = wait(&updates, "the shelf paged", |p| {
        p.shelves.first().is_some_and(|s| s.scroll_left > 0.0)
    });
    let paged = paged.shelves[0].clip_to_screen();
    // The fixture's box scrolls 300 px at most: a page reaches its end,
    // and the items that were cut or out of the box come into it.
    let most = paged.scroll_width - paged.client_width;
    assert!(
        (paged.scroll_left - by.min(most)).abs() < 2.0,
        "scrolled by a page: {} of {by}",
        paged.scroll_left
    );
    assert_eq!(paged.can_page(), (true, false));
    let (first, count) = paged.shown();
    assert!(
        first > 0 && first + count > shelf.shown().1,
        "{:?}",
        paged.shown()
    );
}

/// What a 100-column screen of 8 px cells shows of a shelf.
trait Screen {
    fn clip_to_screen(&self) -> noctiluca::model::Shelf;
}

impl Screen for noctiluca::model::Shelf {
    fn clip_to_screen(&self) -> noctiluca::model::Shelf {
        self.clipped(100.0 * 8.0)
    }
}
