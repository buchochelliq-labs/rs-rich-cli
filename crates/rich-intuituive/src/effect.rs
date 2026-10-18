//! Drawing over what a node shows ([`effect`]), and filling an area with a
//! glyph ([`fill`]).
//!
//! An effect is a node round another, whose closure runs after
//! everything inside it has drawn, on its canvas: it reads cells with
//! [`Canvas::cell`] and writes them with [`Canvas::set_cell`] and the
//! rest. The closure may read signals, so a timer ([`every`](crate::every))
//! or an animation ([`Ctx::animate`](crate::Ctx::animate)) drives it:
//! ratatui's demo2 "destroy" is an effect round the whole app, reading a
//! phase signal.

use rich::Style;

use crate::node::{Axis, Node};
use crate::screen::Rect;
use crate::widget::{Canvas, DrawCx, MeasureCx, Widget};

type Draw = Box<dyn FnMut(&mut Canvas, &mut DrawCx)>;

/// A node that shows `child`, then runs `f` over what it drew: a
/// highlight, a badge, a transition. `f` gets the canvas, which holds
/// what `child` drew (read it with [`Canvas::cell`]), and the drawing
/// context, as [`Widget::draw_over`] does. It runs whenever the effect or
/// anything inside it drew, and always over what `child` drew afresh, so
/// an effect that moves cells or changes them by what they are comes out
/// the same each time. Signals `f` reads draw the whole effect again when
/// they change.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::rich::Style;
///
/// let app = App::new(|| {
///     // Every other cell reversed: a stripe over a label.
///     effect(label("striped"), |canvas, _cx| {
///         let reverse = Style::parse("reverse").unwrap();
///         for x in (0..canvas.width()).step_by(2) {
///             canvas.restyle(x, 0, 1, 1, &reverse);
///         }
///     })
/// });
/// let mut driver = app.driver(7, 1);
/// driver.settle();
/// let reversed = |x| driver.screen().style(driver.screen().cell(x, 0).style).is_some();
/// assert!(reversed(0) && !reversed(1) && reversed(2));
/// ```
pub fn effect(child: Node, f: impl FnMut(&mut Canvas, &mut DrawCx) + 'static) -> Node {
    child.wrap_in(
        |child| {
            Box::new(Effect {
                child,
                draw: Box::new(f),
                wrote: false,
            })
        },
        "effect",
    )
}

/// What [`effect`] makes.
pub(crate) struct Effect {
    child: Node,
    draw: Draw,
    /// Whether the closure wrote any cells the last time it ran: when it
    /// did not, what is inside need not draw again before the next run.
    pub(crate) wrote: bool,
}

impl Widget for Effect {
    fn name(&self) -> &'static str {
        "effect"
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
        (self.draw)(canvas, cx);
    }
}

/// A node that fills its area with `symbol` (one grapheme; a wide one is
/// fine) in `style` (a style or a theme style's name): ratatui's `Fill`. It
/// takes all the room it is given.
///
/// ```
/// use intuituive::prelude::*;
///
/// let app = App::new(|| fill("·", "bright_black"));
/// let mut driver = app.driver(4, 2);
/// driver.settle();
/// assert_eq!(driver.screen().plain(), ["····", "····"]);
/// ```
pub fn fill(symbol: impl Into<String>, style: &str) -> Node {
    Node::from_widget(
        Box::new(Fill {
            symbol: symbol.into(),
            style: style.to_string(),
        }),
        "fill",
    )
    .access_hidden(true)
}

/// What [`fill`] makes.
struct Fill {
    symbol: String,
    style: String,
}

impl Widget for Fill {
    fn name(&self) -> &'static str {
        "fill"
    }

    fn describe(&self) -> Option<String> {
        Some(format!("{:?}", self.symbol))
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        let style: Option<Style> = (!self.style.is_empty()).then(|| cx.style(&self.style, ""));
        let (width, height) = (canvas.width(), canvas.height());
        let symbol = if crate::a11y::text_mode() {
            " "
        } else {
            &self.symbol
        };
        canvas.fill_with(0, 0, width, height, symbol, style.as_ref());
    }
}
