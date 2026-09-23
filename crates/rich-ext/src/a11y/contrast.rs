//! Colour checks for themes: WCAG 2.x contrast, colour-only distinctions and
//! colour-vision-deficiency (CVD) confusion.
//!
//! * **Contrast** — WCAG 2.x relative luminance and contrast ratio
//!   (<https://www.w3.org/TR/WCAG22/#dfn-contrast-ratio>). Colours are
//!   resolved against each [`TerminalTheme`] in [`CheckOptions::backgrounds`]
//!   (standard colours through the theme palette, 256-colour and truecolour
//!   directly); `reverse` swaps foreground and background and `dim` blends the
//!   foreground halfway to the background, as core's HTML export does.
//! * **Colour-only distinction** — two styles of a group that become identical
//!   once colour is stripped (the monochrome / `NO_COLOR` fallback).
//! * **CVD confusion** — both colours are simulated for protanopia and
//!   deuteranopia with Viénot, Brettel & Mollon (1999) and for tritanopia with
//!   Brettel, Viénot & Mollon (1997), using the linear-RGB matrices published
//!   by DaltonLens (<https://daltonlens.org>, `libDaltonLens`). The simulated
//!   pair is compared with CIEDE2000 ΔE (Sharma, Wu & Dalal 2005) in CIELAB
//!   (D65); a pair under [`CheckOptions::cvd_threshold`] (default 10) is
//!   reported. 2.3 is the usual just-noticeable difference for large patches;
//!   small terminal glyphs need far more to be told apart at a glance, hence 10.

use crate::fidelity::style_without_color;
use rich::terminal_theme::blend_rgb;
use rich::{
    ColorTriplet, Console, ConsoleOptions, Renderable, Segment, Style, Table, TerminalTheme, Text,
    Theme, DEFAULT_TERMINAL_THEME, MONOKAI,
};

/// sRGB channel (0–255) to linear light.
pub fn linearize(channel: u8) -> f64 {
    let c = channel as f64 / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn delinearize(v: f64) -> u8 {
    let v = v.clamp(0.0, 1.0);
    let c = if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    };
    (c * 255.0).round() as u8
}

/// WCAG relative luminance.
pub fn relative_luminance(c: ColorTriplet) -> f64 {
    0.2126 * linearize(c.red) + 0.7152 * linearize(c.green) + 0.0722 * linearize(c.blue)
}

/// WCAG contrast ratio, 1.0 to 21.0.
pub fn contrast_ratio(a: ColorTriplet, b: ColorTriplet) -> f64 {
    let (la, lb) = (relative_luminance(a), relative_luminance(b));
    let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

/// The foreground and background `style` shows under `theme`.
pub fn resolve_style(style: &Style, theme: &TerminalTheme) -> (ColorTriplet, ColorTriplet) {
    let mut fg = style
        .color()
        .map_or(theme.foreground, |c| theme.resolve(c, true));
    let mut bg = style
        .bgcolor()
        .map_or(theme.background, |c| theme.resolve(c, false));
    if style.attr(6) == Some(true) {
        std::mem::swap(&mut fg, &mut bg);
    }
    if style.attr(1) == Some(true) {
        fg = blend_rgb(fg, bg, 0.5);
    }
    (fg, bg)
}

/// A colour vision deficiency.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum Deficiency {
    Protan,
    Deutan,
    Tritan,
}

impl Deficiency {
    pub const ALL: [Deficiency; 3] = [Deficiency::Protan, Deficiency::Deutan, Deficiency::Tritan];
    pub fn name(self) -> &'static str {
        match self {
            Deficiency::Protan => "protanopia",
            Deficiency::Deutan => "deuteranopia",
            Deficiency::Tritan => "tritanopia",
        }
    }
}

type M3 = [[f64; 3]; 3];

// Viénot 1999, linear sRGB: `dl_vienot_*_rgbCvd_from_rgb` in libDaltonLens.c,
// checked value for value.
const VIENOT_PROTAN: M3 = [
    [0.11238, 0.88762, 0.0],
    [0.11238, 0.88762, 0.0],
    [0.00401, -0.00401, 1.0],
];
const VIENOT_DEUTAN: M3 = [
    [0.29275, 0.70725, 0.0],
    [0.29275, 0.70725, 0.0],
    [-0.02234, 0.02234, 1.0],
];
// Brettel 1997 tritan half-planes, linear sRGB: libDaltonLens.c, checked value
// for value.
const BRETTEL_TRITAN_1: M3 = [
    [1.01277, 0.13548, -0.14826],
    [-0.01243, 0.86812, 0.14431],
    [0.07589, 0.80500, 0.11911],
];
const BRETTEL_TRITAN_2: M3 = [
    [0.93678, 0.18979, -0.12657],
    [0.06154, 0.81526, 0.12320],
    [-0.37562, 1.12767, 0.24796],
];
const BRETTEL_TRITAN_NORMAL: [f64; 3] = [0.03901, -0.02788, -0.01113];

fn mul(m: &M3, v: [f64; 3]) -> [f64; 3] {
    [0, 1, 2].map(|r| m[r][0] * v[0] + m[r][1] * v[1] + m[r][2] * v[2])
}

fn linear(c: ColorTriplet) -> [f64; 3] {
    [linearize(c.red), linearize(c.green), linearize(c.blue)]
}

/// `c` as seen with `deficiency` (full dichromacy).
pub fn simulate(c: ColorTriplet, deficiency: Deficiency) -> ColorTriplet {
    let v = linear(c);
    let out = match deficiency {
        Deficiency::Protan => mul(&VIENOT_PROTAN, v),
        Deficiency::Deutan => mul(&VIENOT_DEUTAN, v),
        Deficiency::Tritan => {
            let n = BRETTEL_TRITAN_NORMAL;
            let side = v[0] * n[0] + v[1] * n[1] + v[2] * n[2];
            if side >= 0.0 {
                mul(&BRETTEL_TRITAN_1, v)
            } else {
                mul(&BRETTEL_TRITAN_2, v)
            }
        }
    };
    ColorTriplet::new(
        delinearize(out[0]),
        delinearize(out[1]),
        delinearize(out[2]),
    )
}

/// CIELAB (D65) of an sRGB colour.
pub fn to_lab(c: ColorTriplet) -> [f64; 3] {
    let [r, g, b] = linear(c);
    let x = (0.4124564 * r + 0.3575761 * g + 0.1804375 * b) / 0.95047;
    let y = 0.2126729 * r + 0.7151522 * g + 0.0721750 * b;
    let z = (0.0193339 * r + 0.1191920 * g + 0.9503041 * b) / 1.08883;
    let f = |t: f64| {
        if t > 216.0 / 24389.0 {
            t.cbrt()
        } else {
            (24389.0 / 27.0 * t + 16.0) / 116.0
        }
    };
    let (fx, fy, fz) = (f(x), f(y), f(z));
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

/// CIEDE2000 colour difference between two Lab colours.
pub fn ciede2000(lab1: [f64; 3], lab2: [f64; 3]) -> f64 {
    use std::f64::consts::PI;
    let deg = |r: f64| r * 180.0 / PI;
    let rad = |d: f64| d * PI / 180.0;
    let [l1, a1, b1] = lab1;
    let [l2, a2, b2] = lab2;
    let c1 = (a1 * a1 + b1 * b1).sqrt();
    let c2 = (a2 * a2 + b2 * b2).sqrt();
    let cbar = (c1 + c2) / 2.0;
    let g = 0.5 * (1.0 - (cbar.powi(7) / (cbar.powi(7) + 25f64.powi(7))).sqrt());
    let (a1p, a2p) = ((1.0 + g) * a1, (1.0 + g) * a2);
    let (c1p, c2p) = ((a1p * a1p + b1 * b1).sqrt(), (a2p * a2p + b2 * b2).sqrt());
    let hue = |b: f64, a: f64| {
        if b == 0.0 && a == 0.0 {
            0.0
        } else {
            let h = deg(b.atan2(a));
            if h < 0.0 {
                h + 360.0
            } else {
                h
            }
        }
    };
    let (h1p, h2p) = (hue(b1, a1p), hue(b2, a2p));
    let dlp = l2 - l1;
    let dcp = c2p - c1p;
    let dhp = if c1p * c2p == 0.0 {
        0.0
    } else if (h2p - h1p).abs() <= 180.0 {
        h2p - h1p
    } else if h2p - h1p > 180.0 {
        h2p - h1p - 360.0
    } else {
        h2p - h1p + 360.0
    };
    let dhp_big = 2.0 * (c1p * c2p).sqrt() * rad(dhp / 2.0).sin();
    let lbarp = (l1 + l2) / 2.0;
    let cbarp = (c1p + c2p) / 2.0;
    let hbarp = if c1p * c2p == 0.0 {
        h1p + h2p
    } else if (h1p - h2p).abs() <= 180.0 {
        (h1p + h2p) / 2.0
    } else if h1p + h2p < 360.0 {
        (h1p + h2p + 360.0) / 2.0
    } else {
        (h1p + h2p - 360.0) / 2.0
    };
    let t = 1.0 - 0.17 * rad(hbarp - 30.0).cos()
        + 0.24 * rad(2.0 * hbarp).cos()
        + 0.32 * rad(3.0 * hbarp + 6.0).cos()
        - 0.20 * rad(4.0 * hbarp - 63.0).cos();
    let dtheta = 30.0 * (-((hbarp - 275.0) / 25.0).powi(2)).exp();
    let rc = 2.0 * (cbarp.powi(7) / (cbarp.powi(7) + 25f64.powi(7))).sqrt();
    let sl = 1.0 + 0.015 * (lbarp - 50.0).powi(2) / (20.0 + (lbarp - 50.0).powi(2)).sqrt();
    let sc = 1.0 + 0.045 * cbarp;
    let sh = 1.0 + 0.015 * cbarp * t;
    let rt = -rad(2.0 * dtheta).sin() * rc;
    let (x, y, z) = (dlp / sl, dcp / sc, dhp_big / sh);
    (x * x + y * y + z * z + rt * y * z).sqrt()
}

/// CIEDE2000 between two sRGB colours.
pub fn delta_e(a: ColorTriplet, b: ColorTriplet) -> f64 {
    ciede2000(to_lab(a), to_lab(b))
}

fn to_hsl(c: ColorTriplet) -> (f64, f64, f64) {
    let (r, g, b) = (
        c.red as f64 / 255.0,
        c.green as f64 / 255.0,
        c.blue as f64 / 255.0,
    );
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let l = (max + min) / 2.0;
    if max == min {
        return (0.0, 0.0, l);
    }
    let d = max - min;
    let s = if l > 0.5 {
        d / (2.0 - max - min)
    } else {
        d / (max + min)
    };
    let h = if max == r {
        (g - b) / d + if g < b { 6.0 } else { 0.0 }
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    (h / 6.0, s, l)
}

fn from_hsl(h: f64, s: f64, l: f64) -> ColorTriplet {
    if s == 0.0 {
        let v = (l * 255.0).round() as u8;
        return ColorTriplet::new(v, v, v);
    }
    let q = if l < 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let p = 2.0 * l - q;
    let channel = |mut t: f64| {
        if t < 0.0 {
            t += 1.0;
        }
        if t > 1.0 {
            t -= 1.0;
        }
        let v = if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        };
        (v * 255.0).round() as u8
    };
    ColorTriplet::new(channel(h + 1.0 / 3.0), channel(h), channel(h - 1.0 / 3.0))
}

/// The colour nearest `fg` in HSL lightness (hue and saturation kept) that
/// reaches `min_ratio` against `bg`, if any.
pub fn suggest_color(fg: ColorTriplet, bg: ColorTriplet, min_ratio: f64) -> Option<ColorTriplet> {
    let (h, s, l) = to_hsl(fg);
    // Steps of one 8-bit level of lightness, darker first at each distance.
    (1..=255)
        .flat_map(|step| {
            let d = step as f64 / 255.0;
            [l - d, l + d]
        })
        .filter(|l| (0.0..=1.0).contains(l))
        .map(|l| from_hsl(h, s, l))
        .find(|c| contrast_ratio(*c, bg) >= min_ratio)
}

/// How serious a finding is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum Severity {
    Error,
    Warning,
    Info,
}

/// What a finding is about. Colours are `#rrggbb`.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(tag = "kind", rename_all = "snake_case"))]
pub enum FindingKind {
    /// The style's text contrast is under the minimum ratio.
    LowContrast { ratio: f64, fg: String, bg: String },
    /// Two styles differ only by colour.
    ColorOnlyDistinction { a: String, b: String },
    /// Two styles look alike under a colour vision deficiency.
    ColorBlindConfusable {
        a: String,
        b: String,
        deficiency: Deficiency,
        delta_e: f64,
    },
}

/// One problem in a theme.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct Finding {
    pub style_name: String,
    pub kind: FindingKind,
    pub severity: Severity,
    pub suggestion: String,
}

/// What [`check_theme`] checks against.
#[derive(Clone, Debug)]
pub struct CheckOptions {
    /// Terminal palettes to resolve colours against (default: rich's white
    /// export theme and Monokai, a dark one).
    pub backgrounds: Vec<TerminalTheme>,
    /// Minimum contrast (WCAG AA for normal text: 4.5).
    pub min_ratio: f64,
    /// Below this the finding is an error rather than a warning (3.0, the
    /// WCAG AA minimum for large text, which terminal text never is).
    pub error_ratio: f64,
    /// CIEDE2000 below which a simulated pair is confusable.
    pub cvd_threshold: f64,
    /// Groups of style names that must stay distinguishable from each other.
    /// Names missing from the theme are skipped.
    pub groups: Vec<Vec<String>>,
}

impl Default for CheckOptions {
    fn default() -> Self {
        let group = |names: &[&str]| names.iter().map(|n| (*n).to_owned()).collect();
        Self {
            backgrounds: vec![DEFAULT_TERMINAL_THEME, MONOKAI],
            min_ratio: 4.5,
            error_ratio: 3.0,
            cvd_threshold: 10.0,
            groups: vec![
                group(&[
                    "logging.level.debug",
                    "logging.level.info",
                    "logging.level.warning",
                    "logging.level.error",
                    "logging.level.critical",
                ]),
                group(&["repr.bool_true", "repr.bool_false"]),
                group(&["error", "warning", "info", "success"]),
            ],
        }
    }
}

fn describes_colour(style: &Style) -> bool {
    style.color().is_some()
        || style.bgcolor().is_some()
        || style.attr(1) == Some(true)
        || style.attr(6) == Some(true)
}

/// Check `theme`. Findings are sorted by style name, then kind, then the
/// order of backgrounds, so the report is stable.
pub fn check_theme(theme: &Theme, options: &CheckOptions) -> Vec<Finding> {
    let mut names: Vec<&str> = theme.names().collect();
    names.sort_unstable();
    let mut findings = Vec::new();
    for name in &names {
        let style = theme.get(name).expect("listed name");
        if !describes_colour(style) {
            continue;
        }
        for bg_theme in &options.backgrounds {
            let (fg, bg) = resolve_style(style, bg_theme);
            let ratio = contrast_ratio(fg, bg);
            if ratio + 1e-9 >= options.min_ratio {
                continue;
            }
            let severity = if ratio < options.error_ratio {
                Severity::Error
            } else {
                Severity::Warning
            };
            let suggestion = match suggest_color(fg, bg, options.min_ratio) {
                Some(c) => format!(
                    "use {} ({:.2}:1) on {}",
                    c.hex(),
                    contrast_ratio(c, bg),
                    bg.hex()
                ),
                None => "choose a colour with more lightness contrast".into(),
            };
            findings.push(Finding {
                style_name: (*name).to_owned(),
                kind: FindingKind::LowContrast {
                    ratio: (ratio * 100.0).round() / 100.0,
                    fg: fg.hex(),
                    bg: bg.hex(),
                },
                severity,
                suggestion,
            });
        }
    }
    for group in &options.groups {
        let present: Vec<&String> = group.iter().filter(|n| theme.get(n).is_some()).collect();
        for (i, a) in present.iter().enumerate() {
            for b in &present[i + 1..] {
                let (sa, sb) = (theme.get(a).unwrap(), theme.get(b).unwrap());
                if sa == sb {
                    continue;
                }
                if style_without_color(sa) == style_without_color(sb) {
                    findings.push(Finding {
                        style_name: (*a).clone(),
                        kind: FindingKind::ColorOnlyDistinction {
                            a: (*a).clone(),
                            b: (*b).clone(),
                        },
                        severity: Severity::Warning,
                        suggestion: format!(
                            "{a} and {b} differ only by colour: add an attribute or a status symbol"
                        ),
                    });
                }
                for deficiency in Deficiency::ALL {
                    // The worst case over the backgrounds.
                    let worst = options
                        .backgrounds
                        .iter()
                        .map(|t| {
                            let (fa, _) = resolve_style(sa, t);
                            let (fb, _) = resolve_style(sb, t);
                            (
                                delta_e(simulate(fa, deficiency), simulate(fb, deficiency)),
                                delta_e(fa, fb),
                            )
                        })
                        .fold(None::<(f64, f64)>, |acc, v| match acc {
                            Some(a) if a.0 <= v.0 => Some(a),
                            _ => Some(v),
                        });
                    let Some((simulated, normal)) = worst else {
                        continue;
                    };
                    if simulated < options.cvd_threshold && normal >= options.cvd_threshold {
                        findings.push(Finding {
                            style_name: (*a).clone(),
                            kind: FindingKind::ColorBlindConfusable {
                                a: (*a).clone(),
                                b: (*b).clone(),
                                deficiency,
                                delta_e: (simulated * 10.0).round() / 10.0,
                            },
                            severity: Severity::Warning,
                            suggestion: format!(
                                "{a} and {b} look alike with {}: vary lightness or add a symbol",
                                deficiency.name()
                            ),
                        });
                    }
                }
            }
        }
    }
    findings.sort_by(|x, y| {
        x.style_name
            .cmp(&y.style_name)
            .then(kind_order(x).cmp(&kind_order(y)))
    });
    findings
}

fn kind_order(f: &Finding) -> u8 {
    match f.kind {
        FindingKind::LowContrast { .. } => 0,
        FindingKind::ColorOnlyDistinction { .. } => 1,
        FindingKind::ColorBlindConfusable { .. } => 2,
    }
}

impl Finding {
    /// A one-line description.
    pub fn describe(&self) -> String {
        match &self.kind {
            FindingKind::LowContrast { ratio, fg, bg } => {
                format!("contrast {ratio:.2}:1 ({fg} on {bg})")
            }
            FindingKind::ColorOnlyDistinction { a, b } => {
                format!("{a} and {b} differ only by colour")
            }
            FindingKind::ColorBlindConfusable {
                a,
                b,
                deficiency,
                delta_e,
            } => format!("{a} ~ {b} with {} (ΔE00 {delta_e:.1})", deficiency.name()),
        }
    }
}

/// A table of findings, then a summary line.
pub struct ContrastReport<'a> {
    findings: &'a [Finding],
}

impl<'a> ContrastReport<'a> {
    pub fn new(findings: &'a [Finding]) -> Self {
        Self { findings }
    }
}

impl Renderable for ContrastReport<'_> {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let mut out = Vec::new();
        if !self.findings.is_empty() {
            let mut table = Table::new();
            for header in ["Style", "Severity", "Issue", "Suggestion"] {
                table.add_column(header);
            }
            for f in self.findings {
                let severity = match f.severity {
                    Severity::Error => "error",
                    Severity::Warning => "warning",
                    Severity::Info => "info",
                };
                // Data, not markup: a style name or suggestion can contain brackets.
                table.add_row_text(vec![
                    Text::new(f.style_name.as_str()),
                    Text::new(severity),
                    Text::new(f.describe()),
                    Text::new(f.suggestion.as_str()),
                ]);
            }
            out = table.rich_render(console, options);
            if out.last().is_some_and(|s| !s.text.ends_with('\n')) {
                out.push(Segment::line());
            }
        }
        let errors = self
            .findings
            .iter()
            .filter(|f| f.severity == Severity::Error)
            .count();
        out.push(Segment::new(
            format!(
                "{} finding{}, {errors} error{}",
                self.findings.len(),
                if self.findings.len() == 1 { "" } else { "s" },
                if errors == 1 { "" } else { "s" }
            ),
            None,
        ));
        out.push(Segment::line());
        out
    }
}
