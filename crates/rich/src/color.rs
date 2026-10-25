//! Colors and color-system handling.
//!
//! Port of upstream `rich/color.py`, `rich/color_triplet.py` and the palette
//! data from `rich/_palettes.py` / `rich/palette.py`.
//!
//! Parses the full `ANSI_COLOR_NAMES` table (see `color_names.rs`), `#rrggbb`
//! hex, `rgb(r,g,b)`, and `color(N)` (0–255). Downgrade uses the redmean
//! nearest-color search ported verbatim from `Palette.match_color`.

use compact_str::CompactString;

use crate::errors::{Result, RichError};
use crate::markup::py_repr_str;

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
    ///
    /// Upstream is `f"#{red:02x}{green:02x}{blue:02x}"`. Written digit by
    /// digit rather than through `format!`, which was half of a frame that
    /// builds a colour per cell (docs/PORTING.md, "Performance notes");
    /// `hex_matches_format` pins it to the formatted string.
    pub fn hex(&self) -> String {
        self.hex_name().into()
    }

    /// [`hex`](Self::hex) as a colour's name: inline, no allocation.
    pub(crate) fn hex_name(&self) -> CompactString {
        const DIGITS: &[u8; 16] = b"0123456789abcdef";
        // Built on the stack and copied in once: `CompactString::push` a
        // character at a time costs more than the allocation it saves.
        let mut hex = *b"#000000";
        for (i, value) in [self.red, self.green, self.blue].into_iter().enumerate() {
            hex[1 + 2 * i] = DIGITS[usize::from(value >> 4)];
            hex[2 + 2 * i] = DIGITS[usize::from(value & 0xf)];
        }
        // ASCII digits and `#`.
        CompactString::from_utf8(hex).unwrap_or_default()
    }

    /// `rgb(r,g,b)` (matches `ColorTriplet.rgb`).
    pub fn rgb(&self) -> String {
        format!("rgb({},{},{})", self.red, self.green, self.blue)
    }

    /// The components as floats from 0 to 1 (matches
    /// `ColorTriplet.normalized`).
    pub fn normalized(&self) -> (f64, f64, f64) {
        (
            self.red as f64 / 255.0,
            self.green as f64 / 255.0,
            self.blue as f64 / 255.0,
        )
    }
}

/// A parsed color. Mirrors `rich.color.Color`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Color {
    /// The original name/spec the color was created from: a
    /// [`CompactString`](crate::CompactString), kept inline up to 24 bytes,
    /// so a colour's name (`#rrggbb`, `bright_red`) allocates nothing.
    pub name: crate::CompactString,
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
            name: CompactString::const_new("default"),
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
            name: name.into(),
            kind,
            number: Some(number),
            triplet: None,
        }
    }

    /// A color from an 8-bit ANSI number (0–15 standard, 16–255 palette).
    /// Port of `Color.from_ansi`.
    pub fn from_ansi(number: u8) -> Self {
        Color {
            name: compact_str::format_compact!("color({number})"),
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
            name: triplet.hex_name(),
            kind: ColorType::Truecolor,
            number: None,
            triplet: Some(triplet),
        }
    }

    /// Parse a color from a string spec. Port of `Color.parse`.
    ///
    /// Accepts a color name from `ANSI_COLOR_NAMES`, `default`, `#rrggbb`,
    /// `color(N)` (one to three digits) and `rgb(r,g,b)`, case-insensitively
    /// and ignoring surrounding whitespace. The errors carry upstream's
    /// `ColorParseError` messages.
    pub fn parse(color: &str) -> Result<Self> {
        let original = color;
        let color = color.to_lowercase();
        let color = color.trim();
        let invalid =
            || RichError::ColorParse(format!("{} is not a valid color", py_repr_str(original)));

        if color == "default" {
            return Ok(Color::default_color());
        }
        if let Some(number) = crate::color_names::ansi_color_number(color) {
            return Ok(Color::named(color, number));
        }
        // `RE_COLOR`: `^#([0-9a-f]{6})$`, `color\(([0-9]{1,3})\)$` or
        // `rgb\(([\d\s,]+)\)$`.
        if let Some(hex) = color.strip_prefix('#') {
            if hex.len() != 6 || !hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
                return Err(invalid());
            }
            return Ok(Color {
                name: color.into(),
                kind: ColorType::Truecolor,
                number: None,
                triplet: Some(parse_rgb_hex(hex)),
            });
        }
        if let Some(digits) = color
            .strip_prefix("color(")
            .and_then(|s| s.strip_suffix(')'))
        {
            if digits.is_empty() || digits.len() > 3 || !digits.bytes().all(|b| b.is_ascii_digit())
            {
                return Err(invalid());
            }
            let number: u16 = digits.parse().map_err(|_| invalid())?;
            if number > 255 {
                return Err(RichError::ColorParse(format!(
                    "color number must be <= 255 in {}",
                    py_repr_str(color)
                )));
            }
            return Ok(Color {
                name: color.into(),
                kind: if number < 16 {
                    ColorType::Standard
                } else {
                    ColorType::EightBit
                },
                number: Some(number as u8),
                triplet: None,
            });
        }
        if let Some(inner) = color.strip_prefix("rgb(").and_then(|s| s.strip_suffix(')')) {
            if inner.is_empty()
                || !inner
                    .chars()
                    .all(|c| c.is_ascii_digit() || c == ',' || c.is_whitespace())
            {
                return Err(invalid());
            }
            let components: Vec<&str> = inner.split(',').collect();
            if components.len() != 3 {
                return Err(RichError::ColorParse(format!(
                    "expected three components in {}",
                    py_repr_str(original)
                )));
            }
            let mut values = [0u8; 3];
            for (slot, component) in values.iter_mut().zip(&components) {
                // Python's `int()`, which ignores surrounding whitespace.
                let digits = component.trim();
                if digits.is_empty() || digits.contains(char::is_whitespace) {
                    return Err(RichError::ColorParse(format!(
                        "invalid literal for int() with base 10: {}",
                        py_repr_str(component)
                    )));
                }
                match digits.parse::<u16>() {
                    Ok(value) if value <= 255 => *slot = value as u8,
                    _ => {
                        return Err(RichError::ColorParse(format!(
                            "color components must be <= 255 in {}",
                            py_repr_str(original)
                        )))
                    }
                }
            }
            return Ok(Color {
                name: color.into(),
                kind: ColorType::Truecolor,
                number: None,
                triplet: Some(ColorTriplet::new(values[0], values[1], values[2])),
            });
        }
        Err(invalid())
    }

    /// A truecolor from a triplet; the name is its `#rrggbb` hex. Port of
    /// `Color.from_triplet`.
    pub fn from_triplet(triplet: ColorTriplet) -> Self {
        Color {
            name: triplet.hex_name(),
            kind: ColorType::Truecolor,
            number: None,
            triplet: Some(triplet),
        }
    }

    /// The native color system of this color: standard for the default
    /// color. Port of the `Color.system` property.
    pub fn system(&self) -> ColorSystem {
        match self.kind {
            ColorType::Default | ColorType::Standard => ColorSystem::Standard,
            ColorType::EightBit => ColorSystem::EightBit,
            ColorType::Truecolor => ColorSystem::Truecolor,
            ColorType::Windows => ColorSystem::Windows,
        }
    }

    /// Whether the terminal defines the actual color: true for the default,
    /// standard and Windows colors. Port of `Color.is_system_defined`.
    pub fn is_system_defined(&self) -> bool {
        !matches!(
            self.system(),
            ColorSystem::EightBit | ColorSystem::Truecolor
        )
    }

    /// The RGB triplet this color shows as under `theme` (the default export
    /// theme when `None`). A standard color takes the theme's ANSI color and
    /// the default color the theme's foreground or background. Port of
    /// `Color.get_truecolor(theme, foreground)`.
    pub fn get_truecolor_for(
        &self,
        theme: Option<&crate::terminal_theme::TerminalTheme>,
        foreground: bool,
    ) -> ColorTriplet {
        let theme = theme.unwrap_or(&crate::terminal_theme::DEFAULT_TERMINAL_THEME);
        theme.resolve(self, foreground)
    }

    /// The text upstream prints for a color: `<color 'red' (standard)⬤ >`,
    /// the dot in the color itself. Port of `Color.__rich__`.
    pub fn rich_text(&self) -> crate::text::Text {
        let kind = match self.kind {
            ColorType::Default => "default",
            ColorType::Standard => "standard",
            ColorType::EightBit => "eight_bit",
            ColorType::Truecolor => "truecolor",
            ColorType::Windows => "windows",
        };
        let mut text =
            crate::text::Text::new(format!("<color {} ({kind})", py_repr_str(&self.name)));
        let dot = crate::style::Style::from_color(Some(self.clone()), None);
        text.append("⬤", Some(dot.into()));
        text.append(" >", None);
        text
    }

    /// Resolve this color to an RGB triplet, using the palettes for indexed colors.
    ///
    /// Returns `None` only for [`ColorType::Default`].
    pub fn get_truecolor(&self) -> Option<ColorTriplet> {
        match self.kind {
            ColorType::Default => None,
            ColorType::Truecolor => self.triplet,
            ColorType::Standard => self.number.map(|n| ANSI_BASE_PALETTE[n as usize]),
            // Upstream resolves a Windows color through `WINDOWS_PALETTE`.
            ColorType::Windows => self.number.map(|n| WINDOWS_PALETTE[(n & 15) as usize]),
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
    /// Port of `Color.downgrade`: colors are only ever converted *down* to a
    /// smaller system, never up, and a Windows console matches against the
    /// Windows 10 console palette ([`WINDOWS_PALETTE`]), not the standard one.
    pub fn downgrade(&self, system: ColorSystem) -> Color {
        let same_system = matches!(
            (self.kind, system),
            (ColorType::Standard, ColorSystem::Standard)
                | (ColorType::EightBit, ColorSystem::EightBit)
                | (ColorType::Truecolor, ColorSystem::Truecolor)
                | (ColorType::Windows, ColorSystem::Windows)
        );
        if self.kind == ColorType::Default || same_system {
            return self.clone();
        }
        let is_truecolor = self.kind == ColorType::Truecolor;
        // Upstream's non-truecolor branches assume an 8-bit color and index
        // `EIGHT_BIT_PALETTE` with the number, whatever the actual type.
        let source_triplet = || {
            if is_truecolor {
                self.triplet
            } else {
                self.number.map(eight_bit_triplet)
            }
        };
        match system {
            ColorSystem::EightBit if is_truecolor => {
                let Some(triplet) = self.triplet else {
                    return self.clone();
                };
                Color {
                    name: self.name.clone(),
                    kind: ColorType::EightBit,
                    number: Some(truecolor_to_eight_bit(triplet)),
                    triplet: None,
                }
            }
            ColorSystem::Standard => {
                let Some(triplet) = source_triplet() else {
                    return self.clone();
                };
                Color {
                    name: self.name.clone(),
                    kind: ColorType::Standard,
                    number: Some(match_color(&STANDARD_PALETTE, triplet)),
                    triplet: None,
                }
            }
            ColorSystem::Windows => {
                if !is_truecolor {
                    if let Some(number) = self.number.filter(|&number| number < 16) {
                        return Color {
                            name: self.name.clone(),
                            kind: ColorType::Windows,
                            number: Some(number),
                            triplet: None,
                        };
                    }
                }
                let Some(triplet) = source_triplet() else {
                    return self.clone();
                };
                Color {
                    name: self.name.clone(),
                    kind: ColorType::Windows,
                    number: Some(match_color(&WINDOWS_PALETTE, triplet)),
                    triplet: None,
                }
            }
            _ => self.clone(),
        }
    }
}

/// Six hex digits as a triplet. Port of `rich.color.parse_rgb_hex`.
///
/// # Panics
///
/// When `hex_color` is not six hex digits (upstream asserts its length).
pub fn parse_rgb_hex(hex_color: &str) -> ColorTriplet {
    assert_eq!(hex_color.len(), 6, "must be 6 characters");
    let channel = |range: std::ops::Range<usize>| {
        u8::from_str_radix(&hex_color[range], 16).expect("hex digits")
    };
    ColorTriplet::new(channel(0..2), channel(2..4), channel(4..6))
}

/// Blend two triplets, `cross_fade` of the way from `color1` to `color2`
/// (truncating toward zero, as upstream's `int()` does). Port of
/// `rich.color.blend_rgb` (upstream's default `cross_fade` is 0.5).
pub fn blend_rgb(color1: ColorTriplet, color2: ColorTriplet, cross_fade: f64) -> ColorTriplet {
    let mix = |a: u8, b: u8| (a as f64 + (b as f64 - a as f64) * cross_fade) as u8;
    ColorTriplet::new(
        mix(color1.red, color2.red),
        mix(color1.green, color2.green),
        mix(color1.blue, color2.blue),
    )
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

/// The Windows 10 console palette a truecolor or 8-bit color is matched
/// against on a `windows` color system. Port of upstream's
/// `rich._palettes.WINDOWS_PALETTE`.
pub const WINDOWS_PALETTE: [ColorTriplet; 16] = [
    ColorTriplet::new(12, 12, 12),
    ColorTriplet::new(197, 15, 31),
    ColorTriplet::new(19, 161, 14),
    ColorTriplet::new(193, 156, 0),
    ColorTriplet::new(0, 55, 218),
    ColorTriplet::new(136, 23, 152),
    ColorTriplet::new(58, 150, 221),
    ColorTriplet::new(204, 204, 204),
    ColorTriplet::new(118, 118, 118),
    ColorTriplet::new(231, 72, 86),
    ColorTriplet::new(22, 198, 12),
    ColorTriplet::new(249, 241, 165),
    ColorTriplet::new(59, 120, 255),
    ColorTriplet::new(180, 0, 158),
    ColorTriplet::new(97, 214, 214),
    ColorTriplet::new(242, 242, 242),
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

/// A palette of colors. Port of `rich.palette.Palette`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Palette {
    colors: Vec<ColorTriplet>,
}

impl Palette {
    /// A palette of `colors`, indexed from 0.
    pub fn new(colors: Vec<ColorTriplet>) -> Self {
        Palette { colors }
    }

    /// The color at `number` (`palette[number]`), if there is one.
    pub fn get(&self, number: usize) -> Option<ColorTriplet> {
        self.colors.get(number).copied()
    }

    /// The colors, in order.
    pub fn colors(&self) -> &[ColorTriplet] {
        &self.colors
    }

    /// How many colors the palette holds.
    pub fn len(&self) -> usize {
        self.colors.len()
    }

    /// Whether the palette holds no colors.
    pub fn is_empty(&self) -> bool {
        self.colors.is_empty()
    }

    /// The index of the color nearest to `color` by redmean distance, the
    /// first on a tie. Port of `Palette.match`.
    ///
    /// # Panics
    ///
    /// On an empty palette, where upstream's `min` raises.
    pub fn match_color(&self, color: ColorTriplet) -> usize {
        assert!(!self.colors.is_empty(), "match on an empty palette");
        match_color(&self.colors, color) as usize
    }
}

impl std::ops::Index<usize> for Palette {
    type Output = ColorTriplet;

    fn index(&self, number: usize) -> &ColorTriplet {
        &self.colors[number]
    }
}

/// A palette prints as a table of its colors. Port of `Palette.__rich__`.
impl crate::protocol::Renderable for Palette {
    fn rich_render(
        &self,
        console: &crate::console::Console,
        options: &crate::console::ConsoleOptions,
    ) -> Vec<crate::segment::Segment> {
        self.table().rich_render(console, options)
    }

    fn measure(
        &self,
        console: &crate::console::Console,
        options: &crate::console::ConsoleOptions,
    ) -> crate::measure::Measurement {
        self.table().measure(console, options)
    }
}

impl Palette {
    /// The table `Palette.__rich__` returns.
    fn table(&self) -> crate::table::Table {
        use crate::table::{Cell, Table};
        use crate::text::Text;
        let mut table = Table::new()
            .title("Palette")
            .caption(format!("{} colors", self.colors.len()))
            .highlight(true)
            .caption_justify(crate::console::Justify::Right);
        table.add_column("index");
        table.add_column("RGB");
        table.add_column("Color");
        for (index, color) in self.colors.iter().enumerate() {
            let swatch = crate::style::Style::from_color(None, Some(Color::from_triplet(*color)));
            table.add_row_cells(vec![
                Cell::Markup(index.to_string()),
                Cell::Markup(format!("({}, {}, {})", color.red, color.green, color.blue)),
                Cell::Text(Text::styled(" ".repeat(16), swatch)),
            ]);
        }
        table
    }
}

/// A color prints as its description and a dot in the color. Port of
/// `Color.__rich__`.
impl crate::protocol::Renderable for Color {
    fn rich_render(
        &self,
        console: &crate::console::Console,
        options: &crate::console::ConsoleOptions,
    ) -> Vec<crate::segment::Segment> {
        self.rich_text().rich_render(console, options)
    }

    fn measure(
        &self,
        console: &crate::console::Console,
        options: &crate::console::ConsoleOptions,
    ) -> crate::measure::Measurement {
        self.rich_text().measure(console, options)
    }

    fn printed_text(&self) -> Option<crate::text::Text> {
        Some(self.rich_text())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The hand-written hex is upstream's `"#%02x%02x%02x"` for every
    /// value of every channel.
    #[test]
    fn hex_matches_format() {
        for value in 0..=255u8 {
            let other = value.wrapping_mul(97).wrapping_add(13);
            for (r, g, b) in [(value, other, 0), (other, value, 255), (0, other, value)] {
                assert_eq!(
                    ColorTriplet::new(r, g, b).hex(),
                    format!("#{r:02x}{g:02x}{b:02x}")
                );
            }
        }
    }

    /// A Windows console matches against `WINDOWS_PALETTE`: `#808080` is
    /// bright black (90), not the standard palette's white (37); an 8-bit
    /// color under 16 keeps its number.
    #[test]
    fn windows_downgrade_uses_the_windows_palette() {
        let grey = Color::parse("#808080")
            .unwrap()
            .downgrade(ColorSystem::Windows);
        assert_eq!((grey.kind, grey.number), (ColorType::Windows, Some(8)));
        let low = Color::parse("color(9)")
            .unwrap()
            .downgrade(ColorSystem::Windows);
        assert_eq!((low.kind, low.number), (ColorType::Windows, Some(9)));
        assert!(Color::parse("grey").is_err());
        assert!(Color::parse("gray").is_err());
    }

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
