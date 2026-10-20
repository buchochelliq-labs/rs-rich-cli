//! Text coloured along a gradient: [`GradientText`] puts each character of
//! a [`Text`] at its place along two or more colours, across, down or
//! corner to corner, through [`palette::gradient`](crate::palette::gradient).
//!
//! ```
//! use rich::{Color, ColorSystem, Console, Text};
//! use rich_ext::gradient::GradientText;
//!
//! let stops = [Color::parse("#ff0000").unwrap(), Color::parse("#0000ff").unwrap()];
//! let title = GradientText::new(Text::new("abc"), stops);
//! let console = Console::builder()
//!     .width(20)
//!     .force_terminal(true)
//!     .color_system(Some(ColorSystem::Truecolor))
//!     .no_color(false)
//!     .build();
//! assert_eq!(
//!     console.render_export(&title),
//!     "\x1b[38;2;255;0;0ma\x1b[0m\x1b[38;2;128;0;128mb\x1b[0m\x1b[38;2;0;0;255mc\x1b[0m\n"
//! );
//! ```

use rich::measure::Measurement;
use rich::{Color, Console, ConsoleOptions, Renderable, Segment, Style, Text};

use crate::palette::gradient;

/// Which way a [`GradientText`]'s colours run.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum GradientDirection {
    /// Left to right, the same at each column on every line (the default).
    #[default]
    Horizontal,
    /// Top to bottom, one colour a line.
    Vertical,
    /// From the top left corner to the bottom right.
    Diagonal,
}

/// A [`Text`] whose characters are coloured along a gradient of two or
/// more colours, spaced evenly.
///
/// - The text is laid out as it would be on its own (wrapped, justified,
///   its styles kept), then each character takes the colour at its place:
///   its column across the widest line ([`Horizontal`](GradientDirection::Horizontal)),
///   its line ([`Vertical`](GradientDirection::Vertical)), or both
///   ([`Diagonal`](GradientDirection::Diagonal)).
/// - The gradient replaces the text's own colour (or its background, with
///   [`background`](Self::background)); bold, italic and the rest stay.
/// - It degrades by the console's colour system: each colour is the
///   nearest the console has, so a 256-colour terminal shows steps and a
///   16-colour one a few bands, and neighbours that come out the same are
///   written as one run. Without colour the text is drawn as it is.
///
/// ```
/// use rich::{Color, ColorSystem, Console, Text};
/// use rich_ext::gradient::{GradientDirection, GradientText};
///
/// let stops = ["#ff0000", "#ffff00", "#0000ff"].map(|c| Color::parse(c).unwrap());
/// let banner = GradientText::new(Text::new("one\ntwo\nsix"), stops)
///     .direction(GradientDirection::Vertical);
/// let console = Console::builder()
///     .width(20)
///     .force_terminal(true)
///     .color_system(Some(ColorSystem::Standard))
///     .no_color(false)
///     .build();
/// // Red, yellow and blue, a line each, on 16 colours.
/// assert_eq!(
///     console.render_export(&banner),
///     "\x1b[31mone\x1b[0m\n\x1b[93mtwo\x1b[0m\n\x1b[34msix\x1b[0m\n"
/// );
/// let plain = Console::builder().width(20).color_system(None).build();
/// assert_eq!(plain.render_export(&banner), "one\ntwo\nsix\n");
/// ```
#[derive(Clone, Debug)]
pub struct GradientText {
    text: Text,
    stops: Vec<Color>,
    direction: GradientDirection,
    background: bool,
}

impl GradientText {
    /// `text` coloured along `stops` (the first at the start, the last at
    /// the end). One stop colours it all; none leaves it as it is.
    pub fn new(text: Text, stops: impl IntoIterator<Item = Color>) -> Self {
        GradientText {
            text,
            stops: stops.into_iter().collect(),
            direction: GradientDirection::Horizontal,
            background: false,
        }
    }

    /// Which way the colours run (default [`GradientDirection::Horizontal`]).
    pub fn direction(mut self, direction: GradientDirection) -> Self {
        self.direction = direction;
        self
    }

    /// Colour the background rather than the characters (default off).
    pub fn background(mut self, background: bool) -> Self {
        self.background = background;
        self
    }

    /// The text being coloured.
    pub fn text(&self) -> &Text {
        &self.text
    }
}

/// Where `index` falls between 0 and `count - 1`, as 0 to 1.
fn share(index: usize, count: usize) -> f64 {
    if count < 2 {
        0.0
    } else {
        index as f64 / (count - 1) as f64
    }
}

impl Renderable for GradientText {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let system = match console.color_system() {
            Some(system) if !console.no_color() && !self.stops.is_empty() => system,
            _ => return self.text.rich_render(console, options),
        };
        let lines = console.render_lines(&self.text, options, false);
        let widest = lines
            .iter()
            .map(|line| line.iter().map(Segment::cell_length).sum::<usize>())
            .max()
            .unwrap_or(0);
        let rows = lines.len();
        let mut out: Vec<Segment> = Vec::new();
        // The last run's own style and its colour's escape codes: a
        // character that comes out the same joins it.
        let mut last: Option<(Option<Style>, String)> = None;
        for (row, line) in lines.into_iter().enumerate() {
            if row > 0 {
                out.push(Segment::line());
                last = None;
            }
            let mut col = 0;
            for segment in line {
                if segment.control {
                    out.push(segment);
                    last = None;
                    continue;
                }
                for c in segment.text.chars() {
                    let t = match self.direction {
                        GradientDirection::Horizontal => share(col, widest),
                        GradientDirection::Vertical => share(row, rows),
                        GradientDirection::Diagonal => {
                            (share(col, widest) + share(row, rows)) / 2.0
                        }
                    };
                    col += rich::cells::char_cell_width(c);
                    let colour = gradient(&self.stops, t).downgrade(system);
                    let paint = if self.background {
                        Style::from_color(None, Some(colour))
                    } else {
                        Style::from_color(Some(colour), None)
                    };
                    let key = (segment.style.clone(), paint.ansi_codes(system));
                    match out.last_mut() {
                        Some(run) if last.as_ref() == Some(&key) => run.text.push(c),
                        _ => {
                            let style = match &segment.style {
                                Some(own) => own.combine(&paint),
                                None => paint,
                            };
                            out.push(Segment::new(c.to_string(), Some(style)));
                            last = Some(key);
                        }
                    }
                }
            }
        }
        out
    }

    fn measure(&self, console: &Console, options: &ConsoleOptions) -> Measurement {
        self.text.measure(console, options)
    }
}

/// The text's own words: the gradient is decoration.
impl crate::a11y::AccessibleText for GradientText {
    fn accessible_text(&self, _width: usize) -> String {
        self.text.plain().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shares_run_from_the_first_to_the_last() {
        assert_eq!(share(0, 5), 0.0);
        assert_eq!(share(4, 5), 1.0);
        assert_eq!(share(2, 5), 0.5);
        assert_eq!(share(0, 1), 0.0);
    }
}
