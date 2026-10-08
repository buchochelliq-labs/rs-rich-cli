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
use crate::event::{Button, Event, Key, Mouse, MouseKind};
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

    /// The key named `name` let go (see [`Event::KeyUp`]).
    ///
    /// # Panics
    /// On a name that does not parse.
    pub fn key_up(self, name: &str) -> Script {
        let key = Key::parse(name).unwrap_or_else(|| panic!("unknown key {name:?}"));
        self.event(Event::KeyUp(key))
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

    /// A mouse event at `column`, `row` of the terminal (#476). Headless,
    /// the region starts at the top, so these are the view's rows too.
    pub fn mouse(self, kind: MouseKind, column: u16, row: u16) -> Script {
        self.event(Event::Mouse(Mouse::new(kind, column, row)))
    }

    /// A left click: the button down, then up, at one cell.
    pub fn click(self, column: u16, row: u16) -> Script {
        self.mouse(MouseKind::Down(Button::Left), column, row)
            .mouse(MouseKind::Up(Button::Left), column, row)
    }

    /// A drag with the left button from one cell to another, through the
    /// columns between them on the starting row, as a terminal reports it.
    pub fn drag(self, from: (u16, u16), to: (u16, u16)) -> Script {
        let mut script = self.mouse(MouseKind::Down(Button::Left), from.0, from.1);
        let step: i32 = if to.0 >= from.0 { 1 } else { -1 };
        let mut column = i32::from(from.0);
        while column != i32::from(to.0) {
            column += step;
            let row = if column == i32::from(to.0) {
                to.1
            } else {
                from.1
            };
            script = script.mouse(MouseKind::Drag(Button::Left), column as u16, row);
        }
        script.mouse(MouseKind::Up(Button::Left), to.0, to.1)
    }

    /// The wheel, one notch down (`down`) or up, over a cell.
    pub fn scroll(self, down: bool, column: u16, row: u16) -> Script {
        let kind = if down {
            MouseKind::ScrollDown
        } else {
            MouseKind::ScrollUp
        };
        self.mouse(kind, column, row)
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
    /// How many times Ctrl+Z suspended the run
    /// ([`Headless::suspendable`]).
    pub suspends: usize,
    /// What components put on the clipboard
    /// ([`clipboard::copy`](crate::clipboard::copy)), in order.
    pub copies: Vec<String>,
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
    /// Whether Ctrl+Z suspends, as on a real terminal on Unix, instead of
    /// reaching the component. Off by default.
    pub suspendable: bool,
    /// Whether copies reach a (recorded) clipboard. On by default; off
    /// tests a terminal without OSC 52.
    pub clipboard: bool,
    /// Whether keys arrive as a terminal with the kitty keyboard protocol
    /// reports them, each [`exact`](Key::exact). Off by default: they
    /// arrive as a legacy terminal sends them ([`Key::legacy`]), so a
    /// scripted `ctrl+i` is a Tab, which fires a `tab` or a `ctrl+i`
    /// binding.
    pub exact_keys: bool,
    /// Whether frames are written as synchronized updates, as on a terminal
    /// that knows DEC private mode 2026: each recorded paint is then wrapped
    /// in `CSI ? 2026 h` and `CSI ? 2026 l`. Off by default.
    pub synchronized_output: bool,
}

impl Headless {
    pub fn new(script: Script, columns: u16, rows: u16) -> Headless {
        Headless {
            script: script.0,
            clock: Duration::ZERO,
            size: (columns, rows),
            record: Rc::new(RefCell::new(Record::default())),
            handoff_code: Some(0),
            suspendable: false,
            clipboard: true,
            exact_keys: false,
            synchronized_output: false,
        }
    }

    /// A key event as this terminal reports it.
    fn as_read(&self, event: Event) -> Event {
        let read = |key: Key| {
            if self.exact_keys {
                key.exact()
            } else {
                key.legacy()
            }
        };
        match event {
            Event::Key(key) => Event::Key(read(key)),
            Event::KeyUp(key) => Event::KeyUp(read(key)),
            event => event,
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
                    return Ok(Some(self.as_read(event)));
                }
            }
        }
    }

    fn write(&mut self, text: &str) -> io::Result<()> {
        self.record.borrow_mut().writes.push(text.to_string());
        Ok(())
    }

    fn synchronized_output(&self) -> bool {
        self.synchronized_output
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

    fn can_suspend(&self) -> bool {
        self.suspendable
    }

    fn suspend(&mut self) -> io::Result<()> {
        self.record.borrow_mut().suspends += 1;
        Ok(())
    }

    fn clipboard(&self) -> Result<(), String> {
        if self.clipboard {
            Ok(())
        } else {
            Err("the headless clipboard is off".into())
        }
    }

    fn copy(&mut self, text: &str) -> io::Result<()> {
        self.record.borrow_mut().copies.push(text.to_string());
        Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::KeyCode;

    #[test]
    fn frames_are_written_whole_or_not_at_all() {
        let mut backend = Headless::new(Script::new(), 10, 2);
        let record = backend.record();
        backend.write_frame("plain").unwrap();
        backend.synchronized_output = true;
        backend.write_frame("\x1b[1;1Hhi").unwrap();
        // Nothing to paint: no write, and no empty update.
        backend.write_frame("").unwrap();
        assert_eq!(
            record.borrow().writes,
            ["plain", "\x1b[?2026h\x1b[1;1Hhi\x1b[?2026l"]
        );
    }

    fn read_all(exact: bool) -> Vec<Event> {
        let script = Script::new().keys("ctrl+i ctrl+m q").key_up("q");
        let mut backend = Headless::new(script, 10, 2);
        backend.exact_keys = exact;
        std::iter::from_fn(|| backend.read(None).ok().flatten()).collect()
    }

    #[test]
    fn keys_arrive_as_a_legacy_terminal_sends_them_or_exactly() {
        let legacy = read_all(false);
        assert_eq!(
            legacy,
            [
                Event::Key(Key::new(KeyCode::Tab)),
                Event::Key(Key::new(KeyCode::Enter)),
                Event::Key(Key::char('q')),
                Event::KeyUp(Key::char('q')),
            ]
        );
        let Event::Key(tab) = legacy[0] else {
            unreachable!()
        };
        assert!(!tab.is_exact() && tab.matches(&Key::ctrl('i')));
        let exact = read_all(true);
        assert_eq!(
            exact,
            [
                Event::Key(Key::ctrl('i')),
                Event::Key(Key::ctrl('m')),
                Event::Key(Key::char('q')),
                Event::KeyUp(Key::char('q')),
            ]
        );
        assert!(exact.iter().all(|event| match event {
            Event::Key(key) | Event::KeyUp(key) => key.is_exact(),
            _ => false,
        }));
    }
}
