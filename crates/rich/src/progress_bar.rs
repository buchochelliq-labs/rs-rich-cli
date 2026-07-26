//! Progress bars (static rendering).
//!
//! Port of the `__rich_console__` core of upstream `rich/progress_bar.py`. A
//! [`ProgressBar`] renders a determinate bar at a given completion using
//! half-cell resolution for a smooth edge.
//!
//! Slice scope: determinate bars in a color-capable terminal. The indeterminate
//! "pulse" animation and ASCII/legacy fallbacks are deferred.

use crate::color::Color;
use crate::console::{Console, ConsoleOptions};
use crate::protocol::Renderable;
use crate::segment::Segment;
use crate::style::Style;

const BAR: &str = "━"; // U+2501
const HALF_BAR_RIGHT: &str = "╸"; // U+2578 — trailing edge of the completed run
const HALF_BAR_LEFT: &str = "╺"; // U+257A — leading edge of the background run

/// A determinate progress bar. Mirrors `rich.progress_bar.ProgressBar`.
pub struct ProgressBar {
    total: f64,
    completed: f64,
    width: Option<usize>,
    complete_style: Style,
    finished_style: Style,
    back_style: Style,
}

impl ProgressBar {
    /// A bar of `completed` out of `total`, using upstream's default `bar.*` styles.
    pub fn new(total: f64, completed: f64) -> Self {
        let color = |spec: &str| Style::new().with_color(Color::parse(spec).expect("valid color"));
        ProgressBar {
            total,
            completed,
            width: None,
            complete_style: color("rgb(249,38,114)"), // bar.complete
            finished_style: color("rgb(114,156,31)"), // bar.finished
            back_style: color("color(237)"),          // bar.back (grey23)
        }
    }

    /// Fix the bar width (otherwise it fills the available width).
    pub fn width(mut self, width: usize) -> Self {
        self.width = Some(width);
        self
    }
}

impl Renderable for ProgressBar {
    fn rich_render(&self, _console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let width = self
            .width
            .unwrap_or(options.max_width)
            .min(options.max_width);
        if width == 0 || self.total <= 0.0 {
            return vec![Segment::new(
                BAR.repeat(width),
                Some(self.back_style.clone()),
            )];
        }

        let completed = self.completed.clamp(0.0, self.total);
        // Completion measured in half-cells (double resolution). Port of
        // `int(width * 2 * completed / total)`.
        let complete_halves = (width as f64 * 2.0 * completed / self.total) as usize;
        let bar_count = complete_halves / 2;
        let half_bar_count = complete_halves % 2;

        let is_finished = completed >= self.total;
        let complete_style = if is_finished {
            &self.finished_style
        } else {
            &self.complete_style
        };

        let mut segments: Vec<Segment> = Vec::new();
        if bar_count > 0 {
            segments.push(Segment::new(
                BAR.repeat(bar_count),
                Some(complete_style.clone()),
            ));
        }
        if half_bar_count > 0 {
            segments.push(Segment::new(
                HALF_BAR_RIGHT.repeat(half_bar_count),
                Some(complete_style.clone()),
            ));
        }

        // Background.
        let mut remaining = width - bar_count - half_bar_count;
        if remaining > 0 {
            if half_bar_count == 0 && bar_count > 0 {
                segments.push(Segment::new(
                    HALF_BAR_LEFT.to_string(),
                    Some(self.back_style.clone()),
                ));
                remaining -= 1;
            }
            if remaining > 0 {
                segments.push(Segment::new(
                    BAR.repeat(remaining),
                    Some(self.back_style.clone()),
                ));
            }
        }
        segments
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::ColorSystem;

    fn render(completed: f64) -> String {
        let console = Console::builder()
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .width(20)
            .build();
        console.render_to_string(&ProgressBar::new(100.0, completed).width(20))
    }

    #[test]
    fn empty_bar_is_all_background() {
        assert_eq!(
            render(0.0),
            format!("\x1b[38;5;237m{}\x1b[0m", BAR.repeat(20))
        );
    }

    #[test]
    fn full_bar_uses_finished_style() {
        assert_eq!(
            render(100.0),
            format!("\x1b[38;2;114;156;31m{}\x1b[0m", BAR.repeat(20))
        );
    }

    #[test]
    fn half_bar_has_background_half_cell() {
        // 10 complete, background ╺ + 9 background bars.
        assert_eq!(
            render(50.0),
            "\x1b[38;2;249;38;114m━━━━━━━━━━\x1b[0m\x1b[38;5;237m╺\x1b[0m\x1b[38;5;237m━━━━━━━━━\x1b[0m"
        );
    }
}
