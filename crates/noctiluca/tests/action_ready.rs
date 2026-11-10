//! An action waits for a reading the engine refuses for a moment: Chrome
//! refuses a snapshot while the viewport's metrics are being applied (or
//! the document or viewport changes during the read), and a hint pressed
//! in that window used to be refused as "The page is not ready.".

use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use noctiluca::engine::{
    Engine, EngineEvent, Format, Frame, HistoryStep, PageAction, PageSnapshot, Rect, TabId,
};
use noctiluca::model::Role;
use noctiluca::tab::{Command, Intent, Tab, Update};
use noctiluca::testing::FakeEngine;

/// The scripted engine, refusing the next `refusals` snapshots as Chrome
/// does while it applies new viewport metrics.
struct Settling {
    inner: FakeEngine,
    refusals: Arc<AtomicUsize>,
}

impl Engine for Settling {
    fn resize(&mut self, columns: u16, rows: u16) -> io::Result<()> {
        self.inner.resize(columns, rows)
    }
    fn open(&mut self, tab: TabId, url: &str) -> io::Result<()> {
        self.inner.open(tab, url)
    }
    fn history(&mut self, tab: TabId, step: HistoryStep) -> io::Result<()> {
        self.inner.history(tab, step)
    }
    fn state(&self, tab: TabId) -> rich_embed::PageState {
        self.inner.state(tab)
    }
    fn snapshot(&mut self, tab: TabId) -> io::Result<PageSnapshot> {
        let mut left = self.refusals.load(Ordering::SeqCst);
        let refused = loop {
            if left == 0 {
                break false;
            }
            match self
                .refusals
                .compare_exchange(left, left - 1, Ordering::SeqCst, Ordering::SeqCst)
            {
                Ok(_) => break true,
                Err(now) => left = now,
            }
        };
        if refused {
            return Err(io::Error::other("viewport metrics not applied yet"));
        }
        self.inner.snapshot(tab)
    }
    fn capture(&mut self, tab: TabId, region: Rect, format: Format) -> io::Result<Frame> {
        self.inner.capture(tab, region, format)
    }
    fn act(&mut self, tab: TabId, action: &PageAction) -> io::Result<()> {
        self.inner.act(tab, action)
    }
    fn events(&mut self) -> Vec<EngineEvent> {
        self.inner.events()
    }
}

#[test]
fn a_click_waits_while_the_engine_refuses_a_reading_for_a_moment() {
    let engine =
        FakeEngine::from_dir(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots"))
            .unwrap();
    let state = engine.state.clone();
    let refusals = Arc::new(AtomicUsize::new(0));
    let (send, updates) = mpsc::channel();
    let tab = Tab::spawn(
        Box::new(Settling {
            inner: engine,
            refusals: refusals.clone(),
        }),
        move |u| {
            let _ = send.send(u);
        },
    );
    tab.send(Command::Open("reader.html".into()));
    let start = Instant::now();
    let (page, link) = loop {
        assert!(start.elapsed() < Duration::from_secs(10), "no page");
        if let Ok(Update::Page(page)) = updates.recv_timeout(Duration::from_millis(50))
            && let Some(link) = page.controls.iter().find(|c| c.role == Role::Link).cloned()
        {
            break (page, link);
        }
    };
    // The next readings are refused, as while new viewport metrics are
    // applied; the click is asked for meanwhile.
    refusals.store(4, Ordering::SeqCst);
    let id = tab.send(Command::Act {
        binding: link.binding(&page),
        intent: Intent::Click,
    });
    let start = Instant::now();
    let result = loop {
        assert!(start.elapsed() < Duration::from_secs(10), "no answer");
        if let Ok(Update::Done(done, result)) = updates.recv_timeout(Duration::from_millis(50))
            && done == id
        {
            break result;
        }
    };
    assert!(result.is_ok(), "{result:?}");
    assert!(
        state
            .lock()
            .unwrap()
            .actions
            .iter()
            .any(|a| matches!(a, PageAction::Click(_))),
        "the click did not reach the page"
    );
}
