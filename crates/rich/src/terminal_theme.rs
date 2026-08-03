//! Terminal color themes for export.
//!
//! Port of `rich/terminal_theme.py`. A [`TerminalTheme`] maps the abstract
//! colors of a rendered document — the 16 ANSI colors plus a foreground and
//! background — onto concrete RGB values, so styled output can be exported to
//! HTML/SVG. [`DEFAULT_TERMINAL_THEME`] mirrors upstream's default.

use crate::color::{Color, ColorTriplet, ColorType, ANSI_BASE_PALETTE};

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
    ansi: ANSI_BASE_PALETTE,
};

/// The theme rich uses for SVG export (a dark terminal). Mirrors
/// `rich.terminal_theme.SVG_EXPORT_THEME` — the foundation for `export_svg`
/// (DIVERGENCES #15).
pub const SVG_EXPORT_THEME: TerminalTheme = TerminalTheme {
    background: ColorTriplet::new(41, 41, 41),
    foreground: ColorTriplet::new(197, 200, 198),
    ansi: [
        ColorTriplet::new(75, 78, 85),
        ColorTriplet::new(204, 85, 90),
        ColorTriplet::new(152, 168, 75),
        ColorTriplet::new(208, 179, 68),
        ColorTriplet::new(96, 138, 177),
        ColorTriplet::new(152, 114, 159),
        ColorTriplet::new(104, 160, 179),
        ColorTriplet::new(197, 200, 198),
        ColorTriplet::new(154, 155, 153),
        ColorTriplet::new(255, 38, 39),
        ColorTriplet::new(0, 130, 61),
        ColorTriplet::new(208, 132, 66),
        ColorTriplet::new(25, 132, 233),
        ColorTriplet::new(255, 44, 122),
        ColorTriplet::new(57, 130, 128),
        ColorTriplet::new(253, 253, 197),
    ],
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn svg_export_theme_matches_upstream() {
        // Captured from real rich 15.0.0 `terminal_theme.SVG_EXPORT_THEME`.
        assert_eq!(
            (
                SVG_EXPORT_THEME.background.red,
                SVG_EXPORT_THEME.background.green,
                SVG_EXPORT_THEME.background.blue
            ),
            (41, 41, 41)
        );
        assert_eq!(
            (
                SVG_EXPORT_THEME.foreground.red,
                SVG_EXPORT_THEME.foreground.green,
                SVG_EXPORT_THEME.foreground.blue
            ),
            (197, 200, 198)
        );
        let ansi: Vec<(u8, u8, u8)> = SVG_EXPORT_THEME
            .ansi
            .iter()
            .map(|c| (c.red, c.green, c.blue))
            .collect();
        assert_eq!(
            ansi,
            vec![
                (75, 78, 85),
                (204, 85, 90),
                (152, 168, 75),
                (208, 179, 68),
                (96, 138, 177),
                (152, 114, 159),
                (104, 160, 179),
                (197, 200, 198),
                (154, 155, 153),
                (255, 38, 39),
                (0, 130, 61),
                (208, 132, 66),
                (25, 132, 233),
                (255, 44, 122),
                (57, 130, 128),
                (253, 253, 197),
            ]
        );
    }

    #[test]
    fn blend_rgb_truncates_like_upstream() {
        // Dim blends the fg 40% toward the bg (int() truncation).
        let fg = ColorTriplet::new(197, 200, 198);
        let bg = ColorTriplet::new(41, 41, 41);
        let dim = blend_rgb(fg, bg, 0.4);
        assert_eq!((dim.red, dim.green, dim.blue), (134, 136, 135));
    }
}
