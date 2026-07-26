//! Width constraint.
//!
//! Port of upstream `rich/constrain.py`. [`Constrain`] renders a child within a
//! reduced maximum width.

use crate::console::{Console, ConsoleOptions};
use crate::protocol::Renderable;
use crate::segment::Segment;

/// Limits a child renderable to at most `width` cells. Mirrors `rich.constrain.Constrain`.
pub struct Constrain {
    child: Box<dyn Renderable>,
    width: Option<usize>,
}

impl Constrain {
    /// Constrain `child` to `width` cells (or leave unconstrained when `None`).
    pub fn new(child: Box<dyn Renderable>, width: Option<usize>) -> Self {
        Constrain { child, width }
    }
}

impl Renderable for Constrain {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let width = match self.width {
            Some(width) => width.min(options.max_width),
            None => options.max_width,
        };
        let child_options = options.update_width(width);
        self.child.rich_render(console, &child_options)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::ColorSystem;
    use crate::panel::Panel;
    use crate::r#box::SQUARE;
    use crate::text::Text;

    #[test]
    fn constrains_panel_width() {
        let console = Console::builder()
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .width(20)
            .build();
        let panel = Panel::new(Box::new(Text::new("hi"))).box_set(SQUARE);
        let constrained = Constrain::new(Box::new(panel), Some(10));
        let out = console.render_export(&constrained);
        assert_eq!(out, "┌────────┐\n│ hi     │\n└────────┘\n");
    }
}
