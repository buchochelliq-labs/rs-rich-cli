//! Progress bars.
//!
//! Port of upstream `rich/progress_bar.py`. A [`ProgressBar`] renders a
//! determinate bar at half-cell resolution, or, when pulsing (`pulse=True` or
//! no total), upstream's animated pulse: a cosine fade between `bar.pulse` and
//! `bar.back` that scrolls with the animation time. ASCII-only and legacy
//! Windows consoles get upstream's `-` glyphs.

use crate::color::{Color, ColorSystem, ColorTriplet};
use crate::console::{monotonic, Console, ConsoleOptions};
use crate::measure::Measurement;
use crate::protocol::Renderable;
use crate::segment::Segment;
use crate::style::{Style, StyleType};

/// Segments in one pulse period. Upstream `PULSE_SIZE`.
const PULSE_SIZE: usize = 20;

/// A progress bar. Mirrors `rich.progress_bar.ProgressBar`.
pub struct ProgressBar {
    /// `None` renders the pulse, as upstream's `total=None` does.
    total: Option<f64>,
    completed: f64,
    width: Option<usize>,
    pulse: bool,
    animation_time: Option<f64>,
    style: StyleType,
    complete_style: StyleType,
    finished_style: StyleType,
    pulse_style: StyleType,
}

impl ProgressBar {
    /// A bar of `completed` out of `total`, with upstream's default `bar.*` styles.
    pub fn new(total: f64, completed: f64) -> Self {
        ProgressBar {
            total: Some(total),
            completed,
            width: None,
            pulse: false,
            animation_time: None,
            style: "bar.back".into(),
            complete_style: "bar.complete".into(),
            finished_style: "bar.finished".into(),
            pulse_style: "bar.pulse".into(),
        }
    }

    /// A bar with no total, which always pulses (upstream `total=None`).
    pub fn indeterminate() -> Self {
        ProgressBar {
            total: None,
            ..ProgressBar::new(100.0, 0.0)
        }
    }

    /// Fix the bar width (otherwise it fills the available width).
    pub fn width(mut self, width: usize) -> Self {
        self.width = Some(width);
        self
    }

    /// The background style (upstream `style`, default `bar.back`).
    pub fn style(mut self, style: impl Into<StyleType>) -> Self {
        self.style = style.into();
        self
    }

    /// The completed-part style (upstream `complete_style`, default `bar.complete`).
    pub fn complete_style(mut self, style: impl Into<StyleType>) -> Self {
        self.complete_style = style.into();
        self
    }

    /// The style once finished (upstream `finished_style`, default `bar.finished`).
    pub fn finished_style(mut self, style: impl Into<StyleType>) -> Self {
        self.finished_style = style.into();
        self
    }

    /// The pulse style (upstream `pulse_style`, default `bar.pulse`).
    pub fn pulse_style(mut self, style: impl Into<StyleType>) -> Self {
        self.pulse_style = style.into();
        self
    }

    /// Render the pulse animation instead of the completion (upstream `pulse`).
    pub fn pulse(mut self, pulse: bool) -> Self {
        self.pulse = pulse;
        self
    }

    /// The time, in seconds, the pulse is drawn at (upstream `animation_time`).
    /// Without it the pulse follows a monotonic clock.
    pub fn animation_time(mut self, time: f64) -> Self {
        self.animation_time = Some(time);
        self
    }

    /// Port of `_get_pulse_segments`: one period of the pulse.
    fn pulse_segments(
        fore: &Style,
        back: &Style,
        color_system: Option<ColorSystem>,
        no_color: bool,
        ascii: bool,
    ) -> Vec<Segment> {
        let bar = if ascii { "-" } else { "\u{2501}" };
        // Upstream tests `color_system not in ("standard", "eight_bit",
        // "truecolor")`, but a 256-colour console reports `"256"`, so only
        // standard and truecolor consoles get the blended pulse.
        let colourful = matches!(
            color_system,
            Some(ColorSystem::Standard | ColorSystem::Truecolor)
        );
        if !colourful || no_color {
            let fore_count = PULSE_SIZE / 2;
            let mut segments = vec![Segment::new(bar, Some(fore.clone())); fore_count];
            let back_bar = if no_color { " " } else { bar };
            segments.extend(vec![
                Segment::new(back_bar, Some(back.clone()));
                PULSE_SIZE - fore_count
            ]);
            return segments;
        }
        let triplet = |style: &Style, fallback: ColorTriplet| {
            style
                .color()
                .and_then(Color::get_truecolor)
                .unwrap_or(fallback)
        };
        let fore_color = triplet(fore, ColorTriplet::new(255, 0, 255));
        let back_color = triplet(back, ColorTriplet::new(0, 0, 0));
        (0..PULSE_SIZE)
            .map(|index| {
                let position = index as f64 / PULSE_SIZE as f64;
                let fade = 0.5 + (position * std::f64::consts::PI * 2.0).cos() / 2.0;
                let color = blend_rgb(fore_color, back_color, fade);
                Segment::new(
                    bar,
                    Some(Style::new().with_color(Color::from_rgb(
                        color.red,
                        color.green,
                        color.blue,
                    ))),
                )
            })
            .collect()
    }

    /// Port of `_render_pulse`.
    fn render_pulse(&self, console: &Console, width: usize, ascii: bool) -> Vec<Segment> {
        let fore = style_or(console, &self.pulse_style, "white");
        let back = style_or(console, &self.style, "black");
        let pulse = Self::pulse_segments(
            &fore,
            &back,
            console.color_system(),
            console.no_color(),
            ascii,
        );
        let count = pulse.len();
        let time = self.animation_time.unwrap_or_else(monotonic);
        // `int(-current_time * 15) % segment_count`, with Python's floor modulo.
        let offset = ((-time * 15.0) as i64).rem_euclid(count as i64) as usize;
        pulse
            .iter()
            .cycle()
            .skip(offset)
            .take(width)
            .cloned()
            .collect()
    }
}

/// `console.get_style(name, default=…)`.
fn style_or(console: &Console, style: &StyleType, default: &str) -> Style {
    console
        .get_style(style)
        .unwrap_or_else(|_| Style::parse(default).expect("valid default style"))
}

/// Port of `rich.color.blend_rgb`.
fn blend_rgb(first: ColorTriplet, second: ColorTriplet, cross_fade: f64) -> ColorTriplet {
    let mix = |a: u8, b: u8| (f64::from(a) + (f64::from(b) - f64::from(a)) * cross_fade) as u8;
    ColorTriplet::new(
        mix(first.red, second.red),
        mix(first.green, second.green),
        mix(first.blue, second.blue),
    )
}

impl Renderable for ProgressBar {
    /// Port of `ProgressBar.__rich_measure__`: a fixed width measures exactly,
    /// otherwise the bar takes 4 cells up to the whole width.
    fn measure(&self, _console: &Console, options: &ConsoleOptions) -> Measurement {
        match self.width {
            Some(width) => Measurement::new(width, width),
            None => Measurement::new(4, options.max_width),
        }
    }

    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let width = self
            .width
            .filter(|width| *width > 0)
            .unwrap_or(options.max_width)
            .min(options.max_width);
        let ascii = console.legacy_windows() || console.ascii_only();
        if self.pulse || self.total.is_none() {
            return self.render_pulse(console, width, ascii);
        }
        let total = self.total.unwrap_or(0.0);
        let completed = total.min(self.completed.max(0.0));

        let (bar, half_bar_right, half_bar_left) = if ascii {
            ("-", " ", " ")
        } else {
            ("\u{2501}", "\u{2578}", "\u{257a}")
        };
        // `int(width * 2 * completed / total) if total else width * 2`.
        // `completed <= total`, so the quotient never exceeds `width * 2`
        // except when `width * 2 * completed` overflows to infinity for
        // totals near `f64::MAX` (where upstream's `int(inf)` raises); the
        // clamp keeps that from asking `repeat` for `usize::MAX` cells.
        let complete_halves = if total != 0.0 {
            ((width as f64 * 2.0 * completed / total) as usize).min(width * 2)
        } else {
            width * 2
        };
        let bar_count = complete_halves / 2;
        let half_bar_count = complete_halves % 2;
        let back = style_or(console, &self.style, "none");
        let is_finished = self.completed >= total;
        let complete = style_or(
            console,
            if is_finished {
                &self.finished_style
            } else {
                &self.complete_style
            },
            "none",
        );

        let mut segments: Vec<Segment> = Vec::new();
        if bar_count > 0 {
            segments.push(Segment::new(bar.repeat(bar_count), Some(complete.clone())));
        }
        if half_bar_count > 0 {
            segments.push(Segment::new(
                half_bar_right.repeat(half_bar_count),
                Some(complete),
            ));
        }
        // The background only renders with colour: without it the empty part
        // of the bar is simply left out.
        if !console.no_color() && console.color_system().is_some() {
            let mut remaining = width.saturating_sub(bar_count + half_bar_count);
            if remaining > 0 {
                if half_bar_count == 0 && bar_count > 0 {
                    segments.push(Segment::new(half_bar_left, Some(back.clone())));
                    remaining -= 1;
                }
                if remaining > 0 {
                    segments.push(Segment::new(bar.repeat(remaining), Some(back)));
                }
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
    fn huge_totals_do_not_overflow_the_bar_width() {
        // `width * 2 * completed` overflows to infinity for totals near
        // `f64::MAX`; the bar must still render full rather than asking
        // `repeat` for `usize::MAX` cells.
        let console = Console::builder()
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .width(20)
            .build();
        for (total, completed) in [(1e308, 1e308), (f64::MAX, f64::MAX), (1e308, 5e307)] {
            let got = console.render_to_string(&ProgressBar::new(total, completed).width(20));
            assert!(got.contains('\u{2501}'), "{total}/{completed}: {got:?}");
        }
        assert_eq!(render(100.0), {
            let console = Console::builder()
                .force_terminal(true)
                .color_system(Some(ColorSystem::Truecolor))
                .width(20)
                .build();
            console.render_to_string(&ProgressBar::new(1e308, 1e308).width(20))
        });
    }

    #[test]
    fn empty_bar_is_all_background() {
        assert_eq!(
            render(0.0),
            format!("\x1b[38;5;237m{}\x1b[0m", "\u{2501}".repeat(20))
        );
    }

    #[test]
    fn full_bar_uses_finished_style() {
        assert_eq!(
            render(100.0),
            format!("\x1b[38;2;114;156;31m{}\x1b[0m", "\u{2501}".repeat(20))
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
