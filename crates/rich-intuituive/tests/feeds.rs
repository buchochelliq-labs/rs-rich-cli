//! Signals over time: a thread feed with a state (`latest_with_state`), an
//! iterator played at a pace (`latest_iter`), back and forward through a
//! signal's past (`history`), its last writes (`recent`), a number easing
//! after another (`smoothed`) and `on_write_ref`.

use std::time::Duration;

use intuituive::prelude::*;
use intuituive::{history, latest_iter, latest_with_state, recent, smoothed, Pace};

fn first(driver: &intuituive::Driver) -> String {
    driver.screen().plain()[0].trim_end().to_string()
}

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

#[test]
fn a_feed_stalls_runs_again_and_ends() {
    let app = App::new(|| {
        let (value, state) = latest_with_state(ms(300), |tx| {
            tx.send(1);
            // Quiet for 500 ms: stalled from 300 ms.
            tx.sleep(ms(500));
            tx.send(2);
            tx.sleep(ms(100));
        });
        text(move || format!("{:?} {:?}", value.get(), state.get()))
    })
    .wait_for_tasks(true);
    let mut driver = app.driver(40, 1);
    driver.advance(ms(200));
    assert_eq!(first(&driver), "Some(1) Running");
    driver.advance(ms(200));
    assert_eq!(first(&driver), "Some(1) Stalled(0ns)");
    driver.advance(ms(150));
    assert_eq!(first(&driver), "Some(2) Running");
    driver.advance(ms(100));
    assert_eq!(first(&driver), "Some(2) Ended");
}

#[test]
fn a_feed_that_fails_or_panics_says_so() {
    let app = App::new(|| {
        let (_, failed) = latest_with_state(None, |tx: intuituive::Latest<u8>| {
            tx.send(0);
            Err::<(), _>(std::io::Error::other("no device"))
        });
        let (_, panicked) = latest_with_state::<u8, ()>(None, |_| panic!("boom"));
        text(move || format!("{:?} {:?}", failed.get(), panicked.get()))
    })
    .wait_for_tasks(true);
    let mut driver = app.driver(70, 1);
    driver.advance(ms(10));
    assert_eq!(
        first(&driver),
        "Failed(\"no device\") Failed(\"the feed's thread panicked\")"
    );
}

#[test]
fn an_iterator_plays_at_its_pace_then_what_comes_after() {
    let app = App::new(|| {
        let played = signal(Vec::new());
        let value = latest_iter(["a", "b", "c"], Pace::Every(ms(50))).then_with(|tx| {
            tx.sleep(ms(50));
            tx.send("end");
        });
        on_write(value, move |v, _| {
            played.update(|p| p.push(v.unwrap_or("-")))
        });
        text(move || played.get().join(" "))
    })
    .wait_for_tasks(true);
    let mut driver = app.driver(20, 1);
    driver.advance(ms(120));
    assert_eq!(first(&driver), "a b c");
    driver.advance(ms(40));
    assert_eq!(first(&driver), "a b c end");
}

#[test]
fn a_free_iterator_plays_everything_at_once_in_a_test() {
    let app = App::new(|| {
        let value = latest_iter(0..1000, Pace::Free).signal();
        let count = signal(0);
        on_write(value, move |_, _| count.update(|n| *n += 1));
        text!("{} {count}", value.get().unwrap_or(-1))
    })
    .wait_for_tasks(true);
    let mut driver = app.driver(20, 1);
    driver.settle();
    // The newest wins: the last value, whatever was skipped.
    assert!(first(&driver).starts_with("999 "), "{}", first(&driver));
}

#[test]
fn history_goes_back_and_forward_without_recording_its_moves() {
    let app = App::new(|| {
        let page = signal(1);
        let past = history(page);
        text(move || format!("{} {} {}", page.get(), past.can_back(), past.can_forward()))
            .on_key("n", move |_| page.update(|p| *p += 1))
            .on_key("b", move |_| {
                past.back();
            })
            .on_key("f", move |_| {
                past.forward();
            })
    });
    let mut driver = app.driver(20, 1);
    assert_eq!(
        {
            driver.settle();
            first(&driver)
        },
        "1 false false"
    );
    driver.press("n n");
    assert_eq!(first(&driver), "3 true false");
    driver.press("b b");
    assert_eq!(first(&driver), "1 false true");
    // Past the start: nothing.
    driver.press("b");
    assert_eq!(first(&driver), "1 false true");
    driver.press("f");
    assert_eq!(first(&driver), "2 true true");
    // A new value drops what was after.
    driver.press("n n");
    assert_eq!(first(&driver), "4 true false");
    driver.press("b b b");
    assert_eq!(first(&driver), "1 false true");
}

#[test]
fn history_keeps_a_limit() {
    let app = App::new(|| {
        let page = signal(0);
        let past = history(page).limit(3);
        text!("{page}")
            .on_key("n", move |_| page.update(|p| *p += 1))
            .on_key("b", move |_| {
                past.back();
            })
    });
    let mut driver = app.driver(5, 1);
    driver.press("n n n n b b b b");
    assert_eq!(first(&driver), "2");
}

#[test]
fn recent_keeps_the_last_writes_and_follows_its_length() {
    let app = App::new(|| {
        let (level, n) = (signal(0), signal(3usize));
        let last = recent(level, n);
        text(move || last.with(|l| format!("{l:?}")))
            // An equal value is a write too.
            .on_key("w", move |_| level.update(|_| {}))
            .on_key("+", move |_| level.update(|l| *l += 1))
            .on_key("s", move |_| n.set(1))
    });
    let mut driver = app.driver(20, 1);
    driver.press("+ w + +");
    assert_eq!(first(&driver), "[1, 2, 3]");
    driver.press("s");
    assert_eq!(first(&driver), "[3]");
}

#[test]
fn smoothed_rises_with_its_attack_and_falls_with_its_release() {
    let app = App::new(|| {
        let level = signal(0.0f32);
        let needle = smoothed(level, ms(50), ms(500));
        text(move || format!("{:.3}", needle.get()))
            .on_key("u", move |_| level.set(1.0))
            .on_key("d", move |_| level.set(0.0))
    });
    let mut driver = app.driver(10, 1);
    let shown = |driver: &intuituive::Driver| -> f64 { first(driver).parse().unwrap() };
    driver.press("u");
    driver.advance(ms(50));
    let up = shown(&driver);
    assert!((0.5..0.8).contains(&up), "{up}");
    driver.advance(ms(500));
    assert_eq!(shown(&driver), 1.0);
    driver.press("d");
    driver.advance(ms(50));
    // Falling ten times slower: still near the top.
    let down = shown(&driver);
    assert!((0.85..0.95).contains(&down), "{down}");
    driver.advance(ms(5000));
    assert_eq!(shown(&driver), 0.0);
    // There: nothing more is due.
    assert!(driver.timeout(Duration::from_secs(60)) >= ms(50));
}

#[test]
fn on_write_ref_sees_each_turns_value_without_a_clone() {
    struct Frame(Vec<u32>);
    let app = App::new(|| {
        let feed = latest_iter((0..5).map(|n| Frame(vec![n; 4])), Pace::Every(ms(10))).signal();
        let (sums, other) = (signal(Vec::new()), signal(0));
        on_write_ref(feed, move |frame: &Option<Frame>, _| {
            if let Some(frame) = frame {
                // It may write other signals.
                sums.update(|s| s.push(frame.0.iter().sum::<u32>()));
                other.update(|o| *o += 1);
            }
        });
        text(move || format!("{:?} {other}", sums.get()))
    })
    .wait_for_tasks(true);
    let mut driver = app.driver(30, 1);
    driver.advance(ms(60));
    assert_eq!(first(&driver), "[0, 4, 8, 12, 16] 5");
}
