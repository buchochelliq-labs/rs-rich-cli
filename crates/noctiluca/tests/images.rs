//! Image stills on the tab's thread, over the scripted engine: a lazy
//! image with no picture yet gets no (blank) still, and gets one once its
//! picture loads, as when Chrome's scroll brings it near.

use std::path::Path;
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use noctiluca::engine::EngineEvent;
use noctiluca::model::{build, Page, Revisions};
use noctiluca::tab::{Command, Tab, Update};
use noctiluca::testing::FakeEngine;

#[test]
fn a_lazy_image_gets_its_still_once_its_picture_has_loaded() {
    let engine =
        FakeEngine::from_dir(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots"))
            .unwrap();
    let state = engine.state.clone();
    let node = {
        let state = state.lock().unwrap();
        let page = build(&state.pages["reader"], &mut Revisions::new());
        page.media[0].node
    };
    state.lock().unwrap().not_loaded.insert(node);
    let (send, updates) = mpsc::channel();
    let tab = Tab::spawn(Box::new(engine), move |u| {
        let _ = send.send(u);
    });
    tab.send(Command::Open("reader.html".into()));
    let still = |page: &Page| {
        page.media
            .iter()
            .any(|m| m.node == node && m.still.is_some())
    };
    let mut last: Option<Arc<Page>> = None;
    let mut until = |deadline: Duration, done: &dyn Fn(&Page) -> bool| -> bool {
        let start = Instant::now();
        while start.elapsed() < deadline {
            if let Ok(Update::Page(page)) = updates.recv_timeout(Duration::from_millis(50)) {
                last = Some(page);
            }
            if last.as_deref().is_some_and(done) {
                return true;
            }
        }
        false
    };
    assert!(until(Duration::from_secs(10), &|p: &Page| !p
        .media
        .is_empty()));
    // Not loaded: no still, however long.
    assert!(!until(Duration::from_millis(500), &still));
    // It loads (its load event pings): the still comes.
    {
        let mut state = state.lock().unwrap();
        state.not_loaded.clear();
        state.events.push(EngineEvent::Changed);
    }
    assert!(until(Duration::from_secs(10), &still), "no still");
}
