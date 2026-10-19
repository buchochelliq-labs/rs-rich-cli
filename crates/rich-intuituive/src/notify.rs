//! Toasts: [`Ctx::toast`](crate::Ctx::toast)'s one line of markup, and
//! [`Ctx::notify`](crate::Ctx::notify)'s notification with a level, a title
//! and a body; and the corner they stack in
//! ([`App::toasts_at`](crate::App::toasts_at)).

use rich::{Console, Style};

use crate::app::Theme;
use crate::screen::{Rect, Screen};

/// How much a [notification](crate::Ctx::notify) matters: its title and
/// border are in the theme's `notify.info`, `notify.warn` or
/// `notify.error` style, else its `accent`, `warn` or `bad`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Level {
    #[default]
    Info,
    Warn,
    Error,
}

impl Level {
    /// The theme style its title and border are drawn in.
    fn style(self, theme: &Theme) -> Style {
        let (own, fallback, definition) = match self {
            Level::Info => ("notify.info", "accent", "bright_cyan"),
            Level::Warn => ("notify.warn", "warn", "yellow"),
            Level::Error => ("notify.error", "bad", "bold red"),
        };
        let find = |name: &str| theme.styles.iter().find(|(n, _)| n == name);
        find(own)
            .or_else(|| find(fallback))
            .map(|(_, style)| style.clone())
            .unwrap_or_else(|| Style::parse(definition).unwrap_or_default())
    }

    /// How a screen reader is told it, before the title.
    fn spoken(self) -> &'static str {
        match self {
            Level::Info => "",
            Level::Warn => "Warning: ",
            Level::Error => "Error: ",
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

/// A toast: a line of markup, or a notification's level, title and body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Note {
    pub markup: String,
    pub notice: Option<(Level, String)>,
}

impl Note {
    pub(crate) fn plain(markup: String) -> Note {
        Note {
            markup,
            notice: None,
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

/// The widest a notification grows unless
/// [`App::notify_width`](crate::App::notify_width) says.
pub(crate) const NOTICE_WIDTH: u16 = 48;

/// Draw `toasts` (oldest first) in `corner` of `area`, newest nearest the
/// corner, each a cell in from the side: a plain one as a box round its
/// line, a notification as a box with its title on the top edge and its
/// body wrapped inside, `most` cells wide at most. Where they drew.
pub(crate) fn draw(
    console: &Console,
    screen: &mut Screen,
    area: Rect,
    toasts: &[&Note],
    (theme, corner, most): (&Theme, Corner, u16),
) -> Vec<Rect> {
    let mut damage = Vec::new();
    let top = matches!(corner, Corner::TopLeft | Corner::TopRight);
    let left = matches!(corner, Corner::TopLeft | Corner::BottomLeft);
    // The rows still free: from `next` towards the far edge.
    let mut next = if top { area.y } else { area.bottom() };
    let mut first = true;
    for note in toasts.iter().rev() {
        let text = rich::Text::from_markup(&note.markup)
            .unwrap_or_else(|_| rich::Text::new(note.markup.clone()));
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
                    .min(most)
                    .min(area.width.saturating_sub(2));
                let mut options = console
                    .options()
                    .update_width(w.saturating_sub(4).max(1) as usize);
                options.overflow = Some(rich::Overflow::Fold);
                let rows = console.render_lines(&text, &options, false).len().max(1);
                (w, rows.min(u16::MAX as usize) as u16)
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
                damage.push(rect);
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
                    };
                    crate::border::draw_frame(screen, console, &look, rect);
                }
                let mut options = console.options().update_width(rect.width as usize - 4);
                options.overflow = Some(rich::Overflow::Fold);
                let inner = console.render_lines(&text, &options, false);
                let at = Rect::new(rect.x + 2, rect.y + 1, rect.width - 4, body);
                screen.write_lines(at, &inner[..inner.len().min(body as usize)]);
            }
        }
        damage.push(rect);
        if top {
            next += h;
        } else {
            next -= h;
        }
    }
    damage
}
