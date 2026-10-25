//! Pictures in the app, on intuiTUIve's driver over the scripted engine
//! (Chrome's saved snapshots of the fixture pages, the pictures from the
//! fixture files): the projection draws each picture over its region,
//! in Sixel, iTerm2 or Kitty, or half blocks; an uncaptioned picture gets
//! its `Image:` line; animated GIFs step within the byte budget, and
//! freezing stops their bytes.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use noctiluca::app::{app, Options};
use noctiluca::bookmarks::Bookmarks;
use noctiluca::config::{Config, Images, View};
use noctiluca::testing::FakeEngine;
use rich_intuituive::Driver;

fn snapshots() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots")
}

fn options(url: &str, images: Images) -> Options {
    Options {
        config: Config::default(),
        bookmarks: Bookmarks::default(),
        url: Some(url.to_string()),
        view: Some(View::Projection),
        images: Some(images),
    }
}

fn driver(url: &str, images: Images, columns: u16, rows: u16) -> Driver {
    let engine = FakeEngine::from_dir(&snapshots()).unwrap();
    app(Box::new(engine), options(url, images)).driver(columns, rows)
}

fn screen(driver: &Driver) -> String {
    driver.screen().plain().join("\n")
}

/// Update and render until `done` holds on what was written and the
/// screen, or `deadline` passes; what was written.
fn run_until(
    driver: &mut Driver,
    deadline: Duration,
    done: impl Fn(&str, &str) -> bool,
) -> (String, bool) {
    let start = Instant::now();
    let mut out = String::new();
    while start.elapsed() < deadline {
        driver.update(Duration::ZERO);
        out.push_str(&driver.render().unwrap_or_default());
        if done(&out, &screen(driver)) {
            return (out, true);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    (out, false)
}

#[test]
fn the_projection_draws_each_picture_over_its_region() {
    let mut driver = driver("news.html", Images::Sixel, 100, 40);
    let (out, drawn) = run_until(&mut driver, Duration::from_secs(20), |out, screen| {
        out.contains("\x1bP") && screen.contains('▀')
    });
    assert!(drawn, "no picture drawn:\n{}", screen(&driver));
    // The picture's cells are half blocks under the Sixel; its alt text
    // is under it, not in it.
    let text = screen(&driver);
    let (above, _) = text
        .split_once("Image: Boats at anchor")
        .expect("its caption");
    assert!(!above.contains('░'), "a placeholder left:\n{text}");
    assert!(out.contains("\x1bP"));
}
