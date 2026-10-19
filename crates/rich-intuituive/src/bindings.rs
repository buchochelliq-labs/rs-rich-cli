//! Bindings beyond a key and a handler: ones that apply only in a state
//! ([`Node::bind_when`], [`Node::when`], [`Node::in_mode`],
//! [`Node::modes`]), a help section for any kind of binding
//! ([`Node::section`]), and ones that toggle or step a field of a signal
//! ([`Node::bind_toggle`], [`Node::bind_step`]).
//!
//! A binding that does not apply now is as if it were not there: a key
//! passes it by (to another binding of the key, on the same node or
//! above), and the help, the command palette, the which-key pop-up and a
//! [`key_hints`](crate::widgets::key_hints) footer leave it out. Two
//! bindings may so share a key under different conditions.

use std::ops::RangeInclusive;
use std::rc::Rc;

use crate::app::Ctx;
use crate::node::{BindingCondition, KeyBinding, Node};
use crate::reactive::Signal;

impl Node {
    /// [`bind`](Self::bind), applying (and listed in the help) only while
    /// `when` is true: a key bound in one mode of an app, or one key that
    /// does one thing in one mode and another in the next. `when` is read
    /// as the key comes and as the help opens, and should only read
    /// signals.
    ///
    /// ```
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| {
    ///     let (scope, what) = (signal("osc"), signal(""));
    ///     text!("{what}")
    ///         .bind_when(move || scope.get() == "osc", "p", "Peaks", move |_| what.set("peaks"))
    ///         .bind_when(move || scope.get() == "spec", "p", "Phase", move |_| what.set("phase"))
    ///         .on_key("tab", move |_| scope.set("spec"))
    /// });
    /// let mut driver = app.driver(10, 1);
    /// driver.press("p");
    /// assert_eq!(driver.screen().plain()[0].trim_end(), "peaks");
    /// driver.press("tab p");
    /// assert_eq!(driver.screen().plain()[0].trim_end(), "phase");
    /// ```
    ///
    /// # Panics
    /// On a name that is not a key, or on no names at all.
    pub fn bind_when(
        self,
        when: impl Fn() -> bool + 'static,
        keys: &str,
        description: &str,
        handler: impl FnMut(&mut Ctx) + 'static,
    ) -> Node {
        self.when(when, |node| node.bind(keys, description, handler))
    }

    /// Make every binding `scope` adds (of any kind: [`bind`](Self::bind),
    /// [`bind_scaled`](Self::bind_scaled), [`bind_cycle`](Self::bind_cycle),
    /// [`bind_toggle`](Self::bind_toggle), sequences, …) apply only while
    /// `when` is true, as [`bind_when`](Self::bind_when) does one. Scopes
    /// nest: a binding applies while all of its conditions hold.
    ///
    /// ```
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| {
    ///     let (editing, n) = (signal(false), signal(0));
    ///     text!("{n}")
    ///         .when(move || editing.get(), |node| {
    ///             node.bind("+", "More", move |_| n.update(|n| *n += 1))
    ///                 .bind("-", "Less", move |_| n.update(|n| *n -= 1))
    ///         })
    ///         .on_key("e", move |_| editing.set(true))
    /// });
    /// let mut driver = app.driver(5, 1);
    /// driver.press("+ e + +");
    /// assert_eq!(driver.screen().plain()[0].trim_end(), "2");
    /// ```
    pub fn when(self, when: impl Fn() -> bool + 'static, scope: impl FnOnce(Node) -> Node) -> Node {
        let when: BindingCondition = Rc::new(when);
        self.scoped(scope, move |binding| {
            binding.when = Some(match binding.when.take() {
                // Within another scope: both.
                Some(inner) => {
                    let outer = when.clone();
                    Rc::new(move || outer() && inner())
                }
                None => when.clone(),
            });
        })
    }

    /// List every binding `scope` adds (of any kind) in the help and the
    /// command palette under the section `name`, as
    /// [`bind_in`](Self::bind_in) lists one: `bind_scaled`, `bind_cycle`,
    /// `bind_toggle` and `bind_step` take no section of their own. A
    /// binding given its own section inside (`bind_in`, a nested `section`)
    /// keeps it.
    ///
    /// ```
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| {
    ///     let (scale, mode) = (signal(1.0), signal("osc"));
    ///     text!("{scale}")
    ///         .section("oscilloscope", |node| {
    ///             node.bind_scaled("pageup", &[("", 1.0), ("shift", 10.0)], "Threshold up", move |_, by| {
    ///                 scale.update(|s| *s += by)
    ///             })
    ///             .bind_cycle("tab", "Next mode", mode, &["osc", "spec"])
    ///         })
    ///         .on_key("?", |cx| cx.help())
    /// });
    /// let mut driver = app.driver(60, 12);
    /// driver.press("?");
    /// let help = driver.screen().plain().join("\n");
    /// assert!(help.find("oscilloscope") < help.find("Threshold up"));
    /// assert!(help.find("oscilloscope") < help.find("Next mode"));
    /// ```
    pub fn section(self, name: &str, scope: impl FnOnce(Node) -> Node) -> Node {
        let name = name.to_string();
        self.scoped(scope, move |binding| {
            binding.section.get_or_insert_with(|| name.clone());
        })
    }

    /// The bindings of one mode: what `scope` adds applies, and is listed
    /// in the help under `section`, only while `mode` is `value`. Built on
    /// [`when`](Self::when) and [`section`](Self::section); a mode's view
    /// is a [`switch`](crate::switch) on the same signal.
    ///
    /// ```
    /// use intuituive::prelude::*;
    ///
    /// #[derive(Clone, Copy, PartialEq)]
    /// enum Mode { Osc, Spec }
    ///
    /// let app = App::new(|| {
    ///     let (mode, log) = (signal(Mode::Osc), signal(false));
    ///     text!("log {log}")
    ///         .in_mode(mode, Mode::Spec, "spectroscope", |node| {
    ///             node.bind_toggle("l", "Logarithmic scale", log, |log| log)
    ///         })
    ///         .on_key("tab", move |_| mode.set(Mode::Spec))
    /// });
    /// let mut driver = app.driver(12, 1);
    /// driver.press("l");
    /// assert_eq!(driver.screen().plain()[0].trim_end(), "log false");
    /// driver.press("tab l");
    /// assert_eq!(driver.screen().plain()[0].trim_end(), "log true");
    /// ```
    pub fn in_mode<T: PartialEq + 'static>(
        self,
        mode: Signal<T>,
        value: T,
        section: &str,
        scope: impl FnOnce(Node) -> Node,
    ) -> Node {
        self.when(
            move || mode.with_untracked(|now| *now == value),
            |node| node.section(section, scope),
        )
    }

    /// [`in_mode`](Self::in_mode) for each of `pages`: a mode's value, its
    /// help section, and what binds its keys. Of the modes, only the one
    /// `mode` holds has its bindings apply and listed.
    ///
    /// ```
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| {
    ///     let (mode, said) = (signal(0), signal(""));
    ///     text!("{said}")
    ///         .modes(
    ///             mode,
    ///             &[
    ///                 (0, "oscilloscope", &|node: Node| node.bind("p", "Peaks", move |_| said.set("peaks"))),
    ///                 (1, "spectroscope", &|node: Node| node.bind("p", "Phase", move |_| said.set("phase"))),
    ///             ],
    ///         )
    ///         .on_key("tab", move |_| mode.set(1))
    ///         .on_key("?", |cx| cx.help())
    /// });
    /// let mut driver = app.driver(50, 12);
    /// driver.press("tab p");
    /// assert_eq!(driver.screen().plain()[0].trim_end(), "phase");
    /// driver.press("?");
    /// let help = driver.screen().plain().join("\n");
    /// assert!(help.contains("spectroscope") && !help.contains("oscilloscope"));
    /// ```
    pub fn modes<T: PartialEq + Clone + 'static>(
        self,
        mode: Signal<T>,
        pages: &[ModePage<'_, T>],
    ) -> Node {
        pages.iter().fold(self, |node, (value, section, scope)| {
            node.in_mode(mode, value.clone(), section, scope)
        })
    }

    /// Bind `keys` to flip the `bool` that `field` picks out of `signal`'s
    /// value: a flag of an app's state, with no closure written for it.
    ///
    /// ```
    /// use intuituive::prelude::*;
    ///
    /// #[derive(Default)]
    /// struct View { grid: bool }
    ///
    /// let app = App::new(|| {
    ///     let view = signal(View::default());
    ///     text(move || format!("grid {}", view.with(|v| v.grid)))
    ///         .bind_toggle("g", "Show the grid", view, |v| &mut v.grid)
    /// });
    /// let mut driver = app.driver(12, 1);
    /// driver.press("g");
    /// assert_eq!(driver.screen().plain()[0].trim_end(), "grid true");
    /// ```
    ///
    /// # Panics
    /// On a name that is not a key, or on no names at all.
    pub fn bind_toggle<S: 'static>(
        self,
        keys: &str,
        description: &str,
        signal: Signal<S>,
        field: impl Fn(&mut S) -> &mut bool + 'static,
    ) -> Node {
        self.bind(keys, description, move |_| {
            signal.update(|value| {
                let flag = field(value);
                *flag = !*flag;
            })
        })
    }

    /// Bind `keys` to move the number that `field` picks out of `signal`'s
    /// value on by `step` (negative to go down), kept within `range`: a
    /// zoom, a page size, a count. The number may be any integer or float
    /// type ([`Stepped`]); an integer moves by `step` rounded.
    ///
    /// ```
    /// use intuituive::prelude::*;
    ///
    /// #[derive(Default)]
    /// struct View { samples: u32 }
    ///
    /// let app = App::new(|| {
    ///     let view = signal(View { samples: 4 });
    ///     text(move || view.with(|v| v.samples.to_string()))
    ///         .bind_step("+", "More samples", view, |v| &mut v.samples, 4.0, 1..=10)
    ///         .bind_step("-", "Fewer samples", view, |v| &mut v.samples, -4.0, 1..=10)
    /// });
    /// let mut driver = app.driver(5, 1);
    /// driver.press("+ +");
    /// assert_eq!(driver.screen().plain()[0].trim_end(), "10");
    /// driver.press("- - - -");
    /// assert_eq!(driver.screen().plain()[0].trim_end(), "1");
    /// ```
    ///
    /// # Panics
    /// On a name that is not a key, or on no names at all.
    pub fn bind_step<S: 'static, T: Stepped>(
        self,
        keys: &str,
        description: &str,
        signal: Signal<S>,
        field: impl Fn(&mut S) -> &mut T + 'static,
        step: f64,
        range: RangeInclusive<T>,
    ) -> Node {
        self.bind(keys, description, move |_| {
            signal.update(|value| {
                let number = field(value);
                let moved = number.stepped(step);
                *number = if moved < *range.start() {
                    *range.start()
                } else if moved > *range.end() {
                    *range.end()
                } else {
                    moved
                };
            })
        })
    }

    /// Run `scope` on this node, then `patch` each binding and sequence it
    /// added.
    fn scoped(self, scope: impl FnOnce(Node) -> Node, patch: impl Fn(&mut KeyBinding)) -> Node {
        let (keys, sequences) = (self.keys.borrow().len(), self.sequences.borrow().len());
        let node = scope(self);
        for binding in node.keys.borrow_mut().iter_mut().skip(keys) {
            patch(binding);
        }
        for binding in node.sequences.borrow_mut().iter_mut().skip(sequences) {
            patch(binding);
        }
        node
    }
}

/// One mode of [`Node::modes`]: its value, its help section, and what
/// binds its keys.
pub type ModePage<'a, T> = (T, &'a str, &'a dyn Fn(Node) -> Node);

/// A variant of a [`bind_scaled`](Node::bind_scaled) binding: the
/// modifiers that make its keys, its value, and how the help says it.
/// `("shift", 10.0)` is listed `shift ×10`; `("alt", 0.2, "⅕")` is listed
/// `alt ⅕`.
pub trait Scaled {
    /// `""` for none, `"shift"`, `"ctrl"`, `"alt"`, `"ctrl+alt"`.
    fn modifiers(&self) -> &str;
    fn value(&self) -> f64;
    /// What the help says in place of `×value`.
    fn label(&self) -> Option<&str> {
        None
    }
}

impl Scaled for (&str, f64) {
    fn modifiers(&self) -> &str {
        self.0
    }

    fn value(&self) -> f64 {
        self.1
    }
}

impl Scaled for (&str, f64, &str) {
    fn modifiers(&self) -> &str {
        self.0
    }

    fn value(&self) -> f64 {
        self.1
    }

    fn label(&self) -> Option<&str> {
        Some(self.2)
    }
}

/// A number [`bind_step`](Node::bind_step) can move: the integer and float
/// types.
pub trait Stepped: Copy + PartialOrd + 'static {
    /// This moved by `by`: rounded, and kept within the type, for an
    /// integer.
    fn stepped(self, by: f64) -> Self;
}

macro_rules! stepped_integers {
    ($($t:ty),*) => {$(
        impl Stepped for $t {
            fn stepped(self, by: f64) -> $t {
                // Exact for every integer: whole steps, in a wider type.
                let moved = self as i128 + by.round() as i128;
                moved.clamp(<$t>::MIN as i128, <$t>::MAX as i128) as $t
            }
        }
    )*};
}

stepped_integers!(i8, i16, i32, i64, isize, u8, u16, u32, u64, usize);

impl Stepped for f32 {
    fn stepped(self, by: f64) -> f32 {
        self + by as f32
    }
}

impl Stepped for f64 {
    fn stepped(self, by: f64) -> f64 {
        self + by
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integers_step_exactly_and_stay_within_their_type() {
        assert_eq!(3u8.stepped(-5.0), 0);
        assert_eq!(250u8.stepped(10.0), 255);
        assert_eq!(i64::MAX.stepped(-1.0), i64::MAX - 1);
        assert_eq!(7i32.stepped(2.4), 9);
        assert_eq!(0.5f64.stepped(0.25), 0.75);
    }
}
