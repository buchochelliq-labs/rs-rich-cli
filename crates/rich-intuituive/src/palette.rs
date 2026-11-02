//! Semantic colours for a [`Theme`](crate::Theme): a [`Palette`] of a few
//! roles (primary, secondary, accent, warning, error, success, foreground,
//! background, surface, panel, boost) from which the shades every
//! component draws in are generated.
//!
//! A port of [Textual]'s `ColorSystem` (`textual/design.py`) and of its
//! built-in themes (`textual/theme.py`), Textual 8.2.8. Textual is
//! Copyright (c) 2021 Will McGugan and licensed under the MIT licence; its
//! notice is in this crate's `THIRD_PARTY.md`. The shades take Textual's
//! names (`primary-lighten-2`, `primary-muted`, `text-primary`,
//! `text-muted`, `surface`, `panel`, `boost`, …) and its values: lighter
//! and darker in CIE L\*a\*b\*, blends and tints as Textual computes them.
//! A colour with an alpha (`text` is the contrast text at 87%) is laid
//! over the palette's background, as a terminal shows it.
//!
//! [Textual]: https://github.com/Textualize/textual
//!
//! ```
//! use intuituive::{Palette, Theme};
//! use rich::Color;
//!
//! let palette = Palette::new(Color::parse("#0178D4").unwrap()).dark(true);
//! let colors = palette.colors();
//! let get = |name: &str| colors.iter().find(|(n, _)| n == name).unwrap().1.clone();
//! assert_eq!(get("primary-lighten-1"), "#368AE9");
//! assert_eq!(get("background"), "#121212");
//! let theme = Theme::from_palette(palette);
//! assert!(theme.styles.iter().any(|(name, _)| name == "primary-muted"));
//! ```

use rich::Color;

/// How many shades lighter and darker each role has.
const SHADES: i32 = 3;

/// A colour as Textual keeps one: channels that may leave 0 to 255 until
/// they are clamped, and an alpha.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Rgba {
    r: i32,
    g: i32,
    b: i32,
    a: f64,
}

const WHITE: Rgba = Rgba::rgb(255, 255, 255);
const BLACK: Rgba = Rgba::rgb(0, 0, 0);
const TRANSPARENT: Rgba = Rgba {
    r: 0,
    g: 0,
    b: 0,
    a: 0.0,
};

impl Rgba {
    const fn rgb(r: i32, g: i32, b: i32) -> Rgba {
        Rgba { r, g, b, a: 1.0 }
    }

    /// `#rgb`, `#rrggbb` or `#rrggbbaa`, optionally followed by an alpha
    /// as a percentage (`#81a1c1 35%`), as Textual's variables write them.
    fn parse(text: &str) -> Option<Rgba> {
        let mut parts = text.split_whitespace();
        let hex = parts.next()?.strip_prefix('#')?;
        let channel = |i: usize| i32::from_str_radix(hex.get(i..i + 2)?, 16).ok();
        let mut color = match hex.len() {
            3 => {
                let digit = |i: usize| i32::from_str_radix(hex.get(i..=i)?, 16).ok();
                Rgba::rgb(digit(0)? * 17, digit(1)? * 17, digit(2)? * 17)
            }
            6 => Rgba::rgb(channel(0)?, channel(2)?, channel(4)?),
            8 => Rgba {
                a: channel(6)? as f64 / 255.0,
                ..Rgba::rgb(channel(0)?, channel(2)?, channel(4)?)
            },
            _ => return None,
        };
        if let Some(percent) = parts.next() {
            color.a = percent.strip_suffix('%')?.parse::<f64>().ok()? / 100.0;
        }
        Some(color)
    }

    fn from_color(color: &Color) -> Option<Rgba> {
        let triplet = color.get_truecolor()?;
        Some(Rgba::rgb(
            triplet.red as i32,
            triplet.green as i32,
            triplet.blue as i32,
        ))
    }

    fn clamped(self) -> Rgba {
        Rgba {
            r: self.r.clamp(0, 255),
            g: self.g.clamp(0, 255),
            b: self.b.clamp(0, 255),
            a: self.a.clamp(0.0, 1.0),
        }
    }

    /// `#RRGGBB`, or `#RRGGBBAA` with an alpha.
    fn hex(self) -> String {
        let c = self.clamped();
        if c.a == 1.0 {
            format!("#{:02X}{:02X}{:02X}", c.r, c.g, c.b)
        } else {
            let a = (c.a * 255.0) as i32;
            format!("#{:02X}{:02X}{:02X}{a:02X}", c.r, c.g, c.b)
        }
    }

    fn with_alpha(self, a: f64) -> Rgba {
        Rgba { a, ..self }
    }

    fn inverse(self) -> Rgba {
        Rgba {
            r: 255 - self.r,
            g: 255 - self.g,
            b: 255 - self.b,
            a: self.a,
        }
    }

    fn brightness(self) -> f64 {
        let (r, g, b) = (
            self.r as f64 / 255.0,
            self.g as f64 / 255.0,
            self.b as f64 / 255.0,
        );
        (299.0 * r + 587.0 * g + 114.0 * b) / 1000.0
    }

    /// White on a dark colour, black on a light one, at `alpha`.
    fn contrast_text(self, alpha: f64) -> Rgba {
        let text = if self.brightness() < 0.5 {
            WHITE
        } else {
            BLACK
        };
        text.with_alpha(alpha)
    }

    fn blend(self, to: Rgba, factor: f64, alpha: Option<f64>) -> Rgba {
        if factor <= 0.0 {
            return self;
        }
        if factor >= 1.0 {
            return to;
        }
        let mix = |a: i32, b: i32| (a as f64 + (b - a) as f64 * factor) as i32;
        Rgba {
            r: mix(self.r, to.r),
            g: mix(self.g, to.g),
            b: mix(self.b, to.b),
            a: alpha.unwrap_or(self.a + (to.a - self.a) * factor),
        }
    }

    /// `to` laid over this colour by its alpha, keeping this one's.
    fn tint(self, to: Rgba) -> Rgba {
        let mix = |a: i32, b: i32| (a as f64 + (b - a) as f64 * to.a) as i32;
        Rgba {
            r: mix(self.r, to.r),
            g: mix(self.g, to.g),
            b: mix(self.b, to.b),
            a: self.a,
        }
    }

    /// `over` composited on this colour: Textual's `+`.
    fn over(self, over: Rgba) -> Rgba {
        self.blend(over, over.a, Some(1.0))
    }

    /// Lighter (or darker, for a negative `amount`) by `amount` of CIE
    /// L\*a\*b\*'s lightness.
    fn lighten(self, amount: f64) -> Rgba {
        let (l, a, b) = rgb_to_lab(self);
        lab_to_rgb(l + amount * 100.0, a, b, self.a).clamped()
    }

    fn darken(self, amount: f64) -> Rgba {
        self.lighten(-amount)
    }
}

/// sRGB (D65, 2°) to CIE L\*a\*b\*, as Textual converts it.
fn rgb_to_lab(c: Rgba) -> (f64, f64, f64) {
    let linear = |v: i32| {
        let v = v as f64 / 255.0;
        if v > 0.04045 {
            ((v + 0.055) / 1.055).powf(2.4)
        } else {
            v / 12.92
        }
    };
    let (r, g, b) = (linear(c.r), linear(c.g), linear(c.b));
    let x = (r * 41.24 + g * 35.76 + b * 18.05) / 95.047;
    let y = (r * 21.26 + g * 71.52 + b * 7.22) / 100.0;
    let z = (r * 1.93 + g * 11.92 + b * 95.05) / 108.883;
    let off = 16.0 / 116.0;
    let f = |v: f64| {
        if v > 0.008856 {
            v.powf(1.0 / 3.0)
        } else {
            7.787 * v + off
        }
    };
    let (x, y, z) = (f(x), f(y), f(z));
    (116.0 * y - 16.0, 500.0 * (x - y), 200.0 * (y - z))
}

fn lab_to_rgb(l: f64, a: f64, b: f64, alpha: f64) -> Rgba {
    let y = (l + 16.0) / 116.0;
    let x = a / 500.0 + y;
    let z = y - b / 200.0;
    let off = 16.0 / 116.0;
    let y = if y > 0.206_893_034_4 {
        y.powf(3.0)
    } else {
        (y - off) / 7.787
    };
    let x = if x > 0.206_893_034_4 {
        0.95047 * x.powf(3.0)
    } else {
        0.122059 * (x - off)
    };
    let z = if z > 0.206_893_034_4 {
        1.08883 * z.powf(3.0)
    } else {
        0.139827 * (z - off)
    };
    let r = x * 3.2406 + y * -1.5372 + z * -0.4986;
    let g = x * -0.9689 + y * 1.8758 + z * 0.0415;
    let b = x * 0.0557 + y * -0.2040 + z * 1.0570;
    let gamma = |v: f64| {
        if v > 0.0031308 {
            1.055 * v.powf(1.0 / 2.4) - 0.055
        } else {
            12.92 * v
        }
    };
    Rgba {
        r: (gamma(r) * 255.0) as i32,
        g: (gamma(g) * 255.0) as i32,
        b: (gamma(b) * 255.0) as i32,
        a: alpha,
    }
}

/// One of a palette's generated colours: a colour, or, in an ANSI palette,
/// a terminal colour by name (`blue`, `default`), possibly faded.
#[derive(Clone, Debug, PartialEq)]
enum Value {
    Rgba(Rgba),
    Ansi(String),
    /// A terminal colour at half strength (Textual's `ansi_blue 50%`).
    Faded(String),
}

/// The roles Textual shades, in its order.
const SHADED: [&str; 13] = [
    "primary",
    "secondary",
    "primary-background",
    "secondary-background",
    "background",
    "foreground",
    "panel",
    "boost",
    "surface",
    "warning",
    "error",
    "success",
    "accent",
];

/// The roles with a `text-` and a `-muted` colour.
const ROLES: [&str; 6] = [
    "primary",
    "secondary",
    "warning",
    "error",
    "success",
    "accent",
];

/// The suffix of shade `n`, from `-darken-3` (`-3`) to `-lighten-3`.
fn shade_suffix(n: i32) -> String {
    match n {
        0 => String::new(),
        n if n < 0 => format!("-darken-{}", -n),
        n => format!("-lighten-{n}"),
    }
}

/// The semantic colours of a theme, as Textual's themes give them: from
/// these a [`Theme`](crate::Theme) generates its shades
/// ([`Theme::from_palette`](crate::Theme::from_palette)). Only `primary` is
/// needed; `secondary`, `warning` and `accent` default to it, `error` and
/// `success` to `secondary`, `background` and `surface` to Textual's dark
/// (`#121212`, `#1e1e1e`) or light (`#efefef`, `#f5f5f5`) ones, and
/// `foreground` to the background's inverse.
///
/// A palette whose primary is a terminal colour (`blue`, `bright_red`)
/// is an ANSI palette, as Textual's `ansi-dark` and `ansi-light` are: its
/// shades are the colours themselves, and its background and foreground
/// the terminal's own.
#[derive(Clone, Debug, PartialEq)]
#[must_use = "a palette does nothing until a theme is made from it"]
pub struct Palette {
    primary: Color,
    secondary: Option<Color>,
    warning: Option<Color>,
    error: Option<Color>,
    success: Option<Color>,
    accent: Option<Color>,
    foreground: Option<Color>,
    background: Option<Color>,
    surface: Option<Color>,
    panel: Option<Color>,
    boost: Option<Color>,
    dark: bool,
    luminosity_spread: f64,
    variables: Vec<(String, String)>,
}

/// One of Textual's built-in themes, as data.
struct Builtin {
    name: &'static str,
    primary: &'static str,
    secondary: Option<&'static str>,
    warning: Option<&'static str>,
    error: Option<&'static str>,
    success: Option<&'static str>,
    accent: Option<&'static str>,
    foreground: Option<&'static str>,
    background: Option<&'static str>,
    surface: Option<&'static str>,
    panel: Option<&'static str>,
    boost: Option<&'static str>,
    dark: bool,
    /// Textual's overrides of generated colours, those this port makes.
    variables: &'static [(&'static str, &'static str)],
}

/// Textual's built-in themes (`textual/theme.py`, Textual 8.2.8; MIT).
#[rustfmt::skip]
const BUILTINS: &[Builtin] = &[
    Builtin { name: "textual-dark", primary: "#0178D4", secondary: Some("#004578"), warning: Some("#ffa62b"), error: Some("#ba3c5b"), success: Some("#4EBF71"), accent: Some("#ffa62b"), foreground: Some("#e0e0e0"), background: None, surface: None, panel: None, boost: None, dark: true, variables: &[] },
    Builtin { name: "textual-light", primary: "#004578", secondary: Some("#0178D4"), warning: Some("#ffa62b"), error: Some("#ba3c5b"), success: Some("#4EBF71"), accent: Some("#ffa62b"), foreground: None, background: Some("#E0E0E0"), surface: Some("#D8D8D8"), panel: Some("#D0D0D0"), boost: None, dark: false, variables: &[] },
    Builtin { name: "nord", primary: "#88C0D0", secondary: Some("#81A1C1"), warning: Some("#EBCB8B"), error: Some("#BF616A"), success: Some("#A3BE8C"), accent: Some("#B48EAD"), foreground: Some("#D8DEE9"), background: Some("#2E3440"), surface: Some("#3B4252"), panel: Some("#434C5E"), boost: None, dark: true, variables: &[("block-cursor-background", "#88C0D0")] },
    Builtin { name: "gruvbox", primary: "#85A598", secondary: Some("#A89A85"), warning: Some("#fe8019"), error: Some("#fb4934"), success: Some("#b8bb26"), accent: Some("#fabd2f"), foreground: Some("#fbf1c7"), background: Some("#282828"), surface: Some("#3c3836"), panel: Some("#504945"), boost: None, dark: true, variables: &[] },
    Builtin { name: "catppuccin-mocha", primary: "#F5C2E7", secondary: Some("#cba6f7"), warning: Some("#FAE3B0"), error: Some("#F28FAD"), success: Some("#ABE9B3"), accent: Some("#fab387"), foreground: Some("#cdd6f4"), background: Some("#181825"), surface: Some("#313244"), panel: Some("#45475a"), boost: None, dark: true, variables: &[("border", "#b4befe"), ("border-blurred", "#585b70")] },
    Builtin { name: "dracula", primary: "#BD93F9", secondary: Some("#6272A4"), warning: Some("#FFB86C"), error: Some("#FF5555"), success: Some("#50FA7B"), accent: Some("#FF79C6"), foreground: Some("#F8F8F2"), background: Some("#282A36"), surface: Some("#2B2E3B"), panel: Some("#313442"), boost: None, dark: true, variables: &[] },
    Builtin { name: "tokyo-night", primary: "#BB9AF7", secondary: Some("#7AA2F7"), warning: Some("#E0AF68"), error: Some("#F7768E"), success: Some("#9ECE6A"), accent: Some("#FF9E64"), foreground: Some("#a9b1d6"), background: Some("#1A1B26"), surface: Some("#24283B"), panel: Some("#414868"), boost: None, dark: true, variables: &[] },
    Builtin { name: "monokai", primary: "#AE81FF", secondary: Some("#F92672"), warning: Some("#FD971F"), error: Some("#F92672"), success: Some("#A6E22E"), accent: Some("#66D9EF"), foreground: Some("#d6d6d6"), background: Some("#272822"), surface: Some("#2e2e2e"), panel: Some("#3E3D32"), boost: None, dark: true, variables: &[("foreground-muted", "#797979")] },
    Builtin { name: "flexoki", primary: "#205EA6", secondary: Some("#24837B"), warning: Some("#AD8301"), error: Some("#AF3029"), success: Some("#66800B"), accent: Some("#9B76C8"), foreground: Some("#FFFCF0"), background: Some("#100F0F"), surface: Some("#1C1B1A"), panel: Some("#282726"), boost: None, dark: true, variables: &[] },
    Builtin { name: "catppuccin-latte", primary: "#8839EF", secondary: Some("#DC8A78"), warning: Some("#DF8E1D"), error: Some("#D20F39"), success: Some("#40A02B"), accent: Some("#FE640B"), foreground: Some("#4C4F69"), background: Some("#EFF1F5"), surface: Some("#E6E9EF"), panel: Some("#CCD0DA"), boost: None, dark: false, variables: &[] },
    Builtin { name: "catppuccin-frappe", primary: "#CA9EE6", secondary: Some("#EF9F76"), warning: Some("#E5C890"), error: Some("#E78284"), success: Some("#A6D189"), accent: Some("#F4B8E4"), foreground: Some("#C6D0F5"), background: Some("#303446"), surface: Some("#414559"), panel: Some("#51576D"), boost: None, dark: true, variables: &[("border", "#BABBF1"), ("border-blurred", "#838BA7")] },
    Builtin { name: "catppuccin-macchiato", primary: "#C6A0F6", secondary: Some("#F5A97F"), warning: Some("#EED49F"), error: Some("#ED8796"), success: Some("#A6DA95"), accent: Some("#F5BDE6"), foreground: Some("#CAD3F5"), background: Some("#24273A"), surface: Some("#363A4F"), panel: Some("#494D64"), boost: None, dark: true, variables: &[("border", "#B7BDF8"), ("border-blurred", "#737994")] },
    Builtin { name: "solarized-light", primary: "#268bd2", secondary: Some("#2aa198"), warning: Some("#cb4b16"), error: Some("#dc322f"), success: Some("#859900"), accent: Some("#6c71c4"), foreground: Some("#586e75"), background: Some("#fdf6e3"), surface: Some("#eee8d5"), panel: Some("#eee8d5"), boost: None, dark: false, variables: &[] },
    Builtin { name: "solarized-dark", primary: "#268bd2", secondary: Some("#2aa198"), warning: Some("#cb4b16"), error: Some("#dc322f"), success: Some("#859900"), accent: Some("#6c71c4"), foreground: Some("#839496"), background: Some("#002b36"), surface: Some("#073642"), panel: Some("#073642"), boost: None, dark: true, variables: &[] },
    Builtin { name: "rose-pine", primary: "#c4a7e7", secondary: Some("#31748f"), warning: Some("#f6c177"), error: Some("#eb6f92"), success: Some("#9ccfd8"), accent: Some("#ebbcba"), foreground: Some("#e0def4"), background: Some("#191724"), surface: Some("#1f1d2e"), panel: Some("#26233a"), boost: None, dark: true, variables: &[("border", "#524f67"), ("border-blurred", "#6e6a86"), ("block-cursor-background", "#c4a7e7")] },
    Builtin { name: "rose-pine-moon", primary: "#c4a7e7", secondary: Some("#3e8fb0"), warning: Some("#f6c177"), error: Some("#eb6f92"), success: Some("#9ccfd8"), accent: Some("#ea9a97"), foreground: Some("#e0def4"), background: Some("#232136"), surface: Some("#2a273f"), panel: Some("#393552"), boost: None, dark: true, variables: &[("border", "#56526e"), ("border-blurred", "#6e6a86"), ("block-cursor-background", "#c4a7e7")] },
    Builtin { name: "rose-pine-dawn", primary: "#907aa9", secondary: Some("#286983"), warning: Some("#ea9d34"), error: Some("#b4637a"), success: Some("#56949f"), accent: Some("#d7827e"), foreground: Some("#575279"), background: Some("#faf4ed"), surface: Some("#fffaf3"), panel: Some("#f2e9e1"), boost: None, dark: false, variables: &[("border", "#cecacd"), ("border-blurred", "#9893a5"), ("block-cursor-background", "#575279")] },
    Builtin { name: "atom-one-dark", primary: "#61AFEF", secondary: Some("#C678DD"), warning: Some("#DEB25B"), error: Some("#F06262"), success: Some("#62F062"), accent: Some("#A378C2"), foreground: Some("#ABB2BF"), background: Some("#282C34"), surface: Some("#3B414D"), panel: Some("#4F5666"), boost: None, dark: true, variables: &[] },
    Builtin { name: "atom-one-light", primary: "#4078F2", secondary: Some("#A626A4"), warning: Some("#D8D938"), error: Some("#F23F3F"), success: Some("#6CF23F"), accent: Some("#bf9232"), foreground: Some("#383A42"), background: Some("#FAFAFA"), surface: Some("#E0E0E0"), panel: Some("#CCCCCC"), boost: None, dark: false, variables: &[] },
    Builtin { name: "ansi-dark", primary: "blue", secondary: Some("cyan"), warning: Some("yellow"), error: Some("red"), success: Some("green"), accent: Some("green"), foreground: None, background: None, surface: None, panel: None, boost: None, dark: true, variables: &[("border-blurred", "black"), ("block-cursor-background", "white")] },
    Builtin { name: "ansi-light", primary: "blue", secondary: Some("cyan"), warning: Some("bright_red"), error: Some("red"), success: Some("green"), accent: Some("magenta"), foreground: None, background: None, surface: None, panel: None, boost: None, dark: false, variables: &[("border", "blue"), ("border-blurred", "white"), ("block-cursor-background", "blue"), ("scrollbar", "bright_blue"), ("scrollbar-hover", "blue"), ("scrollbar-background", "white")] },
];

fn parse_builtin(spec: &str) -> Color {
    Color::parse(spec).expect("a built-in colour parses")
}

impl Palette {
    /// A palette of `primary` alone, light, which every other role
    /// defaults from.
    pub fn new(primary: Color) -> Palette {
        Palette {
            primary,
            secondary: None,
            warning: None,
            error: None,
            success: None,
            accent: None,
            foreground: None,
            background: None,
            surface: None,
            panel: None,
            boost: None,
            dark: false,
            luminosity_spread: 0.15,
            variables: Vec::new(),
        }
    }

    pub fn secondary(mut self, color: Color) -> Palette {
        self.secondary = Some(color);
        self
    }

    pub fn warning(mut self, color: Color) -> Palette {
        self.warning = Some(color);
        self
    }

    pub fn error(mut self, color: Color) -> Palette {
        self.error = Some(color);
        self
    }

    pub fn success(mut self, color: Color) -> Palette {
        self.success = Some(color);
        self
    }

    pub fn accent(mut self, color: Color) -> Palette {
        self.accent = Some(color);
        self
    }

    /// The text colour (default: the background's inverse).
    pub fn foreground(mut self, color: Color) -> Palette {
        self.foreground = Some(color);
        self
    }

    /// Where there is no content: the screen.
    pub fn background(mut self, color: Color) -> Palette {
        self.background = Some(color);
        self
    }

    /// What content sits on: a panel's, a list's, an input's ground.
    pub fn surface(mut self, color: Color) -> Palette {
        self.surface = Some(color);
        self
    }

    /// A panel apart from the surface (default: the surface a tenth of the
    /// way to primary, and in a dark palette with the boost over it).
    pub fn panel(mut self, color: Color) -> Palette {
        self.panel = Some(color);
        self
    }

    /// A slight lightening laid over a dark palette's panel (default:
    /// white at 4%).
    pub fn boost(mut self, color: Color) -> Palette {
        self.boost = Some(color);
        self
    }

    /// For a dark terminal (`true`) or a light one (the default).
    pub fn dark(mut self, dark: bool) -> Palette {
        self.dark = dark;
        self
    }

    /// Set the generated colour `name` (`border`, `text-muted`,
    /// `block-cursor-background`) to `value` (`#88C0D0`, `#81a1c1 35%`,
    /// or a terminal colour in an ANSI palette), as a Textual theme's
    /// `variables` do. A value that does not parse is ignored.
    pub fn variable(mut self, name: &str, value: &str) -> Palette {
        self.variables.retain(|(n, _)| n != name);
        self.variables.push((name.to_string(), value.to_string()));
        self
    }

    /// Whether it is for a dark terminal.
    pub fn is_dark(&self) -> bool {
        self.dark
    }

    /// Textual's built-in theme `name`, as a palette: `textual-dark`,
    /// `textual-light`, `nord`, `gruvbox`, `catppuccin-mocha`, `dracula`,
    /// `tokyo-night`, `monokai`, `flexoki`, `catppuccin-latte`,
    /// `catppuccin-frappe`, `catppuccin-macchiato`, `solarized-light`,
    /// `solarized-dark`, `rose-pine`, `rose-pine-moon`, `rose-pine-dawn`,
    /// `atom-one-dark`, `atom-one-light`, `ansi-dark` or `ansi-light`
    /// ([`names`](Self::names)).
    pub fn named(name: &str) -> Option<Palette> {
        let theme = BUILTINS.iter().find(|b| b.name == name)?;
        let optional = |spec: Option<&str>| spec.map(parse_builtin);
        let mut palette = Palette::new(parse_builtin(theme.primary));
        palette.secondary = optional(theme.secondary);
        palette.warning = optional(theme.warning);
        palette.error = optional(theme.error);
        palette.success = optional(theme.success);
        palette.accent = optional(theme.accent);
        palette.foreground = optional(theme.foreground);
        palette.background = optional(theme.background);
        palette.surface = optional(theme.surface);
        palette.panel = optional(theme.panel);
        palette.boost = optional(theme.boost);
        palette.dark = theme.dark;
        for (name, value) in theme.variables {
            palette = palette.variable(name, value);
        }
        Some(palette)
    }

    /// The names of Textual's built-in themes, in Textual's order.
    pub fn names() -> Vec<&'static str> {
        BUILTINS.iter().map(|b| b.name).collect()
    }

    /// Whether the palette is of terminal colours.
    fn is_ansi(&self) -> bool {
        self.primary.is_system_defined()
    }

    /// The colours it generates, by Textual's names, as CSS: `#RRGGBB`,
    /// `#RRGGBBAA` where Textual gives an alpha (`boost`, `text`), or a
    /// terminal colour's name in an ANSI palette (`blue 50%` faded).
    pub fn colors(&self) -> Vec<(String, String)> {
        self.generate()
            .into_iter()
            .map(|(name, value)| {
                let css = match value {
                    Value::Rgba(c) => c.hex(),
                    Value::Ansi(name) => name,
                    Value::Faded(name) => format!("{name} 50%"),
                };
                (name, css)
            })
            .collect()
    }

    /// The colours, each an opaque style colour: one with an alpha laid
    /// over the background, a faded terminal colour dimmed. `(name, colour,
    /// dim)`.
    pub(crate) fn solid(&self) -> Vec<(String, String, bool)> {
        let generated = self.generate();
        let background =
            generated
                .iter()
                .find(|(n, _)| n == "background")
                .and_then(|(_, v)| match v {
                    Value::Rgba(c) => Some(*c),
                    _ => None,
                });
        generated
            .into_iter()
            .map(|(name, value)| match value {
                Value::Rgba(c) => {
                    let c = match (c.a < 1.0, background) {
                        (true, Some(bg)) => bg.over(c),
                        _ => c.with_alpha(1.0),
                    };
                    (name, c.hex(), false)
                }
                Value::Ansi(color) => (name, color, false),
                Value::Faded(color) => (name, color, true),
            })
            .collect()
    }

    fn rgba(color: &Color) -> Option<Rgba> {
        Rgba::from_color(color)
    }

    /// A variable's colour, when it is set and parses.
    fn var(&self, name: &str) -> Option<Rgba> {
        let (_, value) = self.variables.iter().find(|(n, _)| n == name)?;
        Rgba::parse(value)
    }

    fn generate(&self) -> Vec<(String, Value)> {
        if self.is_ansi() {
            return self.generate_ansi();
        }
        let pick = |c: &Option<Color>| c.as_ref().and_then(Palette::rgba);
        let primary = Palette::rgba(&self.primary).unwrap_or(WHITE);
        let secondary = pick(&self.secondary).unwrap_or(primary);
        let warning = pick(&self.warning).unwrap_or(primary);
        let error = pick(&self.error).unwrap_or(secondary);
        let success = pick(&self.success).unwrap_or(secondary);
        let accent = pick(&self.accent).unwrap_or(primary);
        let dark = self.dark;
        let spread = self.luminosity_spread;
        let (default_background, default_surface) = if dark {
            ("#121212", "#1e1e1e")
        } else {
            ("#efefef", "#f5f5f5")
        };
        let background = pick(&self.background)
            .unwrap_or_else(|| Rgba::parse(default_background).expect("parses"));
        let surface =
            pick(&self.surface).unwrap_or_else(|| Rgba::parse(default_surface).expect("parses"));
        let foreground = pick(&self.foreground).unwrap_or_else(|| background.inverse());
        let get = |name: &str, default: Rgba| self.var(name).unwrap_or(default);

        let mut out: Vec<(String, Rgba)> = Vec::new();
        let contrast = background.contrast_text(1.0);
        for (role, color) in ROLES
            .iter()
            .zip([primary, secondary, warning, error, success, accent])
        {
            out.push((
                format!("text-{role}"),
                contrast.tint(color.with_alpha(0.66)),
            ));
        }
        let mut boost = TRANSPARENT;
        let panel = match pick(&self.panel) {
            Some(panel) => panel,
            None => {
                let panel = surface.blend(primary, 0.1, Some(1.0));
                if dark {
                    boost = pick(&self.boost).unwrap_or(contrast.with_alpha(0.04));
                    panel.over(boost)
                } else {
                    panel
                }
            }
        };
        let colors = [
            primary, secondary, primary, secondary, background, foreground, panel, boost, surface,
            warning, error, success, accent,
        ];
        for (name, color) in SHADED.iter().zip(colors) {
            let dark_shade = dark && name.ends_with("-background") && *name != "background";
            for n in -SHADES..=SHADES {
                let delta = n as f64 * spread / 2.0;
                let key = format!("{name}{}", shade_suffix(n));
                let shade = if dark_shade {
                    self.var(&key).unwrap_or_else(|| {
                        background
                            .blend(color, 0.15, Some(1.0))
                            .blend(WHITE, spread + delta, Some(1.0))
                            .clamped()
                    })
                } else {
                    get(&key, color.lighten(delta))
                };
                out.push((key, shade));
            }
        }
        for (name, alpha) in [("text", 0.87), ("text-muted", 0.6), ("text-disabled", 0.38)] {
            out.push((name.to_string(), get(name, background.contrast_text(alpha))));
        }
        for (role, color) in ROLES
            .iter()
            .zip([primary, secondary, warning, error, success, accent])
        {
            let key = format!("{role}-muted");
            let muted = get(&key, color.blend(background, 0.7, None));
            out.push((key, muted));
        }
        out.push((
            "foreground-muted".into(),
            get("foreground-muted", foreground.with_alpha(0.6)),
        ));
        out.push((
            "foreground-disabled".into(),
            get("foreground-disabled", foreground.with_alpha(0.38)),
        ));
        out.push(("border".into(), get("border", primary)));
        out.push((
            "border-blurred".into(),
            get("border-blurred", surface.darken(0.025)),
        ));
        out.push((
            "surface-active".into(),
            get("surface-active", surface.lighten(spread / 2.5)),
        ));
        let darker = out
            .iter()
            .find(|(n, _)| n == "background-darken-1")
            .map(|(_, c)| *c)
            .expect("generated");
        out.push((
            "scrollbar".into(),
            get("scrollbar", darker.over(primary.with_alpha(0.4))),
        ));
        out.push((
            "scrollbar-hover".into(),
            get("scrollbar-hover", darker.over(primary.with_alpha(0.5))),
        ));
        out.push((
            "scrollbar-background".into(),
            get("scrollbar-background", darker),
        ));
        out.push((
            "block-cursor-background".into(),
            get("block-cursor-background", primary),
        ));
        out.into_iter().map(|(n, c)| (n, Value::Rgba(c))).collect()
    }

    /// An ANSI palette's colours: the terminal's, by name.
    fn generate_ansi(&self) -> Vec<(String, Value)> {
        let name = |c: &Color| c.name.to_string();
        let pick = |c: &Option<Color>| c.as_ref().map(name);
        let primary = name(&self.primary);
        let secondary = pick(&self.secondary).unwrap_or_else(|| primary.clone());
        let warning = pick(&self.warning).unwrap_or_else(|| primary.clone());
        let error = pick(&self.error).unwrap_or_else(|| secondary.clone());
        let success = pick(&self.success).unwrap_or_else(|| secondary.clone());
        let accent = pick(&self.accent).unwrap_or_else(|| primary.clone());
        let background = pick(&self.background).unwrap_or_else(|| "default".into());
        let foreground = pick(&self.foreground).unwrap_or_else(|| "default".into());
        let roles = [&primary, &secondary, &warning, &error, &success, &accent];
        let var = |key: &str, default: &str| -> Value {
            let value = self
                .variables
                .iter()
                .find(|(n, _)| n == key)
                .map_or(default, |(_, v)| v.as_str());
            Value::Ansi(value.strip_prefix("ansi_").unwrap_or(value).to_string())
        };
        let mut out: Vec<(String, Value)> = Vec::new();
        for (role, color) in ROLES.iter().zip(roles) {
            out.push((format!("text-{role}"), Value::Ansi(color.clone())));
        }
        let base = [
            primary.clone(),
            secondary.clone(),
            background.clone(),
            background.clone(),
            background.clone(),
            foreground.clone(),
            "default".into(),
            "default".into(),
            "default".into(),
            warning.clone(),
            error.clone(),
            success.clone(),
            accent.clone(),
        ];
        for (name, color) in SHADED.iter().zip(base) {
            for n in -SHADES..=SHADES {
                out.push((
                    format!("{name}{}", shade_suffix(n)),
                    Value::Ansi(color.clone()),
                ));
            }
        }
        out.push(("text".into(), Value::Ansi("default".into())));
        out.push(("text-muted".into(), Value::Faded("default".into())));
        out.push(("text-disabled".into(), Value::Faded("default".into())));
        for (role, color) in ROLES.iter().zip(roles) {
            out.push((format!("{role}-muted"), Value::Faded(color.clone())));
        }
        out.push(("foreground-muted".into(), Value::Faded("default".into())));
        out.push(("foreground-disabled".into(), Value::Faded("default".into())));
        out.push(("border".into(), var("border", "magenta")));
        out.push(("border-blurred".into(), var("border-blurred", "black")));
        out.push(("surface-active".into(), Value::Ansi("default".into())));
        out.push(("scrollbar".into(), var("scrollbar", "blue")));
        out.push(("scrollbar-hover".into(), var("scrollbar-hover", "blue")));
        out.push((
            "scrollbar-background".into(),
            var("scrollbar-background", "black"),
        ));
        out.push((
            "block-cursor-background".into(),
            var("block-cursor-background", "magenta"),
        ));
        out
    }
}

/// White or black, whichever reads on the opaque colour `hex`.
pub(crate) fn contrast_on(hex: &str) -> String {
    Rgba::parse(hex).map_or_else(|| "default".to_string(), |c| c.contrast_text(1.0).hex())
}

/// Whether a generated colour is a ground (its style is `on` it).
pub(crate) fn is_ground(name: &str) -> bool {
    [
        "background",
        "surface",
        "panel",
        "boost",
        "primary-background",
        "secondary-background",
    ]
    .iter()
    .any(|ground| name == *ground || name.starts_with(&format!("{ground}-")))
        || name.ends_with("-background")
        || name == "surface-active"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lab_round_trips_textual_warning() {
        // Textual's `warning` is `#ffa62b` lightened by nothing: `#FEA62B`.
        let warning = Rgba::parse("#ffa62b").unwrap();
        assert_eq!(warning.lighten(0.0).hex(), "#FEA62B");
    }

    #[test]
    fn variables_parse_textual_alphas() {
        assert_eq!(Rgba::parse("#81a1c1 35%").unwrap().hex(), "#81A1C159");
        assert_eq!(Rgba::parse("#689d6a40").unwrap().a, 64.0 / 255.0);
        assert!(Rgba::parse("ansi_blue").is_none());
    }

    #[test]
    fn every_builtin_parses() {
        for name in Palette::names() {
            let palette = Palette::named(name).unwrap();
            assert!(!palette.colors().is_empty(), "{name}");
        }
        assert_eq!(Palette::names().len(), 21);
    }
}
