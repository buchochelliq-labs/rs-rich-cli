//! Ready-made effects, timed and composable: fades, dissolves, sweeps,
//! slides, colour shifts, a glitch, digital rain, a shimmer over text and
//! a pulsing skeleton for what is loading.
//!
//! An [`Fx`] says what happens to a node's cells at each moment of its
//! run. The presets ([`fade_in`], [`dissolve`], [`sweep`], [`shimmer`],
//! [`skeleton`], …) are each one, eased; [`sequence`], [`parallel`] and
//! the methods on [`Fx`] ([`delay`](Fx::delay), [`repeat`](Fx::repeat),
//! [`ping_pong`](Fx::ping_pong), [`forever`](Fx::forever), …) build longer
//! ones out of them. [`Fx::new`] makes your own.
//!
//! An effect plays over a node: [`play`] runs it once when the node first
//! draws, [`play_when`] each time a condition turns true, [`play_while`]
//! for as long as one holds (a skeleton while loading), and [`transition`]
//! forwards while a node is shown and backwards as it goes, hiding it at
//! the end. [`splash`] shows a cover, then fades into the app.
//!
//! Everything is built on [`effect`](crate::effect): each frame draws the
//! node as it is, then the effect reworks its cells for that moment, on
//! the app's clock (so a test steps it exactly). While an effect runs it
//! asks for a frame every [`FRAME`]; once it is over it asks for none, and
//! its last state stays. In accessible and linear mode
//! ([`App::accessible`](crate::App::accessible)), as
//! [`Ctx::animate`](crate::Ctx::animate) does, there is no motion: an
//! effect goes straight to its end (an endless one stays at its start).
//!
//! Colours come from the theme: the `fx` style's colour is the text colour
//! assumed where a cell has none, and its background the colour cells fade
//! to (a dark grey on black unless set); `fx.shimmer`, `fx.rain` and
//! `fx.skeleton` colour those presets.
//!
//! ```
//! use intuituive::prelude::*;
//! use intuituive::fx;
//! use std::time::Duration;
//!
//! let ms = Duration::from_millis;
//! let app = App::new(move || {
//!     // Fade in, then sweep a highlight across, forever.
//!     let intro = fx::sequence([fx::fade_in(ms(300)), fx::shimmer(ms(800))]);
//!     fx::play(label("Welcome"), intro)
//! });
//! let mut driver = app.driver(7, 1);
//! driver.advance(ms(1000));
//! assert_eq!(driver.screen().plain()[0], "Welcome");
//! ```

use std::cell::{Cell, RefCell};
use std::f64::consts::PI;
use std::rc::Rc;
use std::time::Duration;

use rich::{Color, ColorTriplet, Style};

use crate::app::{after, Easing, Timer};
use crate::effect::effect;
use crate::focus::Direction;
use crate::node::{column, label, switch, Node};
use crate::reactive::{signal, watch, Signal};
use crate::widget::{Canvas, DrawCx};

/// How often a running effect draws: about 60 times a second.
pub const FRAME: Duration = crate::sprite::FRAME;

type Shade = dyn Fn(&Frame, &mut Canvas, &mut DrawCx);

/// A moment of an effect, as its drawing ([`Fx::new`]) sees it.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct Frame {
    /// How far through it is, eased: 0 at the start, 1 at the end. Always
    /// 0 for an [endless](Fx::endless) one.
    pub progress: f64,
    /// The time since it started (at most its length).
    pub elapsed: Duration,
}

/// A timed effect over a node's cells: a preset, your own
/// ([`Fx::new`]), or several put together. Cheap to clone. Play it over a
/// node with [`play`] (or [`play_when`], [`play_while`], [`transition`]).
///
/// ```
/// use intuituive::fx::{self, Fx};
/// use intuituive::Easing;
/// use std::time::Duration;
///
/// let ms = Duration::from_millis;
/// let blink: Fx = fx::fade_out(ms(200)).ping_pong().repeat(3);
/// assert_eq!(blink.length(), Some(ms(1200)));
/// let staged = fx::fade_in(ms(400)).easing(Easing::Linear).delay(ms(100));
/// assert_eq!(staged.length(), Some(ms(500)));
/// assert_eq!(fx::rain().length(), None);
/// ```
#[derive(Clone)]
#[must_use = "builder methods return a new value; it does nothing until it is used"]
pub struct Fx(Rc<Kind>);

enum Kind {
    /// What it does at each moment, over `length` seconds (`None`:
    /// endless).
    Shader {
        length: Option<f64>,
        easing: Easing,
        shade: Rc<Shade>,
    },
    Sequence(Vec<Fx>),
    Parallel(Vec<Fx>),
    /// Its start held for so many seconds first.
    Delay(f64, Fx),
    /// Its end held for so many seconds after.
    Hold(f64, Fx),
    /// So many times (`None`: for ever).
    Repeat(Option<u32>, Fx),
    PingPong(Fx),
    Reversed(Fx),
    /// Cut (or held) to so many seconds.
    Limit(f64, Fx),
    /// Its whole run eased.
    Eased(Easing, Fx),
}

fn secs(duration: Duration) -> f64 {
    duration.as_secs_f64()
}

impl Fx {
    fn of(kind: Kind) -> Fx {
        Fx(Rc::new(kind))
    }

    /// An effect of your own, `length` long: `shade` reworks the canvas
    /// (what the node drew) for each [`Frame`], read with
    /// [`Canvas::cell`] and written with [`Canvas::set_cell`] and the rest.
    /// Eased with [`Easing::EaseInOut`] unless [`easing`](Self::easing)
    /// says otherwise. Make it so that a progress of 1 leaves the node as
    /// it should stay.
    ///
    /// ```
    /// use intuituive::prelude::*;
    /// use intuituive::fx::{self, Fx};
    /// use intuituive::Easing;
    /// use std::time::Duration;
    ///
    /// // Letters turn to capitals, left to right.
    /// let shout = Fx::new(Duration::from_millis(100), |frame, canvas, _cx| {
    ///     let upto = (frame.progress * canvas.width() as f64).round() as u16;
    ///     for x in 0..upto {
    ///         let Some(cell) = canvas.cell(x, 0) else { continue };
    ///         let (upper, style) = (cell.symbol().to_uppercase(), cell.style().cloned());
    ///         canvas.set_cell(x, 0, &upper, style.as_ref());
    ///     }
    /// })
    /// .easing(Easing::Linear);
    /// let app = App::new(move || fx::play(label("quiet"), shout.clone()));
    /// let mut driver = app.driver(5, 1);
    /// driver.settle();
    /// assert_eq!(driver.screen().plain()[0], "quiet");
    /// driver.advance(Duration::from_millis(40));
    /// assert_eq!(driver.screen().plain()[0], "QUiet");
    /// driver.advance(Duration::from_millis(60));
    /// assert_eq!(driver.screen().plain()[0], "QUIET");
    /// ```
    pub fn new(length: Duration, shade: impl Fn(&Frame, &mut Canvas, &mut DrawCx) + 'static) -> Fx {
        Fx::of(Kind::Shader {
            length: Some(secs(length)),
            easing: Easing::EaseInOut,
            shade: Rc::new(shade),
        })
    }

    /// An effect of your own that never ends: `shade` gets the time
    /// since it started in [`Frame::elapsed`] (its progress stays 0). At
    /// its start it should be the node's still state: what accessible mode
    /// shows. [`limit`](Self::limit) gives it an end.
    pub fn endless(shade: impl Fn(&Frame, &mut Canvas, &mut DrawCx) + 'static) -> Fx {
        Fx::of(Kind::Shader {
            length: None,
            easing: Easing::Linear,
            shade: Rc::new(shade),
        })
    }

    /// How long it runs; `None` for ever.
    pub fn length(&self) -> Option<Duration> {
        self.seconds().map(Duration::from_secs_f64)
    }

    fn seconds(&self) -> Option<f64> {
        match &*self.0 {
            Kind::Shader { length, .. } => *length,
            Kind::Sequence(all) => all.iter().map(Fx::seconds).sum(),
            Kind::Parallel(all) => all
                .iter()
                .map(Fx::seconds)
                .try_fold(0.0, |most: f64, l| l.map(|l| most.max(l))),
            Kind::Delay(wait, fx) | Kind::Hold(wait, fx) => fx.seconds().map(|l| l + wait),
            Kind::Repeat(times, fx) => Some(fx.seconds()? * f64::from((*times)?)),
            Kind::PingPong(fx) => fx.seconds().map(|l| l * 2.0),
            Kind::Reversed(fx) | Kind::Eased(_, fx) => fx.seconds(),
            Kind::Limit(length, _) => Some(*length),
        }
    }

    /// Eased with `easing`: a preset's own easing replaced, or a whole
    /// composed run's timeline eased.
    pub fn easing(self, easing: Easing) -> Fx {
        match &*self.0 {
            Kind::Shader { length, shade, .. } => Fx::of(Kind::Shader {
                length: *length,
                easing,
                shade: shade.clone(),
            }),
            _ => Fx::of(Kind::Eased(easing, self)),
        }
    }

    /// Wait `wait` first, holding its start: a fade in delayed stays
    /// hidden until it begins (a stagger).
    pub fn delay(self, wait: Duration) -> Fx {
        Fx::of(Kind::Delay(secs(wait), self))
    }

    /// Hold its end for `wait` after it: in a [`sequence`], a fade out held
    /// before what comes next.
    pub fn hold(self, wait: Duration) -> Fx {
        Fx::of(Kind::Hold(secs(wait), self))
    }

    /// Run it `times` times in a row.
    pub fn repeat(self, times: u32) -> Fx {
        Fx::of(Kind::Repeat(Some(times), self))
    }

    /// Run it again and again, for ever.
    pub fn forever(self) -> Fx {
        Fx::of(Kind::Repeat(None, self))
    }

    /// Run it forwards, then backwards: twice as long, ending where it
    /// started.
    pub fn ping_pong(self) -> Fx {
        Fx::of(Kind::PingPong(self))
    }

    /// Run it backwards: a fade in reversed fades out.
    pub fn reversed(self) -> Fx {
        Fx::of(Kind::Reversed(self))
    }

    /// Make it `length` long: an endless one ends, a short one holds its
    /// end.
    pub fn limit(self, length: Duration) -> Fx {
        Fx::of(Kind::Limit(secs(length), self))
    }

    /// Rework `canvas` for `t` seconds into the run.
    fn apply(&self, t: f64, canvas: &mut Canvas, cx: &mut DrawCx) {
        let t = t.max(0.0);
        match &*self.0 {
            Kind::Shader {
                length,
                easing,
                shade,
            } => {
                let (progress, elapsed) = match *length {
                    Some(length) if length > 0.0 => (easing.at(t / length), t.min(length)),
                    Some(_) => (1.0, 0.0),
                    None => (0.0, t),
                };
                let frame = Frame {
                    progress,
                    elapsed: Duration::from_secs_f64(elapsed),
                };
                shade(&frame, canvas, cx);
            }
            Kind::Sequence(all) => {
                let mut start = 0.0;
                for (i, fx) in all.iter().enumerate() {
                    match fx.seconds() {
                        Some(length) if i + 1 < all.len() && t >= start + length => {
                            start += length;
                        }
                        _ => return fx.apply(t - start, canvas, cx),
                    }
                }
            }
            Kind::Parallel(all) => {
                for fx in all {
                    fx.apply(fx.seconds().map_or(t, |l| t.min(l)), canvas, cx);
                }
            }
            Kind::Delay(wait, fx) => fx.apply(t - wait, canvas, cx),
            Kind::Hold(_, fx) => fx.apply(fx.seconds().map_or(t, |l| t.min(l)), canvas, cx),
            Kind::Repeat(times, fx) => match fx.seconds() {
                Some(length) if length > 0.0 => {
                    let round = (t / length).floor();
                    if times.is_some_and(|times| round >= f64::from(times)) {
                        fx.apply(length, canvas, cx);
                    } else {
                        fx.apply(t - round * length, canvas, cx);
                    }
                }
                _ => fx.apply(t, canvas, cx),
            },
            Kind::PingPong(fx) => match fx.seconds() {
                Some(length) if t > length => fx.apply(2.0 * length - t, canvas, cx),
                _ => fx.apply(t, canvas, cx),
            },
            Kind::Reversed(fx) => match fx.seconds() {
                Some(length) => fx.apply(length - t, canvas, cx),
                None => fx.apply(t, canvas, cx),
            },
            Kind::Limit(length, fx) => fx.apply(t.min(*length), canvas, cx),
            Kind::Eased(easing, fx) => match fx.seconds() {
                Some(length) if length > 0.0 => {
                    fx.apply(easing.at(t / length) * length, canvas, cx);
                }
                _ => fx.apply(t, canvas, cx),
            },
        }
    }
}

/// `all`, one after another: each runs in its turn, and the last holds
/// its end. An endless one never lets the next begin.
pub fn sequence(all: impl IntoIterator<Item = Fx>) -> Fx {
    Fx::of(Kind::Sequence(all.into_iter().collect()))
}

/// `all` at once, each over what the ones before it did; one that ends
/// first holds its end. As long as the longest.
pub fn parallel(all: impl IntoIterator<Item = Fx>) -> Fx {
    Fx::of(Kind::Parallel(all.into_iter().collect()))
}

/// Nothing, for `length`: a pause in a [`sequence`].
pub fn wait(length: Duration) -> Fx {
    Fx::new(length, |_, _, _| {})
}

// Playing.

/// Where a run is: anchored at `anchor` (the app's clock, in seconds) at
/// position `from`, going forwards or backwards.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Run {
    anchor: f64,
    from: f64,
    forward: bool,
}

impl Run {
    fn start(now: f64) -> Run {
        Run {
            anchor: now,
            from: 0.0,
            forward: true,
        }
    }

    /// The position at `now`, within `0..=length`.
    fn at(self, now: f64, length: Option<f64>) -> f64 {
        // Within a microsecond of an end is at it: the clock's seconds
        // do not add up exactly as floats.
        const NEAR: f64 = 1e-6;
        let moved = (now - self.anchor).max(0.0);
        if self.forward {
            let at = self.from + moved;
            length.map_or(at, |l| if at >= l - NEAR { l } else { at })
        } else {
            let at = self.from - moved;
            if at <= NEAR {
                0.0
            } else {
                at
            }
        }
    }

    /// How long until it is over, from position `at`; `None` for ever.
    fn left(self, at: f64, length: Option<f64>) -> Option<f64> {
        if self.forward {
            length.map(|l| (l - at).max(0.0))
        } else {
            Some(at)
        }
    }

    /// Where it rests with no motion: the end it is heading for (an
    /// endless one's start).
    fn still(self, length: Option<f64>) -> f64 {
        if self.forward {
            length.unwrap_or(0.0)
        } else {
            0.0
        }
    }
}

/// Whether frames are wanted for motion: not in accessible or linear mode.
fn moving() -> bool {
    !crate::a11y::text_mode()
}

/// `child` with `fx` over it, at the run `control` keeps: it is called
/// on every draw with the app's clock (in seconds) and the run (`None`:
/// the effect is off), and may read signals.
fn drive(child: Node, fx: Fx, mut control: impl FnMut(f64, &mut Option<Run>) + 'static) -> Node {
    let tick = signal(0u64);
    let pending = Pending::default();
    let length = fx.seconds();
    let mut run = None;
    effect(child, move |canvas, cx| {
        tick.get();
        let now = cx.now();
        control(secs(now), &mut run);
        let Some(run) = run else {
            pending.cancel();
            return;
        };
        if !moving() {
            pending.cancel();
            fx.apply(run.still(length), canvas, cx);
            return;
        }
        let at = run.at(secs(now), length);
        fx.apply(at, canvas, cx);
        match run.left(at, length) {
            Some(left) if left <= 0.0 => pending.cancel(),
            left => {
                // The next frame, or the last one exactly at the end.
                let step = left.map_or(FRAME, |left| FRAME.min(Duration::from_secs_f64(left)));
                pending.wake_at(tick, now + step.max(Duration::from_millis(1)), now);
            }
        }
    })
}

/// The draw a running effect has asked for: when, and its timer.
#[derive(Clone, Default)]
struct Pending(Rc<Cell<Option<(Duration, Timer)>>>);

impl Pending {
    /// Ask for a draw at `at`, unless one is asked for already by then.
    fn wake_at(&self, tick: Signal<u64>, at: Duration, now: Duration) {
        if self
            .0
            .get()
            .is_some_and(|(pending, _)| pending <= at && pending > now)
        {
            return;
        }
        self.cancel();
        let mine = self.clone();
        let timer = after(at - now, move |_| {
            if mine.0.get().is_some_and(|(pending, _)| pending == at) {
                mine.0.set(None);
            }
            tick.update(|t| *t = t.wrapping_add(1));
        });
        self.0.set(Some((at, timer)));
    }

    /// Ask for nothing more: the effect is over, or off.
    fn cancel(&self) {
        if let Some((_, timer)) = self.0.take() {
            timer.cancel();
        }
    }
}

/// `child` with `fx` played over it once, from when it first draws (a
/// node built later, on a screen or a [`switch`]'s page, plays when it
/// shows). Once over, its end stays and no more frames are drawn for it.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::fx;
/// use std::time::Duration;
///
/// let app = App::new(|| fx::play(label("Hello"), fx::coalesce(Duration::from_millis(300))));
/// let mut driver = app.driver(5, 1);
/// driver.settle();
/// // Nothing has gathered yet.
/// assert_eq!(driver.screen().plain()[0], "     ");
/// driver.advance(Duration::from_millis(300));
/// assert_eq!(driver.screen().plain()[0], "Hello");
/// ```
pub fn play(child: Node, fx: Fx) -> Node {
    drive(child, fx, |now, run| {
        if run.is_none() {
            *run = Some(Run::start(now));
        }
    })
}

/// `child` with `fx` played from its start each time `when` turns true
/// (and when it first draws, if `when` holds then): a flash when a value
/// changes, a glitch on an error. Before it first plays the node shows as
/// it is; after, the end of its last run stays. `when` may read signals.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::fx;
/// use std::time::Duration;
///
/// let app = App::new(|| {
///     let gone = signal(false);
///     fx::play_when(label("bye"), fx::dissolve(Duration::from_millis(200)), move || gone.get())
///         .on_key("x", move |_| gone.set(true))
/// });
/// let mut driver = app.driver(3, 1);
/// driver.settle();
/// assert_eq!(driver.screen().plain()[0], "bye");
/// driver.press("x");
/// driver.advance(Duration::from_millis(200));
/// assert_eq!(driver.screen().plain()[0], "   ");
/// ```
pub fn play_when(child: Node, fx: Fx, when: impl Fn() -> bool + 'static) -> Node {
    let mut was = false;
    drive(child, fx, move |now, run| {
        let on = when();
        if on && !was {
            *run = Some(Run::start(now));
        }
        was = on;
    })
}

/// `child` with `fx` played while `when` holds, from its start each time
/// it turns true; the node shows as it is while it does not. A
/// [`skeleton`] over a view while its data loads. `when` may read
/// signals.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::fx;
/// use std::time::Duration;
///
/// let app = App::new(|| {
///     let loading = signal(true);
///     let rows = label("Ada Lovelace\nAlan Turing");
///     fx::play_while(rows, fx::skeleton(Duration::from_secs(1)), move || loading.get())
///         .on_key("l", move |_| loading.set(false))
/// });
/// let mut driver = app.driver(12, 2);
/// driver.settle();
/// // Bars where the words are, the gaps between them kept.
/// let screen = driver.screen();
/// let bar = |x| screen.style(screen.cell(x, 0).style).is_some_and(|s| s.bgcolor().is_some());
/// assert!(bar(0) && !bar(3) && bar(4));
/// assert_eq!(screen.plain()[0].trim(), "");
/// driver.press("l");
/// assert_eq!(driver.screen().plain(), ["Ada Lovelace", "Alan Turing "]);
/// ```
pub fn play_while(child: Node, fx: Fx, when: impl Fn() -> bool + 'static) -> Node {
    let mut was = false;
    drive(child, fx, move |now, run| {
        let on = when();
        if !on {
            *run = None;
        } else if !was {
            *run = Some(Run::start(now));
        }
        was = on;
    })
}

/// `child` shown while `shown` holds, with `fx` played forwards as it
/// shows and backwards as it goes: hidden ([`Node::shown_when`]) only
/// once the effect is back at its start. A change half way turns round
/// from where it is. Give it an effect that ends ([`fade_in`],
/// [`slide_in`], [`coalesce`], [`sweep`]). Call it while the app is built
/// (it [watches](crate::watch) `shown`), which may read signals.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::fx;
/// use std::time::Duration;
///
/// let ms = Duration::from_millis;
/// let app = App::new(move || {
///     let open = signal(true);
///     column([
///         fx::transition(label("panel"), fx::coalesce(ms(200)), move || open.get()).fixed(1),
///         label("below"),
///     ])
///     .on_key("x", move |_| open.update(|o| *o = !*o))
/// });
/// let mut driver = app.driver(5, 2);
/// driver.advance(ms(200));
/// assert_eq!(driver.screen().plain(), ["panel", "below"]);
/// driver.press("x");
/// // Still there while it scatters…
/// assert_eq!(driver.screen().plain()[1], "below");
/// driver.advance(ms(200));
/// // …and gone once it has.
/// assert_eq!(driver.screen().plain()[0], "below");
/// ```
pub fn transition(child: Node, fx: Fx, shown: impl Fn() -> bool + 'static) -> Node {
    let length = fx.seconds();
    let state: Rc<Cell<Option<Run>>> = Rc::default();
    let leaving = signal(false);
    let redraw = signal(0u64);
    let generation = Rc::new(Cell::new(0u64));
    let shown = Rc::new(shown);
    let watched = shown.clone();
    let shared = state.clone();
    watch(
        move || watched(),
        move |on, cx| {
            let now = secs(cx.now());
            let at = shared.get().map_or(0.0, |run| run.at(now, length));
            shared.set(Some(Run {
                anchor: now,
                from: at,
                forward: on,
            }));
            generation.set(generation.get() + 1);
            if on {
                leaving.set(false);
            } else if at > 0.0 && moving() {
                // Shown until it is back at its start.
                leaving.set(true);
                let (mine, generation) = (generation.get(), generation.clone());
                cx.after(Duration::from_secs_f64(at), move |_| {
                    if generation.get() == mine {
                        leaving.set(false);
                    }
                });
            } else {
                leaving.set(false);
            }
            redraw.update(|r| *r = r.wrapping_add(1));
        },
    );
    drive(child, fx, move |_, run| {
        redraw.get();
        *run = state.get();
    })
    .shown_when(move || shown() || leaving.get())
}

/// A splash screen: `cover` (a logo, an `image` with the `art` feature, big
/// text) fades in, stays `hold`, and fades out, then `app` fades in in its
/// place. Enter or Esc skips to the app, unless the app takes the key.
/// Call it while the app is built.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::fx;
/// use std::time::Duration;
///
/// let app = App::new(|| {
///     fx::splash(label("ACME"), Duration::from_secs(1), label("ready"))
/// });
/// let mut driver = app.driver(5, 1);
/// driver.advance(Duration::from_millis(1500));
/// assert_eq!(driver.screen().plain()[0], "ACME ");
/// driver.advance(Duration::from_secs(1));
/// assert_eq!(driver.screen().plain()[0], "ready");
/// ```
pub fn splash(cover: Node, hold: Duration, app: Node) -> Node {
    let fade = Duration::from_millis(400);
    let done = signal(false);
    after(hold + fade * 2, move |_| done.set(true));
    let cover = play(cover, sequence([fade_in(fade), fade_out(fade).delay(hold)]));
    let app = play(app, fade_in(fade));
    let pages = RefCell::new([Some(cover), Some(app)]);
    let skip = move |cx: &mut crate::Ctx| {
        if done.get_untracked() {
            cx.pass_key();
        }
        done.set(true);
    };
    switch(
        move || done.get(),
        move |done| {
            pages.borrow_mut()[usize::from(done)]
                .take()
                .unwrap_or_else(|| label(""))
        },
    )
    .on_key("enter", skip)
    .on_key("esc", skip)
}

/// A placeholder while something loads: a bar for each of `widths` (in
/// cells), one a row, pulsing as a [`skeleton`]. Read as "Loading" by
/// assistive technology.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::fx;
///
/// let app = App::new(|| fx::placeholder([6, 4]));
/// let mut driver = app.driver(8, 2);
/// driver.settle();
/// let style = driver.screen().cell(0, 0).style;
/// assert!(driver.screen().style(style).is_some_and(|s| s.bgcolor().is_some()));
/// ```
pub fn placeholder(widths: impl IntoIterator<Item = u16>) -> Node {
    let rows: Vec<Node> = widths
        .into_iter()
        .map(|width| label("x".repeat(usize::from(width))).fixed(1))
        .collect();
    play(
        column(rows).access_hidden(true),
        skeleton(Duration::from_millis(1200)),
    )
    .role(crate::a11y::Role::Status)
    .label("Loading")
    .busy_when(|| true)
}

// Cells and colours.

/// One cell of the canvas, read.
struct Read {
    symbol: String,
    style: Option<Style>,
    width: u16,
}

fn read(canvas: &Canvas, x: u16, y: u16) -> Option<Read> {
    canvas.cell(x, y).map(|cell| Read {
        symbol: cell.symbol().to_string(),
        style: cell.style().cloned(),
        width: cell.width(),
    })
}

/// The text and background colours assumed (the `fx` style's).
#[derive(Clone, Copy)]
struct Colours {
    fg: ColorTriplet,
    bg: ColorTriplet,
}

fn colours(cx: &DrawCx) -> Colours {
    let style = cx.style("fx", "");
    Colours {
        fg: triplet(style.color()).unwrap_or(ColorTriplet::new(192, 192, 192)),
        bg: triplet(style.bgcolor()).unwrap_or(ColorTriplet::new(0, 0, 0)),
    }
}

/// A theme style's colour, or `fallback`.
fn theme_colour(cx: &DrawCx, name: &str, background: bool, fallback: ColorTriplet) -> ColorTriplet {
    let style = cx.style(name, "");
    triplet(if background {
        style.bgcolor()
    } else {
        style.color()
    })
    .unwrap_or(fallback)
}

fn triplet(colour: Option<&Color>) -> Option<ColorTriplet> {
    colour.and_then(Color::get_truecolor)
}

fn colour(c: ColorTriplet) -> Color {
    Color::from_rgb(c.red, c.green, c.blue)
}

/// `t` (0 to 1) of the way from `a` to `b`.
fn mix(a: ColorTriplet, b: ColorTriplet, t: f64) -> ColorTriplet {
    let t = t.clamp(0.0, 1.0);
    let m = |p: u8, q: u8| (f64::from(p) + (f64::from(q) - f64::from(p)) * t).round() as u8;
    ColorTriplet::new(m(a.red, b.red), m(a.green, b.green), m(a.blue, b.blue))
}

/// Every cell (the first column of a wide one) of the canvas recoloured:
/// `f` gets its column, row, text colour (the assumed one where it has
/// none) and background (if it has one) and gives the new ones (`None`
/// leaves that one as it was). Spaces with no background are left.
fn recolour(
    canvas: &mut Canvas,
    cx: &DrawCx,
    mut f: impl FnMut(
        u16,
        u16,
        ColorTriplet,
        Option<ColorTriplet>,
    ) -> (Option<ColorTriplet>, Option<ColorTriplet>),
) {
    let assumed = colours(cx).fg;
    for y in 0..canvas.height() {
        for x in 0..canvas.width() {
            let Some(cell) = read(canvas, x, y) else {
                continue;
            };
            if cell.width == 0 {
                continue;
            }
            let style = cell.style.unwrap_or_default();
            let bg = triplet(style.bgcolor());
            if bg.is_none() && cell.symbol.trim().is_empty() {
                continue;
            }
            let fg = triplet(style.color()).unwrap_or(assumed);
            let (new_fg, new_bg) = f(x, y, fg, bg);
            if new_fg.is_none() && new_bg.is_none() {
                continue;
            }
            let over = Style::from_color(new_fg.map(colour), new_bg.map(colour));
            canvas.set_cell(x, y, &cell.symbol, Some(&style.combine(&over)));
        }
    }
}

/// The cell at `x`, `y` blanked, keeping its background (both columns of
/// a wide one).
fn blank(canvas: &mut Canvas, x: u16, y: u16) {
    let Some(cell) = read(canvas, x, y) else {
        return;
    };
    if cell.width == 0 {
        return;
    }
    let bg = cell.style.as_ref().and_then(|s| s.bgcolor().cloned());
    let style = bg.map(|bg| Style::from_color(None, Some(bg)));
    let spaces = " ".repeat(usize::from(cell.width.max(1)));
    canvas.print(x, y, &spaces, style.as_ref());
}

/// A number from 0 to 1 for `x`, `y` and `seed`, the same each time.
fn noise(x: u32, y: u32, seed: u64) -> f64 {
    let mut z = seed
        ^ (u64::from(x)).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (u64::from(y)).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    (z >> 11) as f64 / (1u64 << 53) as f64
}

// The presets.

/// Fade in from the background colour (the `fx` style's background):
/// each cell's text and background move from it to their own.
pub fn fade_in(length: Duration) -> Fx {
    Fx::new(length, |frame, canvas, cx| {
        fade(frame.progress, canvas, cx);
    })
}

/// Fade out to the background colour; at the end the cells are blank, so
/// nothing shows (in accessible mode too).
pub fn fade_out(length: Duration) -> Fx {
    Fx::new(length, |frame, canvas, cx| {
        if frame.progress >= 1.0 {
            for y in 0..canvas.height() {
                for x in 0..canvas.width() {
                    blank(canvas, x, y);
                }
            }
            return;
        }
        fade(1.0 - frame.progress, canvas, cx);
    })
}

/// Every cell `shown` (0 to 1) of the way from the background colour to
/// its own.
fn fade(shown: f64, canvas: &mut Canvas, cx: &DrawCx) {
    if shown >= 1.0 {
        return;
    }
    let to = colours(cx).bg;
    recolour(canvas, cx, |_, _, fg, bg| {
        (Some(mix(to, fg, shown)), bg.map(|bg| mix(to, bg, shown)))
    });
}

/// The cells go, one by one in a scattered order, leaving their
/// background.
pub fn dissolve(length: Duration) -> Fx {
    Fx::new(length, |frame, canvas, _cx| {
        scatter(canvas, |n| n < frame.progress);
    })
    .easing(Easing::Linear)
}

/// The cells come, one by one in a scattered order: [`dissolve`] the
/// other way round.
pub fn coalesce(length: Duration) -> Fx {
    Fx::new(length, |frame, canvas, _cx| {
        scatter(canvas, |n| n >= frame.progress);
    })
    .easing(Easing::Linear)
}

/// Blank the cells whose noise `gone` says.
fn scatter(canvas: &mut Canvas, gone: impl Fn(f64) -> bool) {
    for y in 0..canvas.height() {
        for x in 0..canvas.width() {
            if gone(noise(u32::from(x), u32::from(y), 0x5EED)) {
                blank(canvas, x, y);
            }
        }
    }
}

/// How far, along `direction`, the cell at `x`, `y` is from the edge it
/// starts at, and how far that edge is from the other.
fn along(direction: Direction, x: u16, y: u16, width: u16, height: u16) -> (f64, f64) {
    let (at, extent) = match direction {
        Direction::Right => (x, width),
        Direction::Left => (width.saturating_sub(1).saturating_sub(x), width),
        Direction::Down => (y, height),
        Direction::Up => (height.saturating_sub(1).saturating_sub(y), height),
    };
    (f64::from(at), f64::from(extent))
}

/// The cells show as an edge moving toward `direction` passes them (from
/// the left edge for [`Direction::Right`]), with a soft edge four cells
/// wide where they fade in. [`reversed`](Fx::reversed), they go.
pub fn sweep(direction: Direction, length: Duration) -> Fx {
    const SOFT: f64 = 4.0;
    Fx::new(length, move |frame, canvas, cx| {
        if frame.progress >= 1.0 {
            return;
        }
        let (width, height) = (canvas.width(), canvas.height());
        let extent = along(direction, 0, 0, width, height).1;
        let front = frame.progress * (extent + SOFT);
        let to = colours(cx).bg;
        // How much of the cell at `at` shows.
        let shown = |at: f64| ((front - at) / SOFT).clamp(0.0, 1.0);
        for y in 0..height {
            for x in 0..width {
                if shown(along(direction, x, y, width, height).0) <= 0.0 {
                    blank(canvas, x, y);
                }
            }
        }
        recolour(canvas, cx, |x, y, fg, bg| {
            let shown = shown(along(direction, x, y, width, height).0);
            if shown >= 1.0 {
                return (None, None);
            }
            (Some(mix(to, fg, shown)), bg.map(|bg| mix(to, bg, shown)))
        });
    })
}

/// The node slides in toward `direction` (from the left edge for
/// [`Direction::Right`]), coming to rest where it belongs; the cells it
/// has left are blank.
pub fn slide_in(direction: Direction, length: Duration) -> Fx {
    Fx::new(length, move |frame, canvas, _cx| {
        if frame.progress >= 1.0 {
            return;
        }
        let (width, height) = (canvas.width(), canvas.height());
        let (dx, dy, extent) = match direction {
            Direction::Right => (1, 0, width),
            Direction::Left => (-1, 0, width),
            Direction::Down => (0, 1, height),
            Direction::Up => (0, -1, height),
        };
        let offset = ((1.0 - frame.progress) * f64::from(extent)).round() as i32;
        let cells: Vec<Vec<Option<Read>>> = (0..height)
            .map(|y| (0..width).map(|x| read(canvas, x, y)).collect())
            .collect();
        for y in 0..height {
            let mut covered = 0u16;
            for x in 0..width {
                let (sx, sy) = (i32::from(x) + dx * offset, i32::from(y) + dy * offset);
                let from = (sx >= 0 && sy >= 0)
                    .then(|| cells.get(sy as usize)?.get(sx as usize)?.as_ref())
                    .flatten();
                if covered > 0 {
                    // The second column of a wide grapheme just written.
                    covered -= 1;
                    continue;
                }
                match from {
                    Some(cell) if cell.width > 0 => {
                        canvas.set_cell(x, y, &cell.symbol, cell.style.as_ref());
                        covered = cell.width.saturating_sub(1);
                    }
                    _ => canvas.set_cell(x, y, " ", None),
                }
            }
        }
    })
}

/// The colours shift in hue by `hue` degrees, and in saturation and
/// lightness by `saturation` and `lightness` (each -1 to 1), and stay so.
/// [`ping_pong`](Fx::ping_pong) brings them back.
pub fn hsl_shift(hue: f64, saturation: f64, lightness: f64, length: Duration) -> Fx {
    Fx::new(length, move |frame, canvas, cx| {
        let p = frame.progress;
        if p <= 0.0 {
            return;
        }
        let shift = |c: ColorTriplet| {
            let (h, s, l) = to_hsl(c);
            from_hsl(h + hue * p, s + saturation * p, l + lightness * p)
        };
        recolour(canvas, cx, |_, _, fg, bg| (Some(shift(fg)), bg.map(shift)));
    })
}

/// Hue (degrees), saturation and lightness (0 to 1) of a colour.
fn to_hsl(c: ColorTriplet) -> (f64, f64, f64) {
    let (r, g, b) = (
        f64::from(c.red) / 255.0,
        f64::from(c.green) / 255.0,
        f64::from(c.blue) / 255.0,
    );
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let l = (max + min) / 2.0;
    let d = max - min;
    if d <= f64::EPSILON {
        return (0.0, 0.0, l);
    }
    let s = d / (1.0 - (2.0 * l - 1.0).abs());
    let h = if max == r {
        60.0 * ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    (h, s, l)
}

fn from_hsl(h: f64, s: f64, l: f64) -> ColorTriplet {
    rich_ext::palette::hsl(h, s, l)
        .get_truecolor()
        .unwrap_or(ColorTriplet::new(0, 0, 0))
}

/// Glyphs a glitch puts in.
const GLITCH: &[&str] = &[
    "▓", "▒", "░", "█", "▚", "▞", "#", "%", "&", "@", "$", "/", "\\",
];

/// A glitch: for its length, a few cells at a time turn to noise glyphs
/// and rows jump a cell or two sideways, changing every 60 ms, worst half
/// way; at the end the node is as it was.
pub fn glitch(length: Duration) -> Fx {
    Fx::new(length, |frame, canvas, cx| {
        if frame.progress >= 1.0 {
            return;
        }
        let step = (frame.elapsed.as_millis() / 60) as u64;
        let rate = 0.08 * (0.25 + 0.75 * (PI * frame.progress).sin());
        let (width, height) = (canvas.width(), canvas.height());
        // Rows that jump.
        for y in 0..height {
            let n = noise(u32::from(y), 7, step);
            if n >= rate * 1.5 {
                continue;
            }
            let shift = if n < rate * 0.75 { 1 } else { 2 };
            let row: Vec<Option<Read>> = (0..width).map(|x| read(canvas, x, y)).collect();
            for x in (0..width).rev() {
                let from = usize::from(x)
                    .checked_sub(shift)
                    .and_then(|i| row[i].as_ref());
                match from {
                    Some(cell) if cell.width == 1 => {
                        canvas.set_cell(x, y, &cell.symbol, cell.style.as_ref())
                    }
                    _ => canvas.set_cell(x, y, " ", None),
                }
            }
        }
        // Cells that turn to noise, in a colour shifted round the wheel.
        let assumed = colours(cx).fg;
        for y in 0..height {
            for x in 0..width {
                let n = noise(u32::from(x), u32::from(y), step ^ 0x61);
                if n >= rate {
                    continue;
                }
                let Some(cell) = read(canvas, x, y) else {
                    continue;
                };
                if cell.width != 1 {
                    continue;
                }
                let glyph = GLITCH[(n / rate * GLITCH.len() as f64) as usize % GLITCH.len()];
                let fg = cell
                    .style
                    .as_ref()
                    .and_then(|s| triplet(s.color()))
                    .unwrap_or(assumed);
                let (h, s, l) = to_hsl(fg);
                let shifted = from_hsl(h + 180.0, s.max(0.6), l.clamp(0.4, 0.7));
                let style = cell
                    .style
                    .unwrap_or_default()
                    .combine(&Style::from_color(Some(colour(shifted)), None));
                canvas.set_cell(x, y, glyph, Some(&style));
            }
        }
    })
}

/// Glyphs the rain falls in: half-width katakana and digits, a cell each.
const RAIN: &[char] = &[
    'ｱ', 'ｲ', 'ｳ', 'ｴ', 'ｵ', 'ｶ', 'ｷ', 'ｸ', 'ｹ', 'ｺ', 'ｻ', 'ｼ', 'ｽ', 'ｾ', 'ｿ', 'ﾀ', 'ﾁ', 'ﾂ', 'ﾃ',
    'ﾄ', 'ﾅ', 'ﾆ', 'ﾇ', 'ﾈ', 'ﾉ', '0', '1', '2', '3', '4', '5', '7', '8', '9', 'Z', ':', '=',
];

/// Digital rain, as in the Matrix, for ever: streaks of glyphs fall down
/// each column at their own speed over the node, a bright head and a
/// trail fading to the background, in the `fx.rain` style's colour (a
/// bright green). At its start nothing has fallen yet; [`limit`](Fx::limit)
/// it to end it.
pub fn rain() -> Fx {
    Fx::endless(|frame, canvas, cx| {
        let t = frame.elapsed.as_secs_f64();
        if t <= 0.0 {
            return;
        }
        let (width, height) = (canvas.width(), canvas.height());
        let green = theme_colour(cx, "fx.rain", false, ColorTriplet::new(0, 255, 65));
        let bg = colours(cx).bg;
        let head = mix(green, ColorTriplet::new(255, 255, 255), 0.7);
        let step = (t * 12.0) as u32;
        for x in 0..width {
            let column = u32::from(x);
            let trail = 4.0 + (noise(column, 2, 0xA1) * 10.0).floor();
            let cycle = f64::from(height) + trail;
            let speed = 6.0 + noise(column, 0, 0xA1) * 18.0;
            let travelled = t * speed - noise(column, 1, 0xA1) * cycle * 1.5;
            if travelled < 0.0 {
                continue;
            }
            let lead = (travelled % cycle).floor() as i32;
            for back in 0..trail as i32 {
                let y = lead - back;
                if y < 0 || y >= i32::from(height) {
                    continue;
                }
                let glyph =
                    RAIN[(noise(column, y as u32, u64::from(step)) * RAIN.len() as f64) as usize];
                let fg = if back == 0 {
                    head
                } else {
                    mix(green, bg, f64::from(back) / trail)
                };
                let style = Style::from_color(Some(colour(fg)), None);
                let mut buffer = [0u8; 4];
                // Over a wide grapheme, the whole of it.
                if read(canvas, x, y as u16).is_some_and(|c| c.width != 1) {
                    blank(canvas, x, y as u16);
                }
                canvas.set_cell(x, y as u16, glyph.encode_utf8(&mut buffer), Some(&style));
            }
        }
    })
}

/// A shimmer over text, for ever: a band of light (the `fx.shimmer`
/// style's colour, white unless set) crosses it on the slant every
/// `period`. At its start the band is off to the left.
pub fn shimmer(period: Duration) -> Fx {
    const BAND: f64 = 6.0;
    let period = secs(period).max(0.001);
    Fx::endless(move |frame, canvas, cx| {
        let t = frame.elapsed.as_secs_f64();
        let light = theme_colour(cx, "fx.shimmer", false, ColorTriplet::new(255, 255, 255));
        let (width, height) = (f64::from(canvas.width()), f64::from(canvas.height()));
        let through = (t % period) / period;
        let centre = -BAND + through * (width + height + 2.0 * BAND);
        if t <= 0.0 {
            return;
        }
        recolour(canvas, cx, |x, y, fg, _| {
            let distance = (f64::from(x) + f64::from(y) - centre).abs();
            let amount = 1.0 - distance / BAND;
            if amount <= 0.0 {
                return (None, None);
            }
            (Some(mix(fg, light, amount * 0.85)), None)
        });
    })
}

/// A skeleton, for ever: what the node shows becomes bars (a space's
/// gap kept between words), pulsing from dim to bright and back every
/// `period` (the `fx.skeleton` style's colour and background, greys unless
/// set). Over a view whose data is loading, with [`play_while`]; or
/// [`placeholder`] bars of their own.
pub fn skeleton(period: Duration) -> Fx {
    let period = secs(period).max(0.001);
    Fx::endless(move |frame, canvas, cx| {
        let dim = theme_colour(cx, "fx.skeleton", false, ColorTriplet::new(48, 48, 48));
        let bright = theme_colour(cx, "fx.skeleton", true, ColorTriplet::new(88, 88, 88));
        let t = frame.elapsed.as_secs_f64();
        let pulse = (1.0 - (2.0 * PI * t / period).cos()) / 2.0;
        let bar = Style::from_color(None, Some(colour(mix(dim, bright, pulse))));
        for y in 0..canvas.height() {
            for x in 0..canvas.width() {
                let Some(cell) = read(canvas, x, y) else {
                    continue;
                };
                if cell.width == 0 || cell.symbol.trim().is_empty() {
                    continue;
                }
                let spaces = " ".repeat(usize::from(cell.width));
                canvas.print(x, y, &spaces, Some(&bar));
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lengths_add_up() {
        let ms = Duration::from_millis;
        let a = fade_in(ms(100));
        assert_eq!(sequence([a.clone(), a.clone()]).length(), Some(ms(200)));
        assert_eq!(
            parallel([a.clone(), a.clone().delay(ms(50))]).length(),
            Some(ms(150))
        );
        assert_eq!(a.clone().hold(ms(30)).length(), Some(ms(130)));
        assert_eq!(a.clone().repeat(3).length(), Some(ms(300)));
        assert_eq!(a.clone().forever().length(), None);
        assert_eq!(a.clone().ping_pong().length(), Some(ms(200)));
        assert_eq!(rain().limit(ms(5)).length(), Some(ms(5)));
        assert_eq!(sequence([rain(), a]).length(), None);
        assert_eq!(sequence([]).length(), Some(Duration::ZERO));
    }

    #[test]
    fn a_run_turns_round_where_it_is() {
        let run = Run::start(1.0);
        assert_eq!(run.at(1.5, Some(2.0)), 0.5);
        assert_eq!(run.at(9.0, Some(2.0)), 2.0);
        assert_eq!(run.left(0.5, Some(2.0)), Some(1.5));
        let back = Run {
            anchor: 1.5,
            from: 0.5,
            forward: false,
        };
        assert_eq!(back.at(1.75, Some(2.0)), 0.25);
        assert_eq!(back.at(3.0, Some(2.0)), 0.0);
        assert_eq!(back.still(Some(2.0)), 0.0);
        assert_eq!(run.still(None), 0.0);
    }

    #[test]
    fn colours_go_round_hsl_and_back() {
        for c in [
            ColorTriplet::new(255, 0, 0),
            ColorTriplet::new(12, 200, 90),
            ColorTriplet::new(40, 40, 160),
            ColorTriplet::new(128, 128, 128),
        ] {
            let (h, s, l) = to_hsl(c);
            assert_eq!(from_hsl(h, s, l), c, "{c:?}");
        }
    }

    #[test]
    fn noise_is_spread_and_repeatable() {
        let values: Vec<f64> = (0..1000).map(|i| noise(i, 3, 1)).collect();
        assert!(values.iter().all(|v| (0.0..1.0).contains(v)));
        let low = values.iter().filter(|v| **v < 0.5).count();
        assert!((400..600).contains(&low), "{low}");
        assert_eq!(noise(5, 6, 7), noise(5, 6, 7));
    }
}
