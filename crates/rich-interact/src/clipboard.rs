//! Copy from a component to the terminal's clipboard (#488).
//!
//! A component cannot write to the terminal itself: the event loop owns
//! it. It calls [`copy`] while it handles an event instead, and the loop
//! sends the text on once the handler returns, through its
//! [`Backend`](crate::Backend): a real [`Session`](crate::Session) writes
//! an OSC 52 sequence when [`rich_ext::clipboard`] says the terminal takes
//! one, and the [headless](crate::headless) driver records the text.
//!
//! [`copy`] answers at once whether the copy will reach a clipboard, so the
//! component can say "copied" or say why not:
//!
//! ```
//! use rich_interact::{clipboard, headless, Component, Context, Event, Flow, View};
//!
//! struct CopyHi;
//!
//! impl Component for CopyHi {
//!     type Output = bool;
//!     fn handle(&mut self, _: &Event, _: &Context<'_>) -> Flow<bool> {
//!         Flow::Done(clipboard::copy("hi").is_ok())
//!     }
//!     fn render(&self, _: &Context<'_>) -> View {
//!         View::default()
//!     }
//! }
//!
//! let (outcome, record) = headless::run(CopyHi, headless::Script::new().keys("enter"), 20, 3);
//! assert_eq!(outcome.unwrap().value(), Some(true));
//! assert_eq!(record.copies, ["hi"]);
//! ```
//!
//! Outside an event loop, or when the terminal does not take OSC 52 (not a
//! terminal, inside tmux, an unknown terminal: see
//! [`rich_ext::clipboard::detect`]), [`copy`] returns
//! [`ClipboardError::Unsupported`] and nothing is written.
//! `RICH_CLIPBOARD=1` forces it on and `RICH_CLIPBOARD=0` off.

use std::cell::RefCell;

pub use rich_ext::clipboard::{ClipboardError, CopyFormat, CLIPBOARD_VAR, MAX_BYTES};

#[derive(Default)]
struct State {
    /// Why copies are off, while a loop that cannot copy is delivering an
    /// event; `None` with `open`: they are on.
    unavailable: Option<String>,
    /// Whether a loop is delivering an event.
    open: bool,
    pending: Vec<String>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

/// Put `text` on the clipboard once the current event is handled. Call it
/// from [`Component::handle`](crate::Component::handle).
pub fn copy(text: impl Into<String>) -> Result<(), ClipboardError> {
    let text = text.into();
    if text.len() > MAX_BYTES {
        return Err(ClipboardError::TooLarge { bytes: text.len() });
    }
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        if !state.open {
            return Err(ClipboardError::Unsupported(
                "no interactive session is running".into(),
            ));
        }
        if let Some(reason) = &state.unavailable {
            return Err(ClipboardError::Unsupported(reason.clone()));
        }
        state.pending.push(text);
        Ok(())
    })
}

/// Whether [`copy`] would reach a clipboard now.
pub fn available() -> bool {
    STATE.with(|state| {
        let state = state.borrow();
        state.open && state.unavailable.is_none()
    })
}

/// A short line saying how a copy of `what` went, for a component's
/// footer: `copied path` or why not.
pub fn report(what: &str, result: &Result<(), ClipboardError>) -> String {
    match result {
        Ok(()) => format!("copied {what}"),
        Err(ClipboardError::Unsupported(_)) => {
            format!("cannot copy {what}: no terminal clipboard (set {CLIPBOARD_VAR}=1 to force OSC 52)")
        }
        Err(error) => format!("cannot copy {what}: {error}"),
    }
}

/// Open delivery of one event: copies are on unless `unavailable` says why
/// not. Returns the copies queued by the time [`close`] is called.
pub(crate) fn open(unavailable: Option<String>) {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.open = true;
        state.unavailable = unavailable;
        state.pending.clear();
    });
}

/// Close delivery and take what was copied.
pub(crate) fn close() -> Vec<String> {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.open = false;
        state.unavailable = None;
        std::mem::take(&mut state.pending)
    })
}

/// The process environment, but a terminal: the session's output is one,
/// whatever standard output is (`x=$(rich explore f)` paints on standard
/// error).
pub(crate) struct SessionEnvironment;

impl rich_ext::capabilities::Environment for SessionEnvironment {
    fn var(&self, name: &str) -> Option<String> {
        std::env::var(name).ok()
    }
    fn is_terminal(&self) -> bool {
        true
    }
    fn size(&self) -> Option<(usize, usize)> {
        None
    }
}
