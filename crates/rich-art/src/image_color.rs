//! Opt-in colour processing for the final sampled image raster.
use image::RgbaImage;

/// Colour precision for ASCII and half-block image rendering.
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
}

/// Error diffusion applied after sizing and compositing, before glyph selection.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Dither {
    /// Map each pixel independently.
    #[default]
    None,
    /// Scan left-to-right, top-to-bottom with 7/16, 3/16, 5/16, 1/16 weights.
    /// Discard error outside the raster; do not wrap or renormalize edge weights.
    FloydSteinberg,
}

/// Squared Euclidean distance in encoded RGB (not linear-light RGB).
/// Exact ties choose the lowest palette index for reproducible output.
fn nearest(rgb: [f64; 3]) -> (u8, [u8; 3]) {
    const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
    let mut best = (16, [0; 3]);
    let mut distance = f64::INFINITY;
    for index in 16u16..=255 {
        let value = index - 16;
        let candidate = if index < 232 {
            [
                LEVELS[(value / 36) as usize],
                LEVELS[(value / 6 % 6) as usize],
                LEVELS[(value % 6) as usize],
            ]
        } else {
            [8 + (index as u8 - 232) * 10; 3]
        };
        let d: f64 = (0..3)
            .map(|c| (rgb[c] - f64::from(candidate[c])).powi(2))
            .sum();
        if d < distance {
            distance = d;
            best = (index as u8, candidate);
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
            let rgb = std::array::from_fn(|c| {
                (f64::from(pixel.0[c]) * alpha + current[x][c]).clamp(0.0, 255.0)
            });
            let (index, color) = nearest(rgb);
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
}
