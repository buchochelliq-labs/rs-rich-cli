//! Round 3 of the demo gaps, the runtime: one-shot timers on the app's
//! clock, stepping the clock, sequences in help sections, cycling back,
//! closing an app that was never finished, continuations that stay on the
//! app's thread, streams counted as tasks, unmount hooks, `render_with`
//! without a quit, the clock and accessible text for widgets, and terminal
//! graphics beside the cells.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use intuituive::prelude::*;
use intuituive::widget::{Canvas, DrawCx};
use intuituive::{after, spawn_stream, spawn_then};

fn first(driver: &intuituive::Driver) -> String {
    driver.screen().plain()[0].trim_end().to_string()
}

#[test]
fn after_runs_once_on_the_apps_clock() {
    let app = App::new(|| {
        let said = signal(String::from("-"));
        after(Duration::from_millis(300), move |_| {
            said.set("built".into())
        });
        text!("{said}").on_key("x", move |cx| {
            // From a handler: `cx.after`, and `after` itself.
            cx.after(Duration::from_millis(100), move |_| said.set("cx".into()));
            after(Duration::from_millis(200), move |_| said.set("free".into()));
        })
    });
    let mut driver = app.driver(10, 1);
    driver.press("x");
    driver.advance(Duration::from_millis(99));
    assert_eq!(first(&driver), "-");
    driver.advance(Duration::from_millis(1));
    assert_eq!(first(&driver), "cx");
    driver.advance(Duration::from_millis(100));
    assert_eq!(first(&driver), "free");
    driver.advance(Duration::from_millis(100));
    assert_eq!(first(&driver), "built");
    // Once: nothing runs again.
    driver.advance(Duration::from_secs(1));
    assert_eq!(first(&driver), "built");
}

#[test]
fn advance_steps_through_each_thing_due() {
    let app = App::new(|| {
        let ticks = signal(Vec::<u128>::new());
        let at = signal(Duration::ZERO);
        every(Duration::from_millis(30), move |_| {
            ticks.update(|t| t.push(at.get_untracked().as_millis()))
        });
        every(Duration::from_millis(1), move |_| {
            at.update(|a| *a += Duration::from_millis(1))
        });
        text(move || format!("{:?}", ticks.get()))
    });
    let mut driver = app.driver(30, 1);
    driver.advance(Duration::from_millis(95));
    // Each 30 ms tick ran at its time, not all at the end.
    assert_eq!(first(&driver), "[29, 59, 89]");
}

#[test]
fn sequences_sit_in_help_sections_and_modes_cycle_back() {
    let app = App::new(|| {
        let mode = signal(1usize);
        text!("{mode}")
            .bind_sequence_in("goto", "g h", "Go home", |_| {})
            .bind_cycle("tab", "Next mode", mode, &[0, 1, 2])
            .bind_cycle_back("shift+tab", "Previous mode", mode, &[0, 1, 2])
            .on_key("?", |cx| cx.help())
    });
    let mut driver = app.driver(60, 16);
    driver.press("shift+tab");
    assert_eq!(first(&driver), "0");
    driver.press("shift+tab");
    assert_eq!(first(&driver), "2");
    driver.press("tab");
    assert_eq!(first(&driver), "0");
    driver.press("?");
    let help = driver.screen().plain().join("\n");
    let section = help.find("goto").expect("a goto section");
    assert!(section < help.find("Go home").unwrap(), "{help}");
}

#[test]
fn a_driver_dropped_unfinished_ends_its_latest_threads() {
    let ended = Arc::new(AtomicBool::new(false));
    let flag = ended.clone();
    let app = App::new(move || {
        let flag = flag.clone();
        let value = intuituive::latest(move |tx| {
            // Asleep on the app's clock, which a test never moves on.
            while tx.send(1) && tx.sleep(Duration::from_secs(3600)) {}
            flag.store(true, Ordering::SeqCst);
        });
        text(move || format!("{:?}", value.get()))
    })
    .wait_for_tasks(true);
    let mut driver = app.driver(10, 1);
    driver.settle();
    drop(driver);
    let start = Instant::now();
    while !ended.load(Ordering::SeqCst) {
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "the thread still sleeps"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn spawn_then_keeps_its_continuation_on_the_apps_thread() {
    let seen = Rc::new(Cell::new(0));
    let kept = seen.clone();
    let app = App::new(move || {
        let kept = kept.clone();
        let shown = signal(0);
        spawn_then(
            || 6 * 7,
            move |n, _| {
                kept.set(n);
                shown.set(n);
            },
        );
        // A cancelled one does not run.
        let task = spawn_then(|| 1, move |n, _| shown.set(n));
        task.cancel();
        text!("{shown}")
    })
    .wait_for_tasks(true);
    let mut driver = app.driver(10, 1);
    driver.settle();
    assert_eq!(first(&driver), "42");
    assert_eq!(seen.get(), 42);
}

#[test]
fn a_stream_is_a_task_until_its_thread_returns() {
    let app = App::new(|| {
        let lines = signal(Vec::<u32>::new());
        let task = signal(None);
        task.replace(Some(spawn_stream(move |stream| {
            for n in 0..50 {
                std::thread::sleep(Duration::from_millis(1));
                stream.proxy().run(move || lines.update(|l| l.push(n)));
            }
        })));
        text(move || {
            let done = task.with(|t: &Option<intuituive::Task>| t.as_ref().unwrap().is_finished());
            format!("{} {done}", lines.with(Vec::len))
        })
    })
    .wait_for_tasks(true);
    let mut driver = app.driver(10, 1);
    driver.settle();
    // One turn waited for the whole stream.
    assert!(first(&driver).starts_with("50"), "{}", first(&driver));
}

#[test]
fn a_cancelled_stream_is_told_to_stop() {
    let stopped = Arc::new(AtomicBool::new(false));
    let flag = stopped.clone();
    let gate = Arc::new(Mutex::new(()));
    let held = gate.lock().unwrap();
    let inner = gate.clone();
    let app = App::new(move || {
        let flag = flag.clone();
        let inner = inner.clone();
        let task = spawn_stream(move |stream| {
            let _wait = inner.lock().unwrap();
            if stream.cancelled() {
                flag.store(true, Ordering::SeqCst);
            }
        });
        task.cancel();
        label("x")
    });
    let mut driver = app.driver(4, 1);
    driver.settle();
    drop(held);
    let start = Instant::now();
    while !stopped.load(Ordering::SeqCst) {
        assert!(start.elapsed() < Duration::from_secs(5));
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn on_unmount_runs_when_a_node_leaves_the_tree() {
    let gone = Rc::new(Cell::new(Vec::<String>::new()));
    let log = gone.clone();
    let note = move |what: &str| {
        let log = log.clone();
        let what = what.to_string();
        move || {
            let mut all = log.take();
            all.push(what);
            log.set(all);
        }
    };
    let app = App::new(move || {
        let rows = signal(vec![1, 2, 3]);
        let row_note = note.clone();
        let screen_note = note.clone();
        column([
            each(
                move || rows.get(),
                move |n| text!("row {n}").on_unmount(row_note(&format!("row {n}"))),
            ),
            label("main").on_unmount(note("main")),
        ])
        .on_key("d", move |_| rows.update(|r| r.retain(|n| *n != 2)))
        .on_key("s", move |cx| {
            let screen_note = screen_note.clone();
            cx.push(move || {
                label("search")
                    .on_unmount(screen_note("search"))
                    .on_key("esc", |cx| cx.pop())
            })
        })
    });
    let mut driver = app.driver(10, 4);
    driver.settle();
    let seen = |gone: &Rc<Cell<Vec<String>>>| {
        let all = gone.take();
        gone.set(all.clone());
        all
    };
    assert!(seen(&gone).is_empty());
    // A keyed row that goes.
    driver.press("d");
    assert_eq!(seen(&gone), ["row 2"]);
    // A screen that closes.
    driver.press("s");
    assert_eq!(seen(&gone), ["row 2"]);
    driver.press("esc");
    assert_eq!(seen(&gone), ["row 2", "search"]);
    // The app's end takes the rest.
    drop(driver);
    let mut all = seen(&gone);
    all.sort();
    assert_eq!(all, ["main", "row 1", "row 2", "row 3", "search"]);
}

#[test]
fn render_with_needs_no_quit_and_settles_what_a_rect_loads() {
    // A watch keyed on a node's width loads after the first frame.
    let app = App::new(|| {
        let body = label("body");
        let handle = body.rect_handle();
        let shown = signal(String::from("-"));
        watch(
            move || handle.get().map(|r| r.width),
            move |width, _| shown.set(format!("width {width:?}")),
        );
        column([body.fixed(1), text!("{shown}")])
    });
    let screen = app.render_with(&[], 20, 2).unwrap();
    assert_eq!(screen[1].trim_end(), "width Some(20)");
    // An error the app failed with is returned.
    let app = App::new(|| label("x").on_key("f", |cx| cx.fail("no device")));
    let error = app.render_with(&["f"], 4, 1).unwrap_err();
    assert_eq!(error.to_string(), "no device");
}

/// A level meter: blocks on screen, words for a screen reader; it holds
/// its peak for a second of the app's clock.
struct Meter {
    level: Signal<u32>,
    peak: (u32, Duration),
}

impl Widget for Meter {
    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        let level = self.level.get();
        let now = cx.now();
        if level >= self.peak.0 || now >= self.peak.1 + Duration::from_secs(1) {
            self.peak = (level, now);
        }
        canvas.print(0, 0, &"█".repeat(level as usize), None);
        canvas.print(self.peak.0 as u16, 0, "|", None);
    }

    fn access_text(&self) -> Option<String> {
        Some(format!("Level {}", self.peak.0))
    }
}

#[test]
fn a_widget_reads_the_apps_clock_and_says_what_it_shows() {
    let app = App::new(|| {
        let level = signal(5u32);
        every(Duration::from_millis(100), move |_| {
            level.update(|l| *l = l.saturating_sub(1))
        });
        widget(Meter {
            level,
            peak: (0, Duration::ZERO),
        })
        .label("Input")
    });
    let mut driver = app.driver(10, 1);
    driver.settle();
    assert_eq!(first(&driver), "█████|");
    // Falling: the peak holds for a second of the app's clock, exactly.
    driver.advance(Duration::from_millis(900));
    assert_eq!(first(&driver), "     |");
    driver.advance(Duration::from_millis(200));
    assert_eq!(first(&driver), "|");
    let tree = driver.accessibility();
    assert_eq!(tree[0].name, "Input: Level 0");
    assert_eq!(tree[0].role, intuituive::a11y::Role::Text);
}

#[derive(Debug)]
struct Picture;

impl rich_ext::frame::Graphic for Picture {
    fn key(&self) -> u64 {
        7
    }
    fn draw(&self, frame: usize) -> String {
        format!("<picture {frame}>")
    }
}

/// Places the picture on the two cells after an `@`.
struct Pictures;

impl rich_ext::frame::PlacementSource for Pictures {
    fn placements(&self, frame: &rich_ext::frame::Frame) -> Vec<rich_ext::frame::Placement> {
        let text = frame.plain();
        text.lines()
            .enumerate()
            .filter_map(|(row, line)| {
                line.find('@').map(|column| rich_ext::frame::Placement {
                    row,
                    column: column + 1,
                    cols: 2,
                    rows: 1,
                    graphic: Arc::new(Picture),
                    frame: 0,
                })
            })
            .collect()
    }

    fn release(&self, _retained: &[rich_ext::frame::Placement]) -> String {
        "<release>".into()
    }
}

#[test]
fn graphics_are_drawn_beside_the_cells_when_they_need_to_be() {
    let app = App::new(|| {
        let at = signal(0usize);
        let tick = signal(0);
        column([
            text(move || format!("{}@..", " ".repeat(at.get()))).fixed(1),
            text!("{tick}").fixed(1),
        ])
        .on_key("m", move |_| at.update(|a| *a += 2))
        .on_key("t", move |_| tick.update(|t| *t += 1))
    })
    .graphics(Arc::new(Pictures));
    let mut driver = app.driver(10, 2);
    driver.update(Duration::ZERO);
    let out = driver.render().unwrap();
    assert!(out.contains("<picture 0>"), "{out:?}");
    // A change elsewhere: the picture stays as it was.
    driver.event(intuituive::interact::Event::Key(
        intuituive::interact::Key::char('t'),
    ));
    driver.update(Duration::ZERO);
    let out = driver.render().unwrap();
    assert!(!out.contains("<picture"), "{out:?}");
    // Moved: drawn again where it is now.
    driver.event(intuituive::interact::Event::Key(
        intuituive::interact::Key::char('m'),
    ));
    driver.update(Duration::ZERO);
    let out = driver.render().unwrap();
    assert!(out.contains("<picture 0>"), "{out:?}");
    assert!(driver.finish().contains("<release>"));
}
