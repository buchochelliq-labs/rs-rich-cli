//! A full-screen renderable.
//!
//! Port of `rich/screen.py`. [`Screen`] fills the entire console region
//! (width × height), rendering its child into it and cropping/padding to an
//! exact rectangle, optionally under a background [`Style`]. Used with the
//! alternate screen buffer (see [`Control::alt_screen`](crate::control::Control)).

use crate::console::{Console, ConsoleOptions};
use crate::protocol::Renderable;
use crate::segment::Segment;
use crate::style::Style;

/// A renderable that fills the screen and crops excess. Mirrors
/// `rich.screen.Screen`.
pub struct Screen {
    renderable: Option<Box<dyn Renderable>>,
    style: Option<Style>,
}

impl Screen {
    /// A screen filled with `renderable`.
    pub fn new(renderable: Box<dyn Renderable>) -> Self {
        Screen {
            renderable: Some(renderable),
            style: None,
        }
    }

    /// An empty screen (blank fill).
    pub fn empty() -> Self {
        Screen {
            renderable: None,
            style: None,
        }
    }

    /// Set a background style applied under the whole screen.
    pub fn style(mut self, style: Style) -> Self {
        self.style = Some(style);
        self
    }
}

impl Renderable for Screen {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let width = options.max_width;
        let height = options.height.unwrap_or_else(|| console.height());

        let lines = match &self.renderable {
            Some(renderable) => {
                let child_options = options.update_dimensions(width, height);
                console.render_lines(renderable.as_ref(), &child_options, true)
            }
            None => Vec::new(),
        };
        let mut lines = Segment::set_shape(lines, width, height);

        // Lay the background style under every cell (content and blank fill).
        if let Some(style) = &self.style {
            for line in &mut lines {
                *line = Segment::apply_style(line, style);
            }
        }

        let mut segments = Vec::new();
        let last = lines.len().saturating_sub(1);
        for (index, line) in lines.into_iter().enumerate() {
            segments.extend(line);
            if index != last {
                segments.push(Segment::line());
            }
        }
        segments
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::ColorSystem;
    use crate::text::Text;

    fn console(width: usize, height: usize) -> Console {
        Console::builder()
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .width(width)
            .height(height)
            .build()
    }

    #[test]
    fn fills_to_width_and_height() {
        let screen = Screen::new(Box::new(Text::new("hi")));
        let out = console(6, 3).capture(|c| c.print(&screen));
        // 3 rows of 6 cells: "hi" then two blank rows. (Upstream's print omits
        // the final row separator for a full-height Screen — DIVERGENCES #12.)
        assert_eq!(out, "hi    \n      \n      \n");
    }

    #[test]
    fn empty_screen_is_blank_rectangle() {
        let out = console(4, 2).capture(|c| c.print(&Screen::empty()));
        assert_eq!(out, "    \n    \n");
    }
}
