//! The headless driver: scripted events in, painted frames out.
//!
//! Every component is tested here. The event loop runs unchanged against a
//! [`Headless`] backend: events come from a script, time is virtual (a
//! `wait` advances it, so ticks and timers fire without sleeping), and each
//! paint is recorded twice, as the exact bytes written and as the plain text
//! of the view, so tests can assert byte-exact output or just what showed.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::io;
use std::process::Command;
use std::rc::Rc;
use std::time::Duration;

use crate::component::Component;
use crate::event::{Event, Key};
use crate::event_loop::{EventLoop, LoopOptions, Outcome};
use crate::session::Backend;

/// One step of a script.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    Event(Event),
    /// Let virtual time pass.
    Wait(Duration),
}

/// A script of steps, built fluently: `Script::new().keys("down down
/// enter")`.
#[derive(Clone, Debug, Default)]
pub struct Script(pub VecDeque<Step>);

impl Script {
    pub fn new() -> Script {
        Script::default()
    }

    /// Key names separated by spaces (see [`Key::parse`]).
    ///
    /// # Panics
    /// On a name that does not parse: a script is test code.
    pub fn keys(mut self, names: &str) -> Script {
        for name in names.split_whitespace() {
            let key = Key::parse(name).unwrap_or_else(|| panic!("unknown key {name:?}"));
            self.0.push_back(Step::Event(Event::Key(key)));
        }
        self
    }

    /// Each character of `text` as a key press.
    pub fn text(mut self, text: &str) -> Script {
        for c in text.chars() {
            self.0.push_back(Step::Event(Event::Key(Key::char(c))));
        }
        self
    }

    pub fn event(mut self, event: Event) -> Script {
        self.0.push_back(Step::Event(event));
        self
    }

    pub fn wait(mut self, duration: Duration) -> Script {
        self.0.push_back(Step::Wait(duration));
        self
    }

    pub fn resize(self, columns: u16, rows: u16) -> Script {
        self.event(Event::Resize { columns, rows })
    }
}

/// What a headless run painted.
#[derive(Clone, Debug, Default)]
pub struct Record {
    /// Every write, in order: paints, finishing, hand-offs.
    pub writes: Vec<String>,
    /// The plain text of every painted view, one entry per paint that
    /// changed something.
    pub frames: Vec<String>,
    /// Commands handed the terminal, by program name.
    pub handoffs: Vec<String>,
}

impl Record {
    /// Everything written, as one string.
    pub fn output(&self) -> String {
        self.writes.concat()
    }

    /// The last frame painted.
    pub fn last_frame(&self) -> &str {
        self.frames.last().map_or("", String::as_str)
    }
}

/// A backend with a scripted keyboard, a virtual clock and a recorder.
pub struct Headless {
    script: VecDeque<Step>,
    clock: Duration,
    size: (u16, u16),
    record: Rc<RefCell<Record>>,
    /// Exit code each hand-off reports.
    pub handoff_code: Option<i32>,
}

impl Headless {
    pub fn new(script: Script, columns: u16, rows: u16) -> Headless {
        Headless {
            script: script.0,
            clock: Duration::ZERO,
            size: (columns, rows),
            record: Rc::new(RefCell::new(Record::default())),
            handoff_code: Some(0),
        }
    }

    /// The recorder, shared: it keeps filling while the loop runs.
    pub fn record(&self) -> Rc<RefCell<Record>> {
        Rc::clone(&self.record)
    }
}

impl Backend for Headless {
    fn size(&self) -> (u16, u16) {
        self.size
    }

    fn read(&mut self, timeout: Option<Duration>) -> io::Result<Option<Event>> {
        loop {
            match self.script.front_mut() {
                None => {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "the headless script ended before the components finished",
                    ))
                }
                Some(Step::Wait(left)) => match timeout {
                    Some(timeout) if timeout <= *left => {
                        self.clock += timeout;
                        *left -= timeout;
                        if left.is_zero() {
                            self.script.pop_front();
                        }
                        return Ok(None);
                    }
                    _ => {
                        self.clock += *left;
                        self.script.pop_front();
                    }
                },
                Some(Step::Event(_)) => {
                    let Some(Step::Event(event)) = self.script.pop_front() else {
                        unreachable!()
                    };
                    if let Event::Resize { columns, rows } = event {
                        self.size = (columns, rows);
                    }
                    return Ok(Some(event));
                }
            }
        }
    }

    fn write(&mut self, text: &str) -> io::Result<()> {
        self.record.borrow_mut().writes.push(text.to_string());
        Ok(())
    }

    fn elapsed(&self) -> Duration {
        self.clock
    }

    fn handoff(&mut self, command: &mut Command) -> io::Result<Option<i32>> {
        let program = command.get_program().to_string_lossy().into_owned();
        self.record.borrow_mut().handoffs.push(program);
        Ok(self.handoff_code)
    }

    fn alternate_screen(&self) -> bool {
        false
    }

    fn painted(&mut self, text: &str) {
        self.record.borrow_mut().frames.push(text.to_string());
    }
}

/// Run one component headless at `columns` × `rows`, and return how it
/// ended and what was painted.
pub fn run<C: Component>(
    component: C,
    script: Script,
    columns: u16,
    rows: u16,
) -> (io::Result<Outcome<C::Output>>, Record) {
    let backend = Headless::new(script, columns, rows);
    let record = backend.record();
    let mut event_loop = EventLoop::new(backend, LoopOptions::default());
    let handle = event_loop.mount(component);
    let result = event_loop
        .run()
        .map(|()| handle.take().expect("a finished component has an outcome"));
    drop(event_loop);
    let record = record.borrow().clone();
    (result, record)
}
