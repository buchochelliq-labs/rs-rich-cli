//! The port of ratatui's `inline` (examples/downloads.rs): headless, on a
//! virtual clock with the workers on the app's timer, the region while it
//! runs and when it is done (80x8 snapshots), and the lines printed above
//! it; then once with the workers on threads, reporting through a
//! `Proxy`.

#[path = "../examples/downloads.rs"]
#[allow(dead_code)]
mod downloads;

use std::time::{Duration, Instant};

use downloads::{downloads, downloads_app, finished_line, Job, Options, ROWS};
use intuituive::{assert_screen, Driver};

const WIDTH: u16 = 80;
const TICK: Duration = Duration::from_millis(100);

fn clock() -> Options {
    Options {
        downloads: 10,
        seed: 42,
        threads: false,
        ..Options::default()
    }
}

/// A driver in a terminal 24 rows tall: the region takes eight of them.
fn driver(options: Options) -> Driver {
    let mut driver = downloads_app(options).driver(WIDTH, 24);
    driver.settle();
    driver
}

/// `n` ticks on from tick `from`; the lines printed meanwhile, taken
/// before each frame writes them.
fn ticks(driver: &mut Driver, from: u32, n: u32) -> Vec<String> {
    let mut printed = Vec::new();
    for k in from + 1..=from + n {
        driver.update(TICK * k);
        printed.extend(driver.take_printed());
        driver.settle();
    }
    printed
}

fn rows(driver: &Driver) -> Vec<String> {
    driver
        .screen()
        .plain()
        .iter()
        .map(|r| r.trim_end().to_string())
        .collect()
}

#[test]
fn downloads_running() {
    let mut driver = driver(clock());
    assert_eq!(driver.screen().plain().len(), ROWS as usize);
    let printed = ticks(&mut driver, 0, 6);
    let rows = rows(&driver);
    assert!(rows[0].contains("Downloads"), "{rows:?}");
    assert_eq!(
        rows.iter().filter(|r| r.contains(" worker ")).count(),
        4,
        "{rows:?}"
    );
    assert!(printed.len() < 10, "{printed:?}");
    assert_screen!(driver, "downloads_running");
}

#[test]
fn downloads_printed() {
    let options = clock();
    let mut driver = driver(options);
    let printed = ticks(&mut driver, 0, 200);
    assert!(
        driver.is_done(),
        "every download finished and the app ended"
    );
    // One line a file, each saying how big it was and how long it took.
    assert_eq!(printed.len(), 10, "{printed:#?}");
    let files = downloads(options.downloads, options.seed);
    for file in &files {
        let ticks = file.size.div_ceil(file.speed) as u32;
        let line = finished_line(
            &Job {
                download: *file,
                done: file.size,
                ticks,
            },
            TICK,
        );
        assert!(printed.contains(&line), "{line}: {printed:#?}");
    }
    assert!(printed[0].starts_with("Finished [bold]"), "{printed:?}");
}

#[test]
fn downloads_done() {
    let mut driver = driver(clock());
    let mut k = 0;
    // Up to the frame before the end: every file done, the app not yet
    // ended.
    while !rows(&driver)[0].contains("10 of 10 done") {
        k += 1;
        ticks(&mut driver, k - 1, 1);
        assert!(k < 200, "{:?}", rows(&driver));
    }
    assert!(!driver.is_done());
    let rows = rows(&driver);
    assert!(rows[1].contains("100%"), "{rows:?}");
    assert_eq!(rows.iter().filter(|r| r.contains("idle")).count(), 4);
    assert_screen!(driver, "downloads_done");
    // The next tick ends it; the region stays as it was.
    ticks(&mut driver, k, 1);
    assert!(driver.is_done());
    let finish = driver.finish();
    assert!(finish.ends_with("\x1b[?25h"), "{finish:?}");
}

#[test]
fn a_tick_redraws_the_workers_and_the_total() {
    let mut driver = driver(clock());
    ticks(&mut driver, 0, 1);
    driver.update(TICK * 2);
    driver.render().expect("a tick draws");
    // The heading, the total, and each worker's name and gauge at most.
    let drawn = driver.stats().drawn;
    assert!((3..=10).contains(&drawn), "{drawn}");
}

#[test]
fn q_stops_early() {
    let mut driver = driver(clock());
    ticks(&mut driver, 0, 2);
    driver.press("q");
    assert!(driver.is_done());
}

#[test]
fn the_workers_run_on_threads() {
    let options = Options {
        downloads: 8,
        seed: 3,
        tick: Duration::from_millis(1),
        threads: true,
    };
    let mut driver = downloads_app(options).driver(WIDTH, 24);
    let start = Instant::now();
    // Lines the threads' reports print, whether taken before a frame or
    // written by it above the region.
    let mut printed = 0;
    while !driver.is_done() {
        assert!(start.elapsed() < Duration::from_secs(30), "{printed}");
        std::thread::sleep(Duration::from_millis(2));
        driver.update(start.elapsed());
        printed += driver.take_printed().len();
        if let Some(frame) = driver.render() {
            printed += frame.matches("Finished").count();
        }
    }
    assert_eq!(printed, 8);
}

#[test]
fn the_command_line() {
    let parse = |line: &str| Options::parse(line.split_whitespace().map(String::from));
    let options = parse("--downloads 5 --seed 9 --tick 50 --clock").unwrap();
    assert_eq!(options.downloads, 5);
    assert_eq!(options.seed, 9);
    assert_eq!(options.tick, Duration::from_millis(50));
    assert!(!options.threads);
    assert!(parse("--downloads many").is_err());
    assert!(parse("--fast").is_err());
}

#[test]
fn the_region_draws_at_small_and_large_sizes() {
    for (width, height) in [(40, 12), (200, 60), (12, 5), (1, 1)] {
        let mut driver = downloads_app(clock()).driver(width, height);
        driver.settle();
        ticks(&mut driver, 0, 30);
    }
}
