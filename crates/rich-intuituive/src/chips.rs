//! Chips: a short label on a coloured ground, rs-rich-ext's
//! [`Badge`](rich_ext::badge::Badge) as a node ([`chip`], [`chip_of`]),
//! that may be removed with a `×`; and a [`tag_input`] that types tags
//! into a row of them.

use rich::{Console, Segment};
use rich_ext::badge::Badge;
use rich_interact::{Button as MouseButton, KeyCode, MouseKind};

use crate::a11y::Role;
use crate::app::Ctx;
use crate::layout::Size;
use crate::node::{Axis, Node};
use crate::reactive::Signal;
use crate::widget::{Canvas, DrawCx, EventCx, MeasureCx, Used, Widget, WidgetEvent, widget};

/// A badge's cells, with a `×` before its closing pad when it can be
/// removed: the cells, and the column of the `×`.
fn chip_cells(console: &Console, badge: &Badge, removable: bool) -> (Vec<Segment>, Option<u16>) {
    let text = badge.to_text(console);
    let options = console.options().update_width(text.cell_len().max(1));
    let mut cells: Vec<Segment> = console
        .render_lines(&text, &options, false)
        .into_iter()
        .next()
        .unwrap_or_default();
    if !removable {
        return (cells, None);
    }
    let style = cells.last().and_then(|s| s.style.clone());
    let used: usize = cells.iter().map(Segment::cell_length).sum();
    // On a coloured chip the `×` goes inside the closing pad; on a plain
    // one (`[beta]`), after it.
    let inside = cells.last().is_some_and(|s| s.text.ends_with(' '));
    let at = if inside { used } else { used + 1 };
    if inside {
        cells.push(Segment::new("× ", style));
    } else {
        cells.push(Segment::new(" ×", style));
    }
    (cells, Some(at.min(u16::MAX as usize) as u16))
}

type Remove = Box<dyn FnMut(&mut Ctx)>;

/// A [`chip`]'s widget.
struct Chip {
    badge: Box<dyn Fn() -> Badge>,
    remove: Option<Remove>,
    /// Where the `×` was last drawn.
    close: Option<u16>,
}

/// A chip: `text` on the theme's `badge.label` style (black on white),
/// padded (` beta `), or `[beta]` where the console shows no colour, as
/// rs-rich-ext's [`Badge::label`] draws it. [`chip_of`] shows any badge
/// (a status, a key and value); [`on_remove`](Node::on_remove) adds a `×`
/// that removes it.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::chip;
///
/// let app = App::new(|| row([chip("beta"), chip("linux")]));
/// let mut driver = app.driver(16, 1);
/// driver.settle();
/// assert_eq!(driver.screen().plain()[0].trim_end(), " beta  linux");
/// ```
pub fn chip(text: impl Into<String>) -> Node {
    let text = text.into();
    chip_of(move || Badge::label(text.clone()))
}

/// A chip showing `badge`, read again whenever a signal it reads changes:
/// a status (`Badge::status(Status::Ok, "build")`), a key and value
/// (`Badge::meta("version", "1.2")`), a label in a style of its own
/// (`Badge::label("new").style("badge.info")`).
pub fn chip_of(badge: impl Fn() -> Badge + 'static) -> Node {
    let mut node = widget(Chip {
        badge: Box::new(badge),
        remove: None,
        close: None,
    })
    .what("chip");
    node.size = Size::Auto;
    node
}

impl Node {
    /// For a [`chip`]: draw a `×` in it that runs `remove` when it is
    /// clicked; the chip takes the focus, and Delete or Backspace runs it
    /// too. Ignored on other nodes.
    ///
    /// ```
    /// use intuituive::prelude::*;
    /// use intuituive::chip;
    ///
    /// let app = App::new(|| {
    ///     let shown = signal(true);
    ///     column([chip("beta").on_remove(move |_| shown.set(false)).shown_when(move || shown.get())])
    /// });
    /// let mut driver = app.driver(10, 1);
    /// driver.settle();
    /// assert_eq!(driver.screen().plain()[0].trim_end(), " beta ×");
    /// assert!(driver.click_text("×"));
    /// assert_eq!(driver.screen().plain()[0].trim_end(), "");
    /// ```
    pub fn on_remove(self, remove: impl FnMut(&mut Ctx) + 'static) -> Node {
        let mut set = false;
        self.widget_mut(|chip: &mut Chip| {
            chip.remove = Some(Box::new(remove));
            set = true;
        });
        if set { self.focusable() } else { self }
    }
}

impl Widget for Chip {
    fn name(&self) -> &'static str {
        "chip"
    }

    fn access_text(&self) -> Option<String> {
        Some((self.badge)().plain())
    }

    fn role(&self) -> Role {
        if self.remove.is_some() {
            Role::Button
        } else {
            Role::Text
        }
    }

    fn measure(&mut self, cx: &MeasureCx, axis: Axis, width: u16, _height: u16) -> u16 {
        match axis {
            Axis::Vertical => 1,
            Axis::Horizontal => {
                let (cells, _) = chip_cells(cx.console(), &(self.badge)(), self.remove.is_some());
                let wanted = cells.iter().map(Segment::cell_length).sum::<usize>();
                let wanted = wanted.min(u16::MAX as usize) as u16;
                if width == 0 {
                    wanted
                } else {
                    wanted.min(width)
                }
            }
        }
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        let (cells, close) = chip_cells(cx.console(), &(self.badge)(), self.remove.is_some());
        canvas.lines(&[cells]);
        if let (true, Some(x)) = (cx.focused(), close) {
            let style = cx.style("chip.focus", "reverse");
            canvas.restyle(x, 0, 1, 1, &style);
        }
        self.close = close;
    }

    fn event(&mut self, cx: &mut EventCx, event: &WidgetEvent) -> Used {
        let Some(remove) = &mut self.remove else {
            return Used::No;
        };
        match event {
            WidgetEvent::Mouse(mouse)
                if Some(mouse.column) == self.close
                    && matches!(
                        mouse.kind,
                        MouseKind::Down(MouseButton::Left) | MouseKind::Up(MouseButton::Left)
                    ) =>
            {
                if mouse.kind == MouseKind::Up(MouseButton::Left) {
                    remove(cx.app());
                }
                Used::Yes
            }
            WidgetEvent::Key(key)
                if matches!(key.code, KeyCode::Delete | KeyCode::Backspace)
                    && !key.modifiers.ctrl
                    && !key.modifiers.alt =>
            {
                remove(cx.app());
                Used::Yes
            }
            _ => Used::No,
        }
    }
}

/// A [`tag_input`]'s widget.
struct TagInput {
    tags: Signal<Vec<String>>,
    draft: String,
    /// Where each tag's `×` was last drawn, by index.
    closes: Vec<(usize, u16)>,
}

/// A box that types tags into `tags`: each tag a removable [`chip`], then
/// what is being typed. Enter, Tab or a comma adds what was typed (trimmed,
/// once: a tag already there is not added again); Backspace with nothing
/// typed removes the last tag, and a click on a chip's `×` removes it.
/// The chips are in the theme's `badge.label` style, what is typed after
/// them with a block caret while it has the focus. One row, cut at its
/// width. Its role is a text box, its value the tags.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::tag_input;
///
/// let app = App::new(|| {
///     let tags = signal(vec!["rust".to_string()]);
///     column([tag_input(tags), text(move || tags.get().join("+"))])
/// });
/// let mut driver = app.driver(30, 2);
/// driver.type_text("tui,");
/// driver.type_text("cli");
/// driver.press("enter");
/// assert_eq!(driver.screen().plain()[1].trim_end(), "rust+tui+cli");
/// assert_eq!(driver.screen().plain()[0].trim_end(), " rust ×   tui ×   cli ×");
/// driver.press("backspace");
/// assert_eq!(driver.screen().plain()[1].trim_end(), "rust+tui");
/// ```
pub fn tag_input(tags: Signal<Vec<String>>) -> Node {
    widget(TagInput {
        tags,
        draft: String::new(),
        closes: Vec::new(),
    })
    .what("tag_input")
    .focusable()
    .fixed(1)
}

impl TagInput {
    /// Add what was typed as a tag.
    fn commit(&mut self) {
        let tag = self.draft.trim().to_string();
        self.draft.clear();
        if tag.is_empty() {
            return;
        }
        self.tags.update(|tags| {
            if !tags.contains(&tag) {
                tags.push(tag);
            }
        });
    }
}

impl Widget for TagInput {
    fn name(&self) -> &'static str {
        "tag_input"
    }

    fn role(&self) -> Role {
        Role::TextBox
    }

    fn access_value(&self) -> Option<String> {
        let mut value = self.tags.get_untracked().join(", ");
        if !self.draft.is_empty() {
            if !value.is_empty() {
                value.push_str(", ");
            }
            value.push_str(&self.draft);
        }
        Some(value)
    }

    fn measure(&mut self, _cx: &MeasureCx, axis: Axis, width: u16, _height: u16) -> u16 {
        match axis {
            Axis::Vertical => 1,
            Axis::Horizontal => width,
        }
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        let console = cx.console();
        let mut line = Vec::new();
        let mut closes = Vec::new();
        for (i, tag) in self.tags.get().iter().enumerate() {
            if i > 0 {
                line.push(Segment::new(" ", None));
            }
            let at: usize = line.iter().map(Segment::cell_length).sum();
            let (cells, close) = chip_cells(console, &Badge::label(tag.clone()), true);
            if let Some(close) = close {
                closes.push((i, (at + close as usize).min(u16::MAX as usize) as u16));
            }
            line.extend(cells);
        }
        if !line.is_empty() {
            line.push(Segment::new(" ", None));
        }
        line.push(Segment::new(self.draft.clone(), None));
        let caret: usize = line.iter().map(Segment::cell_length).sum();
        canvas.lines(&[line]);
        if cx.focused() {
            let style = cx.style("tag_input.caret", "reverse");
            canvas.restyle(caret.min(u16::MAX as usize) as u16, 0, 1, 1, &style);
        }
        self.closes = closes;
    }

    fn event(&mut self, cx: &mut EventCx, event: &WidgetEvent) -> Used {
        match event {
            WidgetEvent::Mouse(mouse)
                if matches!(
                    mouse.kind,
                    MouseKind::Down(MouseButton::Left) | MouseKind::Up(MouseButton::Left)
                ) =>
            {
                let Some(&(i, _)) = self.closes.iter().find(|(_, x)| *x == mouse.column) else {
                    return Used::No;
                };
                if mouse.kind == MouseKind::Up(MouseButton::Left) {
                    self.tags.update(|tags| {
                        if i < tags.len() {
                            tags.remove(i);
                        }
                    });
                }
                Used::Yes
            }
            WidgetEvent::Key(key) if !key.modifiers.ctrl && !key.modifiers.alt => {
                match key.code {
                    KeyCode::Enter | KeyCode::Char(',') => self.commit(),
                    KeyCode::Tab if !self.draft.trim().is_empty() => self.commit(),
                    KeyCode::Backspace if self.draft.is_empty() => {
                        if self.tags.get_untracked().is_empty() {
                            return Used::No;
                        }
                        self.tags.update(|tags| {
                            tags.pop();
                        });
                    }
                    KeyCode::Backspace => {
                        self.draft.pop();
                    }
                    KeyCode::Char(c) => self.draft.push(c),
                    _ => return Used::No,
                }
                cx.redraw();
                Used::Yes
            }
            WidgetEvent::Paste(text) => {
                for c in text.chars() {
                    if c == ',' || c == '\n' {
                        self.commit();
                    } else {
                        self.draft.push(c);
                    }
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
