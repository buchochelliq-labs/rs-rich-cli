//! How smoothly video reaches the terminal: synthetic screencast frames
//! fed to the scripted engine at a steady rate (30 and 60 a second), the
//! app run as `App::run_on` runs it (update, draw, wait for the next thing
//! due or a wake from the tab), and each frame that reaches the screen
//! seen there. Per run it reports the frames a second delivered, the
//! spacing's jitter, the frames dropped, and where the UI thread's time
//! went; then a profile of each step a frame takes on its way.
//!
//! The quick checks run with the other tests. The full report is
//! ignored by default, as it takes a few seconds a protocol and its
//! numbers are only meaningful in a release build:
//!
//! ```text
//! cargo test -p noctiluca --release --test video_bench -- --ignored --nocapture
//! ```

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use noctiluca::app::{Options, app};
use noctiluca::bookmarks::Bookmarks;
use noctiluca::config::{Config, Images, View};
use noctiluca::engine::Playback;
use noctiluca::model::{Revisions, build};
use noctiluca::testing::{FakeEngine, FakeState};
use rich_art::picture::{Picture, Pixels as Protocol};
use rich_embed::{FrameMeta, Pixels, ScreencastFrame};
use rich_intuituive::Driver;

const URL: &str = "https://fixture.example/video.html";
/// The video element's node in the snapshot.
const VIDEO: i64 = 20;
/// The viewport Chrome paints, in CSS pixels.
const VIEW: (u32, u32) = (800, 720);

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
                duration: Some(60.0),
                ready: 4,
                ..Playback::default()
            },
        );
    }
    engine
}

/// A frame of the viewport, one colour that says its number.
fn frame(seq: u64) -> ScreencastFrame {
    let rgb = colour(seq);
    let (w, h) = VIEW;
    viewport(Pixels::new(w, h, rgb.repeat((w * h) as usize)).unwrap())
}

/// A photo-like frame (noise, a different one each `seq`), so the
/// encoders do the work a real video's frames make them do.
fn photo(seq: u64) -> ScreencastFrame {
    let (w, h) = VIEW;
    let salt = seq as u32;
    let rgb: Vec<u8> = (0..w * h * 3)
        .map(|i| (i ^ salt).wrapping_mul(2_654_435_761u32) >> 25)
        .map(|v| v as u8 + 40)
        .collect();
    viewport(Pixels::new(w, h, rgb).unwrap())
}

fn viewport(pixels: Pixels) -> ScreencastFrame {
    let (w, h) = VIEW;
    ScreencastFrame {
        pixels,
        meta: FrameMeta {
            page_scale: 1.0,
            device_width: w as f64,
            device_height: h as f64,
            ..FrameMeta::default()
        },
    }
}

/// Frame `seq`'s colour: its number in red and green, blue fixed.
fn colour(seq: u64) -> [u8; 3] {
    [(seq & 0xff) as u8, ((seq >> 8) & 0xff) as u8, 77]
}

/// The frame number a colour says, if it is one of [`colour`]'s.
fn number(rgb: (u8, u8, u8)) -> Option<u64> {
    (rgb.2 == 77).then(|| u64::from(rgb.0) | (u64::from(rgb.1) << 8))
}

fn wait(driver: &mut Driver, what: &str, ok: impl Fn(&Driver) -> bool) {
    let start = Instant::now();
    loop {
        driver.settle();
        if ok(driver) {
            return;
        }
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "{what} never happened:\n{}",
            driver.screen().plain().join("\n")
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// What one run measured.
#[derive(Debug)]
struct Run {
    protocol: &'static str,
    /// The budget's name.
    link: &'static str,
    offered_fps: u32,
    /// Frames the engine made in the measured window.
    produced: u64,
    /// Frames that reached the terminal in it.
    delivered: u64,
    seconds: f64,
    /// The spacing between frames on screen, in ms.
    mean_ms: f64,
    stddev_ms: f64,
    p95_ms: f64,
    max_ms: f64,
    /// The UI thread's time in the app's update (handlers: the frame
    /// offered to its pacing) and in drawing (the painter: encoding the
    /// picture), a second.
    update_ms_per_s: f64,
    render_ms_per_s: f64,
    /// Bytes written to the terminal a second.
    bytes_per_s: f64,
}

impl Run {
    fn fps(&self) -> f64 {
        self.delivered as f64 / self.seconds
    }

    fn dropped(&self) -> u64 {
        self.produced.saturating_sub(self.delivered)
    }

    /// Frames short of what the ceiling allows of those offered.
    fn missed(&self) -> i64 {
        let cap = f64::from(self.offered_fps.min(noctiluca::video::MAX_FPS));
        (cap * self.seconds).round() as i64 - self.delivered as i64
    }

    fn report(&self) -> String {
        format!(
            "{:<6} {:<6} offered {:>2} fps: delivered {:>5.1} fps ({} of {} frames, {} dropped, \
             {} under the ceiling), spacing {:.1} ms ± {:.1} (p95 {:.1}, max {:.1}); UI thread \
             {:.0} ms/s update + {:.0} ms/s draw; {:.0} kB/s",
            self.protocol,
            self.link,
            self.offered_fps,
            self.fps(),
            self.delivered,
            self.produced,
            self.dropped(),
            self.missed().max(0),
            self.mean_ms,
            self.stddev_ms,
            self.p95_ms,
            self.max_ms,
            self.update_ms_per_s,
            self.render_ms_per_s,
            self.bytes_per_s / 1000.0,
        )
    }
}

/// The budgets runs are made with: rs-rich-art's guess for this machine,
/// a fast local terminal (what the painter measures one taking), and an
/// SSH link.
const LINKS: [(&str, Option<u64>); 3] = [
    ("guess", None),
    ("fast", Some(32 << 20)),
    ("ssh", Some(256 << 10)),
];

/// Play the fixture's video with frames coming at `fps` for `seconds`,
/// pictures drawn as `images` says within the `link`'s budget, and
/// measure what reaches the screen.
fn run(images: Images, link: (&'static str, Option<u64>), fps: u32, seconds: f64) -> Run {
    let engine = engine();
    let state: Arc<Mutex<FakeState>> = engine.state.clone();
    let page = build(&state.lock().unwrap().pages["video"], &mut Revisions::new());
    let rect = page.media.iter().find(|m| m.node == VIDEO).unwrap().rect;
    // The frame rate asked for: as much as the run offers.
    let config = Config {
        fps,
        bandwidth: link.1,
        ..Config::default()
    };
    let options = Options {
        config,
        bookmarks: Bookmarks::default(),
        url: Some(URL.into()),
        view: Some(View::Projection),
        images: Some(images),
        tunnel: None,
    };
    let mut driver = app(Box::new(engine), options).driver(100, 40);
    // Woken by the tab as the real loop is.
    let woke = Arc::new((Mutex::new(false), Condvar::new()));
    {
        let woke = woke.clone();
        driver.set_waker(move || {
            *woke.0.lock().unwrap() = true;
            woke.1.notify_one();
        });
    }
    wait(&mut driver, "the page", |d| {
        d.screen().plain().join("\n").contains("media PAUSED")
    });
    driver.press("p");
    wait(&mut driver, "playing", |d| {
        d.screen().plain().join("\n").contains("media PLAYING")
    });
    // The engine paints frames at `fps`, numbered, on its own clock.
    let stop = Arc::new(AtomicBool::new(false));
    let made = Arc::new(AtomicU64::new(0));
    let painter = {
        let (stop, made, state) = (stop.clone(), made.clone(), state.clone());
        std::thread::spawn(move || {
            let interval = Duration::from_secs_f64(1.0 / f64::from(fps));
            let start = Instant::now();
            let mut seq = 0u64;
            // Cells are told apart by colour; pixels by their escapes, so
            // their frames can be photos.
            let photos: Vec<ScreencastFrame> = (0..4).map(photo).collect();
            while !stop.load(Ordering::SeqCst) {
                seq += 1;
                let frame = if images == Images::Ansi {
                    frame(seq)
                } else {
                    photos[seq as usize % photos.len()].clone()
                };
                state.lock().unwrap().frames.push(frame);
                made.store(seq, Ordering::SeqCst);
                let next = start + interval * seq as u32;
                if let Some(rest) = next.checked_duration_since(Instant::now()) {
                    std::thread::sleep(rest);
                }
            }
        })
    };
    // Where to look for the frame: the region's middle.
    let row = 2 + ((rect.y + rect.height / 2.0) / 18.0) as u16;
    let column = (rect.x / 8.0) as u16 + 2;
    let cells = images == Images::Ansi;
    let start = Instant::now();
    let clock = |d: Duration| d;
    let warm_up = Duration::from_millis(1000);
    let length = Duration::from_secs_f64(seconds);
    let mut shown: Vec<(Instant, u64)> = Vec::new();
    let mut last_seq = 0u64;
    let (mut update_time, mut render_time) = (Duration::ZERO, Duration::ZERO);
    let mut bytes = 0usize;
    let mut first_made = 0;
    let mut measuring = false;
    // The app's clock goes on from where the setup left it.
    let base = {
        // `settle` does not move the clock: it starts where it was.
        Duration::ZERO
    };
    loop {
        let now = start.elapsed();
        if !measuring && now >= warm_up {
            measuring = true;
            first_made = made.load(Ordering::SeqCst);
            update_time = Duration::ZERO;
            render_time = Duration::ZERO;
            bytes = 0;
            shown.clear();
        }
        if now >= warm_up + length {
            break;
        }
        let t = Instant::now();
        driver.update(clock(base + now));
        update_time += t.elapsed();
        let t = Instant::now();
        let out = driver.render();
        render_time += t.elapsed();
        if let Some(out) = out {
            bytes += out.len();
            let at = Instant::now();
            if cells {
                let screen = driver.screen();
                let cell = screen.cell(column, row);
                let seq = screen
                    .style(cell.style)
                    .and_then(|s| s.bgcolor().cloned())
                    .and_then(|c| c.get_truecolor())
                    .and_then(|c| number((c.red, c.green, c.blue)));
                if let Some(seq) = seq
                    && seq != last_seq
                {
                    last_seq = seq;
                    shown.push((at, seq));
                }
            } else if out.contains("\x1b_G") || out.contains("\x1bP") || out.contains("\x1b]1337") {
                last_seq += 1;
                shown.push((at, last_seq));
            }
        }
        // Until the next thing due, or a wake from the tab.
        let wait = driver.timeout(clock(base + start.elapsed()));
        let (lock, wake) = &*woke;
        let mut woken = lock.lock().unwrap();
        if !*woken {
            woken = wake
                .wait_timeout(woken, wait.min(Duration::from_millis(50)))
                .unwrap()
                .0;
        }
        *woken = false;
    }
    stop.store(true, Ordering::SeqCst);
    painter.join().unwrap();
    let produced = made.load(Ordering::SeqCst) - first_made;
    let gaps: Vec<f64> = shown
        .windows(2)
        .map(|w| (w[1].0 - w[0].0).as_secs_f64() * 1000.0)
        .collect();
    let mean = gaps.iter().sum::<f64>() / gaps.len().max(1) as f64;
    let var = gaps.iter().map(|g| (g - mean).powi(2)).sum::<f64>() / gaps.len().max(1) as f64;
    let mut sorted = gaps.clone();
    sorted.sort_by(f64::total_cmp);
    let p95 = sorted
        .get(((sorted.len() as f64 * 0.95) as usize).min(sorted.len().saturating_sub(1)))
        .copied()
        .unwrap_or(0.0);
    let secs = length.as_secs_f64();
    Run {
        protocol: match images {
            Images::Ansi => "cells",
            Images::Kitty => "kitty",
            Images::Sixel => "sixel",
            Images::Iterm2 => "iterm2",
            _ => "other",
        },
        link: link.0,
        offered_fps: fps,
        produced,
        delivered: shown.len() as u64,
        seconds: secs,
        mean_ms: mean,
        stddev_ms: var.sqrt(),
        p95_ms: p95,
        max_ms: sorted.last().copied().unwrap_or(0.0),
        update_ms_per_s: update_time.as_secs_f64() * 1000.0 / secs,
        render_ms_per_s: render_time.as_secs_f64() * 1000.0 / secs,
        bytes_per_s: bytes as f64 / secs,
    }
}

/// The mean time of `f` over `n` runs, in ms.
fn time(n: u32, mut f: impl FnMut()) -> f64 {
    f();
    let start = Instant::now();
    for _ in 0..n {
        f();
    }
    start.elapsed().as_secs_f64() * 1000.0 / f64::from(n)
}

/// Each step a frame takes, timed on its own: Chrome's JPEG decoded, the
/// region cut from the viewport, its pixels made RGBA, and the picture
/// encoded for the terminal in each protocol.
fn profile() -> Vec<String> {
    let viewport = frame(1);
    // A photo-like frame, so the JPEG and the encoders do real work.
    let (w, h) = VIEW;
    let noisy: Vec<u8> = (0..w * h * 3)
        .map(|i| (i.wrapping_mul(2_654_435_761u32) >> 25) as u8 + (i % 97) as u8)
        .collect();
    let image = image::RgbImage::from_raw(w, h, noisy.clone()).unwrap();
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 70)
        .encode_image(&image)
        .unwrap();
    let noisy = Pixels::new(w, h, noisy).unwrap();
    let viewport = ScreencastFrame {
        pixels: noisy,
        ..viewport
    };
    // The fixture's region: 480 x 270 CSS pixels, 60 x 15 cells.
    let (x, y, rw, rh) = (16.0, 120.0, 480.0, 270.0);
    let (cols, rows) = (60usize, 15usize);
    let cut = viewport.crop(x, y, rw, rh).unwrap();
    let rgba = cut.to_rgba();
    let picture = Picture::from_rgba(cut.width, cut.height, rgba.clone()).unwrap();
    let console = rich::Console::builder().width(cols).build();
    let mut out = vec![
        format!(
            "decode JPEG {w}x{h} (q70, {} kB): {:.2} ms",
            jpeg.len() / 1000,
            time(20, || {
                let _ = image::load_from_memory(&jpeg).unwrap().to_rgb8();
            })
        ),
        format!(
            "crop {rw}x{rh} out of the viewport: {:.3} ms",
            time(200, || {
                viewport.crop(x, y, rw, rh).unwrap();
            })
        ),
        format!(
            "RGB to RGBA: {:.3} ms",
            time(200, || {
                cut.to_rgba();
            })
        ),
        format!(
            "Picture::next_rgba: {:.3} ms",
            time(200, || {
                picture.next_rgba(cut.width, cut.height, rgba.clone());
            })
        ),
    ];
    let view = rich_art::picture::ImageView::new()
        .fit(rich_art::ImageFit::Cover)
        .pixels(Protocol::Cells);
    out.push(format!(
        "encode cells (half blocks, {cols}x{rows}): {:.2} ms",
        time(20, || {
            picture.lines(&console, view, cols as u16, rows as u16);
        })
    ));
    for protocol in [Protocol::Kitty, Protocol::Sixel, Protocol::Iterm] {
        let mut size = 0;
        // A new frame each time: a frame's escape is made once.
        let ms = time(10, || {
            let fresh = picture
                .next_rgba(cut.width, cut.height, rgba.clone())
                .unwrap();
            size = fresh.encode(protocol, cols, rows).map_or(0, |s| s.len());
        });
        out.push(format!(
            "encode {} ({cols}x{rows} cells): {ms:.2} ms, {} kB",
            protocol.name(),
            size / 1000
        ));
    }
    out
}

/// The full report: each protocol at 30 and 60 frames a second offered,
/// and the profile.
#[test]
#[ignore = "a benchmark: run with --ignored --nocapture, in release"]
fn report() {
    let seconds = std::env::var("NOCTILUCA_BENCH_SECONDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(4.0);
    let runs = [
        (Images::Ansi, LINKS[0], 30),
        (Images::Ansi, LINKS[0], 60),
        (Images::Ansi, LINKS[2], 30),
        (Images::Kitty, LINKS[0], 30),
        (Images::Kitty, LINKS[1], 30),
        (Images::Kitty, LINKS[1], 60),
        (Images::Sixel, LINKS[1], 30),
    ];
    for (images, link, fps) in runs {
        println!("{}", run(images, link, fps, seconds).report());
    }
    println!("profile of one frame of a 480x270 region:");
    for line in profile() {
        println!("  {line}");
    }
}
