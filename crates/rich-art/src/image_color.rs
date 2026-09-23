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

/// Nearest palette entry for `mode` (never `TrueColor`). Squared Euclidean
/// distance in encoded RGB (luma for `Grayscale`), not linear-light RGB.
/// Candidates are scanned in ascending index order and only a strictly closer
/// one replaces the best, so exact ties choose the lowest palette index.
pub(crate) fn nearest(rgb: [f64; 3], mode: ImageColorMode) -> (u8, [u8; 3]) {
    let mut best = (0, [0; 3]);
    let mut distance = f64::INFINITY;
    let mut consider = |index: u8, candidate: [u8; 3], d: f64| {
        if d < distance {
            distance = d;
            best = (index, candidate);
        }
    };
    let rgb_distance =
        |c: [u8; 3]| -> f64 { (0..3).map(|i| (rgb[i] - f64::from(c[i])).powi(2)).sum() };
    match mode {
        ImageColorMode::TrueColor | ImageColorMode::Ansi256 => {
            for index in 16u8..=255 {
                let c = eight_bit(index);
                consider(index, c, rgb_distance(c));
            }
        }
        ImageColorMode::Ansi16 => {
            for (index, t) in rich::color::STANDARD_PALETTE.iter().enumerate() {
                let c = [t.red, t.green, t.blue];
                consider(index as u8, c, rgb_distance(c));
            }
        }
        ImageColorMode::Grayscale => {
            let luma = (77.0 * rgb[0] + 150.0 * rgb[1] + 29.0 * rgb[2]) / 256.0;
            for index in std::iter::once(16).chain(231..=255) {
                let c = eight_bit(index);
                consider(index, c, (luma - f64::from(c[0])).powi(2));
            }
        }
    }
    best
}

/// Mutate only the final sampled raster; returned indices correspond one-to-one
/// with its row-major pixels. Remaining alpha is composited onto black before
/// diffusion. Two error rows bound auxiliary storage to the sampled width.
pub(crate) fn preprocess(
    raster: &mut RgbaImage,
    mode: ImageColorMode,
    dither: Dither,
) -> Option<Vec<u8>> {
    if mode == ImageColorMode::TrueColor {
        return None;
    }
    let (width, height) = raster.dimensions();
    let width = width as usize;
    let mut indices = Vec::with_capacity(width * height as usize);
    let mut current = vec![[0.0; 3]; width];
    let mut next = vec![[0.0; 3]; width];
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
            let rgb = std::array::from_fn(|c| {
                (f64::from(pixel.0[c]) * alpha + current[x][c] + offset).clamp(0.0, 255.0)
            });
            let (index, color) = nearest(rgb, mode);
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
        }
        std::mem::swap(&mut current, &mut next);
        next.fill([0.0; 3]);
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
            preprocess(&mut pixels, ImageColorMode::TrueColor, Dither::None),
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
            preprocess(&mut pixels, ImageColorMode::Ansi256, Dither::None),
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
            preprocess(&mut pixels, ImageColorMode::Ansi256, Dither::FloydSteinberg),
            Some(vec![232, 233, 232, 232])
        );
        assert_eq!(
            pixels.into_raw(),
            vec![8, 8, 8, 255, 18, 18, 18, 255, 8, 8, 8, 255, 8, 8, 8, 255]
        );
    }

    #[test]
    fn transparent_and_tiny_rasters_are_bounded() {
        let mut transparent = RgbaImage::from_pixel(1, 1, Rgba([255, 255, 255, 0]));
        assert_eq!(
            preprocess(
                &mut transparent,
                ImageColorMode::Ansi256,
                Dither::FloydSteinberg
            ),
            Some(vec![16])
        );
        assert_eq!(transparent.get_pixel(0, 0).0, [0, 0, 0, 255]);
        let mut empty = RgbaImage::new(0, 0);
        assert_eq!(
            preprocess(&mut empty, ImageColorMode::Ansi256, Dither::FloydSteinberg),
            Some(vec![])
        );
    }

    #[test]
    fn ansi16_matches_the_standard_palette_with_lowest_index_ties() {
        let hit = |rgb: [f64; 3]| nearest(rgb, ImageColorMode::Ansi16).0;
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
        let hit = |rgb: [f64; 3]| nearest(rgb, ImageColorMode::Grayscale);
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
            for dither in [Dither::None, Dither::FloydSteinberg, Dither::Bayer4x4] {
                let mut pixels = RgbaImage::from_fn(5, 3, |x, y| {
                    Rgba([(x * 50) as u8, (y * 90) as u8, 120, 255])
                });
                let indices = preprocess(&mut pixels, mode, dither).unwrap();
                assert_eq!(indices.len(), 15);
                for (index, pixel) in indices.iter().zip(pixels.pixels()) {
                    let expected = nearest(pixel.0.map(f64::from)[..3].try_into().unwrap(), mode);
                    assert_eq!(expected.0, *index, "{mode:?} {dither:?}");
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
}
