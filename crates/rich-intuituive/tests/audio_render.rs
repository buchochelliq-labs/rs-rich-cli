//! An output whose device calls a renderer: a synthesizer's.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use intuituive::audio::{output_with, OutputOptions};

/// A 440 Hz sine renderer at 44,100 Hz, counting the frames it made.
fn sine(made: Arc<AtomicU64>) -> impl FnMut(&mut [f32], u16) + Send + 'static {
    let mut n = 0u64;
    move |out, channels| {
        for frame in out.chunks_mut(usize::from(channels)) {
            let t = n as f64 / 44_100.0;
            let s = (t * 440.0 * std::f64::consts::TAU).sin() as f32 * 0.5;
            frame.fill(s);
            n += 1;
        }
        made.fetch_add(
            (out.len() / usize::from(channels)) as u64,
            Ordering::Relaxed,
        );
    }
}

#[test]
fn a_sine_renderer_makes_440_crossing_pairs_a_second() {
    let made = Arc::new(AtomicU64::new(0));
    let out = output_with(OutputOptions::null().rate(44_100).channels(2), sine(made)).unwrap();
    let tap = out.tap(Duration::from_secs(1));
    std::thread::sleep(Duration::from_millis(1300));
    let left: Vec<f32> = tap.samples().chunks(2).map(|f| f[0]).collect();
    assert_eq!(left.len(), 44_100);
    let rising = left
        .windows(2)
        .filter(|w| w[0] < 0.0 && w[1] >= 0.0)
        .count();
    assert!((439..=441).contains(&rising), "{rising}");
}

#[test]
fn volume_zero_is_silence_and_the_renderer_goes_on() {
    let made = Arc::new(AtomicU64::new(0));
    let out = output_with(
        OutputOptions::null().rate(44_100).channels(1),
        sine(made.clone()),
    )
    .unwrap();
    out.set_volume(0.0);
    let tap = out.tap(Duration::from_secs(1));
    std::thread::sleep(Duration::from_millis(200));
    let before = made.load(Ordering::Relaxed);
    std::thread::sleep(Duration::from_millis(200));
    assert!(made.load(Ordering::Relaxed) > before);
    assert!(tap.frames() > 0);
    assert!(tap.samples().iter().all(|s| *s == 0.0));
    assert!(!out.write(&[0.5]), "a renderer's output takes no writes");
}

#[test]
fn a_panicking_renderer_is_reported_not_fatal() {
    let mut calls = 0;
    let out = output_with(
        OutputOptions::null().rate(8000).channels(1),
        move |out, _| {
            calls += 1;
            if calls == 3 {
                panic!("the synthesizer broke");
            }
            out.fill(0.1);
        },
    )
    .unwrap();
    let mut waited = 0;
    while out.failed().is_none() && waited < 100 {
        std::thread::sleep(Duration::from_millis(10));
        waited += 1;
    }
    assert_eq!(out.failed().as_deref(), Some("the renderer panicked"));
    assert!(out.stopped());
}
