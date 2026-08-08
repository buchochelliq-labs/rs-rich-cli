//! Spinners.
//!
//! Port of upstream `rich/spinner.py` + a subset of `rich/_spinners.py`. A
//! [`Spinner`] picks an animation frame for a given elapsed time. The animation
//! itself is driven by a `Live` loop (not yet ported); [`Spinner::render`] gives
//! the frame at a point in time and is the testable surface.
//!
//! Scope: all built-in spinners (vendored in `spinner_data.rs`), an optional
//! trailing text and a frame [`Style`]. Live-loop animation is still deferred.

use crate::console::{Console, ConsoleOptions};
use crate::protocol::Renderable;
use crate::segment::Segment;
use crate::style::Style;
use crate::text::Text;

/// A named terminal spinner. Mirrors `rich.spinner.Spinner`.
pub struct Spinner {
    frames: &'static [&'static str],
    /// Frame interval in milliseconds.
    interval: f64,
    text: String,
    speed: f64,
    style: Option<Style>,
}

impl Spinner {
    /// Look up a built-in spinner by name (falls back to `dots`).
    pub fn new(name: &str) -> Self {
        let (interval, frames) = crate::spinner_data::spinner_data(name)
            .or_else(|| crate::spinner_data::spinner_data("dots"))
            .expect("dots spinner exists");
        Spinner {
            frames,
            interval,
            text: String::new(),
            speed: 1.0,
            style: None,
        }
    }

    /// Add trailing text after the spinner frame.
    pub fn text(mut self, text: impl Into<String>) -> Self {
        self.text = text.into();
        self
    }

    /// Set the animation speed multiplier (default 1.0).
    pub fn speed(mut self, speed: f64) -> Self {
        self.speed = speed;
        self
    }

    /// Style applied to the spinner *frame* (not the trailing text).
    pub fn style(mut self, style: Style) -> Self {
        self.style = Some(style);
        self
    }

    /// The frame index at `time` seconds (from an implicit start of 0).
    fn frame_index(&self, time: f64) -> usize {
        let interval_secs = self.interval / 1000.0;
        ((time * self.speed / interval_secs) as usize) % self.frames.len()
    }

    /// Render the spinner as it appears at `time` seconds. Port of
    /// `Spinner.render` (`Text.assemble(frame, " ", text)`): the frame carries
    /// the spinner style, the trailing `" text"` stays plain.
    pub fn render(&self, time: f64) -> Text {
        let frame = self.frames[self.frame_index(time)];
        let mut text = if self.text.is_empty() {
            Text::new(frame)
        } else {
            Text::new(format!("{frame} {}", self.text))
        };
        if let Some(style) = &self.style {
            text.stylize(style.clone(), 0, frame.len());
        }
        text
    }
}

impl Renderable for Spinner {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        // A bare print shows the first frame (t = 0); animation needs a Live loop.
        self.render(0.0).rich_render(console, options)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::ColorSystem;

    fn frame_at(name: &str, time: f64) -> String {
        let console = Console::builder()
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .width(20)
            .build();
        console.render_to_string(&Spinner::new(name).render(time))
    }

    #[test]
    fn dots_frames_match_upstream() {
        // Captured from real rich 15.0.0 (start time 0).
        assert_eq!(frame_at("dots", 0.0), "⠋");
        assert_eq!(frame_at("dots", 0.1), "⠙");
        assert_eq!(frame_at("dots", 0.25), "⠸");
    }

    #[test]
    fn line_frames_match_upstream() {
        assert_eq!(frame_at("line", 0.0), "-");
        assert_eq!(frame_at("line", 0.1), "-");
        assert_eq!(frame_at("line", 0.25), "\\");
    }

    #[test]
    fn full_table_covers_more_spinners() {
        // "moon"/"bounce" weren't in the original curated subset. moon: 80ms.
        assert_eq!(frame_at("moon", 0.0), "\u{1f311} ");
        assert_eq!(frame_at("moon", 0.08), "\u{1f312} ");
        assert_eq!(frame_at("bounce", 0.0), "\u{2801}");
    }

    #[test]
    fn arrow_and_dots2_match_upstream() {
        // arrow: interval 100ms → frame advances each 0.1s.
        assert_eq!(frame_at("arrow", 0.0), "←");
        assert_eq!(frame_at("arrow", 0.1), "↖");
        assert_eq!(frame_at("dots2", 0.0), "⣾");
    }

    #[test]
    fn text_follows_frame() {
        let console = Console::builder()
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .width(20)
            .build();
        let out = console.render_to_string(&Spinner::new("dots").text("Working").render(0.0));
        assert_eq!(out, "⠋ Working");
    }

    #[test]
    fn styled_frame_only() {
        // Captured from real rich 15.0.0: the frame is green, " Working" plain.
        let console = Console::builder()
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .width(30)
            .no_color(false)
            .build();
        let spinner = Spinner::new("dots")
            .text("Working")
            .style(crate::style::Style::parse("green").unwrap());
        assert_eq!(
            console.render_to_string(&spinner.render(0.0)),
            "\x1b[32m⠋\x1b[0m Working"
        );
    }
}
