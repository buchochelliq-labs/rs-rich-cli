//! `stepped_worker`: a worker thread stepped by the app's clock, exactly
//! under the driver.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use intuituive::prelude::*;
use intuituive::{stepped_worker, stepped_worker_with, Lockstep};

const TIC: Duration = Duration::from_nanos(1_000_000_000 / 35);

#[test]
fn a_second_at_35_a_second_is_35_ticks_and_the_last_output_shows() {
    let ticks = Arc::new(AtomicU64::new(0));
    let seen = ticks.clone();
    let app = App::new(move || {
        let seen = seen.clone();
        let tic = stepped_worker(TIC, 8, move |steps| {
            let mut tic = 0u64;
            while let Some(n) = steps.wait() {
                for _ in 0..n {
                    // Some work a tick.
                    std::thread::sleep(Duration::from_micros(200));
                    tic += 1;
                    seen.fetch_add(1, Ordering::SeqCst);
                }
                steps.publish(tic);
            }
        });
        text!("{}", tic.get().unwrap_or(0))
    });
    let mut driver = app.driver(8, 1);
    driver.advance(Duration::from_secs(1));
    assert_eq!(ticks.load(Ordering::SeqCst), 35);
    assert_eq!(driver.screen().plain()[0].trim_end(), "35");
}

#[test]
fn a_stall_hands_over_at_most_the_catch_up() {
    let batches = Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen = batches.clone();
    let app = App::new(move || {
        let seen = seen.clone();
        let last = stepped_worker(TIC, 4, move |steps| {
            while let Some(n) = steps.wait() {
                seen.lock().unwrap().push(n);
                steps.publish(n);
            }
        });
        text!("{:?}", last.get())
    });
    let mut driver = app.driver(10, 1);
    driver.update(Duration::from_millis(300));
    driver.settle();
    assert_eq!(*batches.lock().unwrap(), [4]);
    assert_eq!(driver.screen().plain()[0].trim_end(), "Some(4)");
}

#[test]
fn a_worker_that_ends_lets_each_step_return_at_once() {
    let app = App::new(|| {
        let out = stepped_worker(TIC, 8, |steps| {
            steps.wait();
            steps.publish("last");
        });
        text!("{:?}", out.get()).on_key("q", |cx| cx.quit())
    });
    let mut driver = app.driver(16, 1);
    let started = Instant::now();
    driver.advance(Duration::from_secs(2));
    // 70 ticks, none of them waiting out the 250 ms timeout.
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(driver.screen().plain()[0].trim_end(), "Some(\"last\")");
    driver.press("q");
    assert!(driver.is_done());
}

#[test]
fn a_slow_worker_times_out_and_catches_up_later() {
    let app = App::new(|| {
        let pace =
            Lockstep::new(Duration::from_millis(100), 100).timeout(Duration::from_millis(50));
        let (out, _timer) = stepped_worker_with(pace, |steps| {
            let mut total = 0;
            let mut first = true;
            while let Some(n) = steps.wait() {
                if first {
                    std::thread::sleep(Duration::from_secs(1));
                    first = false;
                }
                total += n;
                steps.publish(total);
            }
        });
        text!("{:?}", out.get())
    });
    let mut driver = app.driver(10, 1);
    let started = Instant::now();
    driver.advance(Duration::from_millis(300));
    // Three steps, each giving up after 50 ms: the worker is still asleep.
    assert!(
        started.elapsed() < Duration::from_millis(900),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(driver.screen().plain()[0].trim_end(), "None");
    std::thread::sleep(Duration::from_millis(1200));
    // The worker finished its first tick and took the two given since;
    // the next step hands it one more and waits for it.
    driver.advance(Duration::from_millis(100));
    assert_eq!(driver.screen().plain()[0].trim_end(), "Some(4)");
}
