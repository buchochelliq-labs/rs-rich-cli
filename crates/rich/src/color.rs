//! Colors and color-system handling.
//!
//! Port of upstream `rich/color.py`, `rich/color_triplet.py` and the palette
//! data from `rich/_palettes.py` / `rich/palette.py`.
//!
//! Parses the full `ANSI_COLOR_NAMES` table (see `color_names.rs`), `#rrggbb`
//! hex, `rgb(r,g,b)`, and `color(N)` (0–255). Downgrade uses the redmean
//! nearest-color search ported verbatim from `Palette.match_color`.

use crate::errors::{Result, RichError};

/// The kind of terminal a color system supports.
///
/// Mirrors `rich.color.ColorSystem`. Ordering matters: a color of a given
/// [`ColorType`] can always be represented in an equal-or-higher system, so we
/// only ever *downgrade*, never upgrade.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ColorSystem {
    /// 3/4-bit, the 16 standard ANSI colors.
    Standard,
    /// 8-bit, the 256-color palette.
    EightBit,
    /// 24-bit truecolor.
    Truecolor,
    /// Legacy Windows console (16 colors, distinct SGR handling).
    Windows,
}

/// The origin/representation of a [`Color`]. Mirrors `rich.color.ColorType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ColorType {
    /// The terminal's default foreground/background.
    Default,
    /// One of the 16 standard colors (number 0–15).
    Standard,
    /// A 256-palette color (number 0–255).
    EightBit,
    /// A 24-bit color carrying an explicit [`ColorTriplet`].
    Truecolor,
    /// A legacy Windows console color (number 0–15).
    Windows,
}

/// An 8-bit-per-channel RGB triplet. Mirrors `rich.color_triplet.ColorTriplet`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ColorTriplet {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

impl ColorTriplet {
    pub const fn new(red: u8, green: u8, blue: u8) -> Self {
        Self { red, green, blue }
    }

    /// `#rrggbb` hex string (matches `ColorTriplet.hex`).
    pub fn hex(&self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.red, self.green, self.blue)
    }
}

/// A parsed color. Mirrors `rich.color.Color`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Color {
    /// The original name/spec the color was created from.
    pub name: String,
    pub kind: ColorType,
    /// Palette index for `Standard`/`EightBit`/`Windows`.
    pub number: Option<u8>,
    /// Explicit RGB for `Truecolor`.
    pub triplet: Option<ColorTriplet>,
}

impl Color {
    /// Whether this is the terminal default color. Port of `Color.is_default`.
    pub fn is_default(&self) -> bool {
        self.kind == ColorType::Default
    }

    /// The terminal default color.
    pub fn default_color() -> Self {
        Color {
            name: "default".to_string(),
            kind: ColorType::Default,
            number: None,
            triplet: None,
        }
    }

    /// A color from the `ANSI_COLOR_NAMES` table: numbers < 16 are standard SGR
    /// colors, the rest are 8-bit palette colors. Mirrors `Color.parse`.
    fn named(name: &str, number: u8) -> Self {
        let kind = if number < 16 {
            ColorType::Standard
        } else {
            ColorType::EightBit
        };
        Color {
            name: name.to_string(),
            kind,
            number: Some(number),
            triplet: None,
        }
    }

    /// A color from an 8-bit ANSI number (0–15 standard, 16–255 palette).
    /// Port of `Color.from_ansi`.
    pub fn from_ansi(number: u8) -> Self {
        Color {
            name: format!("color({number})"),
            kind: if number < 16 {
                ColorType::Standard
            } else {
                ColorType::EightBit
            },
            number: Some(number),
            triplet: None,
        }
    }

    /// A truecolor from explicit RGB channels. Port of `Color.from_rgb`
    /// (via `from_triplet`): the name is the `#rrggbb` hex.
    pub fn from_rgb(red: u8, green: u8, blue: u8) -> Self {
        let triplet = ColorTriplet::new(red, green, blue);
        Color {
            name: triplet.hex(),
            kind: ColorType::Truecolor,
            number: None,
            triplet: Some(triplet),
        }
    }

    /// Parse a color from a string spec.
    ///
    /// Accepts: a standard color name, `default`, `#rrggbb`, `rgb(r,g,b)`, and
    /// `color(N)`. Mirrors the common paths of `Color.parse`.
    pub fn parse(color: &str) -> Result<Self> {
        let original = color.trim();
        let lower = original.to_ascii_lowercase();

        if lower == "default" {
            return Ok(Color::default_color());
        }
        if let Some(number) = crate::color_names::ansi_color_number(&lower) {
            return Ok(Color::named(&lower, number));
        }
        if let Some(hex) = lower.strip_prefix('#') {
            let triplet = parse_hex(hex)
                .ok_or_else(|| RichError::ColorParse(format!("invalid hex color {original:?}")))?;
            return Ok(Color {
                name: original.to_string(),
                kind: ColorType::Truecolor,
                number: None,
                triplet: Some(triplet),
            });
        }
        if let Some(inner) = lower.strip_prefix("rgb(").and_then(|s| s.strip_suffix(')')) {
            let triplet = parse_rgb(inner)
                .ok_or_else(|| RichError::ColorParse(format!("invalid rgb color {original:?}")))?;
            return Ok(Color {
                name: original.to_string(),
                kind: ColorType::Truecolor,
                number: None,
                triplet: Some(triplet),
            });
        }
        if let Some(inner) = lower
            .strip_prefix("color(")
            .and_then(|s| s.strip_suffix(')'))
        {
            let n: u16 = inner
                .trim()
                .parse()
                .map_err(|_| RichError::ColorParse(format!("invalid color number {original:?}")))?;
            if n > 255 {
                return Err(RichError::ColorParse(format!(
                    "color number must be <= 255, not {n}"
                )));
            }
            // Numbers < 16 are standard SGR colors; the rest are 8-bit palette
            // (matching `Color.parse`'s `color_8` branch).
            return Ok(Color {
                name: original.to_string(),
                kind: if n < 16 {
                    ColorType::Standard
                } else {
                    ColorType::EightBit
                },
                number: Some(n as u8),
                triplet: None,
            });
        }
        Err(RichError::ColorParse(format!(
            "{original:?} is not a valid color"
        )))
    }

    /// Resolve this color to an RGB triplet, using the palettes for indexed colors.
    ///
    /// Returns `None` only for [`ColorType::Default`].
    pub fn get_truecolor(&self) -> Option<ColorTriplet> {
        match self.kind {
            ColorType::Default => None,
            ColorType::Truecolor => self.triplet,
            ColorType::Standard | ColorType::Windows => {
                self.number.map(|n| ANSI_BASE_PALETTE[n as usize])
            }
            ColorType::EightBit => self.number.map(eight_bit_triplet),
        }
    }

    /// The SGR parameter list for this color (without the leading `\x1b[`).
    ///
    /// Port of `Color.get_ansi_codes`.
    pub fn ansi_codes(&self, foreground: bool) -> Vec<String> {
        match self.kind {
            ColorType::Default => vec![if foreground { "39" } else { "49" }.to_string()],
            ColorType::Windows | ColorType::Standard => {
                let number = self.number.unwrap_or(0);
                let (fore, back) = if number < 8 { (30, 40) } else { (82, 92) };
                vec![(if foreground { fore } else { back } + number as u32).to_string()]
            }
            ColorType::EightBit => {
                let number = self.number.unwrap_or(0);
                vec![
                    if foreground { "38" } else { "48" }.to_string(),
                    "5".to_string(),
                    number.to_string(),
                ]
            }
            ColorType::Truecolor => {
                let t = self.triplet.unwrap_or(ColorTriplet::new(0, 0, 0));
                vec![
                    if foreground { "38" } else { "48" }.to_string(),
                    "2".to_string(),
                    t.red.to_string(),
                    t.green.to_string(),
                    t.blue.to_string(),
                ]
            }
        }
    }

    /// Return a version of this color representable in `system`.
    ///
    /// Port of the reducing half of `Color.downgrade`: colors are only ever
    /// converted *down* to a smaller system, never up.
    pub fn downgrade(&self, system: ColorSystem) -> Color {
        if self.kind == ColorType::Default {
            return self.clone();
        }
        let target_rank = match system {
            ColorSystem::Standard | ColorSystem::Windows => 0,
            ColorSystem::EightBit => 1,
            ColorSystem::Truecolor => 2,
        };
        let self_rank = match self.kind {
            ColorType::Standard | ColorType::Windows => 0,
            ColorType::EightBit => 1,
            ColorType::Truecolor => 2,
            ColorType::Default => return self.clone(),
        };
        if self_rank <= target_rank {
            // Already representable; keep as-is (Windows/Standard are equivalent
            // for our SGR purposes in this slice).
            return self.clone();
        }
        let triplet = match self.get_truecolor() {
            Some(t) => t,
            None => return self.clone(),
        };
        match target_rank {
            1 => Color {
                name: self.name.clone(),
                kind: ColorType::EightBit,
                number: Some(truecolor_to_eight_bit(triplet)),
                triplet: None,
            },
            _ => {
                let number = match_color(&STANDARD_PALETTE, triplet);
                Color {
                    name: self.name.clone(),
                    kind: ColorType::Standard,
                    number: Some(number),
                    triplet: None,
                }
            }
        }
    }
}

fn parse_hex(hex: &str) -> Option<ColorTriplet> {
    if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some(ColorTriplet::new(r, g, b))
}

fn parse_rgb(inner: &str) -> Option<ColorTriplet> {
    let parts: Vec<&str> = inner.split(',').map(str::trim).collect();
    if parts.len() != 3 {
        return None;
    }
    let r = parts[0].parse().ok()?;
    let g = parts[1].parse().ok()?;
    let b = parts[2].parse().ok()?;
    Some(ColorTriplet::new(r, g, b))
}

/// The canonical xterm RGB values for the 16 system colors — the first 16
/// entries of the 256-color palette, and the ANSI set of
/// [`DEFAULT_TERMINAL_THEME`](crate::terminal_theme::DEFAULT_TERMINAL_THEME).
///
/// Upstream keeps these values in `EIGHT_BIT_PALETTE[..16]` and in
/// `DEFAULT_TERMINAL_THEME`; they are used to resolve a standard color to RGB.
/// They are **not** the table used to match a truecolor down to a standard
/// color — that is [`STANDARD_PALETTE`], which holds different values.
pub const ANSI_BASE_PALETTE: [ColorTriplet; 16] = [
    ColorTriplet::new(0, 0, 0),
    ColorTriplet::new(128, 0, 0),
    ColorTriplet::new(0, 128, 0),
    ColorTriplet::new(128, 128, 0),
    ColorTriplet::new(0, 0, 128),
    ColorTriplet::new(128, 0, 128),
    ColorTriplet::new(0, 128, 128),
    ColorTriplet::new(192, 192, 192),
    ColorTriplet::new(128, 128, 128),
    ColorTriplet::new(255, 0, 0),
    ColorTriplet::new(0, 255, 0),
    ColorTriplet::new(255, 255, 0),
    ColorTriplet::new(0, 0, 255),
    ColorTriplet::new(255, 0, 255),
    ColorTriplet::new(0, 255, 255),
    ColorTriplet::new(255, 255, 255),
];

/// The palette a truecolor is matched *against* when downgrading to a standard
/// color. Port of upstream's `rich._palettes.STANDARD_PALETTE`.
///
/// These are deliberately **not** [`ANSI_BASE_PALETTE`]'s values: upstream uses
/// a 170/85-based table here, so e.g. `#ff8800` matches bright red (9) rather
/// than the olive (3) that the 128-based table would pick.
pub const STANDARD_PALETTE: [ColorTriplet; 16] = [
    ColorTriplet::new(0, 0, 0),
    ColorTriplet::new(170, 0, 0),
    ColorTriplet::new(0, 170, 0),
    ColorTriplet::new(170, 85, 0),
    ColorTriplet::new(0, 0, 170),
    ColorTriplet::new(170, 0, 170),
    ColorTriplet::new(0, 170, 170),
    ColorTriplet::new(170, 170, 170),
    ColorTriplet::new(85, 85, 85),
    ColorTriplet::new(255, 85, 85),
    ColorTriplet::new(85, 255, 85),
    ColorTriplet::new(255, 255, 85),
    ColorTriplet::new(85, 85, 255),
    ColorTriplet::new(255, 85, 255),
    ColorTriplet::new(85, 255, 255),
    ColorTriplet::new(255, 255, 255),
];

/// The full 256-color palette, generated deterministically (16 system colors +
/// the 6×6×6 cube + 24 grays), matching the xterm layout `rich` uses.
static EIGHT_BIT_PALETTE: [ColorTriplet; 256] = build_eight_bit_palette();

const fn build_eight_bit_palette() -> [ColorTriplet; 256] {
    let mut palette = [ColorTriplet::new(0, 0, 0); 256];
    // 0–15: standard colors.
    let mut i = 0;
    while i < 16 {
        palette[i] = ANSI_BASE_PALETTE[i];
        i += 1;
    }
    // 16–231: 6×6×6 color cube.
    let levels = [0u8, 95, 135, 175, 215, 255];
    let mut r = 0;
    while r < 6 {
        let mut g = 0;
        while g < 6 {
            let mut b = 0;
            while b < 6 {
                let index = 16 + 36 * r + 6 * g + b;
                palette[index] = ColorTriplet::new(levels[r], levels[g], levels[b]);
                b += 1;
            }
            g += 1;
        }
        r += 1;
    }
    // 232–255: grayscale ramp.
    let mut n = 0;
    while n < 24 {
        let value = 8 + 10 * n as u8;
        palette[232 + n] = ColorTriplet::new(value, value, value);
        n += 1;
    }
    palette
}

fn eight_bit_triplet(number: u8) -> ColorTriplet {
    EIGHT_BIT_PALETTE[number as usize]
}

/// Nearest-color search using the redmean distance.
///
/// Direct port of `rich.palette.Palette.match_color`.
/// Lightness and saturation from `colorsys.rgb_to_hls`, for normalised
/// (`0..=1`) components. Only the two values `downgrade` needs are returned.
fn rgb_to_ls(red: f64, green: f64, blue: f64) -> (f64, f64) {
    let max = red.max(green).max(blue);
    let min = red.min(green).min(blue);
    let lightness = (max + min) / 2.0;
    if max == min {
        return (lightness, 0.0);
    }
    let saturation = if lightness <= 0.5 {
        (max - min) / (max + min)
    } else {
        (max - min) / (2.0 - max - min)
    };
    (lightness, saturation)
}

/// Reduce a truecolor triplet to an 8-bit palette index, exactly as upstream's
/// `Color.downgrade` does for `ColorSystem.EIGHT_BIT`.
///
/// Note this is deliberately **not** a nearest-neighbour search over the 256
/// palette: upstream maps into the 6×6×6 colour cube by formula (and into the
/// greyscale ramp when saturation is under 15%), which picks different — and
/// sometimes further — entries than nearest-match would. `#00ff00` becomes cube
/// index 46, not the exact-matching standard bright-green 10.
fn truecolor_to_eight_bit(color: ColorTriplet) -> u8 {
    let (red, green, blue) = (
        color.red as f64 / 255.0,
        color.green as f64 / 255.0,
        color.blue as f64 / 255.0,
    );
    let (lightness, saturation) = rgb_to_ls(red, green, blue);

    // Under 15% saturation upstream treats the colour as greyscale.
    if saturation < 0.15 {
        // `round_ties_even` matches Python's banker's rounding; plain `round`
        // would differ on exact .5 values (Python's round(2.5) == 2).
        let gray = (lightness * 25.0).round_ties_even() as i64;
        return match gray {
            0 => 16,
            25 => 231,
            other => (231 + other) as u8,
        };
    }

    // The cube axis is non-linear: the first step spans 0..95, the rest 40 each.
    let axis = |component: u8| -> f64 {
        let value = component as f64;
        if value < 95.0 {
            value / 95.0
        } else {
            1.0 + (value - 95.0) / 40.0
        }
    };
    let six_red = axis(color.red).round_ties_even();
    let six_green = axis(color.green).round_ties_even();
    let six_blue = axis(color.blue).round_ties_even();
    (16.0 + 36.0 * six_red + 6.0 * six_green + six_blue) as u8
}

fn match_color(palette: &[ColorTriplet], color: ColorTriplet) -> u8 {
    let (red1, green1, blue1) = (color.red as i64, color.green as i64, color.blue as i64);
    let mut best_index = 0usize;
    let mut best_distance = i64::MAX;
    for (index, candidate) in palette.iter().enumerate() {
        let (red2, green2, blue2) = (
            candidate.red as i64,
            candidate.green as i64,
            candidate.blue as i64,
        );
        let red_mean = (red1 + red2) / 2;
        let red = red1 - red2;
        let green = green1 - green2;
        let blue = blue1 - blue2;
        // Squared redmean distance (monotonic in the true distance, so the
        // sqrt from upstream is unnecessary for an argmin).
        let distance = (((512 + red_mean) * red * red) >> 8)
            + 4 * green * green
            + (((767 - red_mean) * blue * blue) >> 8);
        if distance < best_distance {
            best_distance = distance;
            best_index = index;
        }
    }
    best_index as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_standard_name() {
        let c = Color::parse("red").unwrap();
        assert_eq!(c.kind, ColorType::Standard);
        assert_eq!(c.number, Some(1));
        assert_eq!(c.ansi_codes(true), vec!["31"]);
        assert_eq!(c.ansi_codes(false), vec!["41"]);
    }

    #[test]
    fn extended_name_is_eight_bit() {
        // orange1 (214) is beyond the 16 standard colors → 8-bit palette.
        let c = Color::parse("orange1").unwrap();
        assert_eq!(c.kind, ColorType::EightBit);
        assert_eq!(c.number, Some(214));
        assert_eq!(c.ansi_codes(true), vec!["38", "5", "214"]);
    }

    #[test]
    fn bright_color_uses_high_intensity_sgr() {
        let c = Color::parse("bright_red").unwrap();
        assert_eq!(c.number, Some(9));
        // 82 + 9 = 91
        assert_eq!(c.ansi_codes(true), vec!["91"]);
    }

    #[test]
    fn parses_hex_truecolor() {
        let c = Color::parse("#ff8800").unwrap();
        assert_eq!(c.kind, ColorType::Truecolor);
        assert_eq!(c.triplet, Some(ColorTriplet::new(0xff, 0x88, 0x00)));
        assert_eq!(c.ansi_codes(true), vec!["38", "2", "255", "136", "0"]);
    }

    /// Values captured from real rich 15.0.0 `Color.downgrade`.
    ///
    /// These previously asserted the *wrong* answers, because the matching used
    /// the 128-based ANSI table rather than upstream's separate 170/85-based
    /// `STANDARD_PALETTE` — `#ff0000` was said to give 9 (bright red) when
    /// upstream gives 1 (maroon, the nearer entry under redmean distance).
    #[test]
    fn downgrade_to_standard_matches_upstream() {
        let standard = |hex: &str| {
            let down = Color::parse(hex).unwrap().downgrade(ColorSystem::Standard);
            assert_eq!(down.kind, ColorType::Standard);
            down.number
        };
        assert_eq!(standard("#ff0000"), Some(1));
        assert_eq!(standard("#00ff00"), Some(2));
        assert_eq!(standard("#0000ff"), Some(4));
        assert_eq!(standard("#ffffff"), Some(15));
        assert_eq!(standard("#808080"), Some(7));
        assert_eq!(standard("#ff8800"), Some(9));
    }

    /// Upstream maps into the 6×6×6 cube (and the greyscale ramp) by formula,
    /// *not* by nearest-neighbour over the 256 palette — so `#00ff00` becomes
    /// cube index 46 rather than the exactly-matching system green 10.
    #[test]
    fn downgrade_to_eight_bit_matches_upstream() {
        let eight_bit = |hex: &str| {
            let down = Color::parse(hex).unwrap().downgrade(ColorSystem::EightBit);
            assert_eq!(down.kind, ColorType::EightBit);
            down.number
        };
        assert_eq!(eight_bit("#ff0000"), Some(196));
        assert_eq!(eight_bit("#00ff00"), Some(46));
        assert_eq!(eight_bit("#0000ff"), Some(21));
        assert_eq!(eight_bit("#ff8800"), Some(208));
        // Low saturation takes the greyscale-ramp branch.
        assert_eq!(eight_bit("#ffffff"), Some(231));
        assert_eq!(eight_bit("#808080"), Some(244));
    }

    #[test]
    fn eight_bit_palette_cube_is_correct() {
        // index 196 is the top of the cube red (5,0,0) -> 255,0,0
        assert_eq!(eight_bit_triplet(196), ColorTriplet::new(255, 0, 0));
    }
}
