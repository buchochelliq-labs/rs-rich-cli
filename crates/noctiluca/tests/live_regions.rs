//! Live regions over the scripted engine: a drawing the page changes is
//! drawn again. An inline `<svg>`'s text and title are read into the
//! model and its still is captured again when its content changes
//! (`tests/fixtures/gauge-svg.html`, reduced); a `<canvas>` gets a still,
//! and while the view shows it, frames when its pixels change, at most
//! four canvases at once (`tests/fixtures/gauge-canvas.html`, reduced).

use std::collections::HashSet;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use noctiluca::engine::{EngineEvent, NodeId, PageSnapshot, Rect};
use noctiluca::layout::{Line, Marks, Piece, reader};
use noctiluca::model::{MediaKind, Page, Revisions, build};
use noctiluca::tab::{Command, Tab, Update};
use noctiluca::testing::{FakeEngine, FakeState, PageBuilder};

/// `gauge-svg.html` as Chrome lays it out, reading `value`: a dial drawn
/// as an inline `<svg>` with its tick labels and the number as `<text>`,
/// the needle turned by a `transform`, and a `<title>` when `title` is
/// given.
fn gauge_svg(value: &str, turn: &str, title: Option<&str>) -> PageSnapshot {
    let mut p = PageBuilder::new(
        "file:///noctiluca/fixtures/gauge-svg.html",
        "Speed gauge (SVG) fixture",
        800.0,
        400.0,
    );
    let body = p.body();
    let h1 = p.element(body, "H1", &[], &[], Some(Rect::new(8.0, 0.0, 784.0, 37.0)));
    p.text(h1, "Speed test", Rect::new(8.0, 0.0, 180.0, 37.0));
    let svg = p.element(
        body,
        "svg",
        &[("id", "dial"), ("width", "300"), ("height", "240")],
        &[("display", "inline")],
        Some(Rect::new(8.0, 60.0, 300.0, 240.0)),
    );
    if let Some(title) = title {
        let t = p.element(svg, "title", &[], &[], None);
        p.text(t, title, Rect::default());
    }
    p.element(
        svg,
        "path",
        &[("d", "M 45 200 A 120 120 0 1 1 255 200")],
        &[],
        Some(Rect::new(53.0, 80.0, 210.0, 200.0)),
    );
    let ticks = p.element(
        svg,
        "g",
        &[("font-size", "12")],
        &[],
        Some(Rect::new(38.0, 85.0, 240.0, 200.0)),
    );
    for (x, label) in [(40.0, "0"), (150.0, "50"), (260.0, "500")] {
        let t = p.element(
            ticks,
            "text",
            &[],
            &[],
            Some(Rect::new(x, 270.0, 20.0, 14.0)),
        );
        p.text(t, label, Rect::new(x, 270.0, 20.0, 14.0));
    }
    p.element(
        svg,
        "line",
        &[("id", "needle"), ("transform", turn)],
        &[],
        Some(Rect::new(150.0, 100.0, 4.0, 90.0)),
    );
    let number = p.element(
        svg,
        "text",
        &[("id", "dial-value")],
        &[],
        Some(Rect::new(110.0, 230.0, 90.0, 40.0)),
    );
    p.text(number, value, Rect::new(110.0, 230.0, 90.0, 40.0));
    let unit = p.element(
        svg,
        "text",
        &[],
        &[],
        Some(Rect::new(140.0, 270.0, 30.0, 14.0)),
    );
    p.text(unit, "Mbps", Rect::new(140.0, 270.0, 30.0, 14.0));
    p.snapshot()
}

/// `count` canvases of `width` by `height`, one under another from the
/// top of a page 800 pixels wide.
fn canvases(count: usize, width: f64, height: f64) -> PageSnapshot {
    let mut p = PageBuilder::new(
        "file:///noctiluca/fixtures/gauge-canvas.html",
        "Speed gauge (canvas) fixture",
        800.0,
        40.0 + (height + 10.0) * count as f64,
    );
    let body = p.body();
    let h1 = p.element(body, "H1", &[], &[], Some(Rect::new(8.0, 0.0, 784.0, 30.0)));
    p.text(h1, "Speed test", Rect::new(8.0, 0.0, 180.0, 30.0));
    for k in 0..count {
        p.element(
            body,
            "CANVAS",
            &[("width", "240"), ("height", "200")],
            &[("display", "inline")],
            Some(Rect::new(
                8.0,
                40.0 + (height + 10.0) * k as f64,
                width,
                height,
            )),
        );
    }
    p.snapshot()
}

/// A tab over a scripted engine showing `page` as `name`, opened.
fn open(name: &str, page: PageSnapshot) -> (Tab, Receiver<Update>, Arc<Mutex<FakeState>>) {
    let dir = std::env::temp_dir().join(format!("noctiluca-live-regions-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let engine = FakeEngine::from_dir(&dir).unwrap();
    let state = engine.state.clone();
    state.lock().unwrap().pages.insert(name.into(), page);
    let (send, updates) = mpsc::channel();
    let tab = Tab::spawn(Box::new(engine), move |u| {
        let _ = send.send(u);
    });
    tab.send(Command::Resize(100, 40));
    tab.send(Command::Open(format!("{name}.html")));
    (tab, updates, state)
}

/// Every update until `deadline`, or until `done` holds of the pages and
/// frames so far.
#[derive(Default)]
struct Seen {
    pages: Vec<Arc<Page>>,
    frames: Vec<(NodeId, Vec<u8>)>,
    stills: Vec<NodeId>,
}

impl Seen {
    fn until(
        &mut self,
        updates: &Receiver<Update>,
        deadline: Duration,
        done: impl Fn(&Seen) -> bool,
    ) -> bool {
        let start = Instant::now();
        while start.elapsed() < deadline {
            match updates.recv_timeout(Duration::from_millis(20)) {
                Ok(Update::Page(page)) => self.pages.push(page),
                Ok(Update::Frame(frame)) => self
                    .frames
                    .push((frame.node, frame.pixels.rgb[..3].to_vec())),
                Ok(Update::Still(node)) => self.stills.push(node),
                _ => {}
            }
            if done(self) {
                return true;
            }
        }
        false
    }

    fn page(&self) -> Option<&Page> {
        self.pages.last().map(|p| &**p)
    }
}

#[test]
fn an_svgs_text_and_title_are_read_into_the_model() {
    let page = build(
        &gauge_svg("287.40", "rotate(20 150 140)", Some("Download speed")),
        &mut Revisions::new(),
    );
    let dial = page
        .media
        .iter()
        .find(|m| m.kind == MediaKind::Svg)
        .expect("the dial");
    // The tick labels, the number and the unit, in document order.
    assert_eq!(dial.text, "0 50 500 287.40 Mbps");
    // No name of its own (no AX tree here): its <title> is its name.
    assert_eq!(dial.alt, "Download speed");
    // With no title, its words stand for it.
    let untitled = build(
        &gauge_svg("287.40", "rotate(20 150 140)", None),
        &mut Revisions::new(),
    );
    assert_eq!(untitled.media[0].alt, "0 50 500 287.40 Mbps");
    // The words are in the dump, and in reader mode under its name.
    let dump = noctiluca::dump::json_of(&page);
    assert_eq!(dump["media"][0]["text"], "0 50 500 287.40 Mbps");
    let lines: Vec<String> = reader(&page, 80, &Marks::default())
        .iter()
        .filter_map(|piece| match piece {
            Piece::Text(lines) => Some(lines.iter().map(Line::plain).collect::<Vec<_>>()),
            _ => None,
        })
        .flatten()
        .collect();
    let at = lines
        .iter()
        .position(|l| l == "drawing: Download speed")
        .expect("the drawing's name");
    assert_eq!(lines[at + 1], "0 50 500 287.40 Mbps");
}

#[test]
fn an_svgs_revision_follows_its_content() {
    let revision = |value: &str, turn: &str| {
        build(&gauge_svg(value, turn, None), &mut Revisions::new()).media[0].revision
    };
    let at_rest = revision("0.00", "rotate(-135 150 140)");
    assert_eq!(at_rest, revision("0.00", "rotate(-135 150 140)"));
    // The number changed; the needle turned with the number unchanged.
    assert_ne!(at_rest, revision("287.40", "rotate(-135 150 140)"));
    assert_ne!(at_rest, revision("0.00", "rotate(20 150 140)"));
    // Other media keep 0.
    let page = build(&canvases(1, 240.0, 200.0), &mut Revisions::new());
    assert_eq!(page.media[0].revision, 0);
    assert!(page.media[0].text.is_empty());
}

#[test]
fn a_changed_svg_drawing_is_captured_again_and_its_picture_stays_meanwhile() {
    let (_tab, updates, state) = open("gauge-svg", gauge_svg("0.00", "rotate(-135 150 140)", None));
    let mut seen = Seen::default();
    let dial = |p: &Page| p.media.iter().find(|m| m.kind == MediaKind::Svg).cloned();
    assert!(
        seen.until(&updates, Duration::from_secs(10), |s| {
            s.page().and_then(dial).is_some_and(|d| d.picture.is_some())
        }),
        "the dial never got a picture"
    );
    let first = seen.page().and_then(dial).unwrap();
    let pictured = seen.pages.len();
    // The script moves the dial: the number, the needle, Chrome's pixels.
    {
        let mut state = state.lock().unwrap();
        state.paint = Some([200, 40, 40]);
        state.pages.insert(
            "gauge-svg".into(),
            gauge_svg("287.40", "rotate(20 150 140)", None),
        );
        state.events.push(EngineEvent::Changed);
    }
    assert!(
        seen.until(&updates, Duration::from_secs(10), |s| {
            s.page().and_then(dial).is_some_and(|d| {
                d.text.contains("287.40")
                    && d.still.as_deref() != first.still.as_deref()
                    && d.picture.is_some()
                    && d.picture.as_ref().map(|p| p.id()) != first.picture.as_ref().map(|p| p.id())
            })
        }),
        "the dial was not captured again: {:?}",
        seen.page().and_then(dial).map(|d| d.text)
    );
    // Its old picture showed until the new one came: never a gap.
    for page in &seen.pages[pictured..] {
        assert!(
            dial(page).is_some_and(|d| d.picture.is_some() && d.still.is_some()),
            "the dial lost its picture while it was captured again"
        );
    }
}

/// Paint a new colour into every capture every 100 ms, `times` times, on a
/// thread of its own: a canvas's script drawing.
fn draw(state: &Arc<Mutex<FakeState>>, times: u8) -> std::thread::JoinHandle<()> {
    let state = state.clone();
    std::thread::spawn(move || {
        for k in 0..times {
            state.lock().unwrap().paint = Some([k.wrapping_mul(17), 100, 200]);
            std::thread::sleep(Duration::from_millis(100));
        }
    })
}

fn canvas_nodes(page: &Page) -> Vec<NodeId> {
    page.media
        .iter()
        .filter(|m| m.kind == MediaKind::Canvas)
        .map(|m| m.node)
        .collect()
}

#[test]
fn a_canvas_gets_a_picture_and_frames_while_the_view_shows_it_and_it_changes() {
    let (tab, updates, state) = open("gauge-canvas", canvases(1, 240.0, 200.0));
    let mut seen = Seen::default();
    assert!(
        seen.until(&updates, Duration::from_secs(10), |s| {
            s.page()
                .is_some_and(|p| p.media.iter().any(|m| m.picture.is_some()))
        }),
        "the canvas never got a picture"
    );
    let node = canvas_nodes(seen.page().unwrap())[0];
    // Not on screen: it changes, and nothing is captured.
    let drawing = draw(&state, 5);
    let captured = state.lock().unwrap().captures.len();
    seen.until(&updates, Duration::from_millis(700), |_| false);
    drawing.join().unwrap();
    assert!(seen.frames.is_empty(), "frames for a canvas off screen");
    assert_eq!(state.lock().unwrap().captures.len(), captured);
    // On screen while it draws: frames, each different from the last.
    tab.send(Command::Regions {
        on_screen: vec![node],
        intervals: Vec::new(),
        scale: 1.0,
    });
    let drawing = draw(&state, 15);
    assert!(
        seen.until(&updates, Duration::from_secs(5), |s| s.frames.len() >= 3),
        "a changing canvas on screen sent {} frames",
        seen.frames.len()
    );
    drawing.join().unwrap();
    for pair in seen.frames.windows(2) {
        assert_ne!(pair[0].1, pair[1].1, "a frame the same as the last");
    }
    assert!(seen.frames.iter().all(|(n, _)| *n == node));
    // It holds still: once the last change is in, no more frames.
    seen.until(&updates, Duration::from_millis(700), |_| false);
    let sent = seen.frames.len();
    seen.until(&updates, Duration::from_millis(1500), |_| false);
    assert_eq!(seen.frames.len(), sent, "frames for a canvas holding still");
    // Off screen again: it stops, and is captured no more.
    tab.send(Command::Regions {
        on_screen: Vec::new(),
        intervals: Vec::new(),
        scale: 1.0,
    });
    assert!(
        seen.until(&updates, Duration::from_secs(2), |s| s
            .stills
            .contains(&node)),
        "the canvas was not stopped"
    );
    let captured = state.lock().unwrap().captures.len();
    let drawing = draw(&state, 8);
    seen.until(&updates, Duration::from_millis(1000), |_| false);
    drawing.join().unwrap();
    assert_eq!(state.lock().unwrap().captures.len(), captured);
}

#[test]
fn at_most_four_canvases_are_live_nearest_the_view() {
    let (tab, updates, state) = open("gauge-canvas", canvases(6, 120.0, 60.0));
    let mut seen = Seen::default();
    assert!(
        seen.until(&updates, Duration::from_secs(10), |s| {
            s.page()
                .is_some_and(|p| p.media.len() == 6 && p.media.iter().all(|m| m.picture.is_some()))
        }),
        "the canvases never got pictures"
    );
    let nodes = canvas_nodes(seen.page().unwrap());
    tab.send(Command::Regions {
        on_screen: nodes.clone(),
        intervals: Vec::new(),
        scale: 1.0,
    });
    let drawing = draw(&state, 20);
    seen.until(&updates, Duration::from_secs(2), |_| false);
    drawing.join().unwrap();
    let live: HashSet<NodeId> = seen.frames.iter().map(|(n, _)| *n).collect();
    // The four at the top (Chrome's view is at the top), not the others.
    assert_eq!(live, nodes[..4].iter().copied().collect::<HashSet<_>>());
}

#[test]
fn a_big_canvas_that_changes_all_the_time_is_cut_from_the_screencast() {
    // Bigger than a quarter of the 800 by 720 viewport.
    let (tab, updates, state) = open("gauge-canvas", canvases(1, 600.0, 400.0));
    let mut seen = Seen::default();
    assert!(
        seen.until(&updates, Duration::from_secs(10), |s| {
            s.page()
                .is_some_and(|p| p.media.iter().any(|m| m.picture.is_some()))
        }),
        "the canvas never got a picture"
    );
    let node = canvas_nodes(seen.page().unwrap())[0];
    tab.send(Command::Regions {
        on_screen: vec![node],
        intervals: Vec::new(),
        scale: 1.0,
    });
    assert!(!state.lock().unwrap().screencast);
    // Changing at every capture for longer than two seconds.
    let drawing = draw(&state, 40);
    let start = Instant::now();
    let mut cast = false;
    while start.elapsed() < Duration::from_secs(4) && !cast {
        seen.until(&updates, Duration::from_millis(50), |_| false);
        cast = state.lock().unwrap().screencast;
    }
    assert!(cast, "the busy canvas never went to the screencast");
    drawing.join().unwrap();
    // It holds still: it goes back to being captured now and then, and
    // the screencast stops.
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(5) && cast {
        seen.until(&updates, Duration::from_millis(50), |_| false);
        cast = state.lock().unwrap().screencast;
    }
    assert!(!cast, "the screencast ran on for a canvas holding still");
}
