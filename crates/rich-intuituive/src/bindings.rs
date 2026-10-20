//! Bindings beyond a key and a handler: ones that apply only in a state
//! ([`Node::bind_when`], [`Node::when`], [`Node::in_mode`],
//! [`Node::modes`]), a help section for any kind of binding
//! ([`Node::section`]), ones that toggle or step a field of a signal
//! ([`Node::bind_toggle`], [`Node::bind_step`],
//! [`Node::bind_step_scaled`]), and what a binding added last does with
//! its key ([`Node::any_modifiers`], [`Node::fall_through`],
//! [`Node::unlisted`], [`Node::bind_hidden`], [`Node::repeat`]), and
//! bindings for a key let go ([`Node::on_key_up`],
//! [`Node::bind_release`]).
//!
//! A binding that does not apply now is as if it were not there: a key
//! passes it by (to another binding of the key, on the same node or
//! above), and the help, the command palette, the which-key pop-up and a
//! [`key_hints`](crate::widgets::key_hints) footer leave it out. Two
//! bindings may so share a key under different conditions.

use std::ops::RangeInclusive;
use std::rc::Rc;
use std::time::Duration;

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
            signal.update(|value| step_within(field(value), step, &range))
        })
    }

    /// [`bind_step`](Self::bind_step) with a step scaled by the modifiers
    /// held, as [`bind_scaled`](Self::bind_scaled) scales a value: each of
    /// `variants` pairs modifiers with a factor of `step` (`("shift",
    /// 10.0)`, `("alt", 0.2, "⅕")`), all in one row of the help. A
    /// threshold or a window's size stepped ten times as far with Shift,
    /// still kept within `range`. An integer moves by the scaled step
    /// rounded.
    ///
    /// ```
    /// use intuituive::prelude::*;
    ///
    /// #[derive(Default)]
    /// struct View { samples: u32 }
    ///
    /// let app = App::new(|| {
    ///     let view = signal(View { samples: 100 });
    ///     text(move || view.with(|v| v.samples.to_string())).bind_step_scaled(
    ///         "right",
    ///         &[("", 1.0), ("shift", 10.0), ("ctrl", 5.0)],
    ///         "More samples",
    ///         view,
    ///         |v| &mut v.samples,
    ///         2.0,
    ///         1..=150,
    ///     )
    /// });
    /// let mut driver = app.driver(5, 1);
    /// driver.press("right shift+right ctrl+right");
    /// assert_eq!(driver.screen().plain()[0].trim_end(), "132");
    /// driver.press("shift+right shift+right");
    /// assert_eq!(driver.screen().plain()[0].trim_end(), "150");
    /// ```
    ///
    /// # Panics
    /// As [`bind_scaled`](Self::bind_scaled): on a name that is not a key,
    /// on no names or no variants, or on a modifier that makes no key.
    #[allow(clippy::too_many_arguments)]
    pub fn bind_step_scaled<S: 'static, T: Stepped, V: Scaled>(
        self,
        keys: &str,
        variants: &[V],
        description: &str,
        signal: Signal<S>,
        field: impl Fn(&mut S) -> &mut T + 'static,
        step: f64,
        range: RangeInclusive<T>,
    ) -> Node {
        self.bind_scaled(keys, variants, description, move |_, factor| {
            signal.update(|value| step_within(field(value), step * factor, &range))
        })
    }

    /// Let the binding added last fire with any modifiers held: its keys
    /// with Shift, Ctrl or Alt (or none) are all it. A binding that names
    /// the key with its modifiers, here or on a node deeper, still wins.
    /// scope-tui's PgUp steps by one whatever is held.
    ///
    /// ```
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| {
    ///     let n = signal(0);
    ///     text!("{n}")
    ///         .bind("pageup", "Up one", move |_| n.update(|n| *n += 1))
    ///         .any_modifiers()
    ///         .bind("ctrl+pageup", "Up ten", move |_| n.update(|n| *n += 10))
    /// });
    /// let mut driver = app.driver(5, 1);
    /// driver.press("pageup shift+pageup alt+pageup ctrl+pageup");
    /// assert_eq!(driver.screen().plain()[0].trim_end(), "13");
    /// ```
    pub fn any_modifiers(self) -> Node {
        self.last_binding("any_modifiers", |binding| binding.any_modifiers = true)
    }

    /// Let the key go on after the binding added last runs, as if it had
    /// not taken it: to the next binding of the key on this node, then up
    /// to the nodes above. One key, two handlers: a view's Esc that resets
    /// it, then the app's. A handler can also decide as it runs, with
    /// [`Ctx::pass_key`](crate::Ctx::pass_key).
    ///
    /// ```
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| {
    ///     let said = signal(String::new());
    ///     column([text!("{said}")
    ///         .bind("esc", "Reset the view", move |_| said.update(|s| s.push_str("view ")))
    ///         .fall_through()
    ///         .focusable()])
    ///     .bind("esc", "Stop", move |_| said.update(|s| s.push_str("app")))
    /// });
    /// let mut driver = app.driver(10, 1);
    /// driver.press("esc");
    /// assert_eq!(driver.screen().plain()[0].trim_end(), "view app");
    /// ```
    pub fn fall_through(self) -> Node {
        self.last_binding("fall_through", |binding| binding.fall_through = true)
    }

    /// Keep the binding added last out of the help, the which-key pop-up
    /// and a [`key_hints`](crate::widgets::key_hints) footer while it
    /// still works; its description is its name, which the command palette
    /// lists and a test can run ([`Driver::run_named`](crate::Driver::run_named)),
    /// as Textual's `Binding(show=False)`. An empty description hides a
    /// binding with no name.
    ///
    /// ```
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| {
    ///     let n = signal(0);
    ///     text!("{n}")
    ///         .bind("+", "Add one", move |_| n.update(|n| *n += 1))
    ///         .bind("shift+up", "Add one (shifted)", move |_| n.update(|n| *n += 1))
    ///         .unlisted()
    ///         .on_key("?", |cx| cx.help())
    /// });
    /// let mut driver = app.driver(40, 10);
    /// driver.press("shift+up");
    /// assert_eq!(driver.screen().plain()[0].trim_end(), "1");
    /// driver.press("?");
    /// let help = driver.screen().plain().join("\n");
    /// assert!(help.contains("Add one") && !help.contains("shifted"));
    /// ```
    pub fn unlisted(self) -> Node {
        self.last_binding("unlisted", |binding| binding.hidden = true)
    }

    /// [`bind`](Self::bind), [`unlisted`](Self::unlisted): bound under
    /// `name` (for the palette and tests), not listed.
    ///
    /// # Panics
    /// On a name that is not a key, or on no names at all.
    pub fn bind_hidden(
        self,
        keys: &str,
        name: &str,
        handler: impl FnMut(&mut Ctx) + 'static,
    ) -> Node {
        self.bind(keys, name, handler).unlisted()
    }

    /// Have the key binding added last repeat under the app's control
    /// while its key is held: it fires as the key goes down, again after
    /// `delay`, then every `rate` until the key is let go, whatever the
    /// terminal's own repeat is set to (a game's DAS and ARR: blocks'
    /// ← and → with 167 ms and 33 ms). The terminal's repeats of the key
    /// are passed over.
    ///
    /// It needs to know when the key is let go: where the terminal
    /// reports the key's release ([`Ctx::releases_reported`]), or with
    /// [`App::synthesize_releases`](crate::App::synthesize_releases) on.
    /// Elsewhere the binding fires on each of the terminal's repeats, as
    /// one without `repeat` does.
    ///
    /// ```
    /// use intuituive::interact::{Event, Key, Releases};
    /// use intuituive::prelude::*;
    /// use std::time::Duration;
    ///
    /// let ms = Duration::from_millis;
    /// let app = App::new(|| {
    ///     let x = signal(0);
    ///     text!("{x}")
    ///         .bind("right", "Move right", move |_| x.update(|x| *x += 1))
    ///         .repeat(ms(167), ms(33))
    /// });
    /// let mut driver = app.driver(5, 1);
    /// driver.set_releases(Releases::ExceptText);
    /// driver.press("right");
    /// // At once, at 167 ms, then every 33 ms: 200, 233, 266.
    /// driver.advance(ms(280));
    /// driver.send(Event::KeyUp(Key::parse("right").unwrap()));
    /// driver.advance(ms(500));
    /// assert_eq!(driver.screen().plain()[0].trim_end(), "5");
    /// ```
    pub fn repeat(self, delay: Duration, rate: Duration) -> Node {
        let rate = rate.max(Duration::from_millis(1));
        self.last_binding("repeat", |binding| binding.repeat = Some((delay, rate)))
    }

    /// Run `handler` when one of `keys` is let go while this node or a
    /// node inside it has the focus (and nothing deeper used the
    /// release): soft drop while ↓ is held, walking while W is. A binding
    /// without modifiers takes the key's release whatever modifiers are
    /// held by then (`w` is let go as `W` once Shift is down). Releases
    /// come from a terminal that reports them (see
    /// [`Ctx::releases_reported`]), or are made up by the app
    /// ([`App::synthesize_releases`](crate::App::synthesize_releases)).
    /// [`Ctx::key`] says which key it was.
    ///
    /// ```
    /// use intuituive::interact::{Event, Key};
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| {
    ///     let state = signal("up");
    ///     text!("{state}")
    ///         .on_key("down", move |_| state.set("down"))
    ///         .on_key_up("down", move |_| state.set("up again"))
    /// });
    /// let mut driver = app.driver(10, 1);
    /// driver.press("down");
    /// assert_eq!(driver.screen().plain()[0].trim_end(), "down");
    /// driver.send(Event::KeyUp(Key::parse("down").unwrap()));
    /// assert_eq!(driver.screen().plain()[0].trim_end(), "up again");
    /// ```
    ///
    /// # Panics
    /// On a name that is not a key.
    pub fn on_key_up(self, keys: &str, handler: impl FnMut(&mut Ctx) + 'static) -> Node {
        self.bind_release(keys, "", handler)
    }

    /// [`on_key_up`](Self::on_key_up) with a description, which the help
    /// lists as the key's, marked "(on release)".
    ///
    /// # Panics
    /// On a name that is not a key.
    pub fn bind_release(
        self,
        keys: &str,
        description: &str,
        mut handler: impl FnMut(&mut Ctx) + 'static,
    ) -> Node {
        let parsed = crate::node::parse_keys(keys);
        let handler: crate::node::KeyHandler = Box::new(move |cx, _| handler(cx));
        self.releases
            .borrow_mut()
            .push(KeyBinding::new(parsed, description, handler));
        self
    }

    /// Change the key binding added last (not a sequence); a debug build
    /// says so on stderr when there is none.
    fn last_binding(self, what: &str, change: impl FnOnce(&mut KeyBinding)) -> Node {
        match self.keys.borrow_mut().last_mut() {
            Some(binding) => change(binding),
            None => {
                if cfg!(debug_assertions) {
                    eprintln!("intuituive: {what}() on a node with no key binding does nothing");
                }
            }
        }
        self
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

/// Move `number` on by `step`, kept within `range`.
fn step_within<T: Stepped>(number: &mut T, step: f64, range: &RangeInclusive<T>) {
    let moved = number.stepped(step);
    *number = if moved < *range.start() {
        *range.start()
    } else if moved > *range.end() {
        *range.end()
    } else {
        moved
    };
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
    #[must_use]
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
