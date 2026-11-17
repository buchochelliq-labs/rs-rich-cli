//! Toasts: [`Ctx::toast`](crate::Ctx::toast)'s one line of markup, and
//! [`Ctx::notify`](crate::Ctx::notify)'s notification with a level, a title
//! and a body; and the corner they stack in
//! ([`App::toasts_at`](crate::App::toasts_at)).

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use rich::{Console, Style};

use crate::app::{Ctx, Theme};
use crate::reactive::{Runtime, Signal};
use crate::screen::{Rect, Screen};

/// How much a [notification](crate::Ctx::notify) matters: its title and
/// border are in the theme's `notify.info`, `notify.success`,
/// `notify.warn` or `notify.error` style, else its `accent`, `good`,
/// `warn` or `bad`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Level {
    #[default]
    Info,
    Warn,
    Error,
    /// Something finished well (Textual has no such severity; Toad's
    /// and most toolkits' toasts do).
    Success,
}

impl Level {
    /// Its theme style's name, the theme style it falls back on, and the
    /// definition used when the theme has neither.
    pub(crate) fn names(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Level::Info => ("notify.info", "accent", "bright_cyan"),
            Level::Warn => ("notify.warn", "warn", "yellow"),
            Level::Error => ("notify.error", "bad", "bold red"),
            Level::Success => ("notify.success", "good", "green"),
        }
    }

    /// The mark shown before its title where there is no border: `ℹ`,
    /// `✓`, `⚠`, `✗`.
    pub(crate) fn mark(self) -> &'static str {
        match self {
            Level::Info => "ℹ",
            Level::Warn => "⚠",
            Level::Error => "✗",
            Level::Success => "✓",
        }
    }

    /// The theme style its title and border are drawn in.
    fn style(self, theme: &Theme) -> Style {
        let (own, fallback, definition) = self.names();
        let find = |name: &str| theme.styles.iter().find(|(n, _)| n == name);
        find(own)
            .or_else(|| find(fallback))
            .map(|(_, style)| style.clone())
            .unwrap_or_else(|| Style::parse(definition).unwrap_or_default())
    }

    /// How a screen reader is told it, before the title.
    pub(crate) fn spoken(self) -> &'static str {
        match self {
            Level::Info => "",
            Level::Warn => "Warning: ",
            Level::Error => "Error: ",
            Level::Success => "",
        }
    }
}

/// The corner of the screen toasts stack in, newest nearest the corner:
/// [`App::toasts_at`](crate::App::toasts_at). Bottom right by default;
/// Yazi's are top right.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    #[default]
    BottomRight,
}

type Run = Rc<RefCell<Box<dyn FnMut(&mut Ctx)>>>;

/// A notification with buttons, for [`Ctx::notify_with`](crate::Ctx::notify_with):
/// a level, a title and a body as [`Ctx::notify`](crate::Ctx::notify)
/// takes them, how long it shows, and [actions](Self::action).
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::{Level, Notification};
///
/// let app = App::new(|| {
///     let undone = signal(0);
///     column([text!("undone {undone}")]).on_key("d", move |cx| {
///         cx.notify_with(
///             Notification::new(Level::Success, "Deleted", "3 files")
///                 .action("Undo", move |_| undone.update(|n| *n += 1)),
///         )
///     })
/// });
/// let mut driver = app.driver(40, 6);
/// driver.press("d");
/// assert!(driver.click_text("Undo"));
/// assert_eq!(driver.screen().plain()[0].trim_end(), "undone 1");
/// // Clicked, it went.
/// assert!(driver.find_text("Deleted").is_none());
/// ```
#[must_use = "a notification does nothing until it is passed to Ctx::notify_with"]
pub struct Notification {
    level: Level,
    title: String,
    body: String,
    duration: Duration,
    actions: Vec<(String, Run)>,
}

impl Notification {
    /// A notification at `level` with `title` and `body` (markup), shown
    /// for five seconds.
    pub fn new(level: Level, title: impl Into<String>, body: impl Into<String>) -> Notification {
        Notification {
            level,
            title: title.into(),
            body: body.into(),
            duration: Duration::from_secs(5),
            actions: Vec::new(),
        }
    }

    /// Show it for `duration`.
    pub fn timeout(mut self, duration: Duration) -> Notification {
        self.duration = duration;
        self
    }

    /// A button labelled `label` (plain text) under the body that runs
    /// `run` and dismisses the notification, after those added before it.
    pub fn action(
        mut self,
        label: impl Into<String>,
        run: impl FnMut(&mut Ctx) + 'static,
    ) -> Notification {
        self.actions
            .push((label.into(), Rc::new(RefCell::new(Box::new(run)))));
        self
    }

    pub(crate) fn into_note(self) -> (Note, Duration) {
        let note = Note {
            markup: self.body,
            notice: Some((self.level, self.title)),
            actions: self.actions,
            id: 0,
        };
        (note, self.duration)
    }
}

/// A toast: a line of markup, or a notification's level, title and body,
/// and its buttons.
#[derive(Clone)]
pub(crate) struct Note {
    pub markup: String,
    pub notice: Option<(Level, String)>,
    pub actions: Vec<(String, Run)>,
    /// Which toast it is, once shown.
    pub id: u64,
}

impl Note {
    pub(crate) fn plain(markup: String) -> Note {
        Note {
            markup,
            notice: None,
            actions: Vec::new(),
            id: 0,
        }
    }

    pub(crate) fn notice(level: Level, title: String, markup: String) -> Note {
        Note {
            markup,
            notice: Some((level, title)),
            actions: Vec::new(),
            id: 0,
        }
    }

    /// What the history keeps of it, shown at `at`.
    pub(crate) fn record(&self, at: Duration) -> Notified {
        let (level, title) = match &self.notice {
            Some((level, title)) => (Some(*level), title.clone()),
            None => (None, String::new()),
        };
        Notified {
            level,
            title,
            body: self.markup.clone(),
            at,
        }
    }

    /// What a screen reader is told.
    pub(crate) fn spoken(&self) -> String {
        let text = |markup: &str| {
            rich::Text::from_markup(markup)
                .map(|text| text.plain().to_string())
                .unwrap_or_else(|_| markup.to_string())
        };
        match &self.notice {
            None => text(&self.markup),
            Some((level, title)) => {
                let body = text(&self.markup);
                let title = text(title);
                match (title.is_empty(), body.is_empty()) {
                    (true, _) => format!("{}{body}", level.spoken()),
                    (false, true) => format!("{}{title}", level.spoken()),
                    (false, false) => format!("{}{title}: {body}", level.spoken()),
                }
            }
        }
    }
}

/// A toast or notification shown, as [`notifications`] keeps it.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Notified {
    /// A notification's level; `None` for a plain toast.
    pub level: Option<Level>,
    /// Its title (markup); empty for a plain toast.
    pub title: String,
    /// Its body, or a toast's line (markup).
    pub body: String,
    /// When it was shown, on the app's clock.
    pub at: Duration,
}

/// The most [`notifications`] keeps.
pub(crate) const HISTORY: usize = 100;

/// The app's notification history: every toast and notification shown,
/// oldest first, the last 100, as a signal that changes as they come, so
/// a panel of past notifications is a node that reads it. Call it inside
/// an app (where nodes are built, or in a handler).
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::{notifications, Level};
///
/// let app = App::new(|| {
///     let history = notifications();
///     text(move || format!("{} seen", history.get().len()))
///         .on_key("n", |cx| cx.notify(Level::Info, "Saved", "notes.txt"))
///         .on_key("t", |cx| cx.toast("Copied"))
/// });
/// let mut driver = app.driver(40, 6);
/// driver.press("n t");
/// assert!(driver.screen().plain()[0].starts_with("2 seen"));
/// ```
pub fn notifications() -> Signal<Vec<Notified>> {
    history_of(&Runtime::current_for("notifications()"))
}

/// `runtime`'s history signal, made the first time it is asked for.
pub(crate) fn history_of(runtime: &Rc<Runtime>) -> Signal<Vec<Notified>> {
    match runtime.notifications.get() {
        Some(history) => history,
        None => {
            let history = runtime.enter(|| crate::reactive::signal(Vec::new()).untraced());
            runtime.notifications.set(Some(history));
            history
        }
    }
}

/// Where a toast was drawn: which one, its box, and its buttons' places
/// with their indices.
pub(crate) struct Drawn {
    pub id: u64,
    pub rect: Rect,
    pub actions: Vec<(Rect, usize)>,
}

/// The widest a notification grows unless
/// [`App::notify_width`](crate::App::notify_width) says.
pub(crate) const NOTICE_WIDTH: u16 = 48;

/// Draw `toasts` (oldest first) in `corner` of `area`, newest nearest the
/// corner, each a cell in from the side: a plain one as a box round its
/// line, a notification as a box with its title on the top edge and its
/// body wrapped inside, `most` cells wide at most, and its buttons on a
/// row under the body. Where they drew.
pub(crate) fn draw(
    console: &Console,
    screen: &mut Screen,
    area: Rect,
    toasts: &[&Note],
    (theme, corner, most): (&Theme, Corner, u16),
) -> Vec<Drawn> {
    let mut damage = Vec::new();
    let top = matches!(corner, Corner::TopLeft | Corner::TopRight);
    let left = matches!(corner, Corner::TopLeft | Corner::BottomLeft);
    // The rows still free: from `next` towards the far edge.
    let mut next = if top { area.y } else { area.bottom() };
    let mut first = true;
    for note in toasts.iter().rev() {
        let text = rich::Text::from_markup(&note.markup)
            .unwrap_or_else(|_| rich::Text::new(note.markup.clone()));
        // The buttons' row: each label padded, a cell between them.
        let buttons: u16 = note
            .actions
            .iter()
            .map(|(label, _)| rich::cells::cell_len(label) as u16 + 2)
            .sum::<u16>()
            + (note.actions.len() as u16).saturating_sub(1);
        let button_row = u16::from(!note.actions.is_empty());
        let (w, body) = match &note.notice {
            None => {
                let w = (text.cell_len() as u16 + 4).min(area.width.saturating_sub(2));
                (w, 1)
            }
            Some((_, title)) => {
                let title = crate::widget::markup_width(title).saturating_add(6);
                let widest = text
                    .plain()
                    .lines()
                    .map(rich::cells::cell_len)
                    .max()
                    .unwrap_or(0)
                    .min(u16::MAX as usize) as u16;
                let w = widest
                    .saturating_add(4)
                    .max(title)
                    .max(buttons.saturating_add(4))
                    .min(most)
                    .min(area.width.saturating_sub(2));
                let mut options = console
                    .options()
                    .update_width(w.saturating_sub(4).max(1) as usize);
                options.overflow = Some(rich::Overflow::Fold);
                let rows = console.render_lines(&text, &options, false).len().max(1);
                (
                    w,
                    (rows.min(u16::MAX as usize) as u16).saturating_add(button_row),
                )
            }
        };
        let h = body.saturating_add(2);
        let room = if top {
            area.bottom().saturating_sub(next)
        } else {
            next.saturating_sub(area.y)
        };
        if w < 5 || room < h {
            // No room for a box: the newest toast on the edge row,
            // reversed, so it is still seen.
            if first && area.width > 0 && area.height > 0 {
                let y = if top { area.y } else { area.bottom() - 1 };
                let rect = Rect::new(area.x, y, area.width, 1);
                let mut options = console.options().update_width(rect.width as usize);
                options.no_wrap = Some(true);
                options.overflow = Some(rich::Overflow::Ellipsis);
                let shown = match &note.notice {
                    Some((_, title)) => {
                        rich::Text::from_markup(&format!("{title}: {}", note.markup))
                            .unwrap_or_else(|_| text.clone())
                    }
                    None => text.clone(),
                };
                let line = console.render_lines(&shown, &options, false);
                let reverse = Style::parse("reverse").unwrap_or_default();
                screen.write_lines(rect, &line);
                for x in rect.x..rect.right() {
                    screen.restyle(x, rect.y, &reverse);
                }
                damage.push(Drawn {
                    id: note.id,
                    rect,
                    actions: Vec::new(),
                });
            }
            break;
        }
        first = false;
        let x = if left {
            area.x.saturating_add(1)
        } else {
            area.right().saturating_sub(w + 1)
        };
        let y = if top { next } else { next - h };
        let rect = Rect::new(x, y, w, h);
        match &note.notice {
            None => {
                let style = theme.border(false);
                let lines = crate::node::border("", &style, &style, rect);
                screen.write_lines(rect, &lines);
                let mut options = console.options().update_width(rect.width as usize - 4);
                options.no_wrap = Some(true);
                options.overflow = Some(rich::Overflow::Ellipsis);
                let inner = console.render_lines(&text, &options, false);
                screen.write_lines(Rect::new(rect.x + 2, rect.y + 1, rect.width - 4, 1), &inner);
            }
            Some((level, title)) => {
                let style = level.style(theme);
                let title_style = style.combine(&theme.title);
                let lines = crate::node::border("", &style, &style, rect);
                screen.write_lines(rect, &lines);
                let titles = [crate::border::Title::new(
                    crate::border::Edge::Top,
                    crate::layout::Align::Start,
                    title.clone(),
                )];
                if !title.is_empty() {
                    let look = crate::border::Frame {
                        border: crate::border::Border::Rounded,
                        sides: crate::border::Sides::ALL,
                        style: &style,
                        title_style: &title_style,
                        titles: &titles,
                        flush: false,
                    };
                    crate::border::draw_frame(screen, console, &look, rect);
                }
                let mut options = console.options().update_width(rect.width as usize - 4);
                options.overflow = Some(rich::Overflow::Fold);
                let inner = console.render_lines(&text, &options, false);
                let rows = body - button_row;
                let at = Rect::new(rect.x + 2, rect.y + 1, rect.width - 4, rows);
                screen.write_lines(at, &inner[..inner.len().min(rows as usize)]);
            }
        }
        let mut places = Vec::new();
        if button_row > 0 {
            let face = theme.named_style("button", "reverse");
            let (mut x, y) = (rect.x + 2, rect.y + body);
            for (i, (label, _)) in note.actions.iter().enumerate() {
                let w = rich::cells::cell_len(label) as u16 + 2;
                if x + w > rect.right().saturating_sub(2) {
                    break;
                }
                let place = Rect::new(x, y, w, 1);
                let line = vec![rich::Segment::new(format!(" {label} "), Some(face.clone()))];
                screen.write_lines(place, &[line]);
                places.push((place, i));
                x += w + 1;
            }
        }
        damage.push(Drawn {
            id: note.id,
            rect,
            actions: places,
        });
        if top {
            next += h;
        } else {
            next -= h;
        }
    }
    damage
}
