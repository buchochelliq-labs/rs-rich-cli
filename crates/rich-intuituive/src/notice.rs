//! A notice bar: [`notice_bar`], a message docked at the top or bottom of
//! a screen that folds to one row, opens to show all of its text, has
//! action buttons and can be dismissed. A browser's "this page is
//! translated" strip, an editor's "the file changed on disk", an update
//! that is ready.

use rich::{Segment, Style};
use rich_interact::{Button as MouseButton, KeyCode, MouseKind};

use crate::a11y::{AccessState, Role};
use crate::app::Ctx;
use crate::layout::Size;
use crate::node::{Axis, Node};
use crate::notify::Level;
use crate::reactive::{Signal, signal};
use crate::widget::{Canvas, DrawCx, EventCx, MeasureCx, Used, Widget, WidgetEvent, widget};

type Read = Box<dyn Fn() -> String>;
type Run = Box<dyn FnMut(&mut Ctx)>;

/// What can be picked on the bar's first row: the text (which opens and
/// folds it), an action, or the close mark.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Part {
    Text,
    Action(usize),
    Close,
}

/// A [`notice_bar`]'s widget.
struct NoticeBar {
    title: Read,
    text: Read,
    level: Level,
    shown: Signal<bool>,
    open: Signal<bool>,
    actions: Vec<(String, Run)>,
    /// An action closes the bar after it runs.
    closes: bool,
    picked: Part,
    /// Where each part was drawn on the first row: its first column and
    /// the column after it.
    hits: Vec<(Part, u16, u16)>,
}

/// A bar that shows `title` and `text` (console markup, both read again
/// whenever a signal they read changes) while `shown` holds: one row,
/// the text cut with `…` where it does not fit, until it is opened; then
/// the title on the first row and the whole text wrapped under it. Its
/// [actions](Node::notice_action) are buttons at the right of the first
/// row, and `×` at the end dismisses it (sets `shown` false).
///
/// With the focus, Enter or Space runs what is picked (the text opens and
/// folds the bar), ← and → pick, and Esc dismisses it; a click runs what
/// it is on. Its [level](Node::notice_level) puts a mark before the title
/// (`ℹ`, `✓`, `⚠`, `✗`) in the level's style, the theme's `notify.info`,
/// `notify.success`, `notify.warn` or `notify.error` as a notification's;
/// the bar is drawn on the theme's `notice` style (`reverse` by default),
/// the picked part in `notice.picked` (`bold underline`). Its role is
/// [`Status`](crate::a11y::Role::Status), named by the title and text, so
/// a screen reader is told when it changes. Put it first or last in a
/// column, or dock it with a stylesheet's `dock: top`.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::{notice_bar, Level};
///
/// let app = App::new(|| {
///     let shown = signal(true);
///     let reloads = signal(0);
///     column([
///         notice_bar(shown, || "Changed".into(), || "notes.txt changed on disk since it was opened".into())
///             .notice_level(Level::Warn)
///             .notice_action("Reload", move |_| reloads.update(|n| *n += 1)),
///         text!("{reloads}"),
///     ])
/// });
/// let mut driver = app.driver(40, 4);
/// driver.settle();
/// assert_eq!(driver.screen().plain()[0], "▸ ⚠ Changed notes.txt chang…  Reload  × ");
/// // The first thing that takes the focus: Enter opens it.
/// driver.press("enter");
/// // Open: the title, then the text wrapped.
/// assert_eq!(driver.screen().plain()[1].trim_end(), "  notes.txt changed on disk since it was");
/// assert!(driver.click_text("Reload"));
/// assert_eq!(driver.screen().plain()[3].trim_end(), "1");
/// assert!(driver.click_text("×"));
/// assert_eq!(driver.screen().plain()[0].trim_end(), "1");
/// ```
pub fn notice_bar(
    shown: Signal<bool>,
    title: impl Fn() -> String + 'static,
    text: impl Fn() -> String + 'static,
) -> Node {
    let mut node = widget(NoticeBar {
        title: Box::new(title),
        text: Box::new(text),
        level: Level::Info,
        shown,
        open: signal(false),
        actions: Vec::new(),
        closes: false,
        picked: Part::Text,
        hits: Vec::new(),
    })
    .what("notice_bar")
    .focusable()
    .shown_when(move || shown.get());
    node.size = Size::Auto;
    node
}

impl Node {
    /// For a [`notice_bar`]: how much it matters, the mark before its
    /// title and that mark's style. Ignored on other nodes.
    pub fn notice_level(self, level: Level) -> Node {
        self.widget_mut(|bar: &mut NoticeBar| bar.level = level);
        self
    }

    /// For a [`notice_bar`]: a button labelled `label` (plain text) that
    /// runs `action`, after those added before it. Ignored on other nodes.
    pub fn notice_action(self, label: &str, action: impl FnMut(&mut Ctx) + 'static) -> Node {
        let label = label.to_string();
        self.widget_mut(|bar: &mut NoticeBar| bar.actions.push((label, Box::new(action))));
        self
    }

    /// For a [`notice_bar`]: whether running an action dismisses the bar
    /// too (`false` by default). Ignored on other nodes.
    pub fn notice_closes(self, closes: bool) -> Node {
        self.widget_mut(|bar: &mut NoticeBar| bar.closes = closes);
        self
    }

    /// For a [`notice_bar`]: open it while `open` holds, and set `open` as
    /// it opens and folds, so the app can open it (or keep it open) and
    /// read whether it is. Ignored on other nodes.
    pub fn notice_open(self, open: Signal<bool>) -> Node {
        self.widget_mut(|bar: &mut NoticeBar| bar.open = open);
        self
    }
}

impl NoticeBar {
    /// What is on the right of the first row, as `(part, text)`.
    fn buttons(&self) -> Vec<(Part, String)> {
        let mut out: Vec<(Part, String)> = self
            .actions
            .iter()
            .enumerate()
            .map(|(i, (label, _))| (Part::Action(i), format!(" {label} ")))
            .collect();
        out.push((Part::Close, " × ".into()));
        out
    }

    /// The parts in the order ← and → move through them.
    fn parts(&self) -> Vec<Part> {
        let mut parts = vec![Part::Text];
        parts.extend((0..self.actions.len()).map(Part::Action));
        parts.push(Part::Close);
        parts
    }

    /// The whole text, wrapped at `width` cells.
    fn wrapped(&self, console: &rich::Console, width: u16) -> Vec<Vec<Segment>> {
        let markup = (self.text)();
        if markup.is_empty() {
            return Vec::new();
        }
        let text = rich::Text::from_markup(&markup).unwrap_or_else(|_| rich::Text::new(markup));
        let options = console.options().update_width(width.max(1) as usize);
        console.render_lines(&text, &options, false)
    }

    fn run(&mut self, cx: &mut Ctx, part: Part) {
        match part {
            Part::Text => self.open.update(|open| *open = !*open),
            Part::Action(i) => {
                if let Some((_, action)) = self.actions.get_mut(i) {
                    action(cx);
                }
                if self.closes {
                    self.shown.set(false);
                }
            }
            Part::Close => self.shown.set(false),
        }
    }
}

impl Widget for NoticeBar {
    fn name(&self) -> &'static str {
        "notice_bar"
    }

    fn role(&self) -> Role {
        Role::Status
    }

    fn access_text(&self) -> Option<String> {
        let plain = |markup: String| {
            rich::Text::from_markup(&markup)
                .map(|text| text.plain().to_string())
                .unwrap_or(markup)
        };
        let (title, text) = (plain((self.title)()), plain((self.text)()));
        Some(match (title.is_empty(), text.is_empty()) {
            (true, _) => format!("{}{text}", self.level.spoken()),
            (false, true) => format!("{}{title}", self.level.spoken()),
            (false, false) => format!("{}{title}: {text}", self.level.spoken()),
        })
    }

    fn access_state(&self) -> AccessState {
        AccessState {
            expanded: Some(self.open.get_untracked()),
            ..AccessState::default()
        }
    }

    fn measure(&mut self, cx: &MeasureCx, axis: Axis, width: u16, _height: u16) -> u16 {
        match axis {
            Axis::Horizontal => width,
            Axis::Vertical if self.open.get() => {
                let rows = self.wrapped(cx.console(), width.saturating_sub(2)).len();
                (1 + rows).min(u16::MAX as usize) as u16
            }
            Axis::Vertical => 1,
        }
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        let console = cx.console();
        let width = canvas.width();
        let open = self.open.get();
        let ground = cx.style("notice", "reverse");
        let picked = cx.style("notice.picked", "bold underline");
        let (own, fallback, definition) = self.level.names();
        let fallback = crate::widget::theme_style(console, fallback, definition);
        let mark_style = crate::widget::named_style(console, own).unwrap_or(fallback);
        canvas.fill(0, 0, width, canvas.height(), Some(&ground));
        // The right of the first row: the actions and the close mark.
        let buttons = self.buttons();
        let used: u16 = buttons
            .iter()
            .map(|(_, text)| rich::cells::cell_len(text) as u16)
            .sum::<u16>()
            .saturating_add(1);
        let mut hits = Vec::new();
        let mut x = width.saturating_sub(used).saturating_add(1);
        for (part, text) in &buttons {
            let w = rich::cells::cell_len(text) as u16;
            let mut style = ground.clone();
            if *part == self.picked && cx.focused() {
                style = style.combine(&picked);
            }
            canvas.print(x, 0, text, Some(&style));
            hits.push((*part, x, x + w));
            x += w;
        }
        // The left: the marker, the level's mark, the title, and folded,
        // the text cut to the room left.
        let room = width.saturating_sub(used);
        let marker = if open { "▾ " } else { "▸ " };
        canvas.print(0, 0, marker, Some(&ground));
        let mark = format!("{} ", self.level.mark());
        canvas.print(2, 0, &mark, Some(&ground.combine(&mark_style)));
        let title = (self.title)();
        let at = 2 + rich::cells::cell_len(&mark) as u16;
        let title_style = ground.combine(&Style::parse("bold").unwrap_or_default());
        let taken = canvas.markup(
            console,
            at,
            0,
            room.saturating_sub(at),
            &title,
            Some(&title_style),
        );
        let mut end = at + taken;
        if !open {
            let start = if taken > 0 { end + 1 } else { end };
            let left = room.saturating_sub(start);
            if left > 0 {
                end = start + canvas.markup(console, start, 0, left, &(self.text)(), Some(&ground));
            }
        } else {
            let lines = self.wrapped(console, width.saturating_sub(2));
            let lines: Vec<Vec<Segment>> = lines
                .into_iter()
                .map(|line| {
                    line.into_iter()
                        .map(|s| {
                            let style = match &s.style {
                                Some(own) => ground.combine(own),
                                None => ground.clone(),
                            };
                            Segment::new(s.text, Some(style))
                        })
                        .collect()
                })
                .collect();
            canvas.lines_at(
                2,
                1,
                width.saturating_sub(2),
                canvas.height().saturating_sub(1),
                &lines,
            );
        }
        if self.picked == Part::Text && cx.focused() {
            canvas.restyle(0, 0, end.max(2), 1, &picked);
        }
        hits.insert(0, (Part::Text, 0, end.max(2)));
        self.hits = hits;
    }

    fn event(&mut self, cx: &mut EventCx, event: &WidgetEvent) -> Used {
        match event {
            WidgetEvent::Mouse(mouse)
                if matches!(
                    mouse.kind,
                    MouseKind::Down(MouseButton::Left) | MouseKind::Up(MouseButton::Left)
                ) =>
            {
                // Under the first row, the text opens and folds it too.
                let part = if mouse.row > 0 {
                    Some(Part::Text)
                } else {
                    self.hits
                        .iter()
                        .find(|(_, start, end)| (*start..*end).contains(&mouse.column))
                        .map(|(part, _, _)| *part)
                };
                let Some(part) = part else {
                    return Used::No;
                };
                if mouse.kind == MouseKind::Up(MouseButton::Left) {
                    self.picked = part;
                    self.run(cx.app(), part);
                    cx.redraw();
                }
                Used::Yes
            }
            WidgetEvent::Key(key) if !key.modifiers.ctrl && !key.modifiers.alt => {
                let parts = self.parts();
                let at = parts.iter().position(|p| *p == self.picked).unwrap_or(0);
                match key.code {
                    KeyCode::Enter | KeyCode::Char(' ') => {
                        let part = self.picked;
                        self.run(cx.app(), part);
                    }
                    KeyCode::Escape => self.shown.set(false),
                    KeyCode::Right if at + 1 < parts.len() => self.picked = parts[at + 1],
                    KeyCode::Left if at > 0 => self.picked = parts[at - 1],
                    _ => return Used::No,
                }
                cx.redraw();
                Used::Yes
            }
            _ => Used::No,
        }
    }

    fn focusable(&self) -> bool {
        true
    }
}
