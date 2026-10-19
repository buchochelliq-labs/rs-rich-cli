//! The runtime and the world outside: `latest`, the newest value from a
//! thread (on the app's clock in a test), `on_write` and `rate`,
//! `App::try_new`, and `Ctx::handoff`, the terminal handed to another
//! program.

use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use intuituive::interact::headless::{Headless, Script};
use intuituive::prelude::*;

fn first(driver: &intuituive::Driver) -> String {
    driver.screen().plain()[0].trim_end().to_string()
}

/// Settle `driver` until its first row is `want`, for at most 10 s.
fn wait_for(driver: &mut intuituive::Driver, want: &str) {
    let start = Instant::now();
    while first(driver) != want {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "still {:?}",
            first(driver)
        );
        std::thread::sleep(Duration::from_millis(1));
        driver.settle();
    }
}

#[test]
fn latest_is_none_until_a_value_and_then_the_newest() {
    let (go, wait) = mpsc::channel::<()>();
    let app = App::new(move || {
        let reading = latest(move |tx| {
            let _ = wait.recv();
            tx.send(1);
            tx.send(2);
            tx.send(3);
        });
        text(move || format!("{:?}", reading.get()))
    });
    let mut driver = app.driver(10, 1);
    driver.settle();
    assert_eq!(first(&driver), "None");
    go.send(()).unwrap();
    wait_for(&mut driver, "Some(3)");
}

#[test]
fn values_the_app_had_no_time_for_are_skipped() {
    let draws = Arc::new(AtomicUsize::new(0));
    let seen = draws.clone();
    let app = App::new(move || {
        let reading = latest(|tx| {
            for n in 0..=100_000u32 {
                tx.send(n);
            }
        });
        text(move || {
            seen.fetch_add(1, Ordering::SeqCst);
            format!("{:?}", reading.get())
        })
    });
    let mut driver = app.driver(20, 1);
    wait_for(&mut driver, "Some(100000)");
    // Far fewer frames than values: nothing queued.
    assert!(
        draws.load(Ordering::SeqCst) < 10_000,
        "{}",
        draws.load(Ordering::SeqCst)
    );
}

#[test]
fn send_says_false_once_the_app_has_finished() {
    let stopped = Arc::new(AtomicBool::new(false));
    let done = stopped.clone();
    let app = App::new(move || {
        let reading = latest(move |tx| {
            let mut n = 0u64;
            while tx.send(n) {
                n += 1;
                std::thread::sleep(Duration::from_millis(1));
            }
            done.store(true, Ordering::SeqCst);
        });
        text(move || format!("{:?}", reading.get().is_some()))
    });
    let mut driver = app.driver(10, 1);
    wait_for(&mut driver, "true");
    let _ = driver.finish();
    let start = Instant::now();
    while !stopped.load(Ordering::SeqCst) {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "the thread went on"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn a_handoff_runs_the_program_and_redraws_after() {
    let app = App::new(|| {
        let code = signal(String::from("-"));
        text!("{code}")
            .on_key("o", |cx| cx.handoff(Command::new("less")))
            .on_key("e", move |cx| {
                let mut vi = Command::new("vi");
                vi.arg("notes.txt");
                cx.handoff_then(vi, move |result, _| {
                    code.set(format!("{:?}", result.ok().flatten()));
                })
            })
            .on_key("q", |cx| cx.quit())
    });
    let mut backend = Headless::new(Script::new().keys("o e q"), 20, 2);
    backend.handoff_code = Some(3);
    let record = backend.record();
    app.run_on(&mut backend).unwrap();
    let record = record.borrow();
    assert_eq!(record.handoffs, ["less", "vi"]);
    // The arguments too: the editor got the file.
    let commands: Vec<(&str, Vec<&str>)> = record
        .handoff_commands
        .iter()
        .map(|(program, args)| (program.as_str(), args.iter().map(String::as_str).collect()))
        .collect();
    assert_eq!(commands, [("less", vec![]), ("vi", vec!["notes.txt"])]);
    assert_eq!(
        record.last_frame().lines().next().unwrap().trim_end(),
        "Some(3)"
    );
}

#[test]
fn a_loop_of_your_own_takes_the_handoff_and_gives_it_back() {
    let app = App::new(|| {
        let back = signal(false);
        text!("{back}").on_key("o", move |cx| {
            cx.handoff_then(Command::new("true"), move |_, _| back.set(true))
        })
    });
    let mut driver = app.driver(10, 1);
    driver.press("o");
    let handoff = driver.take_handoff().expect("asked for");
    assert_eq!(handoff.command.get_program(), "true");
    assert!(driver.take_handoff().is_none());
    let _ = driver.suspend();
    driver.resize(10, 1);
    driver.handed_back(handoff, Ok(Some(0)));
    driver.settle();
    assert_eq!(first(&driver), "true");
}

/// Step `driver` to `ms` milliseconds and draw.
fn at(driver: &mut intuituive::Driver, ms: u64) {
    driver.update(Duration::from_millis(ms));
    driver.render();
}

#[test]
fn a_thread_on_the_apps_clock_is_frame_exact() {
    let app = App::new(|| {
        // The same buffer every 50 ms: a sound card's digital silence.
        let feed = latest(
            |tx| {
                while tx.send(vec![0.0f32; 4]) && tx.sleep(Duration::from_millis(50)) {}
            },
        );
        let (changes, writes) = (signal(0u32), signal(0u32));
        watch(move || feed.get(), move |_, _| changes.update(|n| *n += 1));
        on_write(feed, move |_, _| writes.update(|n| *n += 1));
        text!("{changes} {writes}")
    })
    .wait_for_tasks(true);
    let mut driver = app.driver(20, 1);
    for step in 0..=10 {
        at(&mut driver, 50 * step);
    }
    // Eleven buffers (at 0, 50, …, 500 ms), every one written; the watch
    // ran once, as they are all equal.
    assert_eq!(first(&driver), "1 11");
}

#[test]
fn the_driver_waits_for_a_thread_still_making_its_value() {
    let app = App::new(|| {
        let mut n = 0;
        let made = latest_every(Duration::from_millis(100), move || {
            // Slower than a turn of the loop.
            std::thread::sleep(Duration::from_millis(20));
            n += 1;
            n
        });
        text!("{}", made.get().unwrap_or(0))
    })
    .wait_for_tasks(true);
    let mut driver = app.driver(10, 1);
    at(&mut driver, 0);
    assert_eq!(first(&driver), "1");
    for step in 1..=4 {
        at(&mut driver, 100 * step);
        assert_eq!(first(&driver), (step + 1).to_string());
    }
    // A jump: the beats it missed are not made up, as a timer's.
    at(&mut driver, 1000);
    assert_eq!(first(&driver), "6");
    at(&mut driver, 1099);
    assert_eq!(first(&driver), "6");
    at(&mut driver, 1100);
    assert_eq!(first(&driver), "7");
}

#[test]
fn rate_counts_the_writes_of_each_second() {
    let app = App::new(|| {
        let feed = latest_every(Duration::from_millis(20), || 0u8);
        let fps = rate(feed);
        text!("{fps}")
    })
    .wait_for_tasks(true);
    let mut driver = app.driver(10, 1);
    for step in 0..=50 {
        at(&mut driver, 20 * step);
    }
    // The meter read at 1 s: the buffers of 0 to 980 ms, equal or not.
    assert_eq!(first(&driver), "50");
    for step in 51..=75 {
        at(&mut driver, 20 * step);
    }
    assert_eq!(first(&driver), "50", "updated once a second");
    for step in 76..=100 {
        at(&mut driver, 20 * step);
    }
    assert_eq!(first(&driver), "50");
}

#[test]
fn a_builder_that_fails_gives_the_error_and_starts_nothing() {
    let started = Arc::new(AtomicBool::new(false));
    let flag = started.clone();
    let built = App::try_new(move || {
        let _ = latest(move |tx: intuituive::Latest<u8>| {
            flag.store(true, Ordering::SeqCst);
            tx.send(1);
        });
        Err::<Node, _>("no sound card")
    });
    assert_eq!(built.err(), Some("no sound card"));
    std::thread::sleep(Duration::from_millis(20));
    assert!(!started.load(Ordering::SeqCst), "the thread started");
    let app = App::try_new(|| Ok::<_, String>(label("ready"))).unwrap();
    let mut driver = app.driver(10, 1);
    driver.settle();
    assert_eq!(first(&driver), "ready");
}

#[test]
fn replace_sets_a_value_without_partial_eq() {
    struct Handle(#[allow(dead_code)] u8);
    let app = App::new(|| {
        let handle = signal(Handle(1));
        text!("{}", handle.with(|h| h.0)).on_key("r", move |_| {
            let old = handle.replace(Handle(2));
            assert_eq!(old.0, 1);
        })
    });
    let mut driver = app.driver(10, 1);
    driver.press("r");
    assert_eq!(first(&driver), "2");
}
