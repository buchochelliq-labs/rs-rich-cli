//! Opt-in colour processing for the final sampled image raster.
use image::RgbaImage;

/// Colour precision for image rendering.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ImageColorMode {
    /// Preserve existing RGB samples and rendering behavior.
    #[default]
    TrueColor,
    /// Quantize to the fixed ANSI256 cube and grayscale entries (16–255).
    /// System entries 0–15 are excluded because terminal themes redefine them.
    /// Nearest color uses squared Euclidean distance in encoded RGB; exact
    /// ties select the lowest palette index.
    Ansi256,
    /// Quantize to the 16 system colours, matched against rich's fixed
    /// `STANDARD_PALETTE` (the 170/85 VGA table). The terminal theme decides
    /// how those entries finally look, so this trades fidelity for output that
    /// follows the user's theme. Same distance and tie rule as `Ansi256`.
    Ansi16,
    /// Quantize to the 26 neutral ANSI256 entries: 16 (black), the 232–255
    /// ramp and 231 (white). Each pixel's luma (`(77R + 150G + 29B) / 256`,
    /// the weights `--image-grayscale` uses) picks the nearest level; exact
    /// ties select the lowest palette index.
    Grayscale,
}

/// Dithering applied after sizing and compositing, before glyph selection.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Dither {
    /// Map each pixel independently.
    #[default]
    None,
    /// Scan left-to-right, top-to-bottom with 7/16, 3/16, 5/16, 1/16 weights.
    /// Discard error outside the raster; do not wrap or renormalize edge weights.
    FloydSteinberg,
    /// Ordered 4×4 Bayer matrix, anchored at the final raster origin.
    /// Offset each encoded RGB channel by twice the matrix entry minus 15.
    Bayer4x4,
    /// Bill Atkinson's diffusion: 1/8 of the error to each of (x+1, y),
    /// (x+2, y), (x−1, y+1), (x, y+1), (x+1, y+1) and (x, y+2), scanning
    /// left-to-right, top-to-bottom. The remaining quarter is dropped, which
    /// keeps highlights and shadows cleaner than Floyd–Steinberg. Error outside
    /// the raster is discarded, as with Floyd–Steinberg.
    Atkinson,
}

/// How the nearest palette entry is measured when quantizing.
///
/// Error diffusion always accumulates in encoded RGB; this only changes which
/// palette entry a (diffused) pixel snaps to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ColorDistance {
    /// Squared Euclidean distance in encoded sRGB, or in luma for
    /// [`ImageColorMode::Grayscale`]. The default; it leaves every existing
    /// output unchanged.
    #[default]
    Rgb,
    /// Squared Euclidean distance in [OKLab](https://bottosson.github.io/posts/oklab/),
    /// a perceptual space: steps of equal size look about equally different.
    /// It keeps hues truer on the small ANSI16 palette, at some cost in speed.
    /// Ties still select the lowest palette index.
    Oklab,
}

/// Encoded RGB of an ANSI256 cube or ramp entry (16–255).
fn eight_bit(index: u8) -> [u8; 3] {
    const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
    let value = index - 16;
    if index < 232 {
        [
            LEVELS[(value / 36) as usize],
            LEVELS[(value / 6 % 6) as usize],
            LEVELS[(value % 6) as usize],
        ]
    } else {
        [8 + (index - 232) * 10; 3]
    }
}

/// Encoded RGB of any palette entry: rich's `STANDARD_PALETTE` for 0–15,
/// the ANSI256 cube and grayscale ramp above that.
pub(crate) fn palette_rgb(index: u8) -> [u8; 3] {
    if index < 16 {
        let t = rich::color::STANDARD_PALETTE[index as usize];
        [t.red, t.green, t.blue]
    } else {
        eight_bit(index)
    }
}

/// OKLab coordinates of an encoded sRGB colour with channels in `0.0..=255.0`.
pub(crate) fn oklab(rgb: [f64; 3]) -> [f64; 3] {
    let [r, g, b] = rgb.map(|v| {
        let c = v / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    });
    let l = (0.412_221_470_8 * r + 0.536_332_536_3 * g + 0.051_445_992_9 * b).cbrt();
    let m = (0.211_903_498_2 * r + 0.680_699_545_1 * g + 0.107_396_956_6 * b).cbrt();
    let s = (0.088_302_461_9 * r + 0.281_718_837_6 * g + 0.629_978_700_5 * b).cbrt();
    [
        0.210_454_255_3 * l + 0.793_617_785_0 * m - 0.004_072_046_8 * s,
        1.977_998_495_1 * l - 2.428_592_205_0 * m + 0.450_593_709_9 * s,
        0.025_904_037_1 * l + 0.782_771_766_2 * m - 0.808_675_766_0 * s,
    ]
}

/// OKLab of every palette entry, computed once.
fn palette_oklab() -> &'static [[f64; 3]; 256] {
    static TABLE: std::sync::OnceLock<[[f64; 3]; 256]> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| std::array::from_fn(|i| oklab(palette_rgb(i as u8).map(f64::from))))
}

/// Encoded RGB of every palette entry, computed once.
fn palette_table() -> &'static [[u8; 3]; 256] {
    static TABLE: std::sync::OnceLock<[[u8; 3]; 256]> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| std::array::from_fn(|i| palette_rgb(i as u8)))
}

/// `N` consecutive palette indices from `start`.
const fn index_run<const N: usize>(start: u8) -> [u8; N] {
    let mut out = [0; N];
    let mut i = 0;
    while i < N {
        out[i] = start + i as u8;
        i += 1;
    }
    out
}

const ANSI256_ENTRIES: [u8; 240] = index_run(16);
const ANSI16_ENTRIES: [u8; 16] = index_run(0);
const GRAYSCALE_ENTRIES: [u8; 26] = {
    let ramp: [u8; 25] = index_run(231);
    let mut out = [16; 26];
    let mut i = 0;
    while i < 25 {
        out[i + 1] = ramp[i];
        i += 1;
    }
    out
};

/// The palette entries `mode` may choose, in ascending index order.
fn candidates(mode: ImageColorMode) -> &'static [u8] {
    match mode {
        ImageColorMode::TrueColor | ImageColorMode::Ansi256 => &ANSI256_ENTRIES,
        ImageColorMode::Ansi16 => &ANSI16_ENTRIES,
        ImageColorMode::Grayscale => &GRAYSCALE_ENTRIES,
    }
}

/// Squared Euclidean distance in encoded RGB. Every search uses this one
/// expression, so the fast and exhaustive paths compute identical values.
fn rgb_distance(rgb: [f64; 3], c: [u8; 3]) -> f64 {
    (0..3).map(|i| (rgb[i] - f64::from(c[i])).powi(2)).sum()
}

/// Nearest palette entry for `mode` (never `TrueColor`) under `distance`.
/// [`ColorDistance::Rgb`] is squared Euclidean distance in encoded RGB (luma
/// for `Grayscale`), not linear-light RGB. Candidates are scanned in
/// ascending index order and only a strictly closer one replaces the best, so
/// exact ties choose the lowest palette index.
pub(crate) fn nearest(
    rgb: [f64; 3],
    mode: ImageColorMode,
    distance: ColorDistance,
) -> (u8, [u8; 3]) {
    let direct = matches!(mode, ImageColorMode::TrueColor | ImageColorMode::Ansi256)
        && distance == ColorDistance::Rgb
        && rgb.iter().all(|v| (-1e6..=1e6).contains(v));
    if direct {
        return nearest_ansi256_rgb(rgb);
    }
    nearest_exhaustive(rgb, mode, distance)
}

/// [`nearest`] by scanning every candidate.
fn nearest_exhaustive(
    rgb: [f64; 3],
    mode: ImageColorMode,
    distance: ColorDistance,
) -> (u8, [u8; 3]) {
    let luma = (77.0 * rgb[0] + 150.0 * rgb[1] + 29.0 * rgb[2]) / 256.0;
    let lab = (distance == ColorDistance::Oklab).then(|| oklab(rgb));
    let (table, labs) = (palette_table(), palette_oklab());
    let mut best = (0, [0; 3]);
    let mut closest = f64::INFINITY;
    for &index in candidates(mode) {
        let c = table[index as usize];
        let d = match (lab, mode) {
            (Some(lab), _) => {
                let p = labs[index as usize];
                (0..3).map(|i| (lab[i] - p[i]).powi(2)).sum()
            }
            (None, ImageColorMode::Grayscale) => (luma - f64::from(c[0])).powi(2),
            (None, _) => rgb_distance(rgb, c),
        };
        if d < closest {
            closest = d;
            best = (index, c);
        }
    }
    best
}

/// [`nearest`] for the ANSI256 cube and ramp in encoded RGB, without a scan.
///
/// The cube distance is a sum of per-channel terms, so its minimum lies at
/// the two cube levels bracketing each channel; any other level is at least
/// 1600 (squared units) further away, far beyond rounding. Likewise the ramp
/// minimum lies within one step of the channel mean. Scoring only those 8 + 4
/// candidates, in ascending index order with the same distance expression and
/// strict comparison, therefore picks exactly what the full scan picks.
fn nearest_ansi256_rgb(rgb: [f64; 3]) -> (u8, [u8; 3]) {
    const LEVELS: [f64; 6] = [0.0, 95.0, 135.0, 175.0, 215.0, 255.0];
    // Index of the lower of the two levels bracketing `v` (clamped).
    let lower = |v: f64| LEVELS[1..5].iter().take_while(|&&level| level <= v).count();
    let [r, g, b] = rgb.map(lower);
    let table = palette_table();
    let mut best = (0, [0; 3]);
    let mut closest = f64::INFINITY;
    let mut consider = |index: usize| {
        let c = table[index];
        let d = rgb_distance(rgb, c);
        if d < closest {
            closest = d;
            best = (index as u8, c);
        }
    };
    for ri in r..=r + 1 {
        for gi in g..=g + 1 {
            for bi in b..=b + 1 {
                consider(16 + 36 * ri + 6 * gi + bi);
            }
        }
    }
    // Ramp entry j is 8 + 10j; take the steps around the mean.
    let mean = (rgb[0] + rgb[1] + rgb[2]) / 3.0;
    let step = (((mean - 8.0) / 10.0).floor().clamp(0.0, 23.0)) as usize;
    for j in step.saturating_sub(1)..=(step + 2).min(23) {
        consider(232 + j);
    }
    best
}

/// Mutate only the final sampled raster; returned indices correspond one-to-one
/// with its row-major pixels. Remaining alpha is composited onto black before
/// diffusion. Three error rows bound auxiliary storage to the sampled width.
///
/// `clear` (from [`clear_mask`](crate::image_art::clear_mask)) marks pixels
/// left unpainted: they still get an index, from their own colour alone, but
/// neither take in nor pass on diffused error, so a transparent hole does not
/// bleed into the opaque pixels around it.
pub(crate) fn preprocess(
    raster: &mut RgbaImage,
    mode: ImageColorMode,
    dither: Dither,
    distance: ColorDistance,
    clear: Option<&[bool]>,
) -> Option<Vec<u8>> {
    if mode == ImageColorMode::TrueColor {
        return None;
    }
    let (width, height) = raster.dimensions();
    let width = width as usize;
    let mut indices = Vec::with_capacity(width * height as usize);
    let mut current = vec![[0.0; 3]; width];
    let mut next = vec![[0.0; 3]; width];
    let mut after = vec![[0.0; 3]; width];
    for y in 0..height {
        for x in 0..width {
            let pixel = raster.get_pixel_mut(x as u32, y);
            let alpha = f64::from(pixel.0[3]) / 255.0;
            const BAYER: [[u8; 4]; 4] =
                [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];
            let offset = if dither == Dither::Bayer4x4 {
                2.0 * f64::from(BAYER[y as usize % 4][x % 4]) - 15.0
            } else {
                0.0
            };
            if clear.is_some_and(|clear| clear[y as usize * width + x]) {
                let rgb = std::array::from_fn(|c| f64::from(pixel.0[c]) * alpha);
                let (index, color) = nearest(rgb, mode, distance);
                indices.push(index);
                pixel.0 = [color[0], color[1], color[2], 255];
                continue;
            }
            let rgb = std::array::from_fn(|c| {
                (f64::from(pixel.0[c]) * alpha + current[x][c] + offset).clamp(0.0, 255.0)
            });
            let (index, color) = nearest(rgb, mode, distance);
            indices.push(index);
            pixel.0 = [color[0], color[1], color[2], 255];
            if dither == Dither::FloydSteinberg {
                for c in 0..3 {
                    let error = rgb[c] - f64::from(color[c]);
                    if x + 1 < width {
                        current[x + 1][c] += error * (7.0 / 16.0);
                    }
                    if y + 1 < height {
                        if x > 0 {
                            next[x - 1][c] += error * (3.0 / 16.0);
                        }
                        next[x][c] += error * (5.0 / 16.0);
                        if x + 1 < width {
                            next[x + 1][c] += error * (1.0 / 16.0);
                        }
                    }
                }
            }
            if dither == Dither::Atkinson {
                for c in 0..3 {
                    let share = (rgb[c] - f64::from(color[c])) / 8.0;
                    for dx in 1..=2 {
                        if x + dx < width {
                            current[x + dx][c] += share;
                        }
                    }
                    if y + 1 < height {
                        if x > 0 {
                            next[x - 1][c] += share;
                        }
                        next[x][c] += share;
                        if x + 1 < width {
                            next[x + 1][c] += share;
                        }
                    }
                    if y + 2 < height {
                        after[x][c] += share;
                    }
                }
            }
        }
        // Rotate the rows: next becomes current, after becomes next.
        std::mem::swap(&mut current, &mut next);
        std::mem::swap(&mut next, &mut after);
        after.fill([0.0; 3]);
    }
    Some(indices)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    #[test]
    fn disabled_processing_preserves_all_bytes_including_alpha() {
        let mut pixels = RgbaImage::from_pixel(2, 3, Rgba([13, 77, 203, 45]));
        let before = pixels.clone();
        assert_eq!(
            preprocess(
                &mut pixels,
                ImageColorMode::TrueColor,
                Dither::None,
                ColorDistance::Rgb,
                None
            ),
            None
        );
        assert_eq!(pixels, before);
    }

    #[test]
    fn exact_cube_and_grayscale_hits_stay_exact() {
        let mut pixels = RgbaImage::from_fn(3, 1, |x, _| {
            Rgba(match x {
                0 => [255, 0, 0, 255],
                1 => [95, 135, 175, 255],
                _ => [128, 128, 128, 255],
            })
        });
        let before = pixels.clone();
        assert_eq!(
            preprocess(
                &mut pixels,
                ImageColorMode::Ansi256,
                Dither::None,
                ColorDistance::Rgb,
                None
            ),
            Some(vec![196, 67, 244])
        );
        assert_eq!(pixels, before);
    }

    #[test]
    fn diffusion_has_known_horizontal_and_vertical_weights() {
        let mut pixels = RgbaImage::from_pixel(2, 2, Rgba([12, 12, 12, 255]));
        // First maps to 8; right receives 4*7/16 and maps to 18.
        // Next row: 12 + 4*5/16 - 4.25*3/16 = 12.453125 -> 8.
        // Last: 12 + 4/16 - 4.25*5/16 + 4.453125*7/16 < 13 -> 8.
        assert_eq!(
            preprocess(
                &mut pixels,
                ImageColorMode::Ansi256,
                Dither::FloydSteinberg,
                ColorDistance::Rgb,
                None
            ),
            Some(vec![232, 233, 232, 232])
        );
        assert_eq!(
            pixels.into_raw(),
            vec![8, 8, 8, 255, 18, 18, 18, 255, 8, 8, 8, 255, 8, 8, 8, 255]
        );
    }

    #[test]
    fn clear_pixels_neither_take_nor_pass_on_diffused_error() {
        // 13 is exactly between the 8 and 18 ramp entries, so it snaps to 8
        // (the lower index) unless some error reaches it.
        let row = |clear: Option<&[bool]>| {
            let mut pixels = RgbaImage::from_fn(3, 1, |x, _| {
                Rgba(if x == 1 {
                    [200, 200, 200, 100]
                } else {
                    [13, 13, 13, 255]
                })
            });
            preprocess(
                &mut pixels,
                ImageColorMode::Ansi256,
                Dither::FloydSteinberg,
                ColorDistance::Rgb,
                clear,
            )
            .unwrap()
        };
        // Unmasked, the premultiplied middle pixel passes its error right.
        assert_eq!(row(None)[2], 233);
        // Masked, the right pixel sees no error at all, as if alone.
        assert_eq!(row(Some(&[false, true, false]))[2], 232);
    }

    #[test]
    fn transparent_and_tiny_rasters_are_bounded() {
        let mut transparent = RgbaImage::from_pixel(1, 1, Rgba([255, 255, 255, 0]));
        assert_eq!(
            preprocess(
                &mut transparent,
                ImageColorMode::Ansi256,
                Dither::FloydSteinberg,
                ColorDistance::Rgb,
                None
            ),
            Some(vec![16])
        );
        assert_eq!(transparent.get_pixel(0, 0).0, [0, 0, 0, 255]);
        let mut empty = RgbaImage::new(0, 0);
        assert_eq!(
            preprocess(
                &mut empty,
                ImageColorMode::Ansi256,
                Dither::FloydSteinberg,
                ColorDistance::Rgb,
                None
            ),
            Some(vec![])
        );
    }

    #[test]
    fn ansi16_matches_the_standard_palette_with_lowest_index_ties() {
        let hit = |rgb: [f64; 3]| nearest(rgb, ImageColorMode::Ansi16, ColorDistance::Rgb).0;
        for (index, t) in rich::color::STANDARD_PALETTE.iter().enumerate() {
            let rgb = [t.red, t.green, t.blue].map(f64::from);
            assert_eq!(hit(rgb), index as u8);
        }
        assert_eq!(hit([200.0, 10.0, 10.0]), 1);
        assert_eq!(hit([250.0, 90.0, 80.0]), 9);
        // #ff8800 is exactly equidistant from 3 (170,85,0) and 9 (255,85,85).
        assert_eq!(hit([255.0, 136.0, 0.0]), 3);
    }

    #[test]
    fn grayscale_uses_luma_and_the_neutral_entries_only() {
        let hit = |rgb: [f64; 3]| nearest(rgb, ImageColorMode::Grayscale, ColorDistance::Rgb);
        assert_eq!(hit([0.0; 3]), (16, [0; 3]));
        assert_eq!(hit([255.0; 3]), (231, [255; 3]));
        assert_eq!(hit([128.0; 3]), (244, [128; 3]));
        // Pure red has luma 76.7, nearest to the 78 entry (239).
        assert_eq!(hit([255.0, 0.0, 0.0]), (239, [78; 3]));
        // 3 sits between 0 (16) and 8 (232): 16 is nearer.
        assert_eq!(hit([3.0; 3]).0, 16);
        // 4 is equidistant from 0 and 8: the lower index (16) wins.
        assert_eq!(hit([4.0; 3]).0, 16);
    }

    #[test]
    fn every_quantized_mode_accepts_every_dither() {
        for mode in [
            ImageColorMode::Ansi256,
            ImageColorMode::Ansi16,
            ImageColorMode::Grayscale,
        ] {
            for (dither, distance) in [
                Dither::None,
                Dither::FloydSteinberg,
                Dither::Bayer4x4,
                Dither::Atkinson,
            ]
            .into_iter()
            .flat_map(|d| [(d, ColorDistance::Rgb), (d, ColorDistance::Oklab)])
            {
                let mut pixels = RgbaImage::from_fn(5, 3, |x, y| {
                    Rgba([(x * 50) as u8, (y * 90) as u8, 120, 255])
                });
                let indices = preprocess(&mut pixels, mode, dither, distance, None).unwrap();
                assert_eq!(indices.len(), 15);
                for (index, pixel) in indices.iter().zip(pixels.pixels()) {
                    let rgb = pixel.0.map(f64::from)[..3].try_into().unwrap();
                    let expected = nearest(rgb, mode, distance);
                    assert_eq!(expected.0, *index, "{mode:?} {dither:?} {distance:?}");
                    match mode {
                        ImageColorMode::Ansi16 => assert!(*index < 16),
                        ImageColorMode::Grayscale => {
                            assert!(*index == 16 || *index >= 231);
                            assert!(pixel.0[0] == pixel.0[1] && pixel.0[1] == pixel.0[2]);
                        }
                        _ => assert!(*index >= 16),
                    }
                }
            }
        }
    }

    /// A direct transcription of Atkinson diffusion over a full error matrix,
    /// to check the three rolling rows against.
    fn atkinson_reference(values: &[Vec<u8>], mode: ImageColorMode) -> Vec<u8> {
        let (h, w) = (values.len(), values[0].len());
        let mut error = vec![vec![0.0f64; w]; h];
        let mut out = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let v = (f64::from(values[y][x]) + error[y][x]).clamp(0.0, 255.0);
                let (index, color) = nearest([v; 3], mode, ColorDistance::Rgb);
                out.push(index);
                let share = (v - f64::from(color[0])) / 8.0;
                for (dx, dy) in [(1i64, 0i64), (2, 0), (-1, 1), (0, 1), (1, 1), (0, 2)] {
                    let (nx, ny) = (x as i64 + dx, y as i64 + dy);
                    if nx >= 0 && (nx as usize) < w && (ny as usize) < h {
                        error[ny as usize][nx as usize] += share;
                    }
                }
            }
        }
        out
    }

    #[test]
    fn atkinson_spreads_six_eighths_of_the_error() {
        // Hand-checked on one gray row: 104 snaps up to 108 (error −4, so −0.5
        // to each of the next two pixels); 103.5 snaps up again (−0.5625 each);
        // 102.9375 is then nearer 98. Floyd–Steinberg alternates instead.
        let row = |dither| {
            let mut pixels = RgbaImage::from_pixel(3, 1, Rgba([104, 104, 104, 255]));
            preprocess(
                &mut pixels,
                ImageColorMode::Grayscale,
                dither,
                ColorDistance::Rgb,
                None,
            )
            .unwrap()
        };
        assert_eq!(row(Dither::Atkinson), vec![242, 242, 241]);
        assert_eq!(row(Dither::FloydSteinberg), vec![242, 241, 242]);

        let values: Vec<Vec<u8>> = (0..5)
            .map(|y| (0..7).map(|x| (x * 37 + y * 23) as u8).collect())
            .collect();
        let mut pixels = RgbaImage::from_fn(7, 5, |x, y| {
            let v = values[y as usize][x as usize];
            Rgba([v, v, v, 255])
        });
        let indices = preprocess(
            &mut pixels,
            ImageColorMode::Grayscale,
            Dither::Atkinson,
            ColorDistance::Rgb,
            None,
        )
        .unwrap();
        assert_eq!(
            indices,
            atkinson_reference(&values, ImageColorMode::Grayscale)
        );
    }

    #[test]
    fn oklab_matches_published_reference_values() {
        let close = |a: [f64; 3], b: [f64; 3]| (0..3).all(|i| (a[i] - b[i]).abs() < 1e-6);
        assert!(close(oklab([0.0; 3]), [0.0; 3]));
        assert!(close(oklab([255.0; 3]), [1.0, 0.0, 0.0]));
        assert!(close(
            oklab([255.0, 0.0, 0.0]),
            [0.627_955_4, 0.224_863_1, 0.125_846_3]
        ));
    }

    #[test]
    fn oklab_keeps_hue_where_rgb_distance_falls_back_to_gray() {
        let pick = |rgb, distance| nearest(rgb, ImageColorMode::Ansi16, distance).0;
        // A muted green and a dark red are nearest the grays in encoded RGB,
        // but perceptually they are green (2) and red (1).
        assert_eq!(pick([90.0, 160.0, 90.0], ColorDistance::Rgb), 8);
        assert_eq!(pick([90.0, 160.0, 90.0], ColorDistance::Oklab), 2);
        assert_eq!(pick([120.0, 40.0, 40.0], ColorDistance::Rgb), 8);
        assert_eq!(pick([120.0, 40.0, 40.0], ColorDistance::Oklab), 1);
        // Exact palette colours are still exact hits.
        for index in 0..16u8 {
            let rgb = palette_rgb(index).map(f64::from);
            assert_eq!(pick(rgb, ColorDistance::Oklab), index);
        }
    }

    /// The search as it was before the direct ANSI256 path and static tables:
    /// a boxed iterator over every candidate. Kept verbatim as the oracle.
    fn nearest_reference(
        rgb: [f64; 3],
        mode: ImageColorMode,
        distance: ColorDistance,
    ) -> (u8, [u8; 3]) {
        let candidates: Box<dyn Iterator<Item = u8>> = match mode {
            ImageColorMode::TrueColor | ImageColorMode::Ansi256 => Box::new(16u8..=255),
            ImageColorMode::Ansi16 => Box::new(0u8..16),
            ImageColorMode::Grayscale => Box::new(std::iter::once(16).chain(231..=255)),
        };
        let luma = (77.0 * rgb[0] + 150.0 * rgb[1] + 29.0 * rgb[2]) / 256.0;
        let lab = (distance == ColorDistance::Oklab).then(|| oklab(rgb));
        let mut best = (0, [0; 3]);
        let mut closest = f64::INFINITY;
        for index in candidates {
            let c = palette_rgb(index);
            let d = match (lab, mode) {
                (Some(lab), _) => {
                    let p = palette_oklab()[index as usize];
                    (0..3).map(|i| (lab[i] - p[i]).powi(2)).sum()
                }
                (None, ImageColorMode::Grayscale) => (luma - f64::from(c[0])).powi(2),
                (None, _) => (0..3).map(|i| (rgb[i] - f64::from(c[i])).powi(2)).sum(),
            };
            if d < closest {
                closest = d;
                best = (index, c);
            }
        }
        best
    }

    /// Deterministic values in `lo..hi`, from a 64-bit LCG.
    fn random_values(count: usize, lo: f64, hi: f64) -> Vec<f64> {
        let mut state = 0x9E37_79B9_7F4A_7C15u64;
        (0..count)
            .map(|_| {
                state = state
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1_442_695_040_888_963_407);
                lo + (hi - lo) * ((state >> 11) as f64 / (1u64 << 53) as f64)
            })
            .collect()
    }

    #[test]
    fn direct_ansi256_search_matches_the_exhaustive_one() {
        let check = |rgb: [f64; 3]| {
            let expected = nearest_reference(rgb, ImageColorMode::Ansi256, ColorDistance::Rgb);
            assert_eq!(
                nearest(rgb, ImageColorMode::Ansi256, ColorDistance::Rgb),
                expected,
                "{rgb:?}"
            );
            assert_eq!(
                nearest(rgb, ImageColorMode::TrueColor, ColorDistance::Rgb),
                expected
            );
        };
        // A lattice of integer colours, every channel hitting 0 and 255.
        let lattice: Vec<f64> = (0..=255).step_by(15).map(f64::from).collect();
        for &r in &lattice {
            for &g in &lattice {
                for &b in &lattice {
                    check([r, g, b]);
                }
            }
        }
        // Exact ties and near-ties: cube midpoints, ramp midpoints, levels
        // and ramp values themselves, each nudged by an ulp either way.
        let mut edges = vec![-3.0, 0.0, 255.0, 258.0];
        for v in [47.5, 115.0, 155.0, 195.0, 235.0, 95.0, 135.0, 175.0, 215.0] {
            edges.push(v);
        }
        for j in 0..24 {
            edges.push(8.0 + 10.0 * f64::from(j));
            edges.push(13.0 + 10.0 * f64::from(j));
        }
        let nudged: Vec<f64> = edges
            .iter()
            .flat_map(|&v: &f64| [v.next_down(), v, v.next_up()])
            .collect();
        for &r in &nudged {
            for &g in nudged.iter().step_by(3) {
                for &b in nudged.iter().step_by(17) {
                    check([r, g, b]);
                }
            }
        }
        // Grays straddling every ramp and cube boundary.
        for &v in &nudged {
            check([v, v, v]);
            check([v, v + 0.25, v - 0.25]);
        }
        // Random real-valued colours, including slightly out of range ones
        // (diffused error is clamped, but other callers need not be).
        let values = random_values(3 * 10_000, -10.0, 265.0);
        for rgb in values.chunks(3) {
            check([rgb[0], rgb[1], rgb[2]]);
        }
        // Non-finite input falls back to the scan, and so agrees too.
        for v in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1e200] {
            check([v, 10.0, 10.0]);
        }
    }

    /// All 16.7M integer colours; slow in debug builds, so run on demand:
    /// `cargo test --release -p rs-rich-art --features image -- --ignored`.
    #[test]
    #[ignore = "exhaustive; run with --release -- --ignored"]
    fn direct_ansi256_search_matches_for_every_integer_colour() {
        for r in 0..=255u8 {
            for g in 0..=255u8 {
                for b in 0..=255u8 {
                    let rgb = [r, g, b].map(f64::from);
                    assert_eq!(
                        nearest(rgb, ImageColorMode::Ansi256, ColorDistance::Rgb),
                        nearest_reference(rgb, ImageColorMode::Ansi256, ColorDistance::Rgb),
                        "{rgb:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn static_tables_match_the_reference_for_every_mode_and_distance() {
        let values = random_values(3 * 2_000, -5.0, 260.0);
        for mode in [
            ImageColorMode::Ansi256,
            ImageColorMode::Ansi16,
            ImageColorMode::Grayscale,
        ] {
            for distance in [ColorDistance::Rgb, ColorDistance::Oklab] {
                for rgb in values.chunks(3) {
                    let rgb = [rgb[0], rgb[1], rgb[2]];
                    assert_eq!(
                        nearest(rgb, mode, distance),
                        nearest_reference(rgb, mode, distance),
                        "{mode:?} {distance:?} {rgb:?}"
                    );
                }
                for index in 0..=255u8 {
                    let rgb = palette_rgb(index).map(f64::from);
                    assert_eq!(
                        nearest(rgb, mode, distance),
                        nearest_reference(rgb, mode, distance)
                    );
                }
            }
        }
    }
}
