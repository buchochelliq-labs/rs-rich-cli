//! # rich-art
//!
//! ASCII-art renderables for the [`rich`] terminal library — currently
//! **FIGlet-style text banners** (in the spirit of `figlet(6)` / `pyfiglet`).
//!
//! This crate is **not** a port of anything upstream. `rich` itself has no
//! banner support, so this is a local feature and lives outside the faithful
//! mirror (see `AGENTS.md`). It depends only on [`rich`] — for the
//! [`Renderable`] trait — so it can be lifted into its own repository unchanged.
//!
//! ```no_run
//! use rich::Console;
//! use rich_art::Figlet;
//!
//! let console = Console::builder().build();
//! console.print(&Figlet::new("Hello"));
//! ```
//!
//! A banner is a normal renderable, so it composes with the rest of `rich` —
//! wrap it in a `Panel`, style it, export it to SVG, and so on.

pub mod figlet;

#[cfg(feature = "image")]
pub mod ascii;

#[cfg(feature = "gif")]
pub mod gif;

pub use crate::figlet::{FigletFont, FontError, Justify};

#[cfg(feature = "image")]
pub use crate::ascii::{AsciiArt, DEFAULT_RAMP};

#[cfg(feature = "gif")]
pub use crate::gif::{AnimatedArt, Repeat};

use rich::console::{Console, ConsoleOptions};
use rich::protocol::Renderable;
use rich::segment::Segment;
use rich::style::Style;

/// A block of text rendered as a FIGlet banner.
///
/// Sizing follows the console: the banner lays out to `options.max_width`, so a
/// long line wraps onto further banner rows exactly as `figlet` would.
pub struct Figlet {
    text: String,
    font: FigletFont,
    justify: Justify,
    style: Option<Style>,
    /// Overrides the console width when set.
    width: Option<usize>,
}

impl Figlet {
    /// A banner in the bundled `standard` font.
    pub fn new(text: impl Into<String>) -> Self {
        Figlet {
            text: text.into(),
            font: FigletFont::standard(),
            justify: Justify::Left,
            style: None,
            width: None,
        }
    }

    /// Use an explicit font (see [`FigletFont::parse`]).
    pub fn font(mut self, font: FigletFont) -> Self {
        self.font = font;
        self
    }

    /// Position the banner within the available width.
    pub fn justify(mut self, justify: Justify) -> Self {
        self.justify = justify;
        self
    }

    /// Paint the banner in a style.
    pub fn style(mut self, style: Style) -> Self {
        self.style = Some(style);
        self
    }

    /// Lay out to an explicit width instead of the console's.
    pub fn width(mut self, width: usize) -> Self {
        self.width = Some(width);
        self
    }

    /// The banner as plain text (the `figlet(1)` output), without rendering it
    /// through a console.
    pub fn to_text(&self, width: usize) -> String {
        figlet::render(&self.text, &self.font, width, self.justify)
    }
}

impl Renderable for Figlet {
    fn rich_render(&self, _console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let width = self.width.unwrap_or(options.max_width);
        let banner = self.to_text(width);
        // `render` always terminates each row with a newline; split it back into
        // lines so the console controls the final separator.
        let lines: Vec<&str> = banner
            .strip_suffix('\n')
            .unwrap_or(&banner)
            .split('\n')
            .collect();
        let mut segments = Vec::new();
        let last = lines.len().saturating_sub(1);
        for (index, line) in lines.into_iter().enumerate() {
            segments.push(Segment::new(line.to_string(), self.style.clone()));
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
    use rich::color::ColorSystem;

    #[test]
    fn renders_a_banner_through_a_console() {
        let console = Console::builder()
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .width(80)
            .no_color(false)
            .build();
        let out = console.render_to_string(&Figlet::new("Hi").width(80));
        // The `standard` font draws `Hi` with these strokes.
        assert!(out.contains("| | | (_)"), "got:\n{out}");
        assert!(out.contains("|_| |_|_|"), "got:\n{out}");
    }

    #[test]
    fn style_is_applied_to_every_row() {
        let console = Console::builder()
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .width(80)
            .no_color(false)
            .build();
        let banner = Figlet::new("Hi")
            .width(80)
            .style(Style::parse("bold red").unwrap());
        let out = console.render_to_string(&banner);
        assert!(out.contains("\x1b[1;31m"), "expected bold red, got:\n{out}");
    }

    #[test]
    fn to_text_matches_the_renderable() {
        let banner = Figlet::new("Hi");
        assert!(banner.to_text(80).starts_with(" _   _ _ \n"));
    }
}
