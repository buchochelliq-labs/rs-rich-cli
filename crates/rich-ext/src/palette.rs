//! Colour palettes, and colours made from numbers: Tailwind CSS's and
//! Material Design's palettes as `#rrggbb` constants, and helpers that make
//! a [`rich::Color`] from HSL, HSV, Okhsv or Oklab, or mix colours.
//!
//! Every helper returns a truecolor [`Color`], which a console with fewer
//! colours shows as the nearest one it has, as it does any colour.
//!
//! | Item | What |
//! |---|---|
//! | [`tailwind`] | Tailwind's 22 palettes, `c50` to `c950` |
//! | [`material`] | Material Design's 19 palettes, `c50` to `c900`, with accents |
//! | [`hsl`], [`hsv`] | a colour from a hue in degrees and two shares |
//! | [`oklab`] | a colour from Oklab, a space where equal steps look equal |
//! | [`okhsv`] | a colour from Okhsv: HSV, with Oklab's even hues and values |
//! | [`blend`], [`gradient`] | a colour between two, or along several |
//!
//! Credit, and the terms of what is used here:
//!
//! - **Tailwind CSS's palette** (Tailwind CSS 3.4.17, `src/public/colors.js`):
//!   MIT licence, Copyright (c) Tailwind Labs, Inc.
//! - **Material Design's colours** (Material Design Lite,
//!   `src/_color-definitions.scss`): Apache License 2.0, Copyright 2015
//!   Google Inc.
//! - **Oklab and Okhsv** are Björn Ottosson's
//!   (<https://bottosson.github.io/posts/oklab/> and
//!   <https://bottosson.github.io/posts/colorpicker/>, with reference code
//!   under the MIT licence). The conversions here are our own code for his
//!   published formulas and constants.
//!
//! ```
//! use rich_ext::palette::{blend, gradient, hsl, okhsv, tailwind::SLATE};
//! use rich::Color;
//!
//! let hex = |c: Color| c.get_truecolor().unwrap().hex();
//! assert_eq!(SLATE.c950, "#020617");
//! assert_eq!(hex(hsl(120.0, 1.0, 0.25)), "#008000");
//! let black = Color::parse("#000000").unwrap();
//! let white = Color::parse("#ffffff").unwrap();
//! assert_eq!(hex(blend(&black, &white, 0.5)), "#808080");
//! assert_eq!(hex(gradient(&[black, white], 1.0)), "#ffffff");
//! // Okhsv's value 1 and saturation 0 is white, at any hue.
//! assert_eq!(hex(okhsv(200.0, 0.0, 1.0)), "#ffffff");
//! ```

pub mod material;
pub mod tailwind;

use rich::Color;

/// `x` clamped to `0..=1`; NaN is 0.
fn unit(x: f64) -> f64 {
    if x.is_nan() {
        0.0
    } else {
        x.clamp(0.0, 1.0)
    }
}

/// `x`, or 0 when it is not finite.
fn finite(x: f64) -> f64 {
    if x.is_finite() {
        x
    } else {
        0.0
    }
}

/// A hue in degrees, wrapped to `0..360`; not finite is 0.
fn degrees(h: f64) -> f64 {
    finite(h).rem_euclid(360.0)
}

/// An sRGB colour from three shares of full intensity.
fn rgb(r: f64, g: f64, b: f64) -> Color {
    let channel = |x: f64| (unit(x) * 255.0).round() as u8;
    Color::from_rgb(channel(r), channel(g), channel(b))
}

/// The red, green and blue shares for a hue `h` (in sixths of the wheel)
/// with chroma `c`, before the lightness is added.
fn hue_shares(h: f64, c: f64) -> (f64, f64, f64) {
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    }
}

/// A colour from a hue in degrees (wrapping round), a saturation and a
/// lightness from 0 to 1 (clamped). `hsl(0.0, 1.0, 0.5)` is `#ff0000`.
pub fn hsl(h: f64, s: f64, l: f64) -> Color {
    let (s, l) = (unit(s), unit(l));
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let (r, g, b) = hue_shares(degrees(h) / 60.0, c);
    let m = l - c / 2.0;
    rgb(r + m, g + m, b + m)
}

/// A colour from a hue in degrees (wrapping round), a saturation and a
/// value from 0 to 1 (clamped). `hsv(240.0, 1.0, 1.0)` is `#0000ff`.
pub fn hsv(h: f64, s: f64, v: f64) -> Color {
    let (s, v) = (unit(s), unit(v));
    let c = v * s;
    let (r, g, b) = hue_shares(degrees(h) / 60.0, c);
    let m = v - c;
    rgb(r + m, g + m, b + m)
}

/// Linear-light sRGB from Oklab.
fn oklab_to_linear(l: f64, a: f64, b: f64) -> [f64; 3] {
    let l_ = l + 0.396_337_777_4 * a + 0.215_803_757_3 * b;
    let m_ = l - 0.105_561_345_8 * a - 0.063_854_172_8 * b;
    let s_ = l - 0.089_484_177_5 * a - 1.291_485_548 * b;
    let (l, m, s) = (l_.powi(3), m_.powi(3), s_.powi(3));
    [
        4.076_741_662_1 * l - 3.307_711_591_3 * m + 0.230_969_929_2 * s,
        -1.268_438_004_6 * l + 2.609_757_401_1 * m - 0.341_319_396_5 * s,
        -0.004_196_086_3 * l - 0.703_418_614_7 * m + 1.707_614_701 * s,
    ]
}

/// sRGB's transfer function: linear light to the value written.
fn encode(x: f64) -> f64 {
    if x <= 0.003_130_8 {
        12.92 * x
    } else {
        1.055 * x.powf(1.0 / 2.4) - 0.055
    }
}

fn linear_to_colour([r, g, b]: [f64; 3]) -> Color {
    rgb(encode(r), encode(g), encode(b))
}

/// A colour from Oklab: lightness `l` from 0 to 1, and `a` (green to red)
/// and `b` (blue to yellow), each about -0.4 to 0.4. Colours outside sRGB
/// are clipped to it. `oklab(1.0, 0.0, 0.0)` is white.
pub fn oklab(l: f64, a: f64, b: f64) -> Color {
    linear_to_colour(oklab_to_linear(finite(l), finite(a), finite(b)))
}

/// The most saturated an Oklab colour of hue (`a`, `b`), a unit vector,
/// can be within sRGB, as `C / L`: a polynomial fit, then a step of
/// Halley's method on whichever channel reaches zero first (Ottosson's).
fn max_saturation(a: f64, b: f64) -> f64 {
    let (k, w) = if -1.881_703_28 * a - 0.809_364_93 * b > 1.0 {
        // Red reaches zero first.
        (
            [
                1.190_862_77,
                1.765_767_28,
                0.596_626_41,
                0.755_151_97,
                0.567_712_45,
            ],
            [4.076_741_662_1, -3.307_711_591_3, 0.230_969_929_2],
        )
    } else if 1.814_441_04 * a - 1.194_452_76 * b > 1.0 {
        // Green.
        (
            [
                0.739_565_15,
                -0.459_544_04,
                0.082_854_27,
                0.125_410_70,
                0.145_032_04,
            ],
            [-1.268_438_004_6, 2.609_757_401_1, -0.341_319_396_5],
        )
    } else {
        // Blue.
        (
            [
                1.357_336_52,
                -0.009_157_99,
                -1.151_302_10,
                -0.505_596_06,
                0.006_921_67,
            ],
            [-0.004_196_086_3, -0.703_418_614_7, 1.707_614_701],
        )
    };
    let s = k[0] + k[1] * a + k[2] * b + k[3] * a * a + k[4] * a * b;
    let kl = 0.396_337_777_4 * a + 0.215_803_757_3 * b;
    let km = -0.105_561_345_8 * a - 0.063_854_172_8 * b;
    let ks = -0.089_484_177_5 * a - 1.291_485_548 * b;
    let (l_, m_, s_) = (1.0 + s * kl, 1.0 + s * km, 1.0 + s * ks);
    let f = w[0] * l_.powi(3) + w[1] * m_.powi(3) + w[2] * s_.powi(3);
    let f1 = w[0] * 3.0 * kl * l_ * l_ + w[1] * 3.0 * km * m_ * m_ + w[2] * 3.0 * ks * s_ * s_;
    let f2 = w[0] * 6.0 * kl * kl * l_ + w[1] * 6.0 * km * km * m_ + w[2] * 6.0 * ks * ks * s_;
    s - f * f1 / (f1 * f1 - 0.5 * f * f2)
}

/// The lightness and chroma of the most saturated colour of hue (`a`,
/// `b`) within sRGB.
fn cusp(a: f64, b: f64) -> (f64, f64) {
    let s = max_saturation(a, b);
    let [r, g, bl] = oklab_to_linear(1.0, s * a, s * b);
    let l = (1.0 / r.max(g).max(bl)).cbrt();
    (l, l * s)
}

/// Ottosson's toe, undone: from Okhsv's lightness to Oklab's.
fn toe_inv(x: f64) -> f64 {
    const K1: f64 = 0.206;
    const K2: f64 = 0.03;
    const K3: f64 = (1.0 + K1) / (1.0 + K2);
    (x * x + K1 * x) / (K3 * (x + K2))
}

/// A colour from Okhsv: a hue in degrees (wrapping round), and a
/// saturation and a value from 0 to 1 (clamped). Like HSV, but hues are
/// evenly spaced to the eye and a value looks the same lightness at every
/// hue, so a swatch of it has no bright bands. `okhsv(h, 1.0, 1.0)` is the
/// most saturated sRGB colour of that hue, and `okhsv(h, 0.0, 1.0)` white.
pub fn okhsv(h: f64, s: f64, v: f64) -> Color {
    let (s, v) = (unit(s), unit(v));
    if v == 0.0 {
        return Color::from_rgb(0, 0, 0);
    }
    let (a, b) = {
        let h = degrees(h).to_radians();
        (h.cos(), h.sin())
    };
    let (l_cusp, c_cusp) = cusp(a, b);
    let (s_max, t_max) = (c_cusp / l_cusp, c_cusp / (1.0 - l_cusp));
    let s0 = 0.5;
    let k = 1.0 - s0 / s_max;
    // As if the gamut were a triangle, then corrected for the toe and the
    // curve of its top.
    let room = s0 + t_max - t_max * k * s;
    let l_v = 1.0 - s * s0 / room;
    let c_v = s * t_max * s0 / room;
    let (l, c) = (v * l_v, v * c_v);
    let l_vt = toe_inv(l_v);
    let c_vt = c_v * l_vt / l_v;
    let l_new = toe_inv(l);
    let c = c * l_new / l;
    let [r, g, bl] = oklab_to_linear(l_vt, a * c_vt, b * c_vt);
    let top = r.max(g).max(bl);
    let scale = if top > 0.0 { (1.0 / top).cbrt() } else { 1.0 };
    let (l, c) = (l_new * scale, c * scale);
    linear_to_colour(oklab_to_linear(l, c * a, c * b))
}

/// The colour a share `t` (0 to 1, clamped) of the way from `a` to `b`,
/// mixed channel by channel in sRGB, as rich blends colours. The
/// terminal's default colour has no value to mix: with it, the result is
/// whichever end is nearer.
pub fn blend(a: &Color, b: &Color, t: f64) -> Color {
    let t = unit(t);
    match (a.get_truecolor(), b.get_truecolor()) {
        (Some(x), Some(y)) => {
            let mix =
                |p: u8, q: u8| (f64::from(p) + (f64::from(q) - f64::from(p)) * t).round() as u8;
            Color::from_rgb(
                mix(x.red, y.red),
                mix(x.green, y.green),
                mix(x.blue, y.blue),
            )
        }
        _ if t < 0.5 => a.clone(),
        _ => b.clone(),
    }
}

/// The colour a share `t` (0 to 1, clamped) of the way along `stops`,
/// spaced evenly: the first at 0, the last at 1, each pair [`blend`]ed.
/// One stop is that colour throughout; none is the terminal's default.
pub fn gradient(stops: &[Color], t: f64) -> Color {
    match stops {
        [] => Color::default_color(),
        [only] => only.clone(),
        _ => {
            let at = unit(t) * (stops.len() - 1) as f64;
            let i = (at.floor() as usize).min(stops.len() - 2);
            blend(&stops[i], &stops[i + 1], at - i as f64)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn triplet(colour: &Color) -> (i32, i32, i32) {
        let t = colour.get_truecolor().unwrap();
        (i32::from(t.red), i32::from(t.green), i32::from(t.blue))
    }

    fn hex(colour: Color) -> String {
        colour.get_truecolor().unwrap().hex()
    }

    fn close(colour: &Color, want: (i32, i32, i32), by: i32) -> bool {
        let got = triplet(colour);
        (got.0 - want.0).abs() <= by && (got.1 - want.1).abs() <= by && (got.2 - want.2).abs() <= by
    }

    #[test]
    fn hsl_and_hsv_of_known_colours() {
        assert_eq!(hex(hsl(0.0, 1.0, 0.5)), "#ff0000");
        assert_eq!(hex(hsl(120.0, 1.0, 0.25)), "#008000");
        assert_eq!(hex(hsl(240.0, 1.0, 0.5)), "#0000ff");
        assert_eq!(hex(hsl(0.0, 0.0, 1.0)), "#ffffff");
        assert_eq!(hex(hsl(-120.0, 1.0, 0.5)), "#0000ff");
        assert_eq!(hex(hsv(240.0, 1.0, 1.0)), "#0000ff");
        assert_eq!(hex(hsv(60.0, 0.5, 1.0)), "#ffff80");
        assert_eq!(hex(hsv(300.0, 1.0, 0.5)), "#800080");
        assert_eq!(hex(hsv(720.0, 1.0, 1.0)), "#ff0000");
        // Out of range and NaN are clamped.
        assert_eq!(hex(hsv(f64::NAN, 2.0, f64::NAN)), "#000000");
    }

    #[test]
    fn oklab_of_known_colours() {
        assert_eq!(hex(oklab(1.0, 0.0, 0.0)), "#ffffff");
        assert_eq!(hex(oklab(0.0, 0.0, 0.0)), "#000000");
        // sRGB's primaries, as Ottosson lists them.
        assert!(close(
            &oklab(0.627_955, 0.224_863, 0.125_846),
            (255, 0, 0),
            1
        ));
        assert!(close(
            &oklab(0.866_440, -0.233_887, 0.179_498),
            (0, 255, 0),
            1
        ));
        assert!(close(
            &oklab(0.452_014, -0.032_457, -0.311_528),
            (0, 0, 255),
            1
        ));
    }

    #[test]
    fn okhsv_of_known_colours() {
        // The primaries are the most saturated colours of their hues.
        assert!(close(&okhsv(29.233_885, 1.0, 1.0), (255, 0, 0), 3));
        assert!(close(&okhsv(142.495_339, 1.0, 1.0), (0, 255, 0), 3));
        assert!(close(&okhsv(264.052_021, 1.0, 1.0), (0, 0, 255), 3));
        // No saturation is a grey, at full value white; no value is black.
        for h in [0.0, 90.0, 200.0, 359.0] {
            assert_eq!(hex(okhsv(h, 0.0, 1.0)), "#ffffff");
            assert_eq!(hex(okhsv(h, 1.0, 0.0)), "#000000");
            let (r, g, b) = triplet(&okhsv(h, 0.0, 0.5));
            assert!(r == g && g == b, "{h}: {r} {g} {b}");
        }
        // Lower values are darker at a hue.
        let bright = triplet(&okhsv(29.233_885, 1.0, 1.0)).0;
        let dim = triplet(&okhsv(29.233_885, 1.0, 0.5)).0;
        assert!(dim < bright);
    }

    #[test]
    fn blends_and_gradients() {
        let black = Color::parse("#000000").unwrap();
        let white = Color::parse("#ffffff").unwrap();
        assert_eq!(hex(blend(&black, &white, 0.0)), "#000000");
        assert_eq!(hex(blend(&black, &white, 1.0)), "#ffffff");
        assert_eq!(hex(blend(&black, &white, 0.25)), "#404040");
        assert_eq!(hex(blend(&black, &white, 9.0)), "#ffffff");
        // A named colour mixes as its palette value.
        let red = Color::parse("red").unwrap();
        assert_eq!(hex(blend(&red, &white, 0.0)), "#800000");
        // The default colour has no value: the nearer end.
        let default = Color::default_color();
        assert!(blend(&default, &white, 0.4).is_default());
        assert_eq!(hex(blend(&default, &white, 0.6)), "#ffffff");

        let stops = [
            Color::parse("#ff0000").unwrap(),
            Color::parse("#00ff00").unwrap(),
            Color::parse("#0000ff").unwrap(),
        ];
        assert_eq!(hex(gradient(&stops, 0.0)), "#ff0000");
        assert_eq!(hex(gradient(&stops, 0.25)), "#808000");
        assert_eq!(hex(gradient(&stops, 0.5)), "#00ff00");
        assert_eq!(hex(gradient(&stops, 0.75)), "#008080");
        assert_eq!(hex(gradient(&stops, 1.0)), "#0000ff");
        assert_eq!(hex(gradient(&stops, -1.0)), "#ff0000");
        assert_eq!(hex(gradient(&stops[..1], 0.7)), "#ff0000");
        assert!(gradient(&[], 0.5).is_default());
    }
}
