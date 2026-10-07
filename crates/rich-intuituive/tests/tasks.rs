//! Background work: tasks, futures and resources writing signals.

mod common;

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use common::{run, screen};
use intuituive::prelude::*;
use intuituive::spawn_future;
use rich_interact::headless::Script;

#[test]
fn a_tasks_result_can_open_a_screen() {
    let app = App::new(|| {
        label("home")
            .on_key("r", |_| {
                spawn(
                    || "report ready".to_string(),
                    |title, cx| {
                        cx.push(move || label(title).on_key("q", |cx| cx.quit()));
                    },
                );
            })
            .on_key("q", |_| panic!("the report screen has the keys"))
    })
    .wait_for_tasks(true);
    let rows = screen(&run(app, Script::new().keys("r q"), 20, 1));
    assert_eq!(rows[0], "report ready");
}

#[test]
fn a_cancelled_task_drops_its_result() {
    let app = App::new(|| {
        let status = signal("idle");
        let task = spawn(
            || std::thread::sleep(Duration::from_millis(20)),
            move |_, _| status.set("done"),
        );
        task.cancel();
        text!("{status}").on_key("q", |cx| cx.quit())
    })
    .wait_for_tasks(true);
    let rows = screen(&run(app, Script::new().keys("q"), 10, 1));
    assert_eq!(rows[0], "idle");
}

/// A future that is woken from another thread, as an I/O future is.
struct Later {
    state: Arc<Mutex<(bool, Option<Waker>)>>,
}

impl Later {
    fn new(after: Duration) -> Later {
        let state: Arc<Mutex<(bool, Option<Waker>)>> = Arc::new(Mutex::new((false, None)));
        let shared = state.clone();
        std::thread::spawn(move || {
            std::thread::sleep(after);
            let mut state = shared.lock().unwrap();
            state.0 = true;
            if let Some(waker) = state.1.take() {
                waker.wake();
            }
        });
        Later { state }
    }
}

impl Future for Later {
    type Output = &'static str;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<&'static str> {
        let mut state = self.state.lock().unwrap();
        if state.0 {
            Poll::Ready("woken")
        } else {
            state.1 = Some(cx.waker().clone());
            Poll::Pending
        }
    }
}

#[test]
fn a_future_runs_to_completion_off_the_apps_thread() {
    let app = App::new(|| {
        let status = signal("waiting");
        spawn_future(Later::new(Duration::from_millis(10)), move |out, _| {
            status.set(out)
        });
        text!("{status}").on_key("q", |cx| cx.quit())
    })
    .wait_for_tasks(true);
    let rows = screen(&run(app, Script::new().keys("q"), 10, 1));
    assert_eq!(rows[0], "woken");
}

#[test]
fn a_resource_reloads_and_reports_failures() {
    let calls = Arc::new(AtomicU32::new(0));
    let app = App::new(move || {
        let calls = calls.clone();
        let data = resource(move || match calls.fetch_add(1, Ordering::SeqCst) {
            0 => Ok(1u32),
            _ => Err("offline"),
        });
        text(move || match data.get() {
            Load::Loading => "loading".into(),
            Load::Ready(n) => format!("got {n}"),
            Load::Failed(error) => format!("failed: {error}"),
        })
        .on_key("r", move |_| data.reload())
        .on_key("q", |cx| cx.quit())
    })
    .wait_for_tasks(true);
    let record = run(app, Script::new().keys("r q"), 20, 1);
    let frames = &record.frames;
    assert!(frames.iter().any(|f| f.trim_end() == "got 1"), "{frames:?}");
    assert_eq!(screen(&record)[0], "failed: offline");
}

#[test]
fn a_stale_resource_result_is_dropped() {
    let app = App::new(|| {
        let n = Arc::new(AtomicU32::new(0));
        let started = n.clone();
        let data = resource(move || {
            // The first fetch is slow; the reload's is quick and wins.
            let mine = n.fetch_add(1, Ordering::SeqCst);
            if mine == 0 {
                std::thread::sleep(Duration::from_millis(50));
            }
            Ok::<_, String>(mine)
        });
        // Reload once the first fetch is running (and has taken 0).
        while started.load(Ordering::SeqCst) == 0 {
            std::thread::yield_now();
        }
        data.reload();
        text(move || format!("{:?}", data.get())).on_key("q", |cx| cx.quit())
    })
    .wait_for_tasks(true);
    let record = run(app, Script::new().keys("q"), 20, 1);
    assert_eq!(screen(&record)[0], "Ready(1)");
    assert!(record.frames.iter().all(|f| f.trim_end() != "Ready(0)"));
}

#[test]
fn waiting_covers_tasks_that_results_start() {
    let app = App::new(|| {
        let status = signal("idle");
        spawn(
            || (),
            move |_, _| {
                status.set("first");
                spawn(|| (), move |_, _| status.set("second"));
            },
        );
        text!("{status}").on_key("q", |cx| cx.quit())
    })
    .wait_for_tasks(true);
    let rows = screen(&run(app, Script::new().keys("q"), 10, 1));
    assert_eq!(rows[0], "second");
}
