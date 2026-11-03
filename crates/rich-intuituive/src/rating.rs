//! A score in stars: [`rating`] shows one, [`rating_input`] sets one.

use rich_interact::{Button as MouseButton, KeyCode, MouseKind};

use crate::a11y::Role;
use crate::controls::look;
use crate::node::{Axis, Node};
use crate::reactive::Signal;
use crate::widget::{Canvas, DrawCx, EventCx, MeasureCx, Used, Widget, WidgetEvent, widget};

/// What [`rating`] and [`rating_input`] make.
struct Rating {
    value: Box<dyn Fn() -> f64>,
    /// What a key or a click sets, for [`rating_input`].
    set: Option<Signal<f64>>,
    out_of: u32,
    /// The score the last draw showed.
    shown: f64,
}

/// `value` kept in `0..=out_of` and to the nearest half.
fn halves(value: f64, out_of: u32) -> f64 {
    if !value.is_finite() {
        return 0.0;
    }
    ((value * 2.0).round() / 2.0).clamp(0.0, out_of as f64)
}

/// A score as it is said: `4`, `4.5`, at most one decimal.
fn score(value: f64) -> String {
    let text = format!("{value:.1}");
    text.strip_suffix(".0").map(str::to_string).unwrap_or(text)
}

/// A score of `out_of` stars, from what `value` returns (a closure that
/// may read signals): full stars, a half one, the rest empty, then the
/// number, `★★★★½ 4.5`. The value is drawn to the nearest half star; the
/// number has one decimal. One row, as wide as `out_of` stars and the
/// number. The stars are in the theme's `rating.on` style (else `warn`),
/// the empty ones `rating.off` (else `muted`), the number `rating.score`
/// (else none). In [text mode](crate::App::accessible) it is `4.5/5`.
///
/// Its role is [`Img`](crate::a11y::Role::Img), its name the score said in
/// words: `4.5 out of 5`, or, with a [label](Node::label), the label then
/// the score (`Rating: 4.5 out of 5`).
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::rating;
///
/// let app = App::new(|| rating(|| 4.5, 5));
/// let mut driver = app.driver(12, 1);
/// driver.settle();
/// assert_eq!(driver.screen().plain()[0].trim_end(), "★★★★½ 4.5");
/// assert_eq!(driver.accessibility()[0].describe(), "4.5 out of 5, image");
/// ```
pub fn rating(value: impl Fn() -> f64 + 'static, out_of: u32) -> Node {
    widget(Rating {
        value: Box::new(value),
        set: None,
        out_of: out_of.max(1),
        shown: 0.0,
    })
}

/// A score of `out_of` stars that the user sets, held in `value`: drawn as
/// [`rating`] draws it. ←/↓ and →/↑ take a star off and add one, Home and
/// End set none and all, a digit sets that many, a click on a star sets
/// up to it (a click on the last star set takes it back to a half), and
/// the wheel adds or takes off a star. With the focus the stars take
/// `control.focus`. Its role is a [`Slider`](crate::a11y::Role::Slider),
/// its value `3 out of 5`.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::rating_input;
///
/// let app = App::new(|| {
///     let stars = signal(2.0);
///     column([rating_input(stars, 5).label("Your rating"), text!("{stars}")])
/// });
/// let mut driver = app.driver(12, 2);
/// driver.press("right right");
/// assert_eq!(driver.screen().plain()[1].trim_end(), "4");
/// assert_eq!(
///     driver.accessibility()[0].describe(),
///     "Your rating, slider: 4 out of 5"
/// );
/// ```
pub fn rating_input(value: Signal<f64>, out_of: u32) -> Node {
    widget(Rating {
        value: Box::new(move || value.get()),
        set: Some(value),
        out_of: out_of.max(1),
        shown: 0.0,
    })
}

impl Rating {
    fn spoken(&self) -> String {
        format!("{} out of {}", score(self.shown), self.out_of)
    }
}

impl Widget for Rating {
    fn name(&self) -> &'static str {
        "rating"
    }

    fn describe(&self) -> Option<String> {
        Some(self.spoken())
    }

    fn role(&self) -> Role {
        if self.set.is_some() {
            Role::Slider
        } else {
            Role::Img
        }
    }

    /// A picture's name is its score; a slider's is its label, and the
    /// score its value.
    fn access_text(&self) -> Option<String> {
        self.set.is_none().then(|| self.spoken())
    }

    fn access_value(&self) -> Option<String> {
        self.set.is_some().then(|| self.spoken())
    }

    fn measure(&mut self, _cx: &MeasureCx, axis: Axis, _width: u16, _height: u16) -> u16 {
        match axis {
            Axis::Vertical => 1,
            // The stars, a space and the widest number (`10.5`).
            Axis::Horizontal => {
                let widest = score(self.out_of as f64 - 0.5).len().max(1) as u16;
                self.out_of.min(u16::MAX as u32 - 8) as u16 + 1 + widest
            }
        }
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        let raw = (self.value)();
        let value = halves(raw, self.out_of);
        self.shown = if raw.is_finite() {
            raw.clamp(0.0, self.out_of as f64)
        } else {
            0.0
        };
        let number = score(self.shown);
        let number_style = look(cx, &["rating.score"], "none");
        if crate::a11y::text_mode() {
            canvas.print(0, 0, &format!("{number}/{}", self.out_of), None);
            return;
        }
        let mut on = look(cx, &["rating.on", "warn"], "yellow");
        let mut off = look(cx, &["rating.off", "muted"], "dim");
        if self.set.is_some() && cx.focused() {
            let focus = cx.style("control.focus", "reverse");
            on = on.combine(&focus);
            off = off.combine(&focus);
        }
        let full = value.floor() as u32;
        let half = value.fract() > 0.0;
        let mut x = 0u16;
        for star in 0..self.out_of {
            let (glyph, style) = if star < full {
                ('★', &on)
            } else if star == full && half {
                ('½', &on)
            } else {
                ('☆', &off)
            };
            canvas.set(x, 0, glyph, Some(style));
            x += 1;
        }
        let number_w = number.len() as u16;
        if x + 1 + number_w <= canvas.width() {
            canvas.print(x + 1, 0, &number, Some(&number_style));
        }
        if cx.disabled() {
            canvas.restyle(
                0,
                0,
                canvas.width(),
                1,
                &cx.style("control.disabled", "dim"),
            );
        }
    }

    fn event(&mut self, _cx: &mut EventCx, event: &WidgetEvent) -> Used {
        let Some(set) = self.set else {
            return Used::No;
        };
        let max = self.out_of as f64;
        let now = halves(set.get_untracked(), self.out_of);
        let to = match event {
            WidgetEvent::Key(key) => match key.code {
                KeyCode::Left | KeyCode::Down => (now.ceil() - 1.0).max(0.0),
                KeyCode::Right | KeyCode::Up => (now.floor() + 1.0).min(max),
                KeyCode::Home => 0.0,
                KeyCode::End => max,
                KeyCode::Char(c) if c.is_ascii_digit() && !key.modifiers.ctrl => {
                    let n = c.to_digit(10).unwrap_or(0) as f64;
                    if n > max {
                        return Used::No;
                    }
                    n
                }
                _ => return Used::No,
            },
            WidgetEvent::Mouse(mouse) => match mouse.kind {
                MouseKind::Down(MouseButton::Left) if (mouse.column as u32) < self.out_of => {
                    let star = mouse.column as f64 + 1.0;
                    if now == star { star - 0.5 } else { star }
                }
                MouseKind::Down(MouseButton::Left) => return Used::Yes,
                MouseKind::ScrollUp => (now.floor() + 1.0).min(max),
                MouseKind::ScrollDown => (now.ceil() - 1.0).max(0.0),
                _ => return Used::No,
            },
            _ => return Used::No,
        };
        if to != now {
            set.set(to);
        }
        Used::Yes
    }

    fn focusable(&self) -> bool {
        self.set.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scores_round_to_halves_and_say_one_decimal() {
        assert_eq!(halves(4.3, 5), 4.5);
        assert_eq!(halves(4.2, 5), 4.0);
        assert_eq!(halves(7.0, 5), 5.0);
        assert_eq!(halves(f64::NAN, 5), 0.0);
        assert_eq!(score(4.0), "4");
        assert_eq!(score(4.5), "4.5");
        assert_eq!(score(4.66), "4.7");
    }
}
