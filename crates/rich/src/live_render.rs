//! In-place live rendering.
//!
//! Port of `rich/live_render.py`. A [`LiveRender`] wraps a renderable, remembers
//! the shape (width × height) of its last render, and produces the terminal
//! control sequences to move the cursor back over that render — the mechanism a
//! `Live` display uses to redraw in place. The full `Live` loop (threading,
//! timing, stdout management) is deferred; this is its byte-parity-testable core.
//!
//! Scope: rendering + `position_cursor`/`restore_cursor`. Vertical-overflow
//! cropping (when content is taller than the screen) is deferred.

use std::cell::Cell;

use crate::console::{Console, ConsoleOptions};
use crate::control::{Control, ControlType};
use crate::protocol::Renderable;
use crate::segment::Segment;
use crate::style::Style;

/// Wraps a renderable for repeated in-place redraws. Mirrors
/// `rich.live_render.LiveRender`.
pub struct LiveRender {
    renderable: Box<dyn Renderable>,
    style: Option<Style>,
    /// `(width, height)` of the last render, or `None` before the first.
    shape: Cell<Option<(usize, usize)>>,
}

impl LiveRender {
    pub fn new(renderable: Box<dyn Renderable>) -> Self {
        LiveRender {
            renderable,
            style: None,
            shape: Cell::new(None),
        }
    }

    /// Apply a style across the whole live render.
    pub fn style(mut self, style: Style) -> Self {
        self.style = Some(style);
        self
    }

    /// Replace the wrapped renderable (the shape carries over until the next
    /// render). Port of `LiveRender.set_renderable`.
    pub fn set_renderable(&mut self, renderable: Box<dyn Renderable>) {
        self.renderable = renderable;
    }

    /// Control codes to move the cursor to the start of the previous render,
    /// erasing each line. Port of `LiveRender.position_cursor`.
    pub fn position_cursor(&self) -> Control {
        match self.shape.get() {
            Some((_, height)) => {
                let mut codes = vec![ControlType::CarriageReturn, ControlType::EraseInLine(2)];
                for _ in 0..height.saturating_sub(1) {
                    codes.push(ControlType::CursorUp(1));
                    codes.push(ControlType::EraseInLine(2));
                }
                Control::new(&codes)
            }
            None => Control::new(&[]),
        }
    }

    /// Control codes to clear the render and restore the cursor to where it was
    /// before it. Port of `LiveRender.restore_cursor`.
    pub fn restore_cursor(&self) -> Control {
        match self.shape.get() {
            Some((_, height)) => {
                let mut codes = vec![ControlType::CarriageReturn];
                for _ in 0..height {
                    codes.push(ControlType::CursorUp(1));
                    codes.push(ControlType::EraseInLine(2));
                }
                Control::new(&codes)
            }
            None => Control::new(&[]),
        }
    }
}

impl Renderable for LiveRender {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        // Render unpadded lines (`pad=False`), applying the live style if set.
        let mut lines = console.render_lines(self.renderable.as_ref(), options, false);
        if let Some(style) = &self.style {
            for line in &mut lines {
                *line = Segment::apply_style(line, style);
            }
        }

        // Remember the shape for the next `position_cursor`/`restore_cursor`.
        let height = lines.len();
        let width = lines
            .iter()
            .map(|line| line.iter().map(Segment::cell_length).sum::<usize>())
            .max()
            .unwrap_or(0);
        self.shape.set(Some((width, height)));

        let mut segments = Vec::new();
        let last = height.saturating_sub(1);
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

    fn console() -> Console {
        Console::builder()
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .width(20)
            .no_color(false)
            .build()
    }

    #[test]
    fn renders_content_and_control_codes() {
        let live = LiveRender::new(Box::new(Text::new("line one\nline two\nline three")));
        let console = console();
        // Rendering sets the shape.
        assert_eq!(
            console.render_to_string(&live),
            "line one\nline two\nline three"
        );
        // Captured from real rich 15.0.0 (height 3).
        assert_eq!(
            live.position_cursor().as_str(),
            "\r\x1b[2K\x1b[1A\x1b[2K\x1b[1A\x1b[2K"
        );
        assert_eq!(
            live.restore_cursor().as_str(),
            "\r\x1b[1A\x1b[2K\x1b[1A\x1b[2K\x1b[1A\x1b[2K"
        );
    }

    #[test]
    fn single_line_shape() {
        let live = LiveRender::new(Box::new(Text::new("solo")));
        let console = console();
        assert_eq!(console.render_to_string(&live), "solo");
        assert_eq!(live.position_cursor().as_str(), "\r\x1b[2K");
        assert_eq!(live.restore_cursor().as_str(), "\r\x1b[1A\x1b[2K");
    }

    #[test]
    fn no_control_before_first_render() {
        let live = LiveRender::new(Box::new(Text::new("x")));
        assert_eq!(live.position_cursor().as_str(), "");
        assert_eq!(live.restore_cursor().as_str(), "");
    }
}
