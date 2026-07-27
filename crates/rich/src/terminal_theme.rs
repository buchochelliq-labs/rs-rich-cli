//! Terminal color themes for export.
//!
//! Port of `rich/terminal_theme.py`. A [`TerminalTheme`] maps the abstract
//! colors of a rendered document — the 16 ANSI colors plus a foreground and
//! background — onto concrete RGB values, so styled output can be exported to
//! HTML/SVG. [`DEFAULT_TERMINAL_THEME`] mirrors upstream's default.

use crate::color::{Color, ColorTriplet, ColorType, STANDARD_PALETTE};

/// A concrete palette for exporting styled output. Mirrors
/// `rich.terminal_theme.TerminalTheme` (the fields the exporters use).
#[derive(Debug, Clone)]
pub struct TerminalTheme {
    pub background: ColorTriplet,
    pub foreground: ColorTriplet,
    /// The 16 standard ANSI colors (indices 0–15).
    pub ansi: [ColorTriplet; 16],
}

impl TerminalTheme {
    /// Resolve `color` to a concrete RGB triplet under this theme. Standard
    /// colors use the theme's ANSI palette; the terminal default maps to the
    /// theme's fore/background; 8-bit and truecolor resolve directly.
    pub fn resolve(&self, color: &Color, foreground: bool) -> ColorTriplet {
        match color.kind {
            ColorType::Default => {
                if foreground {
                    self.foreground
                } else {
                    self.background
                }
            }
            ColorType::Standard | ColorType::Windows => {
                self.ansi[(color.number.unwrap_or(0) as usize) & 0x0f]
            }
            ColorType::EightBit | ColorType::Truecolor => {
                color.get_truecolor().unwrap_or(self.foreground)
            }
        }
    }
}

/// The default export theme (white background, black text, standard ANSI 16).
/// Mirrors `rich.terminal_theme.DEFAULT_TERMINAL_THEME`.
pub const DEFAULT_TERMINAL_THEME: TerminalTheme = TerminalTheme {
    background: ColorTriplet::new(255, 255, 255),
    foreground: ColorTriplet::new(0, 0, 0),
    ansi: STANDARD_PALETTE,
};

/// Blend two triplets, `cross_fade` of the way from `color1` to `color2`
/// (truncating toward zero, matching upstream's `int()`). Port of
/// `rich.color.blend_rgb`.
pub fn blend_rgb(color1: ColorTriplet, color2: ColorTriplet, cross_fade: f64) -> ColorTriplet {
    let mix = |a: u8, b: u8| (a as f64 + (b as f64 - a as f64) * cross_fade) as u8;
    ColorTriplet::new(
        mix(color1.red, color2.red),
        mix(color1.green, color2.green),
        mix(color1.blue, color2.blue),
    )
}
