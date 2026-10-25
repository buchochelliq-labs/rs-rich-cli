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
    options_with(url, images, Config::default())
}

fn options_with(url: &str, images: Images, config: Config) -> Options {
    Options {
        config,
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

/// The news page, tall enough that its animated GIF is in view, with
/// `bandwidth` bytes a second for moving pictures.
fn news_with_gif(images: Images, bandwidth: u64) -> Driver {
    let engine = FakeEngine::from_dir(&snapshots()).unwrap();
    let config = Config {
        bandwidth: Some(bandwidth),
        ..Config::default()
    };
    let mut driver =
        app(Box::new(engine), options_with("news.html", images, config)).driver(100, 90);
    // Every picture drawn, the GIF among them.
    let (_, ready) = run_until(&mut driver, Duration::from_secs(30), |_, screen| {
        screen.contains("Why the tides") && screen.contains("Image: Lightning")
    });
    assert!(ready, "{}", screen(&driver));
    // Its first pictures out: what follows is the GIF's.
    run_until(&mut driver, Duration::from_millis(1500), |_, _| false);
    driver
}

/// Press `name` without drawing, so what it makes the app write is in
/// the next [`written_for`].
fn key(driver: &mut Driver, name: &str) {
    for key in rich_intuituive::interact::keymap::keys(name) {
        driver.event(rich_intuituive::interact::Event::Key(key));
    }
}

/// Everything written over `time`, updating every 10 ms.
fn written_for(driver: &mut Driver, time: Duration) -> String {
    run_until(driver, time, |_, _| false).0
}

fn screen(driver: &Driver) -> String {
    driver.screen().plain().join("\n")
}

/// The real time since the first call: the app's clock, which the
/// driver takes as an absolute time.
fn clock() -> Duration {
    static BORN: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    BORN.get_or_init(Instant::now).elapsed()
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
        // The app's clock follows the real one, so its timers fire.
        driver.update(clock());
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

#[test]
fn an_animated_gif_moves_within_its_budget() {
    let mut driver = news_with_gif(Images::Sixel, 400_000);
    let out = written_for(&mut driver, Duration::from_secs(1));
    // At least two different frames within a second, at most the ceiling.
    let frames: Vec<&str> = out.split("\x1bP").skip(1).collect();
    let distinct: std::collections::HashSet<&str> = frames.iter().copied().collect();
    assert!(distinct.len() >= 2, "{} frames", frames.len());
    assert!(frames.len() <= 9, "{} frames in a second", frames.len());
    // Within its byte budget, counted over two seconds.
    let out = written_for(&mut driver, Duration::from_secs(2));
    assert!(
        out.len() as f64 <= 2.0 * 400_000.0 * 1.25,
        "{} bytes",
        out.len()
    );
}

#[test]
fn freezing_stops_the_bytes() {
    let mut driver = news_with_gif(Images::Sixel, 400_000);
    assert!(written_for(&mut driver, Duration::from_millis(600)).contains("\x1bP"));
    key(&mut driver, "f4");
    written_for(&mut driver, Duration::from_millis(200));
    assert!(screen(&driver).contains("frozen"), "{}", screen(&driver));
    let frozen = written_for(&mut driver, Duration::from_secs(1));
    assert!(!frozen.contains("\x1bP"), "a frame while frozen");
    // And it moves again.
    key(&mut driver, "f4");
    assert!(written_for(&mut driver, Duration::from_secs(1)).contains("\x1bP"));
}

#[test]
fn a_slow_terminal_steps_the_gif_down_to_character_art() {
    // 20 kB a second: too few Sixel frames, so it steps down to half
    // blocks, which still move.
    let mut driver = news_with_gif(Images::Sixel, 20_000);
    written_for(&mut driver, Duration::from_secs(1));
    let out = written_for(&mut driver, Duration::from_secs(2));
    assert!(!out.contains("\x1bP"), "still Sixel");
    // The half blocks move: their colours are written again (at least one
    // frame's worth, however slow the machine running the test).
    assert!(
        out.matches('▀').count() >= 100,
        "{} cells",
        out.matches('▀').count()
    );
    assert!(
        out.len() as f64 <= 2.0 * 20_000.0 * 1.5,
        "{} bytes",
        out.len()
    );
}

#[test]
fn kitty_plays_the_gif_itself_and_a_freeze_sends_its_still() {
    let mut driver = news_with_gif(Images::Kitty, 400_000);
    let mut out = String::new();
    // Its frames went up once, and Kitty loops them: nothing more is sent.
    let quiet = written_for(&mut driver, Duration::from_secs(1));
    out.push_str(&quiet);
    assert!(!quiet.contains("a=f,"), "frames sent again");
    key(&mut driver, "f4");
    let frozen = written_for(&mut driver, Duration::from_millis(500));
    assert!(frozen.contains("a=T,U=1"), "no still sent");
    assert!(!frozen.contains("s=3"), "still animated");
}
