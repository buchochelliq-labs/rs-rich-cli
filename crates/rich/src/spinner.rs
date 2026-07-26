//! Spinners.
//!
//! Port of upstream `rich/spinner.py` + a subset of `rich/_spinners.py`. A
//! [`Spinner`] picks an animation frame for a given elapsed time. The animation
//! itself is driven by a `Live` loop (not yet ported); [`Spinner::render`] gives
//! the frame at a point in time and is the testable surface.
//!
//! Slice scope: a handful of the built-in spinners and plain (unstyled) frames.

use crate::console::{Console, ConsoleOptions};
use crate::protocol::Renderable;
use crate::segment::Segment;
use crate::text::Text;

/// A named terminal spinner. Mirrors `rich.spinner.Spinner`.
pub struct Spinner {
    frames: Vec<&'static str>,
    /// Frame interval in milliseconds.
    interval: f64,
    text: String,
    speed: f64,
}

impl Spinner {
    /// Look up a built-in spinner by name (falls back to `dots`).
    pub fn new(name: &str) -> Self {
        let (interval, frames) = spinner_data(name);
        Spinner {
            frames,
            interval,
            text: String::new(),
            speed: 1.0,
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

    /// The frame index at `time` seconds (from an implicit start of 0).
    fn frame_index(&self, time: f64) -> usize {
        let interval_secs = self.interval / 1000.0;
        ((time * self.speed / interval_secs) as usize) % self.frames.len()
    }

    /// Render the spinner as it appears at `time` seconds. Port of `Spinner.render`.
    pub fn render(&self, time: f64) -> Text {
        let frame = self.frames[self.frame_index(time)];
        if self.text.is_empty() {
            Text::new(frame)
        } else {
            Text::new(format!("{frame} {}", self.text))
        }
    }
}

impl Renderable for Spinner {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        // A bare print shows the first frame (t = 0); animation needs a Live loop.
        self.render(0.0).rich_render(console, options)
    }
}

/// `(interval_ms, frames)` for a built-in spinner. A representative subset of
/// upstream `_spinners.py`; unknown names fall back to `dots`.
fn spinner_data(name: &str) -> (f64, Vec<&'static str>) {
    match name {
        "line" => (130.0, vec!["-", "\\", "|", "/"]),
        "dots2" => (80.0, vec!["⣾", "⣽", "⣻", "⢿", "⡿", "⣟", "⣯", "⣷"]),
        "simpleDots" => (400.0, vec![".  ", ".. ", "...", "   "]),
        "arrow" => (100.0, vec!["←", "↖", "↑", "↗", "→", "↘", "↓", "↙"]),
        // "dots" and any unknown name.
        _ => (80.0, vec!["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]),
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
}
