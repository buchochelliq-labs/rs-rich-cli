//! Input and timing: the loop drains the input waiting before it draws,
//! `Driver::event` settles, widgets leave modified keys they do not use to
//! their ancestors, held keys and releases, key repeat under the app's
//! control, timers with a handle, a fixed step that catches up, and frame
//! pacing.

use std::time::Duration;

use intuituive::interact::headless::{Headless, Script};
use intuituive::interact::{Event, Key};
use intuituive::prelude::*;

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

fn first(driver: &intuituive::Driver) -> String {
    driver.screen().plain()[0].trim_end().to_string()
}

fn trimmed(frames: &[String]) -> Vec<String> {
    frames.iter().map(|f| f.trim_end().to_string()).collect()
}

/// raycast: six presses queued while a frame was slow were handled one a
/// frame. They are all handled before the next frame is drawn.
#[test]
fn keys_queued_together_are_handled_before_one_frame() {
    let app = App::new(|| {
        let n = signal(0);
        text!("{n}")
            .on_key("e", move |_| n.update(|n| *n += 1))
            .on_key("q", |cx| cx.quit())
    });
    let script = Script::new()
        .wait(ms(10))
        .keys("e e e e e e")
        .wait(ms(10))
        .keys("q");
    let mut backend = Headless::new(script, 5, 1);
    backend.queued = true;
    let record = backend.record();
    app.run_on(&mut backend).unwrap();
    assert_eq!(trimmed(&record.borrow().frames), ["0", "6"]);
}

/// top: a mouse event sent with `Driver::event` needed a `settle()` before
/// the screen showed it; `press` never did. `send` settles as `press`
/// does.
#[test]
fn a_sent_event_settles_as_a_press_does() {
    use intuituive::interact::{Mouse, MouseKind};
    let app = App::new(|| {
        let n = signal(0);
        text!("{n}").on_mouse(move |_, mouse| {
            let wheel = mouse.kind == MouseKind::ScrollDown;
            if wheel {
                n.update(|n| *n += 1);
            }
            wheel
        })
    });
    let mut driver = app.driver(5, 1);
    driver.send(Event::Mouse(Mouse::new(MouseKind::ScrollDown, 0, 0)));
    assert_eq!(first(&driver), "1");
    driver.send(Event::Key(Key::char('x')));
    driver.send(Event::Mouse(Mouse::new(MouseKind::ScrollDown, 0, 0)));
    assert_eq!(first(&driver), "2");
}

/// top: a table and a list took Shift+↑/↓ as ↑/↓, so an ancestor's
/// `shift+up` binding never fired while one had the focus.
#[test]
fn a_table_leaves_shift_arrows_to_its_ancestors() {
    use intuituive::widgets::{table, Column};
    let app = App::new(|| {
        let (selected, moved) = (signal(1usize), signal(0));
        column([
            table(
                vec![Column::new("Name", Size::Auto)],
                || vec![vec!["a".into()], vec!["b".into()], vec!["c".into()]],
                selected,
            )
            .size(Size::Fixed(4)),
            text!("{selected} {moved}"),
        ])
        .on_key("shift+up", move |_| moved.update(|m| *m += 1))
        .on_key("shift+down", move |_| moved.update(|m| *m += 10))
    });
    let mut driver = app.driver(10, 5);
    driver.press("shift+up shift+down");
    assert_eq!(driver.screen().plain()[4].trim_end(), "1 11");
    driver.press("up");
    assert_eq!(driver.screen().plain()[4].trim_end(), "0 11");
}

#[test]
fn a_list_leaves_shift_arrows_to_its_ancestors() {
    let app = App::new(|| {
        let (selected, moved) = (signal(1usize), signal(0));
        column([
            list(|| vec!["a".into(), "b".into(), "c".into()], selected).size(Size::Fixed(3)),
            text!("{selected} {moved}"),
        ])
        .on_key("shift+up", move |_| moved.update(|m| *m += 1))
        .on_key("shift+down", move |_| moved.update(|m| *m += 10))
    });
    let mut driver = app.driver(10, 4);
    driver.press("shift+up shift+down");
    assert_eq!(driver.screen().plain()[3].trim_end(), "1 11");
    driver.press("down");
    assert_eq!(driver.screen().plain()[3].trim_end(), "2 11");
}

/// blocks: `every` returned nothing, so a game ticked for the app's whole
/// life and counted frames itself. Its timer pauses, resumes, changes
/// period and is cancelled.
#[test]
fn a_timer_pauses_resumes_changes_its_period_and_cancels() {
    let app = App::new(|| {
        let ticks = signal(0);
        let timer = every(ms(100), move |_| ticks.update(|t| *t += 1));
        text!("{ticks}")
            .on_key("p", move |_| timer.pause())
            .on_key("r", move |_| timer.resume())
            .on_key("f", move |_| timer.set_period(ms(10)))
            .on_key("c", move |_| timer.cancel())
    });
    let mut driver = app.driver(5, 1);
    driver.advance(ms(250));
    assert_eq!(first(&driver), "2");
    // Paused with 50 ms left: nothing for a second, then due 50 ms after.
    driver.press("p");
    driver.advance(ms(1000));
    assert_eq!(first(&driver), "2");
    driver.press("r");
    driver.advance(ms(49));
    assert_eq!(first(&driver), "2");
    driver.advance(ms(1));
    assert_eq!(first(&driver), "3");
    // Every 10 ms from the last tick.
    driver.press("f");
    driver.advance(ms(100));
    assert_eq!(first(&driver), "13");
    driver.press("c");
    driver.advance(ms(100));
    assert_eq!(first(&driver), "13");
}

/// A timer started from a handler: none before, one a period after.
#[test]
fn a_handler_starts_a_timer_and_a_screen_closing_stops_it() {
    let app = App::new(|| {
        let ticks = signal(0);
        text!("{ticks}").on_key("m", move |cx| {
            cx.modal(Size::Fixed(5), Size::Fixed(1), move || {
                label("over")
                    .on_key("s", move |cx| {
                        cx.every(ms(10), move |_| ticks.update(|t| *t += 1));
                    })
                    .on_key("x", |cx| cx.pop())
            });
        })
    });
    let mut driver = app.driver(10, 3);
    driver.press("m s");
    driver.advance(ms(55));
    driver.press("x");
    driver.advance(ms(100));
    assert_eq!(first(&driver), "5");
}

/// raycast: a timer's function was not told the time nor how many ticks it
/// missed, so the game measured the wall clock, which a test cannot step.
#[test]
fn a_timer_is_told_the_apps_clock_and_the_ticks_it_missed() {
    let app = App::new(|| {
        let seen = signal(String::new());
        every(ms(10), move |cx| {
            seen.set(format!("{:?} {}", cx.now(), cx.missed_ticks()))
        });
        text!("{seen}")
    });
    let mut driver = app.driver(20, 1);
    driver.advance(ms(10));
    assert_eq!(first(&driver), "10ms 0");
    driver.update(ms(55));
    driver.settle();
    assert_eq!(first(&driver), "55ms 3");
    // Back in step: the next tick is at 60 ms.
    driver.advance(ms(5));
    assert_eq!(first(&driver), "60ms 0");
}

/// A fixed step is exact under `advance` and catches up after a stall,
/// at most `max_catch_up` steps a call.
#[test]
fn a_fixed_step_catches_up_and_is_exact_under_advance() {
    let app = App::new(|| {
        let (steps, calls) = (signal(0u64), signal(0u64));
        fixed_step(ms(28), move |_, n| {
            steps.update(|s| *s += n);
            calls.update(|c| *c += 1);
        })
        .max_catch_up(5);
        text!("{steps} {calls}")
    });
    let mut driver = app.driver(10, 1);
    driver.advance(ms(28 * 35));
    assert_eq!(first(&driver), "35 35");
    // 20 steps owed: five run, the rest dropped, and on in step.
    driver.update(ms(28 * 55));
    driver.settle();
    assert_eq!(first(&driver), "40 36");
    driver.advance(ms(28));
    assert_eq!(first(&driver), "41 37");
}

/// `after` returns its timer: cancelled, it never calls.
#[test]
fn an_after_can_be_cancelled_or_held() {
    let app = App::new(|| {
        let said = signal("waiting");
        let gone = after(ms(100), move |_| said.set("gone"));
        let held = after(ms(100), move |_| said.set("held"));
        text!("{said}")
            .on_key("c", move |_| gone.cancel())
            .on_key("p", move |_| held.pause())
            .on_key("r", move |_| held.resume())
    });
    let mut driver = app.driver(10, 1);
    driver.press("c p");
    driver.advance(ms(500));
    assert_eq!(first(&driver), "waiting");
    driver.press("r");
    driver.advance(ms(100));
    assert_eq!(first(&driver), "held");
}

/// Doom: no target frame rate that skips frames, and `App::stats` out of
/// reach once `run` had the app. A timer at 100 Hz draws at most 20
/// frames a second; the frames it skipped are counted; the stats are
/// read after the run.
#[test]
fn max_fps_skips_frames_and_the_stats_outlive_the_run() {
    let app = App::new(|| {
        let n = signal(0);
        every(ms(10), move |_| n.update(|n| *n += 1));
        text!("{n}").on_key("q", |cx| cx.quit())
    })
    .max_fps(20);
    let stats = app.stats_handle();
    let script = Script::new().wait(ms(1000)).keys("q");
    let mut backend = Headless::new(script, 5, 1);
    let record = backend.record();
    app.run_on(&mut backend).unwrap();
    let stats = stats.get();
    assert!((20..=22).contains(&stats.frames), "{stats:?}");
    assert!(stats.skipped >= 70, "{stats:?}");
    // The newest value is drawn: the last frame is at most a frame behind.
    let last: u32 = record.borrow().last_frame().trim().parse().unwrap();
    assert!(last >= 95, "{last}");
}

/// Without `max_fps`, every tick draws.
#[test]
fn without_max_fps_every_change_draws() {
    let app = App::new(|| {
        let n = signal(0);
        every(ms(10), move |_| n.update(|n| *n += 1));
        text!("{n}").on_key("q", |cx| cx.quit())
    });
    let stats = app.stats_handle();
    let script = Script::new().wait(ms(1000)).keys("q");
    app.run_on(&mut Headless::new(script, 5, 1)).unwrap();
    assert_eq!(stats.get().frames, 101);
    assert_eq!(stats.get().skipped, 0);
}

fn up(name: &str) -> Event {
    Event::KeyUp(Key::parse(name).unwrap())
}

/// raycast, blocks: key state. Where releases are reported, keys are held
/// from press to release, a binding is told which key fired it, and a
/// release binding runs on the release, whatever modifiers are held by
/// then.
#[test]
fn held_keys_releases_and_the_key_that_fired() {
    use intuituive::interact::Releases;
    let app = App::new(|| {
        let said = signal(String::new());
        text!("{said}")
            .bind("w up", "Walk", move |cx| {
                let key = cx.key().unwrap();
                let held = cx.is_held("w") as u8;
                said.set(format!("walk {key} {held}"));
            })
            .bind_release("w", "Stop", move |cx| {
                let held = cx.held_keys().len();
                said.set(format!("stop {} {held}", cx.key().unwrap()));
            })
    });
    let mut driver = app.driver(20, 1);
    driver.set_releases(Releases::All);
    driver.press("w");
    assert_eq!(first(&driver), "walk w 1");
    driver.press("up");
    assert_eq!(first(&driver), "walk up 1");
    // Let go with Shift down: `W`.
    driver.send(up("W"));
    assert_eq!(first(&driver), "stop W 1");
    driver.send(up("up"));
    driver.press("up");
    assert_eq!(first(&driver), "walk up 0");
}

/// Without releases, nothing is held: a key would be held for good.
#[test]
fn without_releases_no_key_is_held() {
    let app = App::new(|| {
        let n = signal(0usize);
        text!("{n}").on_key("w", move |cx| n.set(cx.held_keys().len()))
    });
    let mut driver = app.driver(5, 1);
    driver.press("w w");
    assert_eq!(first(&driver), "0");
}

/// blocks: DAS and ARR. Held, the binding fires at once, after the delay,
/// then at the rate, and the terminal's own repeats are passed over; it
/// stops on the release. Exact under `advance`.
#[test]
fn a_repeating_binding_keeps_its_own_delay_and_rate() {
    use intuituive::interact::Releases;
    let app = App::new(|| {
        let x = signal(0);
        text!("{x}")
            .bind("left", "Left", move |_| x.update(|x| *x += 1))
            .repeat(ms(167), ms(33))
    });
    let mut driver = app.driver(5, 1);
    driver.set_releases(Releases::ExceptText);
    driver.press("left");
    assert_eq!(first(&driver), "1");
    driver.advance(ms(166));
    assert_eq!(first(&driver), "1");
    driver.advance(ms(1));
    assert_eq!(first(&driver), "2");
    // The terminal repeats: passed over.
    driver.press("left left");
    driver.advance(ms(33 * 3));
    assert_eq!(first(&driver), "5");
    driver.send(up("left"));
    driver.advance(ms(500));
    assert_eq!(first(&driver), "5");
    // Pressed again: at once, and the delay again.
    driver.press("left");
    driver.advance(ms(100));
    assert_eq!(first(&driver), "6");
}

/// With no releases (and none made up) a repeating binding falls back to
/// the terminal's repeats: each press fires.
#[test]
fn a_repeating_binding_without_releases_fires_on_each_press() {
    let app = App::new(|| {
        let x = signal(0);
        text!("{x}")
            .bind("left", "Left", move |_| x.update(|x| *x += 1))
            .repeat(ms(167), ms(33))
    });
    let mut driver = app.driver(5, 1);
    driver.press("left left left");
    driver.advance(ms(500));
    assert_eq!(first(&driver), "3");
}

/// Releases made up: a key is let go `hold` after its last press or
/// repeat, so holding (repeats coming) keeps it held and a tap ends.
#[test]
fn synthesized_releases_end_a_hold_after_the_timeout() {
    let app = App::new(|| {
        let (x, said) = (signal(0), signal(""));
        text!("{x} {said}")
            .bind("d", "Right", move |_| x.update(|x| *x += 1))
            .repeat(ms(100), ms(50))
            .on_key_up("d", move |_| said.set("up"))
    })
    .synthesize_releases(Some(ms(300)));
    let mut driver = app.driver(10, 1);
    driver.press("d");
    // The terminal's repeats at 250 and 280 ms keep it held.
    driver.advance(ms(250));
    driver.press("d");
    driver.advance(ms(30));
    driver.press("d");
    assert_eq!(first(&driver), "5");
    // 300 ms after the last repeat: let go.
    driver.advance(ms(299));
    assert_eq!(first(&driver), "11");
    driver.advance(ms(1));
    assert_eq!(first(&driver), "11 up");
    driver.advance(ms(500));
    assert_eq!(first(&driver), "11 up");
}

/// Doom: a modifier on its own is a key, bound and held by name.
#[test]
fn a_modifier_alone_is_a_key() {
    use intuituive::interact::Releases;
    let app = App::new(|| {
        let said = signal(String::new());
        text!("{said}")
            .on_key("ctrl", move |_| said.set("fire".into()))
            .on_key("w", move |cx| {
                said.set(format!("run {}", cx.is_held("shift")))
            })
    });
    let mut driver = app.driver(10, 1);
    driver.set_releases(Releases::All);
    driver.press("ctrl");
    assert_eq!(first(&driver), "fire");
    driver.press("shift w");
    assert_eq!(first(&driver), "run true");
    driver.send(up("shift"));
    driver.press("w");
    assert_eq!(first(&driver), "run false");
}
