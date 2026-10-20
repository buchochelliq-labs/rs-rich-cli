//! A spinner ([`spinner`]) and a node shown as loading
//! ([`Node::loading_when`]).
//!
//! A spinner turns on the app's clock: while it spins it asks for one
//! frame when its glyph is next due (an [`after`](crate::after) timer), and
//! while it does not (stopped, or not shown) nothing wakes the app for it.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use rich::Style;

use crate::node::{Axis, Node};
use crate::reactive::{signal, Signal};
use crate::screen::Rect;
use crate::widget::{Canvas, DrawCx, MeasureCx, Widget};

type Condition = Rc<dyn Fn() -> bool>;

/// A spinner's frames, turned on the app's clock.
struct Turn {
    frames: &'static [&'static str],
    interval: Duration,
    /// Written when the next frame is due: reading it draws again then.
    tick: Signal<u64>,
    /// A timer for the next frame is set.
    waiting: Rc<Cell<bool>>,
}

impl Turn {
    /// Spinner `name`'s frames (rich's spinners: `dots`, `line`, `arc`, …;
    /// `dots` for a name it does not know).
    fn new(name: &str) -> Turn {
        let (interval, frames) = rich::spinner::spinner_frames(name)
            .or_else(|| rich::spinner::spinner_frames("dots"))
            .expect("the dots spinner exists");
        Turn {
            frames,
            interval: Duration::from_secs_f64(interval.max(1.0) / 1000.0),
            tick: signal(0),
            waiting: Rc::new(Cell::new(false)),
        }
    }

    /// The widest frame's cells.
    fn width(&self) -> u16 {
        self.frames
            .iter()
            .map(|f| rich::cells::cell_len(f))
            .max()
            .unwrap_or(1)
            .min(u16::MAX as usize) as u16
    }

    /// The frame to draw now, and a frame asked for when the next is due.
    /// In text mode a still `*`: a glyph that keeps changing is read out
    /// again and again.
    fn frame(&self, cx: &DrawCx) -> &'static str {
        if crate::a11y::text_mode() {
            return "*";
        }
        // Reading it draws the spinner again when the timer writes it.
        self.tick.get();
        let now = cx.now();
        let interval = self.interval.as_nanos().max(1);
        let at = (now.as_nanos() / interval) as usize % self.frames.len().max(1);
        if !self.waiting.get() {
            self.waiting.set(true);
            let due = Duration::from_nanos((interval - now.as_nanos() % interval) as u64);
            let (tick, waiting) = (self.tick, self.waiting.clone());
            crate::app::after(due, move |_| {
                waiting.set(false);
                tick.update(|t| *t = t.wrapping_add(1));
            });
        }
        self.frames.get(at).copied().unwrap_or("")
    }
}

/// What [`spinner`] makes.
struct Spinner {
    turn: Turn,
    spinning: Option<Condition>,
}

/// A spinner: rich's spinner `name` (`dots`, `line`, `arc`, `bouncingBar`,
/// … as [`rich::spinner::spinner_names`] lists them; `dots` for a name it
/// does not know), turning at its own speed on the app's clock, in the
/// theme's `spinner` style (else `accent`). It draws again only when its
/// next frame is due, and only while it spins: always, or while
/// [`spinning_when`](Node::spinning_when)'s condition holds (blank
/// otherwise). Its glyph is decoration, [hidden](Node::access_hidden) from
/// assistive technology: say what is happening in text beside it, or mark
/// what is loading [busy](Node::busy_when). In text mode it is a still
/// `*`.
///
/// ```
/// use std::time::Duration;
///
/// use intuituive::prelude::*;
/// use intuituive::spinner;
///
/// let app = App::new(|| row([spinner("line").fixed(2), label("Loading")]));
/// let mut driver = app.driver(10, 1);
/// driver.settle();
/// assert_eq!(driver.screen().plain()[0].trim_end(), "- Loading");
/// // `line` turns every 130 ms.
/// driver.advance(Duration::from_millis(130));
/// assert_eq!(driver.screen().plain()[0].trim_end(), "\\ Loading");
/// ```
pub fn spinner(name: &str) -> Node {
    crate::widget::widget(Spinner {
        turn: Turn::new(name),
        spinning: None,
    })
    .access_hidden(true)
    .auto()
}

impl Node {
    /// For a [`spinner`]: spin only while `condition` holds, blank (and
    /// asking for no frames) while it does not. `condition` may read
    /// signals. Ignored on other nodes.
    ///
    /// ```
    /// use std::time::Duration;
    ///
    /// use intuituive::prelude::*;
    /// use intuituive::spinner;
    ///
    /// let app = App::new(|| {
    ///     let busy = signal(false);
    ///     spinner("line")
    ///         .spinning_when(move || busy.get())
    ///         .on_key("b", move |_| busy.set(true))
    /// });
    /// let mut driver = app.driver(3, 1);
    /// driver.settle();
    /// assert_eq!(driver.screen().plain()[0], "   ");
    /// driver.press("b");
    /// assert_eq!(driver.screen().plain()[0], "-  ");
    /// ```
    pub fn spinning_when(self, condition: impl Fn() -> bool + 'static) -> Node {
        let condition: Condition = Rc::new(condition);
        self.widget_mut(|spinner: &mut Spinner| spinner.spinning = Some(condition));
        self
    }

    /// While `loading` holds, show this node as loading: dimmed (the
    /// theme's `loading` style, else `dim`) under a spinner in its middle,
    /// and [busy](Self::busy_when) for assistive technology, which hears
    /// that it is. It still takes keys and clicks; add
    /// [`disabled_when`](Self::disabled_when) with the same condition to
    /// stop them. `loading` may read signals.
    ///
    /// ```
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| {
    ///     let loading = signal(true);
    ///     list(|| vec!["one".into(), "two".into(), "three".into()], signal(0))
    ///         .label("Files")
    ///         .loading_when(move || loading.get())
    /// });
    /// let mut driver = app.driver(5, 3);
    /// driver.settle();
    /// // The spinner's first frame in the middle, over the dimmed list.
    /// assert_eq!(driver.screen().plain()[1], "t ⠋  ");
    /// assert!(driver.accessibility()[0].state.busy);
    /// ```
    pub fn loading_when(self, loading: impl Fn() -> bool + 'static) -> Node {
        let loading: Condition = Rc::new(loading);
        let busy = loading.clone();
        // Busy is the node's own state, read with its role and name.
        self.busy_when(move || busy()).wrap_in(
            |child| {
                Box::new(Loading {
                    child,
                    loading,
                    turn: Turn::new("dots"),
                })
            },
            "loading",
        )
    }
}

impl Widget for Spinner {
    fn name(&self) -> &'static str {
        "spinner"
    }

    fn measure(&mut self, _cx: &MeasureCx, axis: Axis, _width: u16, _height: u16) -> u16 {
        match axis {
            Axis::Horizontal => self.turn.width(),
            Axis::Vertical => 1,
        }
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        if self.spinning.as_ref().is_some_and(|spinning| !spinning()) {
            return;
        }
        let frame = self.turn.frame(cx);
        let style = spinner_style(cx);
        canvas.print(0, 0, frame, Some(&style));
    }
}

fn spinner_style(cx: &DrawCx) -> Style {
    crate::controls::look(cx, &["spinner", "accent"], "")
}

/// What [`Node::loading_when`] makes.
struct Loading {
    child: Node,
    loading: Condition,
    turn: Turn,
}

impl Widget for Loading {
    fn name(&self) -> &'static str {
        "loading"
    }

    fn measure(&mut self, cx: &MeasureCx, axis: Axis, width: u16, height: u16) -> u16 {
        cx.measure(&self.child, axis, width, height)
    }

    fn children(&self) -> &[Node] {
        std::slice::from_ref(&self.child)
    }

    fn layout(&mut self, _cx: &MeasureCx, rect: Rect) -> Vec<Rect> {
        vec![rect]
    }

    fn draw(&mut self, _cx: &mut DrawCx, _canvas: &mut Canvas) {}

    fn draw_over(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        if !(self.loading)() {
            return;
        }
        let (width, height) = (canvas.width(), canvas.height());
        canvas.restyle(0, 0, width, height, &cx.style("loading", "dim"));
        let frame = self.turn.frame(cx);
        let cells = rich::cells::cell_len(frame) as u16;
        let x = width.saturating_sub(cells) / 2;
        let y = height / 2;
        // A space either side, so it stands out from what it is over.
        canvas.print(x.saturating_sub(1), y, " ", None);
        canvas.print(x + cells, y, " ", None);
        canvas.print(x, y, frame, Some(&spinner_style(cx)));
    }
}
