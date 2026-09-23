//! A status indicator with a spinner.
//!
//! Port of `rich/status.py` (the renderable surface). A [`Status`] shows a
//! spinner animation followed by a status message, parsed as console markup.
//! Upstream drives it with a `Live` loop; here the spinner is the testable
//! surface ([`Status::renderable`] rendered at a point in time), and
//! [`Status::update`] follows upstream: a new spinner name replaces the
//! spinner (restarting its animation), anything else updates it in place.

use crate::console::{Console, ConsoleOptions};
use crate::protocol::Renderable;
use crate::segment::Segment;
use crate::spinner::Spinner;
use crate::style::StyleType;

/// A spinner + message status indicator. Mirrors `rich.status.Status`.
pub struct Status {
    status: String,
    spinner_style: StyleType,
    speed: f64,
    spinner: Spinner,
}

impl Status {
    /// A status showing `message` with the default `dots` spinner, styled
    /// `status.spinner` (green in the default theme).
    pub fn new(message: impl Into<String>) -> Self {
        let status = message.into();
        let spinner_style = StyleType::Name("status.spinner".to_string());
        Status {
            spinner: Spinner::new("dots")
                .text(status.clone())
                .style(spinner_style.clone()),
            status,
            spinner_style,
            speed: 1.0,
        }
    }

    fn rebuild(mut self, name: &str) -> Self {
        self.spinner = Spinner::new(name)
            .text(self.status.clone())
            .style(self.spinner_style.clone())
            .speed(self.speed);
        self
    }

    /// Choose the spinner animation by name (default `dots`).
    pub fn spinner(self, name: &str) -> Self {
        self.rebuild(name)
    }

    /// Style applied to the spinner frame (default `status.spinner`).
    pub fn spinner_style(mut self, style: impl Into<StyleType>) -> Self {
        self.spinner_style = style.into();
        self.spinner = self.spinner.style(self.spinner_style.clone());
        self
    }

    /// Set the spinner animation speed multiplier (default 1.0).
    pub fn speed(mut self, speed: f64) -> Self {
        self.speed = speed;
        self.spinner = self.spinner.speed(speed);
        self
    }

    /// Port of `Status.update`. `None` (and, as upstream, a zero speed) leaves
    /// a field unchanged. A new spinner name builds a fresh spinner; otherwise
    /// the current one is updated in place, a speed change continuing from its
    /// current frame.
    pub fn update(
        &mut self,
        status: Option<&str>,
        spinner: Option<&str>,
        spinner_style: Option<StyleType>,
        speed: Option<f64>,
    ) {
        if let Some(status) = status {
            self.status = status.to_string();
        }
        if let Some(style) = spinner_style {
            self.spinner_style = style;
        }
        if let Some(speed) = speed.filter(|s| *s != 0.0) {
            self.speed = speed;
        }
        if let Some(name) = spinner {
            self.spinner = Spinner::new(name)
                .text(self.status.clone())
                .style(self.spinner_style.clone())
                .speed(self.speed);
        } else {
            self.spinner.update(
                Some(&self.status),
                Some(self.spinner_style.clone()),
                Some(self.speed),
            );
        }
    }

    /// The underlying spinner. Mirrors upstream's `Status.renderable`.
    pub fn renderable(&self) -> &Spinner {
        &self.spinner
    }
}

impl Renderable for Status {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        // Static frame; the live animation needs the Live loop.
        self.spinner.rich_render(console, options)
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
