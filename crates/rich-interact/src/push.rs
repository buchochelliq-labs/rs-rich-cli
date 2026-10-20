//! A backend for push driving: the host owns the clock and the screen.
//!
//! [`EventLoop::run`](crate::EventLoop::run) blocks in [`Backend::read`]. A
//! host that cannot block (a browser tab driving components from wasm,
//! another program's event loop) feeds events with
//! [`EventLoop::step`](crate::EventLoop::step) instead, and moves time on
//! with [`EventLoop::pump`](crate::EventLoop::pump). A [`PushBackend`] is the
//! backend for that: it never reads, its time is what the host last set
//! through its [`PushPort`], and what the loop paints collects in the port
//! for the host to write to its terminal (xterm.js, in the docs
//! playground).
//!
//! ```
//! use std::time::Duration;
//! use rich_interact::{Event, EventLoop, Key, LoopOptions, PushBackend, Select};
//!
//! let backend = PushBackend::new(24, 4);
//! let port = backend.port();
//! let mut event_loop = EventLoop::new(backend, LoopOptions::default());
//! event_loop.mount(Select::new("Pick", vec!["one".to_string(), "two".to_string()]));
//! event_loop.pump().unwrap();
//! let first = port.take_output();
//! assert!(first.contains("one"));
//! port.set_elapsed(Duration::from_millis(16));
//! event_loop.step(Event::Key(Key::char('t'))).unwrap();
//! assert!(port.take_output().contains('t'));
//! ```

use std::cell::RefCell;
use std::io;
use std::process::Command;
use std::rc::Rc;
use std::time::Duration;

use crate::event::Event;
use crate::session::Backend;

struct State {
    size: (u16, u16),
    now: Duration,
    output: String,
    copies: Vec<String>,
}

/// A [`Backend`] a host pushes to: see the [module docs](self).
///
/// It paints on the alternate screen unless told otherwise
/// ([`inline`](Self::inline)), never reads (driving it with
/// [`EventLoop::run`](crate::EventLoop::run) fails with
/// [`Unsupported`](io::ErrorKind::Unsupported)), has a clipboard whose
/// copies the host takes ([`PushPort::take_copies`]), and runs no command:
/// a hand-off returns at once with no exit code.
pub struct PushBackend {
    state: Rc<RefCell<State>>,
    alternate_screen: bool,
}

/// The host's side of a [`PushBackend`]: set the time, take the output.
/// Cheap to clone; every clone is the same port.
#[derive(Clone)]
pub struct PushPort {
    state: Rc<RefCell<State>>,
}

impl PushBackend {
    /// A `columns` x `rows` screen, at time zero.
    pub fn new(columns: u16, rows: u16) -> PushBackend {
        PushBackend {
            state: Rc::new(RefCell::new(State {
                size: (columns, rows),
                now: Duration::ZERO,
                output: String::new(),
                copies: Vec::new(),
            })),
            alternate_screen: true,
        }
    }

    /// Paint inline, from the row the host's cursor is on, instead of on
    /// the alternate screen.
    #[must_use]
    pub fn inline(mut self) -> PushBackend {
        self.alternate_screen = false;
        self
    }

    /// The port the host keeps once the backend goes into an
    /// [`EventLoop`](crate::EventLoop).
    pub fn port(&self) -> PushPort {
        PushPort {
            state: Rc::clone(&self.state),
        }
    }
}

impl PushPort {
    /// The time since the start, as the loop sees it: set it before each
    /// [`step`](crate::EventLoop::step) and [`pump`](crate::EventLoop::pump).
    /// Time does not go back: an earlier time than the last is ignored.
    pub fn set_elapsed(&self, now: Duration) {
        let mut state = self.state.borrow_mut();
        state.now = state.now.max(now);
    }

    /// The time last set.
    pub fn elapsed(&self) -> Duration {
        self.state.borrow().now
    }

    /// Everything painted since the last call: bytes for the terminal.
    pub fn take_output(&self) -> String {
        std::mem::take(&mut self.state.borrow_mut().output)
    }

    /// What components put on the clipboard since the last call, in order.
    pub fn take_copies(&self) -> Vec<String> {
        std::mem::take(&mut self.state.borrow_mut().copies)
    }

    /// Keep the screen's size: the loop learns it from the
    /// [`Event::Resize`] the host steps with.
    pub fn set_size(&self, columns: u16, rows: u16) {
        self.state.borrow_mut().size = (columns, rows);
    }
}

impl Backend for PushBackend {
    fn size(&self) -> (u16, u16) {
        self.state.borrow().size
    }

    fn read(&mut self, _timeout: Option<Duration>) -> io::Result<Option<Event>> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "a push backend is not read: drive the loop with EventLoop::step and pump",
        ))
    }

    fn write(&mut self, text: &str) -> io::Result<()> {
        self.state.borrow_mut().output.push_str(text);
        Ok(())
    }

    fn elapsed(&self) -> Duration {
        self.state.borrow().now
    }

    fn handoff(&mut self, _command: &mut Command) -> io::Result<Option<i32>> {
        Ok(None)
    }

    fn alternate_screen(&self) -> bool {
        self.alternate_screen
    }

    fn clipboard(&self) -> Result<(), String> {
        Ok(())
    }

    fn copy(&mut self, text: &str) -> io::Result<()> {
        self.state.borrow_mut().copies.push(text.to_string());
        Ok(())
    }
}
