//! The event loop, headless: routing, ticks, timers, repaint on change,
//! resize, hand-off, several components at once, and degradation.

use std::cell::Cell;
use std::process::Command;
use std::rc::Rc;
use std::time::Duration;

use rich::Segment;
use rich_interact::headless::{self, Headless, Script};
use rich_interact::policy::{Fallback, LineIo, NotInteractive, Reason, ScriptedLineIo};
use rich_interact::{
    degrade, Component, Context, Error, Event, EventLoop, Flow, KeyCode, LoopOptions, Outcome,
    View, Viewport,
};

/// Counts Up presses and ticks; Enter returns the count, Escape cancels,
/// `e` hands the terminal to `true`.
#[derive(Default)]
struct Counter {
    count: u32,
    ticks: u32,
    tick: Option<Duration>,
    returned: Option<Option<i32>>,
}

impl Component for Counter {
    type Output = u32;

    fn handle(&mut self, event: &Event, _: &Context<'_>) -> Flow<u32> {
        match event {
            Event::Tick => self.ticks += 1,
            Event::Returned(code) => self.returned = Some(*code),
            Event::Key(key) => match key.code {
                KeyCode::Up => self.count += 1,
                KeyCode::Enter => return Flow::Done(self.count),
                KeyCode::Escape => return Flow::Cancel,
                KeyCode::Char('e') => return Flow::Handoff(Command::new("true")),
                _ => {}
            },
            _ => {}
        }
        Flow::Continue
    }

    fn render(&self, context: &Context<'_>) -> View {
        let mut text = format!("count {} ticks {}", self.count, self.ticks);
        if let Some(code) = self.returned {
            text.push_str(&format!(" returned {code:?}"));
        }
        View::new(context.markup(&text))
    }

    fn tick(&self) -> Option<Duration> {
        self.tick
    }

    fn default_value(&self) -> Option<u32> {
        Some(7)
    }

    fn prompt(&mut self, io: &mut dyn LineIo) -> Result<Option<u32>, NotInteractive> {
        io.write("count? ");
        match io.read_line() {
            None => Ok(None),
            Some(line) => line
                .trim()
                .parse()
                .map(Some)
                .map_err(|_| NotInteractive::Invalid(format!("not a number: {line:?}"))),
        }
    }
}

#[test]
fn routes_keys_and_returns_the_outcome() {
    let (outcome, record) = headless::run(
        Counter::default(),
        Script::new().keys("up up up enter"),
        30,
        5,
    );
    assert_eq!(outcome.unwrap(), Outcome::Done(3));
    assert_eq!(
        record.frames,
        [
            "count 0 ticks 0",
            "count 1 ticks 0",
            "count 2 ticks 0",
            "count 3 ticks 0"
        ]
    );
    // The last view stays, and the cursor ends below it, shown.
    assert!(
        record.output().ends_with("\r\n\x1b[?25h"),
        "{:?}",
        record.output()
    );
}

#[test]
fn escape_cancels_and_ctrl_c_interrupts() {
    let (outcome, _) = headless::run(Counter::default(), Script::new().keys("up escape"), 30, 5);
    assert_eq!(outcome.unwrap(), Outcome::Cancelled);
    let (outcome, _) = headless::run(Counter::default(), Script::new().keys("up ctrl+c"), 30, 5);
    assert_eq!(outcome.unwrap(), Outcome::Interrupted);
}

#[test]
fn a_script_that_ends_early_is_an_error() {
    let (outcome, _) = headless::run(Counter::default(), Script::new().keys("up"), 30, 5);
    assert_eq!(
        outcome.unwrap_err().kind(),
        std::io::ErrorKind::UnexpectedEof
    );
}

#[test]
fn keys_that_change_nothing_write_nothing() {
    let (_, record) = headless::run(
        Counter::default(),
        Script::new().keys("down down left enter"),
        30,
        5,
    );
    // One paint, then the finish: the three ignored keys wrote nothing.
    assert_eq!(record.frames, ["count 0 ticks 0"]);
    assert_eq!(record.writes.len(), 2, "{:?}", record.writes);
}

#[test]
fn ticks_arrive_on_virtual_time() {
    let counter = Counter {
        tick: Some(Duration::from_millis(100)),
        ..Counter::default()
    };
    let script = Script::new().wait(Duration::from_millis(350)).keys("enter");
    let (outcome, record) = headless::run(counter, script, 30, 5);
    assert_eq!(outcome.unwrap(), Outcome::Done(0));
    assert_eq!(record.last_frame(), "count 0 ticks 3");
}

#[test]
fn an_idle_loop_writes_nothing() {
    // A tick that changes nothing: a component that ignores ticks.
    struct Still;
    impl Component for Still {
        type Output = ();
        fn handle(&mut self, event: &Event, _: &Context<'_>) -> Flow<()> {
            if event.key().is_some() {
                Flow::Done(())
            } else {
                Flow::Continue
            }
        }
        fn render(&self, _: &Context<'_>) -> View {
            View::new(vec![vec![Segment::new("still", None)]])
        }
        fn tick(&self) -> Option<Duration> {
            Some(Duration::from_millis(10))
        }
    }
    let script = Script::new().wait(Duration::from_secs(5)).keys("x");
    let (outcome, record) = headless::run(Still, script, 20, 3);
    assert_eq!(outcome.unwrap(), Outcome::Done(()));
    // 500 ticks, one paint and the finish.
    assert_eq!(record.writes.len(), 2, "{:?}", record.writes);
}

#[test]
fn timers_fire_until_they_stop() {
    let fired = Rc::new(Cell::new(0));
    let counter = Rc::clone(&fired);
    let backend = Headless::new(
        Script::new()
            .wait(Duration::from_millis(1000))
            .keys("enter"),
        30,
        5,
    );
    let mut event_loop = EventLoop::new(backend, LoopOptions::default());
    event_loop.every(Duration::from_millis(100), move || {
        counter.set(counter.get() + 1);
        counter.get() < 4
    });
    let handle = event_loop.mount(Counter::default());
    event_loop.run().unwrap();
    assert_eq!(handle.take(), Some(Outcome::Done(0)));
    assert_eq!(fired.get(), 4);
}

#[test]
fn a_resize_repaints_at_the_new_width() {
    let lines = (1..=30)
        .map(|i| vec![Segment::new(format!("line {i}"), None)])
        .collect();
    let script = Script::new().keys("pagedown").resize(40, 6).keys("enter");
    let (outcome, record) = headless::run(Viewport::new(lines), script, 40, 11);
    // Ten rows (less the status line) per page, then five after the resize.
    assert_eq!(outcome.unwrap(), Outcome::Done(10));
    let last = record.last_frame();
    assert!(last.starts_with("line 11\nline 12"), "{last}");
    assert!(last.contains("lines 11–15 of 30"), "{last}");
    assert_eq!(last.lines().count(), 6);
}

#[test]
fn hands_the_terminal_off_and_takes_it_back() {
    let (outcome, record) = headless::run(Counter::default(), Script::new().keys("e enter"), 40, 5);
    assert_eq!(outcome.unwrap(), Outcome::Done(0));
    assert_eq!(record.handoffs, ["true"]);
    assert_eq!(record.last_frame(), "count 0 ticks 0 returned Some(0)");
}

#[test]
fn runs_several_components_first_one_first() {
    let backend = Headless::new(Script::new().keys("up enter up up enter"), 30, 5);
    let record = backend.record();
    let mut event_loop = EventLoop::new(backend, LoopOptions::default());
    let first = event_loop.mount(Counter::default());
    let second = event_loop.mount(Counter::default());
    event_loop.run().unwrap();
    assert_eq!(first.take(), Some(Outcome::Done(1)));
    assert_eq!(second.take(), Some(Outcome::Done(2)));
    assert_eq!(
        record.borrow().last_frame(),
        "count 1 ticks 0\ncount 2 ticks 0"
    );
}

#[test]
fn a_borrowed_component_keeps_its_state() {
    let mut counter = Counter::default();
    let (outcome, _) = headless::run(&mut counter, Script::new().keys("up up escape"), 30, 5);
    assert_eq!(outcome.unwrap(), Outcome::Cancelled);
    assert_eq!(counter.count, 2);
}

#[test]
fn inline_height_cuts_the_view() {
    let lines = (1..=30)
        .map(|i| vec![Segment::new(format!("line {i}"), None)])
        .collect();
    let backend = Headless::new(Script::new().keys("enter"), 40, 24);
    let record = backend.record();
    let mut event_loop = EventLoop::new(
        backend,
        LoopOptions {
            height: Some(4),
            ..LoopOptions::default()
        },
    );
    event_loop.mount(Viewport::new(lines));
    event_loop.run().unwrap();
    assert_eq!(
        record.borrow().last_frame(),
        "line 1\nline 2\nline 3\nlines 1–3 of 30 · ↑↓ PgUp PgDn · q quits"
    );
}

#[test]
fn transient_loops_clear_their_region() {
    let backend = Headless::new(Script::new().keys("enter"), 30, 5);
    let record = backend.record();
    let mut event_loop = EventLoop::new(
        backend,
        LoopOptions {
            transient: true,
            ..LoopOptions::default()
        },
    );
    event_loop.mount(Counter::default());
    event_loop.run().unwrap();
    assert!(record.borrow().output().ends_with("\r\x1b[J\x1b[?25h"));
}

#[test]
fn styled_output_is_byte_exact() {
    struct Styled;
    impl Component for Styled {
        type Output = ();
        fn handle(&mut self, _: &Event, _: &Context<'_>) -> Flow<()> {
            Flow::Done(())
        }
        fn render(&self, context: &Context<'_>) -> View {
            View::new(context.markup("[bold red]hi[/] there"))
        }
    }
    let (_, record) = headless::run(Styled, Script::new().keys("x"), 20, 3);
    assert_eq!(record.writes[0], "\r\x1b[1;31mhi\x1b[0m there");
}

#[test]
fn degrades_by_policy() {
    let mut counter = Counter::default();
    let reason = Reason::StdinNotTerminal;
    let mut io = ScriptedLineIo::new(["12"]);
    let outcome = degrade(&mut counter, Fallback::Prompt, reason, &mut io).unwrap();
    assert_eq!(outcome, Outcome::Done(12));
    assert_eq!(io.written, "count? ");
    let mut io = ScriptedLineIo::new(Vec::<String>::new());
    assert_eq!(
        degrade(&mut counter, Fallback::Prompt, reason, &mut io).unwrap(),
        Outcome::Cancelled,
        "end of input cancels"
    );
    let mut io = ScriptedLineIo::new(["twelve"]);
    assert!(matches!(
        degrade(&mut counter, Fallback::Prompt, reason, &mut io),
        Err(Error::NotInteractive(NotInteractive::Invalid(_)))
    ));
    assert_eq!(
        degrade(&mut counter, Fallback::Default, reason, &mut io).unwrap(),
        Outcome::Done(7)
    );
    assert!(matches!(
        degrade(&mut counter, Fallback::Error, reason, &mut io),
        Err(Error::NotInteractive(NotInteractive::Terminal(
            Reason::StdinNotTerminal
        )))
    ));
    // A component with no line form and no default.
    struct Bare;
    impl Component for Bare {
        type Output = ();
        fn handle(&mut self, _: &Event, _: &Context<'_>) -> Flow<()> {
            Flow::Continue
        }
        fn render(&self, _: &Context<'_>) -> View {
            View::default()
        }
    }
    assert!(matches!(
        degrade(&mut Bare, Fallback::Prompt, Reason::Ci, &mut io),
        Err(Error::NotInteractive(NotInteractive::NoDefault(Reason::Ci)))
    ));
}
