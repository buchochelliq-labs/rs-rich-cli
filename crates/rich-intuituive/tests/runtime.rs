//! The runtime and the world outside: `latest`, the newest value from a
//! thread, and `Ctx::handoff`, the terminal handed to another program.

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
