//! A section that opens and closes: [`collapsible`], and
//! [`collapsible_with`], whose title follows state, with a status beside
//! it and a summary while it is closed (a tool call's card).

use std::cell::RefCell;
use std::rc::Rc;

use rich_interact::{Button as MouseButton, KeyCode, MouseKind};

use crate::a11y::{AccessState, Role};
use crate::controls::{look, plain};
use crate::node::{Axis, Node, column};
use crate::reactive::Signal;
use crate::spinner::Turn;
use crate::widget::{
    Canvas, DrawCx, EventCx, MeasureCx, Used, Widget, WidgetEvent, markup_width, widget,
};

/// What a [`collapsible`] shows beside its title, at the right of the
/// header: [`Node::status`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Status {
    /// Nothing.
    #[default]
    None,
    /// A spinner turning on the app's clock: one of rich's spinners
    /// (`dots`, `line`, `arc`, …; `dots` for a name it does not know).
    /// Busy, for a screen reader.
    Spinner(&'static str),
    /// Console markup: `[green]✓[/]`, `[red]✗ 2[/]`, `3.2 s`.
    Markup(String),
}

type Read<T> = Box<dyn Fn() -> T>;

/// The parts of a header set after it is made: shared by the header and
/// the summary line under it.
#[derive(Default)]
struct Slots {
    status: Option<Read<Status>>,
    summary: Option<Read<String>>,
    /// The markers closed and open, over `▸` and `▾`.
    symbols: Option<(String, String)>,
}

/// What a [`collapsible`]'s header is.
struct Header {
    title: Read<String>,
    open: Signal<bool>,
    slots: Rc<RefCell<Slots>>,
    /// The spinner last drawn, by name.
    turn: Option<(&'static str, Turn)>,
}

/// A section titled `title` (console markup) over `child`, shown while
/// `open` holds: a header with a disclosure marker, `▾ Advanced` open and
/// `▸ Advanced` closed (`[-]` and `[+]` in text mode). Enter, Space or a
/// click on the header opens or closes it; → opens it and ← closes it.
/// Closed, the child takes no room and leaves the Tab order. The title is
/// drawn in the theme's `collapsible.title` style (else `bold`), with
/// `control.focus` over it while the header has the focus. The header's
/// role is [`Button`](crate::a11y::Role::Button), named by the title, with
/// its expanded state. The section is as tall as what it shows.
/// [`collapsible_with`] takes a title that follows state; both take a
/// [status](Node::status), a [summary](Node::summary) and their own
/// [markers](Node::symbols).
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::collapsible;
///
/// let app = App::new(|| {
///     collapsible("Advanced", signal(false), label("Cache size: 64 MB"))
/// });
/// let mut driver = app.driver(20, 2);
/// driver.settle();
/// assert_eq!(driver.screen().plain()[0].trim_end(), "▸ Advanced");
/// assert_eq!(driver.accessibility()[0].describe(), "Advanced, button, collapsed");
/// driver.press("enter");
/// assert_eq!(driver.screen().plain()[1].trim_end(), "Cache size: 64 MB");
/// assert_eq!(driver.accessibility()[0].describe(), "Advanced, button, expanded");
/// ```
pub fn collapsible(title: impl Into<String>, open: Signal<bool>, child: Node) -> Node {
    let title = title.into();
    collapsible_with(move || title.clone(), open, child)
}

/// [`collapsible`], titled by `title`, read again whenever a signal it
/// reads changes: only the header draws again. With a
/// [status](Node::status) at the right of the header and a
/// [summary](Node::summary) under it while it is closed, it is a tool
/// call's card: the tool and its argument, running then done or failed,
/// and the output's last line.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::{collapsible_with, Status};
///
/// let app = App::new(|| {
///     let done = signal(false);
///     collapsible_with(|| "[b]bash[/] ls".into(), signal(false), label("a.txt\nb.txt"))
///         .status(move || match done.get() {
///             false => Status::Spinner("line"),
///             true => Status::Markup("ok".into()),
///         })
///         .summary(|| "b.txt".into())
///         .on_key("d", move |_| done.set(true))
/// });
/// let mut driver = app.driver(16, 3);
/// driver.settle();
/// assert_eq!(driver.screen().plain()[0], "▸ bash ls      -");
/// assert_eq!(driver.screen().plain()[1].trim_end(), "  b.txt");
/// driver.press("d");
/// assert_eq!(driver.screen().plain()[0], "▸ bash ls     ok");
/// driver.press("enter");
/// assert_eq!(driver.screen().plain()[1].trim_end(), "a.txt");
/// ```
pub fn collapsible_with(
    title: impl Fn() -> String + 'static,
    open: Signal<bool>,
    child: Node,
) -> Node {
    let slots = Rc::new(RefCell::new(Slots::default()));
    let header = widget(Header {
        title: Box::new(title),
        open,
        slots: slots.clone(),
        turn: None,
    })
    .auto();
    let shown = slots.clone();
    let summary = widget(Summary {
        slots: slots.clone(),
    })
    .fixed(1)
    .access_hidden(true)
    .shown_when(move || !open.get() && shown.borrow().summary.is_some());
    column([header, summary, child.shown_when(move || open.get())])
        .auto()
        .name("collapsible")
}

/// The slots of a [`collapsible`]: those of its header, its first child.
fn slots_of(node: &Node) -> Option<Rc<RefCell<Slots>>> {
    let body = node.body.borrow();
    let header = body.widget.children().first()?;
    header.widget_mut(|header: &mut Header| header.slots.clone())
}

/// A [`collapsible`]'s status, summary and markers. On any other node
/// they do nothing.
impl Node {
    /// On a [`collapsible`]: show `status` at the right of the header, read
    /// again whenever a signal it reads changes. A spinner while a tool
    /// runs, then `✓` or `✗ 2`. A spinner turns on the app's clock and
    /// marks the header busy for a screen reader; markup is said after
    /// the title.
    pub fn status(self, status: impl Fn() -> Status + 'static) -> Node {
        if let Some(slots) = slots_of(&self) {
            slots.borrow_mut().status = Some(Box::new(status));
        }
        self
    }

    /// On a [`collapsible`]: a line under the header while it is closed,
    /// read again whenever a signal it reads changes, drawn in the theme's
    /// `collapsible.summary` style (else `dim`): a tool's last line of
    /// output. Plain text, its first line, cut off at the width.
    pub fn summary(self, summary: impl Fn() -> String + 'static) -> Node {
        if let Some(slots) = slots_of(&self) {
            slots.borrow_mut().summary = Some(Box::new(summary));
        }
        self
    }

    /// On a [`collapsible`]: the markers drawn closed and open, over `▸`
    /// and `▾` (Textual's `collapsed_symbol` and `expanded_symbol`). Text
    /// mode keeps `[+]` and `[-]`.
    pub fn symbols(self, closed: &str, open: &str) -> Node {
        if let Some(slots) = slots_of(&self) {
            slots.borrow_mut().symbols = Some((closed.to_string(), open.to_string()));
        }
        self
    }
}

impl Header {
    /// The disclosure marker.
    fn marker(&self, open: bool) -> String {
        match (crate::a11y::text_mode(), open, &self.slots.borrow().symbols) {
            (false, false, Some((closed, _))) => closed.clone(),
            (false, true, Some((_, opened))) => opened.clone(),
            (false, true, None) => "▾".into(),
            (false, false, None) => "▸".into(),
            (true, true, _) => "[-]".into(),
            (true, false, _) => "[+]".into(),
        }
    }

    fn status(&self) -> Status {
        self.slots
            .borrow()
            .status
            .as_ref()
            .map_or(Status::None, |status| status())
    }

    /// The title, read without subscribing: for what is said about it.
    fn untracked_title(&self) -> String {
        plain(&crate::reactive::untracked(|| (self.title)()))
    }
}

impl Widget for Header {
    fn name(&self) -> &'static str {
        "collapsible header"
    }

    fn describe(&self) -> Option<String> {
        Some(self.untracked_title())
    }

    fn role(&self) -> Role {
        Role::Button
    }

    fn access_text(&self) -> Option<String> {
        let title = self.untracked_title();
        Some(match crate::reactive::untracked(|| self.status()) {
            Status::Markup(markup) if !plain(&markup).trim().is_empty() => {
                format!("{title}, {}", plain(&markup).trim())
            }
            _ => title,
        })
    }

    fn access_state(&self) -> AccessState {
        AccessState {
            expanded: Some(self.open.get_untracked()),
            busy: matches!(
                crate::reactive::untracked(|| self.status()),
                Status::Spinner(_)
            ),
            ..AccessState::default()
        }
    }

    fn measure(&mut self, _cx: &MeasureCx, axis: Axis, _width: u16, _height: u16) -> u16 {
        match axis {
            Axis::Horizontal => {
                let marker = rich::cells::cell_len(&self.marker(true)) as u16;
                let status = match self.status() {
                    Status::None => 0,
                    Status::Spinner(name) => Turn::new(name).width() + 1,
                    Status::Markup(markup) => markup_width(&markup) + 1,
                };
                markup_width(&(self.title)())
                    .saturating_add(marker + 1)
                    .saturating_add(status)
            }
            Axis::Vertical => 1,
        }
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        let open = self.open.get();
        let marker = self.marker(open);
        let cells = rich::cells::cell_len(&marker).min(u16::MAX as usize) as u16;
        let style = look(cx, &["collapsible.title"], "bold");
        canvas.print(0, 0, &marker, Some(&style));
        // The status first, at the right: the title has what is left.
        let width = canvas.width();
        let status = match self.status() {
            Status::None => None,
            Status::Spinner(name) => {
                if self.turn.as_ref().is_none_or(|(had, _)| *had != name) {
                    self.turn = Some((name, Turn::new(name)));
                }
                let (_, turn) = self.turn.as_ref().expect("just set");
                let frame = turn.frame(cx);
                Some((frame.to_string(), rich::cells::cell_len(frame) as u16, true))
            }
            Status::Markup(markup) => {
                let cells = markup_width(&markup);
                Some((markup, cells, false))
            }
        };
        let mut room = width.saturating_sub(cells + 1);
        if let Some((shown, size, spinner)) = status
            && size < room
        {
            let x = width - size;
            let look = look(cx, &["collapsible.status"], "none");
            if spinner {
                canvas.print(x, 0, &shown, Some(&look));
            } else {
                canvas.markup(cx.console(), x, 0, size, &shown, Some(&look));
            }
            room = room.saturating_sub(size + 1);
        }
        let title = (self.title)();
        let shown = canvas.markup(cx.console(), cells + 1, 0, room, &title, Some(&style));
        let over = match (cx.disabled(), cx.focused()) {
            (true, _) => Some(cx.style("control.disabled", "dim")),
            (false, true) => Some(cx.style("control.focus", "reverse")),
            (false, false) => None,
        };
        if let Some(over) = over {
            canvas.restyle(0, 0, cells + 1 + shown, 1, &over);
        }
    }

    fn event(&mut self, _cx: &mut EventCx, event: &WidgetEvent) -> Used {
        let open = self.open.get_untracked();
        let to = match event {
            WidgetEvent::Key(key) => match key.code {
                KeyCode::Enter | KeyCode::Char(' ') => !open,
                KeyCode::Right if !open => true,
                KeyCode::Left if open => false,
                _ => return Used::No,
            },
            WidgetEvent::Mouse(mouse) if mouse.kind == MouseKind::Down(MouseButton::Left) => !open,
            _ => return Used::No,
        };
        self.open.set(to);
        Used::Yes
    }

    fn focusable(&self) -> bool {
        true
    }
}

/// The line under a closed [`collapsible`]: [`Node::summary`].
struct Summary {
    slots: Rc<RefCell<Slots>>,
}

impl Widget for Summary {
    fn name(&self) -> &'static str {
        "collapsible summary"
    }

    fn measure(&mut self, _cx: &MeasureCx, axis: Axis, _width: u16, _height: u16) -> u16 {
        match axis {
            Axis::Horizontal => {
                let line = self.line();
                (rich::cells::cell_len(&line) + 2).min(u16::MAX as usize) as u16
            }
            Axis::Vertical => 1,
        }
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        let line = self.line();
        let room = (canvas.width() as usize).saturating_sub(2);
        let shown = rich::cells::set_cell_size(&line, room);
        let style = look(cx, &["collapsible.summary"], "dim");
        canvas.print(2, 0, shown.trim_end(), Some(&style));
    }
}

impl Summary {
    /// The summary's first line.
    fn line(&self) -> String {
        let slots = self.slots.borrow();
        let text = slots.summary.as_ref().map(|summary| summary());
        text.and_then(|t| t.lines().next().map(str::to_string))
            .unwrap_or_default()
    }
}
