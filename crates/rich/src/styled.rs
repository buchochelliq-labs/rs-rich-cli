//! Applying a style across a whole renderable.
//!
//! Port of `rich/styled.py`. [`Styled`] renders its child and lays `style`
//! underneath every segment (each segment's own style still wins on top), so a
//! single style can tint an entire composite renderable.

use crate::console::{Console, ConsoleOptions};
use crate::measure::Measurement;
use crate::protocol::Renderable;
use crate::segment::Segment;
use crate::style::Style;

/// Apply a [`Style`] to an entire renderable. Mirrors `rich.styled.Styled`.
pub struct Styled {
    renderable: Box<dyn Renderable>,
    style: Style,
}

impl Styled {
    /// Wrap `renderable`, applying `style` beneath its own styling.
    pub fn new(renderable: Box<dyn Renderable>, style: Style) -> Self {
        Styled { renderable, style }
    }
}

impl Renderable for Styled {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let segments = self.renderable.rich_render(console, options);
        Segment::apply_style(&segments, &self.style)
    }

    fn measure(&self, console: &Console, options: &ConsoleOptions) -> Measurement {
        // Styling doesn't change size — defer to the child. Port of
        // `Styled.__rich_measure__`.
        self.renderable.measure(console, options)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::ColorSystem;
    use crate::text::Text;

    fn console() -> Console {
        Console::builder()
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .width(20)
            .build()
    }

    #[test]
    fn applies_base_style() {
        // "on red" under plain "hi" → background red. Captured from real rich.
        let styled = Styled::new(Box::new(Text::new("hi")), Style::parse("on red").unwrap());
        assert_eq!(console().render_to_string(&styled), "\x1b[41mhi\x1b[0m");
    }

    #[test]
    fn child_style_wins_on_top() {
        // The child's own bold survives; the applied green fills the fg.
        let inner = Text::new("x");
        let mut inner = inner;
        inner.set_base_style(Style::parse("bold").unwrap());
        let styled = Styled::new(Box::new(inner), Style::parse("green").unwrap());
        assert_eq!(console().render_to_string(&styled), "\x1b[1;32mx\x1b[0m");
    }
}
