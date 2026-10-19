//! A full screen of truecolour pixels, frame after frame: the style table
//! stays bounded and the frame stays fast (the raycaster's and the sprite
//! stage's finding: a full-screen framebuffer added thousands of styles a
//! frame that were never dropped, and half its frame time was hashing
//! them).
//!
//! The timing is an ignored test, meaningful only in release:
//!
//!     cargo test --release -p rs-rich-intuituive --test pixel_frames -- --ignored --nocapture

#[path = "../examples/dashboard.rs"]
#[allow(dead_code)]
mod dashboard;

use std::time::Instant;

use intuituive::interact::{Event, Key};
use intuituive::pixels::{pixels_fit, PixelMode};
use intuituive::prelude::*;
use intuituive::Driver;

/// A small, fast generator: the same noise on every run.
struct Noise(u64);

impl Noise {
    fn next(&mut self) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 33) as u32
    }
}

/// A pixel node filled with fresh random colours each time `t` is pressed.
fn noise_app(mode: PixelMode) -> App {
    App::new(move || {
        let tick = signal(0u64);
        let mut noise = Noise(7);
        pixels_fit(move |buf| {
            tick.get();
            for pixel in buf.pixels_mut() {
                let [r, g, b, _] = noise.next().to_le_bytes();
                *pixel = [r, g, b, 255];
            }
        })
        .pixel_mode(mode)
        .on_key("t", move |_| tick.update(|t| *t += 1))
    })
}

/// Press `t` and draw: the bytes of one frame.
fn frame(driver: &mut Driver) -> String {
    driver.event(Event::Key(Key::parse("t").unwrap()));
    driver.render().unwrap_or_default()
}

#[test]
fn the_style_table_stays_bounded_over_a_thousand_random_frames() {
    let (width, height) = (40, 12);
    let mut driver = noise_app(PixelMode::HalfBlocks).driver(width, height);
    driver.settle();
    let cells = width as usize * height as usize;
    let mut most = 0;
    for _ in 0..1000 {
        let out = frame(&mut driver);
        assert!(!out.is_empty());
        most = most.max(driver.screen().style_count());
    }
    // Every cell is a new pair of colours each frame: what is kept is what
    // one or two frames use, not what a thousand did.
    assert!(
        most <= 4 * cells + 1024,
        "{most} styles kept for {cells} cells"
    );
}

#[test]
fn quadrants_and_braille_stay_bounded_too() {
    for mode in [PixelMode::Quadrants, PixelMode::Braille] {
        let mut driver = noise_app(mode).driver(20, 6);
        driver.settle();
        for _ in 0..300 {
            frame(&mut driver);
        }
        assert!(driver.screen().style_count() <= 4 * 120 + 1024, "{mode:?}");
    }
}

/// Milliseconds and bytes a frame, the best of three runs of `frames`.
fn measure(mut driver: Driver, frames: usize) -> (f64, f64) {
    driver.settle();
    let mut best = (f64::MAX, 0.0);
    for _ in 0..3 {
        let mut bytes = 0;
        let start = Instant::now();
        for _ in 0..frames {
            bytes += frame(&mut driver).len();
        }
        let ms = start.elapsed().as_secs_f64() * 1e3 / frames as f64;
        if ms < best.0 {
            best = (ms, bytes as f64 / frames as f64);
        }
    }
    best
}

#[test]
#[ignore = "a benchmark: run it in release with --ignored"]
fn a_full_screen_of_random_truecolour_is_fast() {
    println!("| Scene | Size | ms a frame | bytes a frame |");
    println!("|---|---|---:|---:|");
    for (w, h) in [(80, 24), (160, 50)] {
        let (ms, bytes) = measure(noise_app(PixelMode::HalfBlocks).driver(w, h), 100);
        println!("| random half blocks | {w}x{h} | {ms:.2} | {bytes:.0} |");
        let ui = dashboard::dashboard(false).driver(w, h);
        let (ui_ms, ui_bytes) = measure(ui, 300);
        println!("| dashboard tick | {w}x{h} | {ui_ms:.3} | {ui_bytes:.0} |");
        if (w, h) == (160, 50) {
            assert!(ms < 16.0, "{ms:.2} ms a frame at 160x50");
        }
    }
}
