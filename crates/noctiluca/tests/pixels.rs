//! The pixel view (#818) on intuiTUIve's driver over the scripted engine:
//! the screencast's frames as tiles in Kitty graphics, one changed tile
//! sent as one upload, input to the page at its exact point, keys passed
//! through, and the projection with a message where there is no Kitty.
//! Then the benchmark: bytes and frames a second, whole frames against
//! tiles, for a page scrolling and for one where only a caret blinks.

use std::path::Path;
use std::time::{Duration, Instant};

use noctiluca::app::{Options, app};
use noctiluca::bookmarks::Bookmarks;
use noctiluca::cdp::CELL;
use noctiluca::config::{Config, Images, View};
use noctiluca::engine::PageAction;
use noctiluca::testing::FakeEngine;
use rich_art::image::{Rgb, RgbImage, Rgba, RgbaImage};
use rich_art::picture::{Picture, Pixels};
use rich_art::tiles::Tiler;
use rich_embed::{FrameMeta, ScreencastFrame};
use rich_intuituive::Driver;
use rich_intuituive::interact::Key;

const COLUMNS: u16 = 100;
const ROWS: u16 = 40;

fn engine() -> FakeEngine {
    FakeEngine::from_dir(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots")).unwrap()
}

fn options(images: Images) -> Options {
    Options {
        config: Config::default(),
        bookmarks: Bookmarks::default(),
        url: Some("reader.html".into()),
        view: Some(View::Pixels),
        images: Some(images),
        tunnel: None,
        control: None,
    }
}

/// A screencast frame of `image`.
fn frame(image: &RgbImage) -> ScreencastFrame {
    ScreencastFrame {
        pixels: rich_embed::Pixels::new(image.width(), image.height(), image.as_raw().clone())
            .unwrap(),
        meta: FrameMeta {
            scroll_x: 0.0,
            scroll_y: 0.0,
            offset_top: 0.0,
            page_scale: 1.0,
            device_width: image.width() as f64,
            device_height: image.height() as f64,
            timestamp: None,
        },
    }
}

/// What the driver writes until `until` holds, over at most `rounds`.
fn written(driver: &mut Driver, rounds: usize, until: impl Fn(&str) -> bool) -> String {
    let mut out = String::new();
    for _ in 0..rounds {
        // Not `settle`, which draws and drops what it wrote.
        driver.update(clock());
        out.push_str(&driver.render().unwrap_or_default());
        if until(&out) {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    out
}

/// The driver's clock: real time since the first call, so it never goes
/// back.
fn clock() -> Duration {
    static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    START.get_or_init(Instant::now).elapsed()
}

fn screen(driver: &Driver) -> String {
    driver.screen().plain().join("\n")
}

#[test]
fn the_screencast_shows_as_tiles_and_one_change_sends_one_tile() {
    let engine = engine();
    let state = engine.state.clone();
    let mut driver = app(Box::new(engine), options(Images::Kitty)).driver(COLUMNS, ROWS);
    driver.settle();
    // The tab starts the screencast for the view.
    let start = Instant::now();
    while !state.lock().unwrap().screencast {
        driver.settle();
        assert!(start.elapsed() < Duration::from_secs(5), "no screencast");
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(state.lock().unwrap().screencast_scale, 1.0);
    let mut page = RgbImage::from_pixel(800, 648, Rgb([250, 250, 250]));
    state.lock().unwrap().frames.push(frame(&page));
    // 100 x 36 cells in tiles of 8 x 4: 13 x 9 tiles go up.
    let out = written(&mut driver, 300, |o| o.matches("a=T").count() >= 117);
    assert_eq!(out.matches("a=T").count(), 117);
    // A caret in one place: one tile goes up, and nothing else.
    std::thread::sleep(Duration::from_millis(400));
    for y in 40..60 {
        page.put_pixel(20, y, Rgb([0, 0, 0]));
    }
    state.lock().unwrap().frames.push(frame(&page));
    let mut out = written(&mut driver, 100, |o| o.contains("a=T"));
    out.push_str(&written(&mut driver, 10, |_| false));
    assert_eq!(out.matches("a=T").count(), 1);
    assert!(screen(&driver).contains("pixels"), "{}", screen(&driver));
}

#[test]
fn a_click_lands_at_its_point_and_keys_pass_through() {
    let engine = engine();
    let state = engine.state.clone();
    let mut driver = app(Box::new(engine), options(Images::Kitty)).driver(COLUMNS, ROWS);
    driver.settle();
    state
        .lock()
        .unwrap()
        .frames
        .push(frame(&RgbImage::from_pixel(800, 648, Rgb([9, 9, 9]))));
    written(&mut driver, 100, |o| o.contains("a=T"));
    // The view starts under the two rows of the address bar.
    driver.click(10, 5);
    driver.press("a");
    driver.press("enter");
    let start = Instant::now();
    loop {
        driver.settle();
        let actions = state.lock().unwrap().actions.clone();
        let point = actions.iter().find_map(|a| match a {
            PageAction::ClickPoint { x, y } => Some((*x, *y)),
            _ => None,
        });
        let keys: Vec<Key> = actions
            .iter()
            .filter_map(|a| match a {
                PageAction::Key(k) => Some(*k),
                _ => None,
            })
            .collect();
        if let Some((x, y)) = point
            && keys.len() == 2
        {
            // The middle of cell 10 of row 3 of the view.
            assert_eq!(x, 10.5 * CELL.0 as f64);
            assert_eq!(y, 3.5 * CELL.1 as f64);
            assert_eq!(keys, [Key::char('a'), Key::parse("enter").unwrap()]);
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(5), "{actions:?}");
        std::thread::sleep(Duration::from_millis(10));
    }
    // Alt+X (a key with Alt) is Noctiluca's: it leaves the view, and the
    // screencast stops.
    driver.press("alt+x");
    let start = Instant::now();
    while state.lock().unwrap().screencast || !screen(&driver).contains("projection view") {
        driver.settle();
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "{}",
            screen(&driver)
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn without_kitty_graphics_the_projection_shows_and_says_why() {
    let engine = engine();
    let state = engine.state.clone();
    let mut driver = app(Box::new(engine), options(Images::Ansi)).driver(COLUMNS, ROWS);
    let start = Instant::now();
    while !screen(&driver).contains("Tiny drifters") {
        driver.settle();
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "{}",
            screen(&driver)
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        screen(&driver).contains("pixel view needs Kitty graphics"),
        "{}",
        screen(&driver)
    );
    assert!(!state.lock().unwrap().screencast);
    // The key says so too, and changes nothing.
    driver.press("alt+x");
    driver.settle();
    assert!(!state.lock().unwrap().screencast);
}

/// A long page of text-like rows: dark marks on light, varied by row.
fn long_page(width: u32, height: u32) -> RgbaImage {
    RgbaImage::from_fn(width, height, |x, y| {
        let line = y / 18;
        let ink = (y % 18) < 12 && (x / 7 + line * 3) % 11 < 7 && x % 7 < 5 && line % 6 != 5;
        if ink {
            Rgba([20, 20, 30 + (line % 40) as u8, 255])
        } else {
            Rgba([250, 250, 248, 255])
        }
    })
}

/// Bytes a frame costs whole, through Kitty, in `cols` x `rows` cells.
fn whole(frame: &RgbaImage, cols: usize, rows: usize) -> usize {
    Picture::from_rgba(frame.width(), frame.height(), frame.as_raw().clone())
        .unwrap()
        .encode(Pixels::Kitty, cols, rows)
        .unwrap()
        .len()
}

#[test]
fn benchmark_whole_frames_against_tiles() {
    let (cols, rows) = (100usize, 36usize);
    let (width, height) = (800u32, 648u32);
    let page = long_page(width, height * 8);
    let budget = 4.0 * 1024.0 * 1024.0;
    let fps = |bytes_per_frame: f64| (budget / bytes_per_frame.max(1.0)).min(30.0);
    let mut report = Vec::new();
    // Scrolling: 54 pixels (three rows) a frame, for 30 frames.
    let mut tiler = Tiler::default();
    let (mut full, mut tiled) = (0usize, 0usize);
    for k in 0..30u32 {
        let view = rich_art::image::imageops::crop_imm(&page, 0, k * 54, width, height).to_image();
        full += whole(&view, cols, rows);
        let changed = tiler.update(&view, cols, rows);
        if k > 0 {
            tiled += tiler
                .encode(&changed, Pixels::Kitty)
                .iter()
                .map(String::len)
                .sum::<usize>();
        } else {
            full -= whole(&view, cols, rows);
        }
    }
    let frames = 29.0;
    report.push(("scrolling", full as f64 / frames, tiled as f64 / frames));
    let scroll = (full, tiled);
    // A caret blinking on a still page, for 30 frames.
    let still = rich_art::image::imageops::crop_imm(&page, 0, 0, width, height).to_image();
    let mut tiler = Tiler::default();
    tiler.update(&still, cols, rows);
    let (mut full, mut tiled) = (0usize, 0usize);
    for k in 0..30u32 {
        let mut view = still.clone();
        if k % 2 == 0 {
            for y in 300..318 {
                for x in 400..402 {
                    view.put_pixel(x, y, Rgba([0, 0, 0, 255]));
                }
            }
        }
        full += whole(&view, cols, rows);
        let changed = tiler.update(&view, cols, rows);
        tiled += tiler
            .encode(&changed, Pixels::Kitty)
            .iter()
            .map(String::len)
            .sum::<usize>();
    }
    report.push(("caret", full as f64 / 30.0, tiled as f64 / 30.0));
    let caret = (full, tiled);
    eprintln!("case       whole B/frame  fps@4MB/s   tiled B/frame  fps@4MB/s");
    for (case, whole, tiles) in &report {
        eprintln!(
            "{case:<10} {whole:>13.0}  {:>9.1}   {tiles:>13.0}  {:>9.1}",
            fps(*whole),
            fps(*tiles)
        );
    }
    // A blinking caret is one tile: a small fraction of a frame.
    assert!(caret.1 * 50 < caret.0, "{caret:?}");
    // Scrolling moves every row: tiles cost about a whole frame, not more
    // than a tenth over.
    assert!(scroll.1 <= scroll.0 + scroll.0 / 10, "{scroll:?}");
}
