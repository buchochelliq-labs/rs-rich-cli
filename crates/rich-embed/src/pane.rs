//! The terminal pane: any program, drawn in a node of the app.

use std::cell::RefCell;
use std::rc::Rc;

use rich::Style;
use rich_intuituive::interact::{Key, KeyCode};
use rich_intuituive::widget::{widget, Canvas, DrawCx, EventCx, Used, Widget, WidgetEvent};
use rich_intuituive::{signal, Ctx, Node, Signal};

use crate::host::{Command, ExitStatus, LocalPty, Notify, PtyHost};
use crate::term::TermCore;
use crate::wake::{connect, Fed};

/// Rows a pane keeps that scrolled off its top, unless told otherwise.
pub const DEFAULT_SCROLLBACK: usize = 1000;

type ExitHandler = Box<dyn FnMut(ExitStatus, &mut Ctx)>;

struct PaneState {
    core: TermCore,
    /// Bumped when the screen changed: the pane's draw reads it.
    wake: Signal<u64>,
    status: Signal<Option<ExitStatus>>,
    on_exit: Option<ExitHandler>,
}

impl Fed for PaneState {
    fn install(&mut self, notify: Notify) {
        self.core.set_notify(notify);
    }

    fn pump(this: &Rc<RefCell<Self>>, cx: &mut Ctx) {
        let (pumped, wake, status) = {
            let mut state = this.borrow_mut();
            (state.core.pump(), state.wake, state.status)
        };
        if pumped.output || pumped.exit.is_some() {
            wake.update(|n| *n = n.wrapping_add(1));
        }
        if let Some(exit) = pumped.exit {
            status.set(Some(exit.clone()));
            let handler = this.borrow_mut().on_exit.take();
            if let Some(mut handler) = handler {
                handler(exit, cx);
            }
        }
    }
}

/// A terminal pane being built: [`terminal`] or [`terminal_with`], then
/// [`node`](Self::node) (or `.into()`) to place it.
///
/// The program starts when the pane is first laid out, at the pane's size,
/// and is told every new size after. While the pane has the focus, every
/// key goes to the program (Ctrl+C and Tab included) except the ones
/// [`release_keys`](Self::release_keys) names, which go to the app; the
/// mouse goes to it when it asked for the mouse, and pastes always do.
/// Shift+PgUp and Shift+PgDn, and the wheel when the program does not want
/// the mouse, scroll back through what scrolled off the top; any other key
/// returns to the live screen. When the program exits its last screen
/// stays, [`status`](Self::status) is set, [`on_exit`](Self::on_exit)
/// runs, and keys go to the app again. The program is ended when the pane
/// leaves the tree.
pub struct TerminalPane {
    state: Rc<RefCell<PaneState>>,
    release: Vec<Key>,
}

/// A pane running `command` on this machine through a [`LocalPty`]: a
/// program name, or an array of the program and its arguments.
///
/// ```no_run
/// # extern crate rich_intuituive as intuituive;
/// use intuituive::prelude::*;
/// use rich_embed::terminal;
///
/// App::new(|| {
///     row([
///         terminal("bash").on_exit(|_, cx| cx.quit()).node().panel("shell"),
///         terminal(["htop", "-d", "10"]).node().panel("htop"),
///     ])
/// })
/// .run()
/// # ; Ok::<(), std::io::Error>(())
/// ```
///
/// # Panics
/// Outside a running or building app, like [`signal`].
pub fn terminal(command: impl Into<Command>) -> TerminalPane {
    terminal_with(LocalPty::new(command))
}

/// A pane over any [`PtyHost`]: an SSH session, a container, a
/// [`ReplayHost`](crate::ReplayHost) in a test.
///
/// # Panics
/// Outside a running or building app, like [`signal`].
pub fn terminal_with(host: impl PtyHost + 'static) -> TerminalPane {
    let state = Rc::new(RefCell::new(PaneState {
        core: TermCore::new(Box::new(host), DEFAULT_SCROLLBACK),
        wake: signal(0),
        status: signal(None),
        on_exit: None,
    }));
    connect(&state);
    TerminalPane {
        state,
        // Released by default: nothing. The pane is a terminal.
        release: Vec::new(),
    }
}

impl TerminalPane {
    /// Run `handler` on the app's thread when the program exits, with how
    /// it ended.
    pub fn on_exit(self, handler: impl FnMut(ExitStatus, &mut Ctx) + 'static) -> TerminalPane {
        self.state.borrow_mut().on_exit = Some(Box::new(handler));
        self
    }

    /// How the program ended: `None` while it runs. Read it in a node to
    /// show it.
    pub fn status(&self) -> Signal<Option<ExitStatus>> {
        self.state.borrow().status
    }

    /// Keep up to `rows` rows that scrolled off the top (default
    /// [`DEFAULT_SCROLLBACK`]). Before the pane is first laid out.
    pub fn scrollback(self, rows: usize) -> TerminalPane {
        self.state.borrow_mut().core.set_scrollback(rows);
        self
    }

    /// Keys (space-separated names, as [`Node::on_key`] takes them) that
    /// are not sent to the program but go on to the app's bindings: a way
    /// out of the pane, such as `"ctrl+q f10"`.
    pub fn release_keys(mut self, keys: &str) -> TerminalPane {
        self.release
            .extend(keys.split_whitespace().filter_map(Key::parse));
        self
    }

    /// The pane as a node: focusable, and flexible like any node until
    /// sized.
    pub fn node(self) -> Node {
        widget(PaneWidget {
            state: self.state,
            release: self.release,
            caret: None,
        })
        // The app quits on Ctrl+C unless a node on the focused path binds
        // it: bound here, so it reaches the program. Once the program has
        // exited the pane leaves it to this binding, which quits as before.
        .on_key("ctrl+c", |cx| cx.quit())
    }
}

impl From<TerminalPane> for Node {
    fn from(pane: TerminalPane) -> Node {
        pane.node()
    }
}

struct PaneWidget {
    state: Rc<RefCell<PaneState>>,
    release: Vec<Key>,
    caret: Option<(u16, u16)>,
}

impl Widget for PaneWidget {
    fn name(&self) -> &'static str {
        "terminal"
    }

    fn describe(&self) -> Option<String> {
        let state = self.state.borrow();
        Some(match &state.core.exit {
            Some(status) => status.to_string(),
            None if state.core.started() => "running".into(),
            None => "not started".into(),
        })
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        let mut state = self.state.borrow_mut();
        state.wake.get();
        let (width, height) = (canvas.width(), canvas.height());
        state.core.fit(width, height);
        if let Some(error) = state.core.error.clone() {
            let style = cx.style("error", "red");
            canvas.markup(cx.console(), 0, 0, width, &escape(&error), Some(&style));
            self.caret = None;
            return;
        }
        let lines = state.core.lines();
        canvas.lines(&lines);
        let back = state.core.scrolled();
        if back > 0 {
            let note = format!(" ↑{back} ");
            let w = rich::cells::cell_len(&note) as u16;
            let style = Style::parse("reverse").unwrap_or_default();
            canvas.print(width.saturating_sub(w), 0, &note, Some(&style));
        }
        self.caret = state.core.cursor();
    }

    fn event(&mut self, cx: &mut EventCx, event: &WidgetEvent) -> Used {
        let mut state = self.state.borrow_mut();
        if state.core.exit.is_some() || !state.core.started() {
            return Used::No;
        }
        match event {
            WidgetEvent::Key(key) => {
                if self.release.contains(key) {
                    return Used::No;
                }
                let page = cx.size().1.max(1) as isize;
                match (key.code, key.modifiers.shift) {
                    (KeyCode::PageUp, true) => state.core.scroll_by(page),
                    (KeyCode::PageDown, true) => state.core.scroll_by(-page),
                    _ => state.core.key(*key),
                }
                cx.redraw();
                Used::Yes
            }
            WidgetEvent::Paste(text) => {
                state.core.paste(text);
                cx.redraw();
                Used::Yes
            }
            WidgetEvent::Mouse(mouse) => {
                if !state.core.mouse(*mouse) {
                    return Used::No;
                }
                if state.core.wants_mouse() {
                    if mouse.kind
                        == rich_intuituive::interact::MouseKind::Down(
                            rich_intuituive::interact::Button::Left,
                        )
                    {
                        cx.capture_mouse();
                    }
                } else {
                    cx.redraw();
                }
                Used::Yes
            }
            _ => Used::No,
        }
    }

    fn focusable(&self) -> bool {
        true
    }

    fn caret(&self) -> Option<(u16, u16)> {
        self.caret
    }
}

/// Text shown as console markup, as it is.
fn escape(text: &str) -> String {
    text.replace('[', "\\[")
}
