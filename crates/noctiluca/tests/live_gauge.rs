//! Live regions against a real browser: the speed gauge fixtures, drawn
//! as an inline `<svg>` (`gauge-svg.html`) and on `<canvas>`es
//! (`gauge-canvas.html`), stay live as their scripts move them. Runs only
//! when `RICH_EMBED_CHROME` names a Chrome or Chromium binary, as
//! `tests/live.rs` does; without it each test says it skipped and passes.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use noctiluca::cdp::{ChromeBackend, ChromeOptions};
use noctiluca::engine::NodeId;
use noctiluca::file_url;
use noctiluca::model::{MediaKind, Page};
use noctiluca::tab::{Command, Tab, Update};

fn chrome() -> Option<PathBuf> {
    let binary = std::env::var_os("RICH_EMBED_CHROME");
    if binary.is_none() {
        eprintln!("skipped: set RICH_EMBED_CHROME to a Chrome or Chromium binary to run");
    }
    binary.map(PathBuf::from)
}

fn fixture(name: &str) -> String {
    file_url(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name),
    )
    .unwrap()
}

/// A tab on a real browser, and what it sent so far.
struct Live {
    tab: Tab,
    updates: Receiver<Update>,
    page: Option<Arc<Page>>,
    /// Each region's frames' pixels, in order.
    frames: HashMap<NodeId, Vec<Vec<u8>>>,
}

impl Live {
    fn start(binary: PathBuf) -> Live {
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
        Live {
            tab,
            updates,
            page: None,
            frames: HashMap::new(),
        }
    }

    /// Take the updates for `time`.
    fn pump(&mut self, time: Duration) {
        let start = Instant::now();
        while start.elapsed() < time {
            match self.updates.recv_timeout(Duration::from_millis(20)) {
                Ok(Update::Page(page)) => self.page = Some(page),
                Ok(Update::Frame(frame)) => self
                    .frames
                    .entry(frame.node)
                    .or_default()
                    .push(frame.pixels.rgb.clone()),
                _ => {}
            }
        }
    }

    /// Wait up to 30 s for a page that `ok` accepts.
    fn wait(&mut self, what: &str, ok: impl Fn(&Page) -> bool) -> Arc<Page> {
        let start = Instant::now();
        loop {
            if let Some(page) = self.page.as_ref().filter(|p| ok(p)) {
                return page.clone();
            }
            assert!(
                start.elapsed() < Duration::from_secs(30),
                "timed out waiting for {what}"
            );
            self.pump(Duration::from_millis(50));
        }
    }
}

#[test]
fn an_svg_dial_is_captured_again_as_its_script_moves_it() {
    let Some(binary) = chrome() else { return };
    let mut live = Live::start(binary);
    let url = fixture("gauge-svg.html");
    live.tab.send(Command::Open(url.clone()));
    let dial = |p: &Page| p.media.iter().find(|m| m.kind == MediaKind::Svg).cloned();
    let first = live.wait("the dial's picture", |p| {
        p.url == url && dial(p).is_some_and(|d| d.still.is_some())
    });
    let first = dial(&first).unwrap();
    // The script runs for about eight seconds and stops at 287.40: the
    // number is the drawing's text, and its still is not the first one.
    let end = live.wait("the dial at its end", |p| {
        dial(p).is_some_and(|d| {
            d.text.contains("287.40")
                && d.picture.is_some()
                && d.still.as_deref() != first.still.as_deref()
        })
    });
    let end = dial(&end).unwrap();
    assert!(end.text.contains("Mbps"), "{:?}", end.text);
    assert_ne!(first.revision, end.revision);
}

#[test]
fn canvas_dials_get_pictures_and_frames_while_they_draw() {
    let Some(binary) = chrome() else { return };
    let mut live = Live::start(binary);
    let url = fixture("gauge-canvas.html");
    live.tab.send(Command::Open(url.clone()));
    let canvases = |p: &Page| -> Vec<NodeId> {
        p.media
            .iter()
            .filter(|m| m.kind == MediaKind::Canvas)
            .map(|m| m.node)
            .collect()
    };
    let page = live.wait("the canvases' pictures", |p| {
        p.url == url
            && p.media
                .iter()
                .filter(|m| m.kind == MediaKind::Canvas)
                .filter(|m| m.picture.is_some())
                .count()
                == 3
    });
    let opened = Instant::now();
    let nodes = canvases(&page);
    live.tab.send(Command::Regions {
        on_screen: nodes.clone(),
        intervals: Vec::new(),
        scale: 1.0,
    });
    // While the script draws (about eight seconds): frames, each one new.
    live.pump(Duration::from_secs(3));
    for node in &nodes {
        let frames = live.frames.get(node).map_or(0, Vec::len);
        assert!(
            frames >= 2,
            "canvas {node} sent {frames} frames while drawing"
        );
        let all = &live.frames[node];
        assert!(all.windows(2).all(|w| w[0] != w[1]), "a frame repeated");
    }
    // Once it has stopped, so do the frames.
    live.pump(Duration::from_secs(11).saturating_sub(opened.elapsed()));
    let sent: usize = live.frames.values().map(Vec::len).sum();
    live.pump(Duration::from_secs(3));
    let after: usize = live.frames.values().map(Vec::len).sum();
    assert_eq!(after, sent, "frames from canvases holding still");
}

#[test]
fn a_canvas_gauge_with_a_meter_role_is_drawn_as_intuituives_dial() {
    let Some(binary) = chrome() else { return };
    let mut live = Live::start(binary);
    let url = fixture("gauge-canvas.html");
    live.tab.send(Command::Open(url.clone()));
    // The script moves the meter's value to its end, 95.8 Mbps of 500.
    let page = live.wait("the meter at its end", |p| {
        p.url == url
            && p.indicators
                .iter()
                .any(|i| i.label == "Upload" && i.value.is_some_and(|v| v > 95.0))
    });
    let upload = page
        .indicators
        .iter()
        .find(|i| i.label == "Upload")
        .unwrap();
    assert!(upload.media.is_some(), "the canvas is the meter");
    let lines: Vec<String> = noctiluca::layout::project(
        &page,
        100,
        (8.0, 16.0),
        &noctiluca::layout::Marks::default(),
    )
    .lines
    .iter()
    .map(noctiluca::layout::Line::plain)
    .collect();
    let braille = |c: char| ('\u{2801}'..='\u{28ff}').contains(&c);
    assert!(
        lines.iter().any(|l| l.chars().any(braille)),
        "the dial's arc: {lines:#?}"
    );
    assert!(lines.iter().any(|l| l.contains("95.8 Mbps")), "{lines:#?}");
    assert!(lines.iter().any(|l| l.contains("500")), "the range's end");
}
