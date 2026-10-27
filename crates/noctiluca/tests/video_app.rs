//! Video in the app, over the scripted engine and Chrome's saved snapshot
//! of `video.html`: no browser. The header and the status bar as the PoC
//! drew them, the play mark, the media keys, frames from the screencast
//! drawn in the video's region, and the screencast stopping when the
//! video pauses or the pictures freeze.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use noctiluca::app::{app, Options};
use noctiluca::bookmarks::Bookmarks;
use noctiluca::config::{Config, Images, View};
use noctiluca::engine::{MediaOp, PageAction, Playback};
use noctiluca::model::{build, Revisions};
use noctiluca::testing::{FakeEngine, FakeState};
use rich_embed::{FrameMeta, Pixels, ScreencastFrame};
use rich_intuituive::Driver;

const URL: &str = "https://fixture.example/video.html";
/// The video element's node in the snapshot.
const VIDEO: i64 = 20;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests")
}

fn engine() -> FakeEngine {
    let engine = FakeEngine::from_dir(&root().join("snapshots")).unwrap();
    {
        let mut state = engine.state.lock().unwrap();
        for (name, snapshot) in state.pages.iter_mut() {
            let doc = &snapshot.dom["documents"][0];
            let at = doc["documentURL"].as_u64().unwrap() as usize;
            snapshot.dom["strings"][at] = format!("https://fixture.example/{name}.html").into();
        }
        state.resource_root = Some(root().join("fixtures"));
        state.media.insert(
            VIDEO,
            Playback {
                paused: true,
                volume: 1.0,
                duration: Some(5.0),
                ready: 4,
                ..Playback::default()
            },
        );
    }
    engine
}

fn options(view: View) -> Options {
    Options {
        config: Config::default(),
        bookmarks: Bookmarks::default(),
        url: Some(URL.into()),
        view: Some(view),
        images: Some(Images::Ansi),
    }
}

fn screen(driver: &Driver) -> String {
    driver.screen().plain().join("\n")
}

fn wait(driver: &mut Driver, what: &str, ok: impl Fn(&Driver) -> bool) {
    let start = Instant::now();
    loop {
        driver.settle();
        if ok(driver) {
            return;
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "{what} never happened:\n{}",
            screen(driver)
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn wait_for(driver: &mut Driver, text: &str) {
    wait(driver, &format!("{text:?}"), |d| screen(d).contains(text));
}

fn state_is(
    driver: &mut Driver,
    state: &std::sync::Arc<std::sync::Mutex<FakeState>>,
    what: &str,
    ok: impl Fn(&FakeState) -> bool,
) {
    let start = Instant::now();
    while !ok(&state.lock().unwrap()) {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "{what} never happened:\n{}",
            screen(driver)
        );
        driver.settle();
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// A frame of the viewport, all of one colour, with the page unscrolled.
fn frame(rgb: [u8; 3]) -> ScreencastFrame {
    let (w, h) = (800, 720);
    ScreencastFrame {
        pixels: Pixels::new(w, h, rgb.repeat((w * h) as usize)).unwrap(),
        meta: FrameMeta {
            page_scale: 1.0,
            device_width: w as f64,
            device_height: h as f64,
            ..FrameMeta::default()
        },
    }
}

#[test]
fn the_header_and_status_bar_say_the_mode_the_address_and_the_media() {
    let mut driver = app(Box::new(engine()), options(View::Projection)).driver(100, 30);
    wait_for(&mut driver, "The bloom from the air");
    let lines = driver.screen().plain();
    // The header: the browser, the view and the pictures' protocol, the
    // title; then the address.
    assert!(lines[0].contains("NOCTILUCA"), "{}", lines[0]);
    assert!(lines[0].contains("PROJECTION + CELLS"), "{}", lines[0]);
    assert!(lines[0].contains("Video fixture"), "{}", lines[0]);
    assert_eq!(lines[1].trim(), URL);
    // The status bar: the media state and the row, then the keys.
    wait_for(&mut driver, "media PAUSED 0:00 / 0:05");
    let lines = driver.screen().plain();
    let status = &lines[lines.len() - 2];
    assert!(status.contains("row 1/"), "{status}");
    assert!(status.contains("sound off (--audio)"), "{status}");
    let keys = &lines[lines.len() - 1];
    assert!(keys.contains("p play/pause"), "{keys}");
    assert!(keys.contains("[/] seek"), "{keys}");
    // The play mark on the paused video.
    assert!(screen(&driver).contains(" ▶ "), "{}", screen(&driver));
}

#[test]
fn a_playing_video_shows_its_frames_and_the_keys_seek() {
    let engine = engine();
    let state = engine.state.clone();
    let page = build(&state.lock().unwrap().pages["video"], &mut Revisions::new());
    let rect = page.media.iter().find(|m| m.node == VIDEO).unwrap().rect;
    let mut driver = app(Box::new(engine), options(View::Projection)).driver(100, 40);
    wait_for(&mut driver, "media PAUSED");
    // Paused: no screencast.
    assert!(!state.lock().unwrap().screencast);
    driver.press("p");
    wait_for(&mut driver, "media PLAYING");
    // Playing on screen: the screencast runs, and its frames show in the
    // region in place of the play mark.
    state_is(&mut driver, &state, "the screencast", |s| s.screencast);
    state.lock().unwrap().frames.push(frame([255, 0, 0]));
    // The region's middle row, at its column: half blocks of the frame.
    let row = 2 + ((rect.y + rect.height / 2.0) / 18.0) as usize;
    let column = (rect.x / 8.0) as usize + 2;
    wait(&mut driver, "a red frame in the region", |d| {
        let screen = d.screen();
        let cell = screen.cell(column as u16, row as u16);
        let red = rich::Color::from_rgb(255, 0, 0);
        screen
            .style(cell.style)
            .is_some_and(|s| s.color() == Some(&red) && s.bgcolor() == Some(&red))
    });
    assert!(!screen(&driver).contains(" ▶ "), "{}", screen(&driver));
    // `]` seeks five seconds on, `[` five back.
    driver.press("]");
    driver.press("[");
    state_is(&mut driver, &state, "two seeks", |s| {
        let seeks: Vec<_> = s
            .actions
            .iter()
            .filter_map(|a| match a {
                PageAction::Media {
                    op: MediaOp::Seek(d),
                    ..
                } => Some(*d),
                _ => None,
            })
            .collect();
        seeks == [5.0, -5.0]
    });
    // Frozen: the screencast stops, the video plays on.
    driver.press("f4");
    state_is(&mut driver, &state, "the screencast off", |s| !s.screencast);
    assert!(!state.lock().unwrap().media[&VIDEO].paused);
    driver.press("f4");
    state_is(&mut driver, &state, "the screencast again", |s| {
        s.screencast
    });
    // Paused: off again, and the play mark is back.
    driver.press("p");
    wait_for(&mut driver, "media PAUSED");
    state_is(&mut driver, &state, "the screencast off", |s| !s.screencast);
    wait_for(&mut driver, " ▶ ");
}
