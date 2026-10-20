//! A command line: [`command_line`], a bar of one or two rows that opens
//! in a [`Mode`] (vim's `:` command, a `/` search, a filter that follows
//! each key), asks yes or no in place ([`CommandLine::confirm`]), and
//! says what went wrong in red. Its input completes and keeps a history
//! as any [`input`](crate::input)'s. Generic: taskwarrior-tui's command
//! bar, Yazi's and lazygit's prompts are modes of it.

use std::cell::RefCell;
use std::rc::Rc;

use rich_interact::{Key, KeyCode};

use crate::app::Ctx;
use crate::complete::{CompleteOptions, Completer, Completion, InputHistory};
use crate::node::{Axis, Node};
use crate::reactive::{signal, NodeId, Runtime, Signal};
use crate::screen::Rect;
use crate::text_area::{Options, TextArea};
use crate::widget::{widget, Canvas, DrawCx, EventCx, MeasureCx, Used, Widget, WidgetEvent};

/// What a mode runs on Enter: `Ok` closes the bar, `Err(message)` keeps
/// it open with the message in red.
type OnSubmit = Box<dyn FnMut(&str, &mut Ctx) -> Result<(), String>>;
type OnChange = Box<dyn FnMut(&str, &mut Ctx)>;
type OnCancel = Box<dyn FnMut(&mut Ctx)>;

/// A way the command line opens: its prefix (`:`, `/`), its title, the
/// text it starts with, its completion and history, and what hears
/// Enter, each edit and Esc.
///
/// ```
/// use intuituive::command_line::Mode;
/// use intuituive::complete_words;
///
/// let filter = Mode::new("/")
///     .title("Filter Tasks")
///     .complete(complete_words(["project:", "status:"]))
///     .on_change(|text, _| println!("filter: {text}"))
///     .on_submit(|_, _| Ok(()));
/// # let _ = filter;
/// ```
#[derive(Default)]
pub struct Mode {
    prefix: String,
    title: Option<String>,
    initial: String,
    complete: Option<(Completer, CompleteOptions)>,
    history: Option<InputHistory>,
    submit: Option<OnSubmit>,
    change: Option<OnChange>,
    cancel: Option<OnCancel>,
}

impl Mode {
    /// A mode whose line starts with `prefix` (`":"`, `"/"`, or `""`).
    pub fn new(prefix: impl Into<String>) -> Mode {
        Mode {
            prefix: prefix.into(),
            ..Mode::default()
        }
    }

    /// The bar's title (console markup): above the line in a two-row bar,
    /// before it in a one-row one.
    pub fn title(mut self, title: impl Into<String>) -> Mode {
        self.title = Some(title.into());
        self
    }

    /// The text the line starts with, the caret at its end.
    pub fn initial(mut self, text: impl Into<String>) -> Mode {
        self.initial = text.into();
        self
    }

    /// Complete the line, as [`Node::complete`](crate::Node::complete).
    pub fn complete(self, complete: impl Fn(&str, usize) -> Vec<Completion> + 'static) -> Mode {
        self.complete_with(complete, CompleteOptions::default())
    }

    /// [`complete`](Self::complete) as `options` say.
    pub fn complete_with(
        mut self,
        complete: impl Fn(&str, usize) -> Vec<Completion> + 'static,
        options: CompleteOptions,
    ) -> Mode {
        self.complete = Some((Rc::new(complete), options));
        self
    }

    /// Go through `history` with ↑ and ↓, and add each line entered to it,
    /// as [`Node::history`](crate::Node::history).
    pub fn history(mut self, history: impl Into<InputHistory>) -> Mode {
        self.history = Some(history.into());
        self
    }

    /// Run `submit` with the line on Enter: `Ok(())` closes the bar,
    /// `Err(message)` keeps it open and shows the message in red.
    pub fn on_submit(
        mut self,
        submit: impl FnMut(&str, &mut Ctx) -> Result<(), String> + 'static,
    ) -> Mode {
        self.submit = Some(Box::new(submit));
        self
    }

    /// Run `change` with the line after each edit: a filter as you type.
    pub fn on_change(mut self, change: impl FnMut(&str, &mut Ctx) + 'static) -> Mode {
        self.change = Some(Box::new(change));
        self
    }

    /// Run `cancel` when Esc closes the bar (a filter put back).
    pub fn on_cancel(mut self, cancel: impl FnMut(&mut Ctx) + 'static) -> Mode {
        self.cancel = Some(Box::new(cancel));
        self
    }
}

/// A yes-or-no question's answer.
type OnYes = Box<dyn FnOnce(&mut Ctx)>;

/// What the bar shows.
#[derive(Clone, Default)]
enum Bar {
    /// Closed, with a message (markup) or nothing.
    #[default]
    Idle,
    Message(String),
    Error(String),
    /// Typing in a mode, with the error its last Enter gave.
    Typing(Rc<RefCell<Mode>>, Option<String>),
    /// Asking, with the keys that answer yes.
    Confirm(String, Vec<Key>, Rc<RefCell<Option<OnYes>>>),
}

impl PartialEq for Bar {
    /// The same mode or question (not one alike), and the same texts.
    fn eq(&self, other: &Bar) -> bool {
        match (self, other) {
            (Bar::Idle, Bar::Idle) => true,
            (Bar::Message(a), Bar::Message(b)) | (Bar::Error(a), Bar::Error(b)) => a == b,
            (Bar::Typing(a, x), Bar::Typing(b, y)) => Rc::ptr_eq(a, b) && x == y,
            (Bar::Confirm(_, _, a), Bar::Confirm(_, _, b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }
}

/// A command line's state: open it in a [`Mode`], ask a question, show an
/// error. Made by [`CommandLine::new`] while the app is built and given
/// to [`command_line`]; `Copy`, so key bindings take it.
#[derive(Clone, Copy)]
pub struct CommandLine {
    bar: Signal<Bar>,
    /// Bumped on each open, so the bar sets its input up again.
    opened: Signal<u64>,
    value: Signal<String>,
    id: Signal<Option<NodeId>>,
    /// Where the focus was when it opened, to give it back.
    before: Signal<Option<NodeId>>,
}

impl Default for CommandLine {
    fn default() -> CommandLine {
        CommandLine::new()
    }
}

impl CommandLine {
    /// A closed command line.
    pub fn new() -> CommandLine {
        CommandLine {
            bar: signal(Bar::Idle),
            opened: signal(0),
            value: signal(String::new()),
            id: signal(None),
            before: signal(None),
        }
    }

    /// Open the line in `mode`, taking the focus (given back when it
    /// closes).
    pub fn open(&self, cx: &mut Ctx, mode: Mode) {
        self.value.set(mode.initial.clone());
        self.bar.set(Bar::Typing(Rc::new(RefCell::new(mode)), None));
        self.opened.update(|n| *n += 1);
        self.take_focus(cx);
    }

    /// Ask `question` (markup) in the bar: `y` runs `on_yes`, `n` or Esc
    /// does not, and either closes it; other keys are ignored.
    pub fn confirm(
        &self,
        cx: &mut Ctx,
        question: impl Into<String>,
        on_yes: impl FnOnce(&mut Ctx) + 'static,
    ) {
        self.confirm_keys(cx, question, "y", on_yes);
    }

    /// [`confirm`](Self::confirm) answered yes by `keys` (`"x"`: press `x`
    /// again to confirm a delete, as taskwarrior-tui asks); `n`, `q` and
    /// Esc answer no.
    pub fn confirm_keys(
        &self,
        cx: &mut Ctx,
        question: impl Into<String>,
        keys: &str,
        on_yes: impl FnOnce(&mut Ctx) + 'static,
    ) {
        let keys = keys.split_whitespace().filter_map(Key::parse).collect();
        let on_yes: OnYes = Box::new(on_yes);
        self.bar.set(Bar::Confirm(
            question.into(),
            keys,
            Rc::new(RefCell::new(Some(on_yes))),
        ));
        self.take_focus(cx);
    }

    /// Show `message` (plain text) in red until the next open or message;
    /// while typing, over the line's title, the line kept open.
    pub fn error(&self, message: impl Into<String>) {
        let message = message.into();
        let typing = self.bar.with_untracked(|bar| match bar {
            Bar::Typing(mode, _) => Some(mode.clone()),
            _ => None,
        });
        match typing {
            Some(mode) => self.bar.set(Bar::Typing(mode, Some(message))),
            None => self.bar.set(Bar::Error(message)),
        }
    }

    /// Show `markup` in the closed bar (a hint, a count).
    pub fn message(&self, markup: impl Into<String>) {
        self.bar.set(Bar::Message(markup.into()));
    }

    /// Close it (an error or message goes too), giving the focus back.
    pub fn close(&self, cx: &mut Ctx) {
        let open = self.is_open_untracked();
        self.bar.set(Bar::Idle);
        if open {
            if let Some(before) = self.before.get_untracked() {
                cx.focus(before);
            }
        }
    }

    /// Whether it is open (typing or asking), subscribing.
    pub fn is_open(&self) -> bool {
        self.bar
            .with(|bar| matches!(bar, Bar::Typing(..) | Bar::Confirm(..)))
    }

    fn is_open_untracked(&self) -> bool {
        self.bar
            .with_untracked(|bar| matches!(bar, Bar::Typing(..) | Bar::Confirm(..)))
    }

    /// The line as typed, subscribing.
    pub fn text(&self) -> String {
        self.value.get()
    }

    fn take_focus(&self, cx: &mut Ctx) {
        let Some(id) = self.id.get_untracked() else {
            return;
        };
        let focused = Runtime::current()
            .focus_path()
            .and_then(|path| path.last().copied());
        if focused != Some(id) {
            self.before.set(focused);
        }
        cx.focus(id);
    }
}

/// How a [`command_line_with`] looks: one row (the default) or two (the
/// title on its own row above the line, taskwarrior-tui's).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CommandLineOptions {
    pub(crate) two_rows: bool,
}

impl CommandLineOptions {
    pub fn new() -> CommandLineOptions {
        CommandLineOptions::default()
    }

    /// The title on a row of its own above the line.
    pub fn two_rows(mut self) -> CommandLineOptions {
        self.two_rows = true;
        self
    }
}

/// The bar of command line `state`, one row tall: closed, its message
/// (or an error, in the theme's `error` style); open, the title (in
/// `command.title`, reversed), the mode's prefix and the line; asking,
/// the question (`command.question`, bold). Enter runs the mode's
/// `on_submit`, Esc its `on_cancel`, and both close it (Esc closes the
/// completion pop-up first); the focus goes back where it was.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::command_line::{command_line, CommandLine, Mode};
///
/// let app = App::new(|| {
///     let cmd = CommandLine::new();
///     let ran = signal(String::new());
///     column([
///         text!("ran {ran}").flex(1),
///         command_line(cmd),
///     ])
///     .on_key(":", move |cx| {
///         cmd.open(cx, Mode::new(":").on_submit(move |text, _| match text {
///             "quit" | "write" => Ok(ran.set(text.to_string())),
///             _ => Err(format!("Not an editor command: {text}")),
///         }))
///     })
///     .on_key("d", move |cx| cmd.confirm(cx, "Delete it? (y/n)", move |_| ran.set("delete".into())))
/// });
/// let mut driver = app.driver(40, 3);
/// driver.press(":");
/// driver.type_text("wq");
/// driver.press("enter");
/// // One row: the error before the line, which stays open.
/// assert_eq!(driver.screen().plain()[2].trim_end(), "Not an editor command: wq :wq");
/// driver.press("backspace");
/// driver.type_text("rite");
/// driver.press("enter");
/// assert_eq!(driver.screen().plain()[0].trim_end(), "ran write");
/// driver.press("d");
/// assert_eq!(driver.screen().plain()[2].trim_end(), "Delete it? (y/n)");
/// driver.press("y");
/// assert_eq!(driver.screen().plain()[0].trim_end(), "ran delete");
/// ```
pub fn command_line(state: CommandLine) -> Node {
    command_line_with(state, CommandLineOptions::default())
}

/// A [`command_line`] that looks as `options` say.
pub fn command_line_with(state: CommandLine, options: CommandLineOptions) -> Node {
    let area = TextArea::new(
        state.value,
        Options {
            single_line: true,
            ..Options::default()
        },
    );
    let node = widget(CommandBar {
        state,
        area,
        options,
        opened: u64::MAX,
        input: Rect::default(),
    })
    .what("command_line");
    state.id.set(Some(node.id()));
    node
}

struct CommandBar {
    state: CommandLine,
    area: TextArea,
    options: CommandLineOptions,
    /// The open the input was last set up for.
    opened: u64,
    /// Where the line's input was drawn, in the bar.
    input: Rect,
}

impl CommandBar {
    /// Set the input up for the mode it opened in.
    fn set_up(&mut self) {
        let opened = self.state.opened.get_untracked();
        if opened == self.opened {
            return;
        }
        self.opened = opened;
        let (complete, history) = self.state.bar.with_untracked(|bar| match bar {
            Bar::Typing(mode, _) => {
                let mode = mode.borrow();
                (mode.complete.clone(), mode.history)
            }
            _ => (None, None),
        });
        let (completer, options) = match complete {
            Some((completer, options)) => (Some(completer), options),
            None => (None, CompleteOptions::default()),
        };
        self.area.assist.completer = completer;
        self.area.assist.options = options;
        self.area.assist.history = history.map(|h| Rc::new(move || Some(h)) as _);
        self.area.assist.popup = None;
        self.area.assist.browse = None;
    }

    fn mode(&self) -> Option<Rc<RefCell<Mode>>> {
        self.state.bar.with_untracked(|bar| match bar {
            Bar::Typing(mode, _) => Some(mode.clone()),
            _ => None,
        })
    }

    /// A key while typing.
    fn typing(&mut self, cx: &mut EventCx, key: Key, mode: Rc<RefCell<Mode>>) -> Used {
        let plain = !key.modifiers.ctrl && !key.modifiers.alt;
        let popup = self.area.assist.popup.is_some();
        match key.code {
            KeyCode::Enter if plain && !popup => {
                let text = self.state.value.get_untracked();
                if let Some(history) = self.area.assist.history() {
                    history.push(&text);
                }
                let result = match mode.borrow_mut().submit.as_mut() {
                    Some(submit) => submit(&text, cx.app()),
                    None => Ok(()),
                };
                // The handler may have opened another mode or asked.
                let still = self.mode().is_some_and(|m| Rc::ptr_eq(&m, &mode));
                match result {
                    Ok(()) if still => self.state.close(cx.app()),
                    Ok(()) => {}
                    Err(message) => self.state.error(message),
                }
                cx.redraw();
                Used::Yes
            }
            KeyCode::Escape if !popup && self.area.buffer_selection().is_none() => {
                self.state.close(cx.app());
                if let Some(cancel) = mode.borrow_mut().cancel.as_mut() {
                    cancel(cx.app());
                }
                cx.redraw();
                Used::Yes
            }
            // Tab with nothing to complete stays in the line.
            KeyCode::Tab | KeyCode::BackTab => {
                self.area.event(cx, &WidgetEvent::Key(key));
                Used::Yes
            }
            _ => {
                let before = self.state.value.get_untracked();
                let used = self.area.event(cx, &WidgetEvent::Key(key));
                let after = self.state.value.get_untracked();
                if after != before {
                    if let Some(change) = mode.borrow_mut().change.as_mut() {
                        change(&after, cx.app());
                    }
                }
                used
            }
        }
    }
}

impl Widget for CommandBar {
    fn name(&self) -> &'static str {
        "command_line"
    }

    fn role(&self) -> crate::a11y::Role {
        crate::a11y::Role::TextBox
    }

    fn focusable(&self) -> bool {
        self.state.is_open_untracked()
    }

    fn measure(&mut self, _cx: &MeasureCx, axis: Axis, width: u16, _height: u16) -> u16 {
        match axis {
            Axis::Vertical => 1 + self.options.two_rows as u16,
            Axis::Horizontal => width.max(20),
        }
    }

    fn caret(&self) -> Option<(u16, u16)> {
        self.mode()?;
        let (x, y) = self.area.caret()?;
        Some((self.input.x + x, self.input.y + y))
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        let bar = self.state.bar.get();
        self.state.opened.get();
        let (width, height) = (canvas.width(), canvas.height());
        let error = cx.theme().error_style("error");
        let title_style = cx.style("command.title", "reverse");
        let question = cx.style("command.question", "bold");
        // The row the line goes on, and the one above it for a title.
        let line = height.saturating_sub(1);
        let mut input = None;
        match &bar {
            Bar::Idle => {}
            Bar::Message(markup) => {
                canvas.markup(cx.console(), 0, line, width, markup, None);
            }
            Bar::Error(message) => {
                canvas.print(0, line, message, Some(&error));
            }
            Bar::Confirm(text, _, _) => {
                canvas.markup(cx.console(), 0, line, width, text, Some(&question));
            }
            Bar::Typing(mode, failed) => {
                let mode = mode.borrow();
                let mut x = 0;
                match (&mode.title, failed, self.options.two_rows) {
                    (_, Some(message), true) => {
                        canvas.fill(0, 0, width, 1, Some(&error));
                        canvas.print(0, 0, message, Some(&error));
                    }
                    (Some(title), None, true) => {
                        canvas.fill(0, 0, width, 1, Some(&title_style));
                        canvas.markup(cx.console(), 0, 0, width, title, Some(&title_style));
                    }
                    (_, Some(message), false) => {
                        x = canvas.markup(
                            cx.console(),
                            0,
                            line,
                            width,
                            &crate::rich::markup::escape(message),
                            Some(&error),
                        );
                        x = (x + 1).min(width);
                    }
                    (Some(title), None, false) => {
                        x = canvas.markup(cx.console(), 0, line, width, title, Some(&title_style));
                        x = (x + 1).min(width);
                    }
                    (None, None, _) => {}
                }
                if !mode.prefix.is_empty() {
                    canvas.print(x, line, &mode.prefix, None);
                    x = (x + crate::rich::cells::cell_len(&mode.prefix) as u16).min(width);
                }
                input = Some(Rect::new(x, line, width - x, 1));
            }
        }
        let Some(at) = input else {
            self.input = Rect::default();
            // A closed bar has no pop-up.
            if std::mem::take(&mut self.area.assist.floated) {
                cx.float(None);
            }
            return;
        };
        self.set_up();
        self.input = at;
        let rect = Rect::new(
            canvas.rect.x + at.x,
            canvas.rect.y + at.y,
            at.width,
            at.height,
        );
        let mut part = Canvas {
            screen: &mut *canvas.screen,
            rect,
            written: &mut *canvas.written,
        };
        let mut inner = DrawCx {
            console: cx.console,
            theme: cx.theme,
            id: cx.id,
            rect: Rect::new(cx.rect.x + at.x, cx.rect.y + at.y, at.width, at.height),
            repaint: true,
            focus_path: cx.focus_path,
            hover_path: cx.hover_path,
            pointer: cx.pointer,
            shift: cx.shift,
            wants_hover: cx.wants_hover,
            watchers: cx.watchers,
            disabled: cx.disabled,
        };
        self.area.draw(&mut inner, &mut part);
    }

    fn event(&mut self, cx: &mut EventCx, event: &WidgetEvent) -> Used {
        let bar = self.state.bar.get_untracked();
        match (event, bar) {
            (WidgetEvent::Key(key), Bar::Confirm(_, yes, on_yes)) => {
                let key = *key;
                let no = ["n", "q", "esc"]
                    .iter()
                    .any(|k| Key::parse(k).is_some_and(|k| k == key));
                if yes.contains(&key) {
                    self.state.close(cx.app());
                    if let Some(on_yes) = on_yes.borrow_mut().take() {
                        on_yes(cx.app());
                    }
                } else if no {
                    self.state.close(cx.app());
                } else if key.code == KeyCode::Char('c') && key.modifiers.ctrl {
                    return Used::No;
                }
                cx.redraw();
                Used::Yes
            }
            (WidgetEvent::Key(key), Bar::Typing(mode, failed)) => {
                self.set_up();
                // A key after an error clears it.
                if failed.is_some() {
                    self.state.bar.set(Bar::Typing(mode.clone(), None));
                }
                self.typing(cx, *key, mode)
            }
            (WidgetEvent::Paste(_), Bar::Typing(mode, _)) => {
                self.set_up();
                let before = self.state.value.get_untracked();
                let used = self.area.event(cx, event);
                let after = self.state.value.get_untracked();
                if after != before {
                    if let Some(change) = mode.borrow_mut().change.as_mut() {
                        change(&after, cx.app());
                    }
                }
                used
            }
            (WidgetEvent::Focus(_), Bar::Typing(..)) => self.area.event(cx, event),
            _ => Used::No,
        }
    }

    fn claims_key(&self, key: Key) -> bool {
        self.state.is_open_untracked() && self.area.claims_key(key)
    }
}
