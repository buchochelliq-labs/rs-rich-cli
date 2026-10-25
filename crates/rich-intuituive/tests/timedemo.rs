//! Frame timings and counts in `FrameStats`, and the timedemo.

use std::time::Duration;

use intuituive::interact::headless::{Headless, Script};
use intuituive::prelude::*;
use intuituive::timedemo::script_from_tape;
use intuituive::widget::{Canvas, DrawCx};

/// A widget that takes 5 ms to draw.
struct Slow(Signal<u32>);

impl intuituive::Widget for Slow {
    fn draw(&mut self, _cx: &mut DrawCx, canvas: &mut Canvas) {
        let n = self.0.get();
        std::thread::sleep(Duration::from_millis(5));
        canvas.lines(&[vec![rich::Segment::new(format!("n {n}"), None)]]);
    }
}

fn slow_app() -> App {
    App::new(|| {
        let n = signal(0u32);
        intuituive::widget(Slow(n))
            .on_key("+", move |_| n.update(|n| *n += 1))
            .on_key("q", |cx| cx.quit())
    })
}

#[test]
fn a_slow_draw_shows_in_the_draw_time() {
    let mut driver = slow_app().driver(10, 1);
    driver.update(Duration::ZERO);
    let out = driver.render().unwrap();
    let stats = driver.stats();
    assert!(stats.draw >= Duration::from_millis(5), "{stats:?}");
    assert!(stats.paint < stats.draw, "{stats:?}");
    assert_eq!(stats.bytes, out.len());
    assert_eq!(stats.total_bytes, out.len() as u64);
    assert_eq!(stats.total_draw, stats.draw);
    // A driver does not write: that is the loop's.
    assert_eq!(stats.write, Duration::ZERO);
}

#[test]
fn totals_add_up_every_frame() {
    let mut driver = slow_app().driver(10, 1);
    let mut sum = 0;
    let mut draw = Duration::ZERO;
    for key in ["+", "+", "+"] {
        driver.update(Duration::ZERO);
        if let Some(out) = driver.render() {
            sum += out.len();
            draw += driver.stats().draw;
            // Bytes by kind add up to the cells' share of the frame.
            let painted = driver.stats().painted;
            assert!(painted.bytes() <= out.len(), "{painted:?}");
        }
        driver.event(rich_interact::Event::Key(
            rich_interact::Key::parse(key).unwrap(),
        ));
    }
    let stats = driver.stats();
    assert_eq!(stats.total_bytes, sum as u64);
    assert_eq!(stats.total_draw, draw);
    // The last frame changed one cell.
    assert_eq!(stats.painted.written, 1, "{:?}", stats.painted);
    assert_eq!(stats.painted.text_bytes, 1);
}

#[test]
fn the_loop_times_its_writes() {
    let app = slow_app();
    let stats = app.stats_handle();
    app.run_on(&mut Headless::new(Script::new().keys("+ + q"), 10, 1))
        .unwrap();
    let stats = stats.get();
    assert!(stats.total_bytes > 0);
    assert!(stats.total_draw >= Duration::from_millis(15), "{stats:?}");
    assert!(stats.total_write > Duration::ZERO, "{stats:?}");
}

#[test]
fn a_timedemo_runs_timers_on_the_virtual_clock() {
    let app = App::new(|| {
        let n = signal(0u32);
        every(Duration::from_millis(100), move |_| n.update(|n| *n += 1));
        text!("tick {n}")
    });
    // Ten seconds of ticks, drawn at once.
    let demo = app.timedemo(Script::new().wait(Duration::from_secs(10)), 20, 1);
    assert_eq!(demo.frames.len(), 101, "{demo}");
    assert_eq!(demo.covered, Duration::from_secs(10));
    assert!(demo.elapsed < Duration::from_secs(5), "{demo}");
    let last = demo.frames.last().unwrap();
    assert_eq!(last.at, Duration::from_secs(10));
    assert!(last.painted.written >= 1);
    assert_eq!(
        demo.bytes(),
        demo.frames.iter().map(|f| f.bytes).sum::<usize>()
    );
}

#[test]
fn a_timedemo_replays_a_tape() {
    let tape =
        "Set Size 30x3\nSet TypingDelay 0ms\nType \"++++\"\nSleep 1s\nType \"q\"\nType \"+\"\n";
    let (script, (w, h)) = script_from_tape(tape).unwrap();
    let demo = slow_app().timedemo(script, w, h);
    // The first frame and four presses; the app quit before the last.
    assert_eq!(demo.frames.len(), 5, "{demo}");
    assert!(demo
        .frames
        .iter()
        .skip(1)
        .all(|f| f.draw >= Duration::from_millis(5)));
    let text = demo.to_string();
    assert!(text.starts_with("5 frames in "), "{text}");
}
