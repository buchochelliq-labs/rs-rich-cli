//! A status indicator with a spinner.
//!
//! Port of `rich/status.py` (the renderable surface). A [`Status`] shows a
//! spinner animation followed by a status message. Upstream drives it with a
//! `Live` loop; here the renderable shows the first frame (`t = 0`), and the
//! live animation lands with the Live-loop work (see the Live/progress issue).

use crate::console::{Console, ConsoleOptions};
use crate::protocol::Renderable;
use crate::segment::Segment;
use crate::spinner::Spinner;
use crate::style::Style;

/// A spinner + message status indicator. Mirrors `rich.status.Status`.
pub struct Status {
    message: String,
    spinner: String,
    spinner_style: Style,
    speed: f64,
}

impl Status {
    /// A status showing `message` with the default `dots` spinner (green).
    pub fn new(message: impl Into<String>) -> Self {
        Status {
            message: message.into(),
            spinner: "dots".to_string(),
            spinner_style: Style::parse("green").expect("valid built-in style"),
            speed: 1.0,
        }
    }

    /// Choose the spinner animation by name (default `dots`).
    pub fn spinner(mut self, name: impl Into<String>) -> Self {
        self.spinner = name.into();
        self
    }

    /// Style applied to the spinner frame (default `status.spinner` = green).
    pub fn spinner_style(mut self, style: Style) -> Self {
        self.spinner_style = style;
        self
    }

    /// Set the spinner animation speed multiplier (default 1.0).
    pub fn speed(mut self, speed: f64) -> Self {
        self.speed = speed;
        self
    }

    /// The underlying spinner. Mirrors upstream's `Status.renderable`.
    pub fn renderable(&self) -> Spinner {
        Spinner::new(&self.spinner)
            .text(&self.message)
            .style(self.spinner_style.clone())
            .speed(self.speed)
    }
}

impl Renderable for Status {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        // Static first frame; the live animation needs the Live loop.
        self.renderable().render(0.0).rich_render(console, options)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::ColorSystem;

    fn render(status: &Status) -> String {
        Console::builder()
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .width(30)
            .no_color(false)
            .build()
            .render_to_string(status)
    }

    #[test]
    fn default_status_frame() {
        // Captured from real rich 15.0.0 (dots spinner, status.spinner=green, t=0).
        assert_eq!(
            render(&Status::new("Loading data")),
            "\x1b[32m⠋\x1b[0m Loading data"
        );
    }

    #[test]
    fn custom_spinner() {
        assert_eq!(
            render(&Status::new("Building").spinner("line")),
            "\x1b[32m-\x1b[0m Building"
        );
    }
}
