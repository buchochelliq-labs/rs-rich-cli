//! Small widgets after Textual's: [`link`], [`progress_bar`], [`header`],
//! [`key_panel`], [`masked_input`] and [`placeholder`].

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use rich::Style;
use rich_interact::{Button, KeyCode, MouseKind};

use crate::app::Ctx;
use crate::layout::Size;
use crate::node::{Axis, Node};
use crate::reactive::{Runtime, Signal, signal};
use crate::widget::{Canvas, DrawCx, EventCx, MeasureCx, Used, Widget, WidgetEvent, widget};

/// Ask for a draw `after` from now, once: `tick` changes then. For a
/// widget that moves with the app's clock (a pulse, a clock).
fn wake(tick: Signal<u64>, pending: &Rc<Cell<bool>>, after: Duration) {
    if pending.replace(true) {
        return;
    }
    let pending = pending.clone();
    crate::app::after(after, move |_| {
        pending.set(false);
        tick.update(|t| *t = t.wrapping_add(1));
    });
}

// Link.

type Opener = Rc<dyn Fn(&str)>;
type Action = Box<dyn FnMut(&mut Ctx)>;

pub(crate) struct Link {
    text: String,
    url: String,
    opener: Opener,
    action: Option<Action>,
}

impl Link {
    fn activate(&mut self, cx: &mut Ctx) {
        match &mut self.action {
            Some(action) => action(cx),
            None => (self.opener)(&self.url),
        }
    }
}

/// Text that can take the focus and opens `url` (through the system's
/// opener, rs-rich-ext's `open::system_opener`) on Enter or a click, as
/// Textual's `Link` does. It is drawn in the theme's `link` style
/// (underlined by default) and is an OSC 8 hyperlink where the terminal
/// has them. [`on_activate`](Node::on_activate) runs an action instead;
/// [`open_with`](Node::open_with) opens the URL another way (a test's
/// stub).
///
/// ```
/// use intuituive::prelude::*;
/// use std::{cell::RefCell, rc::Rc};
///
/// let opened = Rc::new(RefCell::new(Vec::new()));
/// let seen = opened.clone();
/// let app = App::new(move || {
///     let opened = opened.clone();
///     intuituive::link("Docs", "https://example.com")
///         .open_with(move |url| opened.borrow_mut().push(url.to_string()))
/// });
/// let mut driver = app.driver(10, 1);
/// driver.press("enter");
/// assert_eq!(*seen.borrow(), vec!["https://example.com".to_string()]);
/// ```
pub fn link(text: impl Into<String>, url: impl Into<String>) -> Node {
    widget(Link {
        text: text.into(),
        url: url.into(),
        opener: Rc::new(|url: &str| {
            let _ = rich_ext::open::system_opener().arg(url).spawn();
        }),
        action: None,
    })
    .what("link")
    .focus_style("reverse")
}

impl Widget for Link {
    fn name(&self) -> &'static str {
        "link"
    }

    fn describe(&self) -> Option<String> {
        Some(self.url.clone())
    }

    fn access_text(&self) -> Option<String> {
        Some(self.text.clone())
    }

    fn role(&self) -> crate::a11y::Role {
        crate::a11y::Role::Button
    }

    fn measure(&mut self, _cx: &MeasureCx, axis: Axis, _width: u16, _height: u16) -> u16 {
        match axis {
            Axis::Vertical => 1,
            Axis::Horizontal => rich::cells::cell_len(&self.text).min(u16::MAX as usize) as u16,
        }
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        let style = cx
            .style("link", "underline")
            .combine(&Style::parse(&format!("link {}", self.url)).unwrap_or_default());
        canvas.print(0, 0, &self.text, Some(&style));
    }

    fn event(&mut self, cx: &mut EventCx, event: &WidgetEvent) -> Used {
        match event {
            WidgetEvent::Key(key)
                if key.code == KeyCode::Enter && !key.modifiers.ctrl && !key.modifiers.alt =>
            {
                self.activate(cx.app());
                Used::Yes
            }
            WidgetEvent::Mouse(mouse) if mouse.kind == MouseKind::Up(Button::Left) => {
                self.activate(cx.app());
                Used::Yes
            }
            WidgetEvent::Mouse(mouse) if mouse.kind == MouseKind::Down(Button::Left) => Used::Yes,
            _ => Used::No,
        }
    }

    fn focusable(&self) -> bool {
        true
    }
}

// Progress bar.

pub(crate) struct ProgressBar {
    done: Signal<f64>,
    total: Signal<Option<f64>>,
    eta: bool,
    percentage: bool,
    /// When it first drew, on the app's clock: what the ETA counts from.
    started: Option<Duration>,
    tick: Signal<u64>,
    pending: Rc<Cell<bool>>,
}

/// A bar of `done` out of `total`, as Textual's `ProgressBar`: with
/// [`show_percentage`](Node::show_percentage) its share (`50%`), with
/// [`show_eta`](Node::show_eta) the time left at the rate so far (counted
/// from when it first drew, on the app's clock). With no total it pulses,
/// a block sweeping across on the app's clock. The bar is in the theme's
/// `progress.bar` style (`magenta` by default) over `progress.track`
/// (`grey23`). One row tall.
///
/// ```
/// use intuituive::prelude::*;
///
/// let app = App::new(|| {
///     let (done, total) = (signal(3.0), signal(Some(4.0)));
///     intuituive::progress_bar(done, total).show_percentage(true)
/// });
/// let mut driver = app.driver(20, 1);
/// driver.settle();
/// assert!(driver.screen().plain()[0].trim_end().ends_with("75%"));
/// ```
pub fn progress_bar(done: Signal<f64>, total: Signal<Option<f64>>) -> Node {
    let mut node = widget(ProgressBar {
        done,
        total,
        eta: false,
        percentage: false,
        started: None,
        tick: signal(0),
        pending: Rc::new(Cell::new(false)),
    })
    .what("progress_bar");
    node.size = Size::Fixed(1);
    node
}

/// `seconds` as `h:mm:ss`, as rich's time-remaining column says it.
fn clock_text(seconds: u64) -> String {
    format!(
        "{}:{:02}:{:02}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    )
}

impl Widget for ProgressBar {
    fn name(&self) -> &'static str {
        "progress_bar"
    }

    fn role(&self) -> crate::a11y::Role {
        crate::a11y::Role::Status
    }

    fn access_text(&self) -> Option<String> {
        let done = self.done.get_untracked();
        Some(match self.total.get_untracked() {
            Some(total) if total > 0.0 => {
                format!("{:.0}%", (done / total * 100.0).clamp(0.0, 100.0))
            }
            _ => "in progress".into(),
        })
    }

    fn measure(&mut self, _cx: &MeasureCx, axis: Axis, width: u16, _height: u16) -> u16 {
        match axis {
            Axis::Vertical => 1,
            Axis::Horizontal => width.max(20),
        }
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        let (done, total) = (self.done.get(), self.total.get());
        let now = cx.now();
        let started = *self.started.get_or_insert(now);
        let width = canvas.width();
        let mut tail = Vec::new();
        let share = total
            .filter(|t| *t > 0.0)
            .map(|t| (done / t).clamp(0.0, 1.0));
        if let Some(share) = share {
            if self.percentage {
                tail.push(format!("{:>3.0}%", share * 100.0));
            }
            if self.eta {
                let elapsed = now.saturating_sub(started).as_secs_f64();
                let left = match share {
                    s if s >= 1.0 => Some(0.0),
                    s if s > 0.0 && elapsed > 0.0 => Some(elapsed / s * (1.0 - s)),
                    _ => None,
                };
                tail.push(match left {
                    Some(left) => format!("ETA {}", clock_text(left.round() as u64)),
                    None => "ETA -:--:--".into(),
                });
            }
        }
        let tail = tail.join(" ");
        let tail_width = rich::cells::cell_len(&tail) as u16;
        let bar = width.saturating_sub(if tail.is_empty() { 0 } else { tail_width + 1 });
        let (on, track) = (
            cx.style("progress.bar", "magenta"),
            cx.style("progress.track", "grey23"),
        );
        match share {
            Some(share) => {
                let filled = (share * bar as f64).round() as u16;
                canvas.print(0, 0, &"━".repeat(filled as usize), Some(&on));
                canvas.print(
                    filled,
                    0,
                    &"━".repeat(bar.saturating_sub(filled) as usize),
                    Some(&track),
                );
            }
            None => {
                // A block a quarter of the bar sweeping across, a cell
                // each 50 ms, reading the tick so it draws again.
                self.tick.get();
                let block = (bar / 4).max(1);
                let span = bar + block;
                let at = (now.as_millis() / 50) as u16 % span.max(1);
                canvas.print(0, 0, &"━".repeat(bar as usize), Some(&track));
                let from = at.saturating_sub(block);
                let to = at.min(bar);
                if to > from {
                    canvas.print(from, 0, &"━".repeat((to - from) as usize), Some(&on));
                }
                if crate::a11y::motion_allowed() && !crate::a11y::text_mode() {
                    wake(self.tick, &self.pending, Duration::from_millis(50));
                }
            }
        }
        if !tail.is_empty() {
            canvas.print(width.saturating_sub(tail_width), 0, &tail, None);
        }
    }
}

// Header.

pub(crate) struct Header {
    heading: Signal<(String, String)>,
    clock: bool,
    tall: Signal<bool>,
    /// The wall clock's seconds when it first drew, and the app's clock
    /// then.
    wall: Option<(u64, Duration)>,
    tick: Signal<u64>,
    pending: Rc<Cell<bool>>,
}

/// The app's [title](crate::App::title) and the top screen's
/// [subtitle](Ctx::set_subtitle) centred on a row, as Textual's `Header`:
/// `Title — subtitle`, in the theme's `header` style (`reverse` by
/// default). [`clock`](Node::clock) adds the time (UTC, `HH:MM:SS`,
/// moving with the app's clock) on the right; [`tall`](Node::tall) makes
/// it three rows tall. A click toggles tall, as Textual's does.
///
/// ```
/// use intuituive::prelude::*;
///
/// let app = App::new(|| {
///     column([intuituive::header(), label("body")])
///         .on_key("s", |cx| cx.set_subtitle("settings"))
/// })
/// .title("Demo");
/// let mut driver = app.driver(30, 2);
/// driver.press("s");
/// assert_eq!(driver.screen().plain()[0].trim(), "Demo — settings");
/// ```
pub fn header() -> Node {
    let runtime = Runtime::current_for("header()");
    let heading = match runtime.heading.get() {
        Some(heading) => heading,
        None => {
            let heading = signal((String::new(), String::new())).untraced();
            runtime.heading.set(Some(heading));
            heading
        }
    };
    let tall = signal(false);
    widget(Header {
        heading,
        clock: false,
        tall,
        wall: None,
        tick: signal(0),
        pending: Rc::new(Cell::new(false)),
    })
    .what("header")
    .size_with(move || Size::Fixed(if tall.get() { 3 } else { 1 }))
}

impl Widget for Header {
    fn name(&self) -> &'static str {
        "header"
    }

    fn role(&self) -> crate::a11y::Role {
        crate::a11y::Role::Region
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        let (title, subtitle) = self.heading.get();
        let (width, height) = (canvas.width(), canvas.height());
        let style = cx.style("header", "reverse");
        canvas.fill(0, 0, width, height, Some(&style));
        let text = match (title.is_empty(), subtitle.is_empty()) {
            (_, true) => title,
            (true, false) => subtitle,
            (false, false) => format!("{title} — {subtitle}"),
        };
        let row = height / 2;
        let cells = rich::cells::cell_len(&text) as u16;
        canvas.print(width.saturating_sub(cells) / 2, row, &text, Some(&style));
        if self.clock {
            self.tick.get();
            let now = cx.now();
            let (wall, at) = *self
                .wall
                .get_or_insert_with(|| (crate::clock::unix_seconds(), now));
            let seconds = wall + now.saturating_sub(at).as_secs();
            let shown = format!(
                "{:02}:{:02}:{:02}",
                seconds / 3600 % 24,
                seconds / 60 % 60,
                seconds % 60
            );
            canvas.print(width.saturating_sub(9), row, &shown, Some(&style));
            // The next second.
            let into = now.saturating_sub(at).as_millis() % 1000;
            wake(
                self.tick,
                &self.pending,
                Duration::from_millis((1000 - into) as u64),
            );
        }
    }

    fn event(&mut self, _cx: &mut EventCx, event: &WidgetEvent) -> Used {
        match event {
            WidgetEvent::Mouse(mouse) if mouse.kind == MouseKind::Down(Button::Left) => {
                self.tall.update(|tall| *tall = !*tall);
                Used::Yes
            }
            _ => Used::No,
        }
    }
}

// Key panel.

pub(crate) struct KeyPanel {
    hints: Signal<crate::reactive::Hints>,
}

/// A panel of the keys that work now, one a row (`key  description`),
/// following the focus as [`key_hints`](crate::widgets::key_hints) does:
/// each described binding of the focused node and its ancestors, innermost
/// first, those whose condition does not hold greyed. Textual's
/// `KeyPanel`, for the right of a layout; 30 cells wide unless sized.
/// [`Ctx::toggle_key_panel`] hides and shows it.
///
/// ```
/// use intuituive::prelude::*;
///
/// let app = App::new(|| {
///     row([
///         label("files").bind("d", "Delete", |_| {}).focusable(),
///         intuituive::key_panel(),
///     ])
/// });
/// let mut driver = app.driver(50, 4);
/// driver.settle();
/// assert!(driver.screen().plain().join("\n").contains("d  Delete"));
/// ```
pub fn key_panel() -> Node {
    let runtime = Runtime::current_for("key_panel()");
    let hints = match runtime.hints.get() {
        Some(hints) => hints,
        None => {
            let hints = signal(Vec::new()).untraced();
            runtime.hints.set(Some(hints));
            hints
        }
    };
    let shown = match runtime.key_panel.get() {
        Some(shown) => shown,
        None => {
            let shown = signal(true).untraced();
            runtime.key_panel.set(Some(shown));
            shown
        }
    };
    widget(KeyPanel { hints })
        .what("key_panel")
        .panel("Keys")
        .fixed(30)
        .shown_when(move || shown.get())
}

impl Widget for KeyPanel {
    fn name(&self) -> &'static str {
        "key_panel"
    }

    fn measure(&mut self, _cx: &MeasureCx, axis: Axis, _width: u16, _height: u16) -> u16 {
        self.hints.with(|hints| match axis {
            Axis::Vertical => hints.len().min(u16::MAX as usize) as u16,
            Axis::Horizontal => hints
                .iter()
                .map(|hint| {
                    crate::help::KeyFormat::Default
                        .keys(&hint.keys, false)
                        .len()
                        + 2
                        + hint.description.len()
                })
                .max()
                .unwrap_or(0)
                .min(u16::MAX as usize) as u16,
        })
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        let (key, dim) = (cx.style("key_hints.key", "bold"), cx.style("dim", "dim"));
        self.hints.with(|hints| {
            let keys: Vec<String> = hints
                .iter()
                .map(|hint| crate::help::KeyFormat::Default.keys(&hint.keys, false))
                .collect();
            let column = keys
                .iter()
                .map(|k| rich::cells::cell_len(k))
                .max()
                .unwrap_or(0) as u16;
            for (row, (hint, keys)) in hints.iter().zip(&keys).enumerate() {
                let row = row as u16;
                if row >= canvas.height() {
                    break;
                }
                let (key_style, text_style) = if hint.disabled {
                    (dim.clone(), Some(dim.clone()))
                } else {
                    (key.clone(), None)
                };
                canvas.print(0, row, keys, Some(&key_style));
                canvas.print(column + 2, row, &hint.description, text_style.as_ref());
            }
        });
    }
}

// Masked input.

/// What a template's character asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Slot {
    /// A character the user types: whether it is required, and what fits.
    Typed(bool, Fits),
    /// A separator, fixed.
    Fixed(char),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Fits {
    Digit,
    Letter,
    Alphanumeric,
    Any,
}

impl Fits {
    fn takes(self, c: char) -> bool {
        match self {
            Fits::Digit => c.is_ascii_digit(),
            Fits::Letter => c.is_alphabetic(),
            Fits::Alphanumeric => c.is_alphanumeric(),
            Fits::Any => !c.is_control(),
        }
    }
}

/// A template's slots, its case (`>` upper, `<` lower) for each, and the
/// blank shown in an empty slot (after `;`).
fn parse_template(template: &str) -> (Vec<(Slot, Option<bool>)>, char) {
    let (body, blank) = match template.rsplit_once(';') {
        Some((body, blank)) if blank.chars().count() == 1 => {
            (body, blank.chars().next().expect("one char"))
        }
        _ => (template, ' '),
    };
    let mut slots = Vec::new();
    let mut case = None;
    let mut chars = body.chars();
    while let Some(c) = chars.next() {
        let slot = match c {
            '>' => {
                case = Some(true);
                continue;
            }
            '<' => {
                case = Some(false);
                continue;
            }
            '!' => {
                case = None;
                continue;
            }
            '\\' => Slot::Fixed(chars.next().unwrap_or('\\')),
            '9' => Slot::Typed(true, Fits::Digit),
            '0' => Slot::Typed(false, Fits::Digit),
            'A' => Slot::Typed(true, Fits::Letter),
            'a' => Slot::Typed(false, Fits::Letter),
            'N' => Slot::Typed(true, Fits::Alphanumeric),
            'n' => Slot::Typed(false, Fits::Alphanumeric),
            'X' => Slot::Typed(true, Fits::Any),
            'x' => Slot::Typed(false, Fits::Any),
            other => Slot::Fixed(other),
        };
        slots.push((slot, case));
    }
    (slots, blank)
}

pub(crate) struct MaskedInput {
    value: Signal<String>,
    slots: Vec<(Slot, Option<bool>)>,
    blank: char,
    caret: usize,
}

impl MaskedInput {
    /// What each slot holds now, read from the value.
    fn cells(&self) -> Vec<Option<char>> {
        let value: Vec<char> = self.value.get_untracked().chars().collect();
        self.slots
            .iter()
            .enumerate()
            .map(|(i, (slot, _))| match slot {
                Slot::Fixed(c) => Some(*c),
                Slot::Typed(..) => value.get(i).copied().filter(|c| *c != self.blank),
            })
            .collect()
    }

    /// The value from the cells: up to the last typed one, an empty slot
    /// before it a blank.
    fn store(&self, cells: &[Option<char>]) {
        let last = self
            .slots
            .iter()
            .zip(cells)
            .rposition(|((slot, _), cell)| matches!(slot, Slot::Typed(..)) && cell.is_some());
        let value: String = match last {
            None => String::new(),
            Some(last) => cells[..=last]
                .iter()
                .map(|cell| cell.unwrap_or(self.blank))
                .collect(),
        };
        if self.value.get_untracked() != value {
            self.value.set(value);
        }
    }

    fn typed_at(&self, i: usize) -> bool {
        matches!(self.slots.get(i), Some((Slot::Typed(..), _)))
    }

    /// The first typed slot at or after `i`.
    fn next_typed(&self, mut i: usize) -> usize {
        while i < self.slots.len() && !self.typed_at(i) {
            i += 1;
        }
        i
    }

    /// The typed slot before `i`.
    fn previous_typed(&self, i: usize) -> Option<usize> {
        (0..i).rev().find(|&j| self.typed_at(j))
    }
}

/// A text box that keeps to `template`, as Textual's `MaskedInput`: `9` a
/// digit, `0` an optional digit, `A`/`a` a letter (required, optional),
/// `N`/`n` a letter or digit, `X`/`x` any character, `>` and `<` upper
/// or lower case for what follows (`!` neither), `\` a template
/// character as it is, anything else a fixed separator, and `;c` at the
/// end the blank shown in an empty slot (a space by default). The caret
/// skips the separators, typing fills the next slot that takes the
/// character, and Backspace clears the slot before it. `value` holds the
/// text as shown, separators included, up to the last slot typed. The node
/// is in error (`:invalid`) while some but not all required slots are
/// filled.
///
/// ```
/// use intuituive::prelude::*;
///
/// let app = App::new(|| intuituive::masked_input(signal(String::new()), "9999-99-99"));
/// let mut driver = app.driver(12, 1);
/// driver.type_text("20261027");
/// assert_eq!(driver.screen().plain()[0].trim_end(), "2026-10-27");
/// ```
pub fn masked_input(value: Signal<String>, template: &str) -> Node {
    let (slots, blank) = parse_template(template);
    let required: Rc<Vec<usize>> = Rc::new(
        slots
            .iter()
            .enumerate()
            .filter(|(_, (slot, _))| matches!(slot, Slot::Typed(true, _)))
            .map(|(i, _)| i)
            .collect(),
    );
    let input = MaskedInput {
        value,
        slots,
        blank,
        caret: 0,
    };
    let caret = input.next_typed(0);
    widget(MaskedInput { caret, ..input })
        .what("masked_input")
        .error_when(move || {
            let text: Vec<char> = value.get().chars().collect();
            let filled = |i: &usize| text.get(*i).is_some_and(|c| *c != blank);
            !text.is_empty() && !required.iter().all(filled)
        })
}

impl Widget for MaskedInput {
    fn name(&self) -> &'static str {
        "masked_input"
    }

    fn role(&self) -> crate::a11y::Role {
        crate::a11y::Role::TextBox
    }

    fn access_value(&self) -> Option<String> {
        Some(self.value.get_untracked())
    }

    fn measure(&mut self, _cx: &MeasureCx, axis: Axis, _width: u16, _height: u16) -> u16 {
        match axis {
            Axis::Vertical => 1,
            Axis::Horizontal => (self.slots.len() + 1).min(u16::MAX as usize) as u16,
        }
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        self.value.get();
        let dim = cx.style("dim", "dim");
        for (i, cell) in self.cells().into_iter().enumerate() {
            let typed = self.typed_at(i);
            let (c, style) = match cell {
                Some(c) if typed => (c, None),
                Some(c) => (c, Some(&dim)),
                None => (self.blank, Some(&dim)),
            };
            canvas.print(i as u16, 0, &c.to_string(), style);
        }
    }

    fn event(&mut self, cx: &mut EventCx, event: &WidgetEvent) -> Used {
        let WidgetEvent::Key(key) = event else {
            return Used::No;
        };
        if key.modifiers.ctrl || key.modifiers.alt {
            return Used::No;
        }
        let len = self.slots.len();
        let mut cells = self.cells();
        match key.code {
            KeyCode::Char(c) => {
                // The next slot that takes it, from the caret.
                let Some(at) = (self.caret..len).find(|&i| match self.slots[i] {
                    (Slot::Typed(_, fits), _) => fits.takes(c),
                    // Typing a separator moves past it.
                    (Slot::Fixed(fixed), _) => fixed == c && i == self.caret,
                }) else {
                    return Used::Yes;
                };
                if let (Slot::Typed(..), case) = self.slots[at] {
                    let c = match case {
                        Some(true) => c.to_uppercase().next().unwrap_or(c),
                        Some(false) => c.to_lowercase().next().unwrap_or(c),
                        None => c,
                    };
                    cells[at] = Some(c);
                    self.store(&cells);
                }
                self.caret = self.next_typed(at + 1);
            }
            KeyCode::Backspace => {
                if let Some(at) = self.previous_typed(self.caret) {
                    cells[at] = None;
                    self.store(&cells);
                    self.caret = at;
                }
            }
            KeyCode::Delete => {
                if self.typed_at(self.caret) {
                    cells[self.caret] = None;
                    self.store(&cells);
                }
            }
            KeyCode::Left => {
                if let Some(at) = self.previous_typed(self.caret) {
                    self.caret = at;
                }
            }
            KeyCode::Right => {
                if self.caret < len {
                    self.caret = self.next_typed(self.caret + 1);
                }
            }
            KeyCode::Home => self.caret = self.next_typed(0),
            KeyCode::End => self.caret = len,
            _ => return Used::No,
        }
        cx.redraw();
        Used::Yes
    }

    fn focusable(&self) -> bool {
        true
    }

    fn caret(&self) -> Option<(u16, u16)> {
        Some((self.caret.min(u16::MAX as usize) as u16, 0))
    }
}

// Placeholder.

/// What a [`placeholder`] shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum PlaceholderVariant {
    /// Its label.
    #[default]
    Default,
    /// Its size, `width x height`.
    Size,
    /// Lorem ipsum, wrapped to its width.
    Text,
}

/// Textual's placeholder colours, in turn.
const PLACEHOLDER_COLOURS: [&str; 12] = [
    "#881177", "#aa3355", "#cc6666", "#ee9944", "#eedd00", "#99dd55", "#44dd88", "#22ccbb",
    "#00bbcc", "#0099cc", "#3366bb", "#663399",
];

const LOREM: &str = "Lorem ipsum dolor sit amet, consectetur adipiscing elit. Etiam feugiat ac elit sit amet accumsan. Suspendisse bibendum nec libero quis gravida. Phasellus id eleifend ligula. Nullam imperdiet sem tellus, sed vehicula nisl faucibus sit amet.";

thread_local! {
    /// The next placeholder's colour.
    static NEXT_COLOUR: Cell<usize> = const { Cell::new(0) };
}

pub(crate) struct Placeholder {
    label: String,
    colour: Style,
    variant: Rc<RefCell<PlaceholderVariant>>,
}

/// A block for laying out a prototype, as Textual's `Placeholder`: filled
/// with the next of twelve colours (in the order placeholders are built on
/// a thread), its `label` centred on it.
/// [`placeholder_variant`](Node::placeholder_variant) shows its size or a paragraph instead, and a
/// click moves to the next variant.
///
/// ```
/// use intuituive::prelude::*;
///
/// let app = App::new(|| row([intuituive::placeholder("sidebar").fixed(12), intuituive::placeholder("main")]));
/// let mut driver = app.driver(30, 3);
/// driver.settle();
/// assert!(driver.screen().plain()[1].contains("sidebar"));
/// ```
pub fn placeholder(label: impl Into<String>) -> Node {
    let n = NEXT_COLOUR.with(|next| next.replace(next.get().wrapping_add(1)));
    let colour = Style::parse(&format!(
        "white on {}",
        PLACEHOLDER_COLOURS[n % PLACEHOLDER_COLOURS.len()]
    ))
    .unwrap_or_default();
    widget(Placeholder {
        label: label.into(),
        colour,
        variant: Rc::default(),
    })
    .what("placeholder")
}

impl Widget for Placeholder {
    fn name(&self) -> &'static str {
        "placeholder"
    }

    fn access_text(&self) -> Option<String> {
        Some(self.label.clone())
    }

    fn draw(&mut self, _cx: &mut DrawCx, canvas: &mut Canvas) {
        let (width, height) = (canvas.width(), canvas.height());
        canvas.fill(0, 0, width, height, Some(&self.colour));
        let lines: Vec<String> = match *self.variant.borrow() {
            PlaceholderVariant::Default => vec![self.label.clone()],
            PlaceholderVariant::Size => vec![format!("{width} x {height}")],
            PlaceholderVariant::Text => {
                let mut lines = Vec::new();
                let mut line = String::new();
                for word in LOREM.split(' ') {
                    if !line.is_empty() && line.len() + 1 + word.len() > width as usize {
                        lines.push(std::mem::take(&mut line));
                    }
                    if !line.is_empty() {
                        line.push(' ');
                    }
                    line.push_str(word);
                }
                lines.push(line);
                lines
            }
        };
        let top = height.saturating_sub(lines.len() as u16) / 2;
        for (i, line) in lines.iter().enumerate() {
            let cells = rich::cells::cell_len(line) as u16;
            canvas.print(
                width.saturating_sub(cells) / 2,
                top + i as u16,
                line,
                Some(&self.colour),
            );
        }
    }

    fn event(&mut self, cx: &mut EventCx, event: &WidgetEvent) -> Used {
        match event {
            WidgetEvent::Mouse(mouse) if mouse.kind == MouseKind::Down(Button::Left) => {
                let mut variant = self.variant.borrow_mut();
                *variant = match *variant {
                    PlaceholderVariant::Default => PlaceholderVariant::Size,
                    PlaceholderVariant::Size => PlaceholderVariant::Text,
                    PlaceholderVariant::Text => PlaceholderVariant::Default,
                };
                cx.redraw();
                Used::Yes
            }
            _ => Used::No,
        }
    }
}

/// Options of the small widgets. On any other node they do nothing.
impl Node {
    /// On a [`link`]: run `action` on Enter or a click, instead of opening
    /// the URL.
    pub fn on_activate(self, action: impl FnMut(&mut Ctx) + 'static) -> Node {
        self.widget_mut(|link: &mut Link| link.action = Some(Box::new(action)));
        self
    }

    /// On a [`link`]: open its URL with `opener` instead of the system's
    /// opener (a test's stub, a browser of the app's own).
    pub fn open_with(self, opener: impl Fn(&str) + 'static) -> Node {
        self.widget_mut(|link: &mut Link| link.opener = Rc::new(opener));
        self
    }

    /// On a [`progress_bar`]: show the time left, `ETA h:mm:ss`.
    pub fn show_eta(self, on: bool) -> Node {
        self.widget_mut(|bar: &mut ProgressBar| bar.eta = on);
        self
    }

    /// On a [`progress_bar`]: show the share done, `50%`.
    pub fn show_percentage(self, on: bool) -> Node {
        self.widget_mut(|bar: &mut ProgressBar| bar.percentage = on);
        self
    }

    /// On a [`header`]: show the time on the right.
    pub fn clock(self) -> Node {
        self.widget_mut(|header: &mut Header| header.clock = true);
        self
    }

    /// On a [`header`]: three rows tall (a click toggles it).
    pub fn tall(self) -> Node {
        self.widget_mut(|header: &mut Header| header.tall.set(true));
        self
    }

    /// On a [`placeholder`]: what it shows.
    pub fn placeholder_variant(self, variant: PlaceholderVariant) -> Node {
        self.widget_mut(|placeholder: &mut Placeholder| {
            *placeholder.variant.borrow_mut() = variant;
        });
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn templates_parse_into_slots_and_a_blank() {
        let (slots, blank) = parse_template(">AA-99;_");
        assert_eq!(blank, '_');
        assert_eq!(slots.len(), 5);
        assert_eq!(slots[0], (Slot::Typed(true, Fits::Letter), Some(true)));
        assert_eq!(slots[2], (Slot::Fixed('-'), Some(true)));
        assert_eq!(clock_text(3725), "1:02:05");
    }
}
