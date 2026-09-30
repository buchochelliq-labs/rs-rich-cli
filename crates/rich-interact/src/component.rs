//! The component contract: one event in, one view out.

use std::time::Duration;

use rich::{Console, Renderable, Segment};

use crate::event::Event;
use crate::keymap::Keymap;
use crate::policy::{LineIo, NotInteractive};

/// What a component wants after handling an event.
#[derive(Debug)]
pub enum Flow<T> {
    /// Keep going. The view is rendered again, and repainted only if it
    /// changed.
    Continue,
    /// Finished with a value.
    Done(T),
    /// The user backed out (Escape, `q`): no value.
    Cancel,
    /// The event was not for this component: it is left to the container
    /// the component is in, which may use it itself (a focus key, a
    /// binding of its own) or pass it on to its own container (0.0.14).
    /// The event loop, at the top, treats it as [`Continue`](Flow::Continue).
    Ignored,
    /// Give the terminal to `command` (`$EDITOR file`, a pager) until it
    /// exits, then take it back and repaint; the component then gets
    /// [`Event::Returned`] with the exit code (#489).
    Handoff(std::process::Command),
}

/// A rendered view: lines of segments, and where the terminal cursor goes
/// (a text caret), or `None` to hide it.
#[derive(Clone, Debug, Default)]
pub struct View {
    pub lines: Vec<Vec<Segment>>,
    /// (row, column) within the view.
    pub cursor: Option<(usize, usize)>,
}

impl View {
    pub fn new(lines: Vec<Vec<Segment>>) -> View {
        View {
            lines,
            cursor: None,
        }
    }

    pub fn with_cursor(mut self, row: usize, column: usize) -> View {
        self.cursor = Some((row, column));
        self
    }

    /// Add `other` below this view. Its cursor, if this view has none, moves
    /// down with it.
    pub fn push(&mut self, other: View) {
        if self.cursor.is_none() {
            self.cursor = other
                .cursor
                .map(|(row, column)| (row + self.lines.len(), column));
        }
        self.lines.extend(other.lines);
    }
}

/// What a component renders for: the console to render with, and the space
/// it may use.
pub struct Context<'a> {
    pub console: &'a Console,
    /// Columns available.
    pub width: usize,
    /// Rows available: the terminal's height, or the inline height the
    /// caller set. A view taller than this is cut off at the bottom.
    pub height: usize,
}

impl Context<'_> {
    /// Render `renderable` at the context's width into lines.
    pub fn lines(&self, renderable: &dyn Renderable) -> Vec<Vec<Segment>> {
        self.lines_at(renderable, self.width)
    }

    /// Render `renderable` at `width` columns into lines.
    pub fn lines_at(&self, renderable: &dyn Renderable, width: usize) -> Vec<Vec<Segment>> {
        let options = self.console.options().update_width(width.max(1));
        self.console.render_lines(renderable, &options, false)
    }

    /// Render console markup into lines.
    pub fn markup(&self, markup: &str) -> Vec<Vec<Segment>> {
        let text = rich::Text::from_markup(markup).unwrap_or_else(|_| rich::Text::new(markup));
        self.lines(&text)
    }
}

/// An interactive component: a state machine that handles one event at a
/// time and renders its state. The same component runs under
/// [`run`](crate::run) and in an [`EventLoop`](crate::EventLoop), and in the
/// [headless](crate::headless) driver for tests.
pub trait Component {
    type Output;

    /// Handle one event. Ctrl+C never arrives here: the event loop ends the
    /// component with [`Outcome::Interrupted`](crate::Outcome::Interrupted).
    fn handle(&mut self, event: &Event, context: &Context<'_>) -> Flow<Self::Output>;

    /// Called once, before the component is first painted, with the
    /// terminal's size: for work that needs it, such as rendering content
    /// at the width. Returning `Done` or `Cancel` finishes the component
    /// without waiting for an event. The default does nothing.
    fn start(&mut self, context: &Context<'_>) -> Flow<Self::Output> {
        let _ = context;
        Flow::Continue
    }

    /// Render the current state.
    fn render(&self, context: &Context<'_>) -> View;

    /// How often the component wants [`Event::Tick`], for a spinner or a
    /// countdown. `None` (the default): never.
    fn tick(&self) -> Option<Duration> {
        None
    }

    /// Whether the component wants mouse events (#476): clicks, drags and
    /// the wheel. Off by default, since reporting the mouse takes text
    /// selection away from the terminal; a component turns it on when its
    /// caller opts in, and [`run`](crate::run) then enables mouse reporting
    /// for the session.
    fn mouse(&self) -> bool {
        false
    }

    /// The bindings this component has right now, for a help overlay, a
    /// shortcut list or status-bar hints to read (see
    /// [`keymap`](crate::keymap)). A container's include its focused
    /// child's, before its own. The default declares none.
    fn keymap(&self) -> Keymap {
        Keymap::default()
    }

    /// Whether the component takes focus in a container: Tab stops on it
    /// and keys go to it. The default: yes. A label, or a component that
    /// has finished, says no.
    fn focusable(&self) -> bool {
        true
    }

    /// Move focus to the next (`forward`) or previous focus stop *inside*
    /// this component, for Tab and Shift+Tab. `false` when there is none
    /// after the current one, so the container moves on to its next
    /// child. The default, for a component that is one stop: `false`.
    fn focus_step(&mut self, forward: bool) -> bool {
        let _ = forward;
        false
    }

    /// Take focus from outside: at the first stop inside when `forward`,
    /// at the last otherwise. `false` when nothing in it can take focus.
    /// The default: whether the component is [`focusable`](Self::focusable).
    fn focus_enter(&mut self, forward: bool) -> bool {
        let _ = forward;
        self.focusable()
    }

    /// The value to return when the terminal is not interactive and the
    /// policy asks for defaults. `None`: there is none.
    fn default_value(&self) -> Option<Self::Output> {
        None
    }

    /// Ask for the value line by line, for when the terminal is not
    /// interactive and the policy asks for a prompt. The default says the
    /// component has no line-based form.
    ///
    /// `Ok(None)` means the user backed out ([`Outcome::Cancelled`](crate::Outcome::Cancelled)).
    /// When input ends before an answer, the built-in components answer
    /// with their default, and without one return [`NotInteractive::Ended`],
    /// which [`degrade`](crate::degrade) reports as
    /// [`NotInteractive::NoDefault`]: end of input is no answer, not a
    /// refusal.
    fn prompt(&mut self, io: &mut dyn LineIo) -> Result<Option<Self::Output>, NotInteractive> {
        let _ = io;
        Err(NotInteractive::NoPrompt)
    }
}
