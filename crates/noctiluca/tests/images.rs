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

/// The tab's pages, as they come, until `done` holds or `deadline` passes.
fn wait_for(
    updates: &mpsc::Receiver<Update>,
    deadline: Duration,
    done: impl Fn(&Page) -> bool,
) -> Option<Arc<Page>> {
    let start = Instant::now();
    let mut last: Option<Arc<Page>> = None;
    while start.elapsed() < deadline {
        if let Ok(Update::Page(page)) = updates.recv_timeout(Duration::from_millis(50)) {
            last = Some(page);
        }
        if last.as_deref().is_some_and(&done) {
            return last;
        }
    }
    None
}

#[test]
fn pictures_are_decoded_from_the_files_the_page_loaded() {
    let engine =
        FakeEngine::from_dir(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots"))
            .unwrap();
    let (send, updates) = mpsc::channel();
    let tab = Tab::spawn(Box::new(engine), move |u| {
        let _ = send.send(u);
    });
    tab.send(Command::Open("news.html".into()));
    let images = |p: &Page| {
        p.media
            .iter()
            .filter(|m| m.kind == noctiluca::model::MediaKind::Image)
            .count()
    };
    let page = wait_for(&updates, Duration::from_secs(20), |p| {
        images(p) >= 5
            && p.media
                .iter()
                .filter(|m| m.kind == noctiluca::model::MediaKind::Image)
                .all(|m| m.picture.is_some())
    })
    .expect("every picture decoded");
    let by_src = |name: &str| {
        page.media
            .iter()
            .find(|m| m.src.ends_with(name))
            .unwrap_or_else(|| panic!("no {name}"))
    };
    // The original file, not Chrome's pixels of its box: its own size.
    let harbour = by_src("harbour.jpg").picture.clone().unwrap();
    assert_eq!(
        (harbour.image().width(), harbour.image().height()),
        (640, 360)
    );
    // The GIF keeps its frames, for Kitty to play or us to step.
    let tide = by_src("tide.gif").picture.clone().unwrap();
    assert!(tide.is_animated());
    assert_eq!(tide.frames().map(<[_]>::len), Some(12));
    // A figure's caption is the page's own; the others get ours.
    assert!(by_src("market.jpg").captioned);
    assert!(!by_src("storm.jpg").captioned);
}

#[test]
fn a_picture_in_a_format_we_cannot_decode_is_taken_from_chromes_pixels() {
    // The Guardian serves AVIF to Chrome (it asks for it). rs-rich-art does
    // not read AVIF, so the picture is Chrome's pixels of its box, not a
    // picture that never comes.
    let engine =
        FakeEngine::from_dir(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots"))
            .unwrap();
    let state = engine.state.clone();
    let harbour = {
        let state = state.lock().unwrap();
        let page = build(&state.pages["news"], &mut Revisions::new());
        let media = page
            .media
            .iter()
            .find(|m| m.src.ends_with("harbour.jpg"))
            .expect("harbour.jpg")
            .clone();
        media
    };
    // An AVIF file's head: recognised as AVIF, not readable here.
    let mut avif = b"\0\0\0\x1cftypavif\0\0\0\0avifmif1miaf".to_vec();
    avif.resize(256, 0);
    assert_eq!(
        image::guess_format(&avif).ok(),
        Some(image::ImageFormat::Avif)
    );
    state
        .lock()
        .unwrap()
        .resources
        .insert(harbour.src.clone(), avif);
    let (send, updates) = mpsc::channel();
    let tab = Tab::spawn(Box::new(engine), move |u| {
        let _ = send.send(u);
    });
    tab.send(Command::Open("news.html".into()));
    let page = wait_for(&updates, Duration::from_secs(20), |p| {
        p.media
            .iter()
            .any(|m| m.src == harbour.src && m.picture.is_some())
    })
    .expect("the AVIF picture never came");
    let picture = page
        .media
        .iter()
        .find(|m| m.src == harbour.src)
        .and_then(|m| m.picture.clone())
        .unwrap();
    // Chrome's pixels of its box (the fake paints the box's size), not
    // the 640x360 file.
    assert_eq!(
        (picture.image().width(), picture.image().height()),
        (
            harbour.rect.width.max(1.0) as u32,
            harbour.rect.height.max(1.0) as u32
        )
    );
}
