//! The push API (`EventLoop::step` / `pump` on a `PushBackend`) gives the
//! same results as `EventLoop::run` on the same scripted input: the same
//! outcome and the same bytes, for the components `rich choose`, `filter`,
//! `pager` and `explore` run.

use std::time::Duration;

use rich::Text;
use rich_interact::headless::{self, Script, Step};
use rich_interact::{
    Component, Event, EventLoop, LoopOptions, MultiSelect, Outcome, Pager, PushBackend, Select,
};

/// Drive `component` with `script` through the push API, as the headless
/// backend feeds `run`: each event after what the loop did with the one
/// before, and a wait cut into the turns its ticks and timers fall due.
fn pushed<C: Component>(
    component: C,
    script: Script,
    columns: u16,
    rows: u16,
) -> (Option<Outcome<C::Output>>, String) {
    let backend = PushBackend::new(columns, rows).inline();
    let port = backend.port();
    let mut event_loop = EventLoop::new(backend, LoopOptions::default());
    let handle = event_loop.mount(component);
    let mut now = Duration::ZERO;
    let mut running = event_loop.pump().unwrap();
    for step in script.0 {
        if !running {
            break;
        }
        match step {
            Step::Wait(mut left) => {
                while let Some(timeout) = event_loop.timeout().filter(|t| *t <= left) {
                    now += timeout;
                    left -= timeout;
                    port.set_elapsed(now);
                    running = event_loop.pump().unwrap();
                    if !running {
                        break;
                    }
                }
                now += left;
                port.set_elapsed(now);
            }
            Step::Event(event) => {
                let event = match event {
                    Event::Key(key) => Event::Key(key.legacy()),
                    event => event,
                };
                running = event_loop.step(event).unwrap();
            }
        }
    }
    event_loop.finish().unwrap();
    (handle.take(), port.take_output())
}

fn same<C: Component>(make: impl Fn() -> C, script: Script, columns: u16, rows: u16)
where
    C::Output: PartialEq + std::fmt::Debug,
{
    let (ran, record) = headless::run(make(), script.clone(), columns, rows);
    let (outcome, output) = pushed(make(), script, columns, rows);
    assert_eq!(outcome, Some(ran.unwrap()));
    assert_eq!(output, record.output());
}

fn colours() -> Vec<String> {
    [
        "red", "orange", "yellow", "green", "blue", "indigo", "violet",
    ]
    .map(String::from)
    .to_vec()
}

#[test]
fn choose_steps_as_it_runs() {
    same(
        || Select::new("Colour", colours()),
        Script::new().keys("down down enter"),
        30,
        8,
    );
}

#[test]
fn filter_steps_as_it_runs() {
    same(
        || Select::new("Filter", colours()),
        Script::new()
            .text("in")
            .keys("backspace")
            .text("e")
            .keys("enter"),
        30,
        8,
    );
}

#[test]
fn choose_multi_steps_as_it_runs() {
    same(
        || MultiSelect::new("Colours", colours()),
        Script::new().keys("space down down space enter"),
        30,
        8,
    );
}

#[test]
fn a_cancel_and_an_interrupt_step_as_they_run() {
    same(
        || Select::new("Colour", colours()),
        Script::new().keys("down escape"),
        30,
        8,
    );
    same(
        || Select::new("Colour", colours()),
        Script::new().keys("down ctrl+c"),
        30,
        8,
    );
}

#[test]
fn ticks_fall_due_as_they_do_in_run() {
    same(
        || Select::new("Colour", colours()).repaint_every(Duration::from_millis(40)),
        Script::new()
            .wait(Duration::from_millis(130))
            .keys("down")
            .wait(Duration::from_millis(25))
            .resize(24, 6)
            .wait(Duration::from_millis(90))
            .keys("enter"),
        30,
        8,
    );
}

#[test]
fn the_pager_steps_as_it_runs() {
    let text = (1..=60)
        .map(|n| format!("line {n}"))
        .collect::<Vec<_>>()
        .join("\n");
    same(
        || Pager::new(Text::new(text.clone())),
        Script::new().keys("down down pagedown end up q"),
        40,
        10,
    );
}

#[cfg(feature = "data")]
#[test]
fn explore_steps_as_it_runs() {
    use rich_ext::data::{parse, Format};
    use rich_interact::DataExplorer;
    let source = r#"{"server": {"host": "localhost", "port": 8080}, "users": [{"name": "ada"}], "debug": true}"#;
    same(
        || DataExplorer::new("config.json", parse(Format::Json, source).unwrap()),
        Script::new().keys("down right down enter"),
        50,
        12,
    );
}

#[test]
fn steps_after_the_end_are_dropped() {
    let backend = PushBackend::new(20, 4);
    let port = backend.port();
    let mut event_loop = EventLoop::new(backend, LoopOptions::default());
    let handle = event_loop.mount(Select::new("Pick", colours()));
    assert!(!event_loop
        .step(Event::Key(rich_interact::Key::parse("enter").unwrap()))
        .unwrap());
    let shown = port.take_output();
    assert!(shown.contains("red"));
    assert!(!event_loop
        .step(Event::Key(rich_interact::Key::parse("down").unwrap()))
        .unwrap());
    assert!(!event_loop.pump().unwrap());
    assert_eq!(port.take_output(), "");
    assert_eq!(handle.take(), Some(Outcome::Done("red".to_string())));
}
