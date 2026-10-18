//! Inputs bound to signals: [`input`], a line of text, and
//! [`number_input`], a whole number in a range.

use std::ops::RangeInclusive;

use rich_interact::{KeyCode, MouseKind};

use crate::node::{Axis, Node};
use crate::reactive::Signal;
use crate::screen::Rect;
use crate::text_area::{Options, TextArea};
use crate::widget::{widget, Canvas, DrawCx, EventCx, MeasureCx, Used, Widget, WidgetEvent};

/// A single-line text box bound to `text`, both ways: what is typed is set
/// on the signal, and text set on the signal from elsewhere replaces what
/// the box holds. It is a [`text_area`](crate::text_area) in a single-line
/// mode: the same editing keys, undo, selection and mouse, but Enter
/// submits rather than starting a line (to [`on_submit`](Node::on_submit),
/// else to the app's bindings), Tab moves the focus on, pasted line breaks
/// become spaces, and a long line scrolls sideways to the caret.
///
/// Its options are builders: [`placeholder`](Node::placeholder),
/// [`mask`](Node::mask), [`max_length`](Node::max_length),
/// [`on_submit`](Node::on_submit) and [`read_only`](Node::read_only).
/// Validate it with [`error_when`](Node::error_when), as any node.
///
/// ```
/// use intuituive::prelude::*;
///
/// let app = App::new(|| {
///     let name = signal(String::new());
///     let said = signal(String::new());
///     column([
///         input(name).placeholder("First name").on_submit(move |value, _| said.set(value.into())),
///         text!("Hello, {said}"),
///     ])
///     .on_key("esc", |cx| cx.quit())
/// });
/// let screen = app.render_with(&["A", "d", "a", "enter", "esc"], 20, 2).unwrap();
/// assert_eq!(screen[0].trim_end(), "Ada");
/// assert_eq!(screen[1].trim_end(), "Hello, Ada");
/// ```
pub fn input(text: Signal<String>) -> Node {
    let area = TextArea::new(
        text,
        Options {
            single_line: true,
            ..Options::default()
        },
    );
    widget(area).what("input")
}

/// A whole number in `range`, bound to `value` both ways. ↑ and ↓ (or `+`
/// and `-` after a digit) step it by [`step`](Node::step), PgUp and PgDn by
/// ten steps, Home and End go to the ends, and the wheel steps it. Digits
/// type a new number (a `-` first for a negative one), Backspace takes one
/// off, and Esc puts back the value. What is typed is set on the signal at
/// once, kept in the range, and shown as typed until Enter or the focus
/// leaves, when it shows the value.
///
/// ```
/// use intuituive::prelude::*;
///
/// let app = App::new(|| {
///     let age = signal(30i64);
///     column([number_input(age, 0..=150).step(5), text!("age {age}")])
///         .on_key("q", |cx| cx.quit())
/// });
/// let screen = app.render_with(&["up", "up", "q"], 12, 2).unwrap();
/// assert_eq!(screen[0].trim_end(), "40");
/// assert_eq!(screen[1].trim_end(), "age 40");
/// ```
pub fn number_input(value: Signal<i64>, range: RangeInclusive<i64>) -> Node {
    widget(NumberInput {
        value,
        range,
        step: 1,
        editing: None,
        width: 0,
    })
    .what("number_input")
}

pub(crate) struct NumberInput {
    value: Signal<i64>,
    range: RangeInclusive<i64>,
    step: i64,
    /// What is being typed, while it is.
    editing: Option<String>,
    /// Its width when it last drew.
    width: u16,
}

impl NumberInput {
    fn clamp(&self, value: i64) -> i64 {
        value.clamp(*self.range.start(), *self.range.end().max(self.range.start()))
    }

    /// Set the value to `value`, kept in the range.
    fn set(&self, value: i64) {
        let value = self.clamp(value);
        if self.value.get_untracked() != value {
            self.value.set(value);
        }
    }

    /// Step `by` steps from the value, ending any typing.
    fn step_by(&mut self, by: i64) {
        self.editing = None;
        let value = self.value.get_untracked();
        self.set(value.saturating_add(by.saturating_mul(self.step)));
    }

    /// What it shows: the typing, or the value.
    fn shown(&self, value: i64) -> String {
        self.editing.clone().unwrap_or_else(|| value.to_string())
    }

    /// The typing changed: set what it reads as.
    fn typed(&mut self, text: String) {
        if let Ok(value) = text.parse::<i64>() {
            self.set(value);
        }
        self.editing = Some(text);
    }
}

impl Widget for NumberInput {
    fn name(&self) -> &'static str {
        "number_input"
    }

    fn describe(&self) -> Option<String> {
        Some(format!("{}..={}", self.range.start(), self.range.end()))
    }

    fn role(&self) -> crate::a11y::Role {
        crate::a11y::Role::TextBox
    }

    fn cursor(&self) -> Option<Rect> {
        Some(Rect::new(0, 0, self.width.max(1), 1))
    }

    fn measure(&mut self, _cx: &MeasureCx, axis: Axis, _width: u16, _height: u16) -> u16 {
        match axis {
            Axis::Vertical => 1,
            // The widest number in the range, and a cell for the caret.
            Axis::Horizontal => {
                let widest = [self.range.start(), self.range.end()]
                    .iter()
                    .map(|n| n.to_string().len())
                    .max()
                    .unwrap_or(1);
                (widest + 1).min(u16::MAX as usize) as u16
            }
        }
    }

    fn draw(&mut self, _cx: &mut DrawCx, canvas: &mut Canvas) {
        let value = self.value.get();
        self.width = canvas.width();
        canvas.print(0, 0, &self.shown(value), None);
    }

    fn event(&mut self, cx: &mut EventCx, event: &WidgetEvent) -> Used {
        match event {
            WidgetEvent::Key(key) if !key.modifiers.ctrl && !key.modifiers.alt => {
                let page = 10;
                match key.code {
                    KeyCode::Up => self.step_by(1),
                    KeyCode::Down => self.step_by(-1),
                    KeyCode::PageUp => self.step_by(page),
                    KeyCode::PageDown => self.step_by(-page),
                    KeyCode::Home => {
                        self.editing = None;
                        self.set(*self.range.start());
                    }
                    KeyCode::End => {
                        self.editing = None;
                        self.set(*self.range.end());
                    }
                    KeyCode::Char(c @ '0'..='9') => {
                        let mut text = self.editing.clone().unwrap_or_default();
                        text.push(c);
                        self.typed(text);
                    }
                    // A minus starts a negative number, where the range has
                    // them; after a digit, `-` and `+` step.
                    KeyCode::Char('-')
                        if self.editing.as_deref().is_none_or(str::is_empty)
                            && *self.range.start() < 0 =>
                    {
                        self.editing = Some("-".into());
                    }
                    KeyCode::Char('-') => self.step_by(-1),
                    KeyCode::Char('+') => self.step_by(1),
                    KeyCode::Backspace => {
                        let mut text = self
                            .editing
                            .clone()
                            .unwrap_or_else(|| self.value.get_untracked().to_string());
                        text.pop();
                        self.typed(text);
                    }
                    KeyCode::Escape if self.editing.is_some() => self.editing = None,
                    // Enter shows the value, and goes on to the app.
                    KeyCode::Enter => {
                        if self.editing.take().is_some() {
                            cx.redraw();
                        }
                        return Used::No;
                    }
                    _ => return Used::No,
                }
            }
            WidgetEvent::Mouse(mouse) => match mouse.kind {
                MouseKind::ScrollUp => self.step_by(1),
                MouseKind::ScrollDown => self.step_by(-1),
                _ => return Used::No,
            },
            WidgetEvent::Focus(false) if self.editing.is_some() => self.editing = None,
            _ => return Used::No,
        }
        cx.redraw();
        Used::Yes
    }

    fn focusable(&self) -> bool {
        true
    }

    fn caret(&self) -> Option<(u16, u16)> {
        let shown = self
            .editing
            .clone()
            .unwrap_or_else(|| self.value.get_untracked().to_string());
        let x = (shown.len() as u16).min(self.width.saturating_sub(1));
        Some((x, 0))
    }
}

/// A [`number_input`]'s option. On any other node it does nothing.
impl Node {
    /// Step a [`number_input`] by `by` (1 by default).
    pub fn step(self, by: i64) -> Node {
        self.widget_mut(|input: &mut NumberInput| input.step = by.max(1));
        self
    }
}
