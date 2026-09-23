//! Still-image geometry and grayscale before fit and final sampling.
use image::{DynamicImage, RgbImage};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Rotation {
    #[default]
    None,
    Clockwise90,
    Clockwise180,
    Clockwise270,
}
/// Still-image adjustments, applied in this fixed order: rotation, horizontal
/// flip, vertical flip, brightness, contrast, gamma, grayscale. Fitting,
/// background compositing, sampling and colour quantization all follow.
///
/// Brightness, contrast and gamma act on each encoded (sRGB) channel value
/// `v` in `0.0..=1.0`, clamping after every step, and never touch alpha:
///
/// * brightness `b`: `v * b`
/// * contrast `c`: `(v - 0.5) * c + 0.5`
/// * gamma `g`: `v.powf(1.0 / g)` (above 1 brightens mid-tones)
///
/// Each defaults to `1.0`, the identity, which leaves the image untouched.
/// Brightness and contrast must be finite and non-negative, gamma finite and
/// positive; [`ImageArt::render`](crate::ImageArt::render) rejects anything
/// else as [`ImageArtError::InvalidAdjustment`](crate::ImageArtError::InvalidAdjustment).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ImageTransforms {
    pub rotation: Rotation,
    pub flip_horizontal: bool,
    pub flip_vertical: bool,
    pub grayscale: bool,
    pub brightness: f32,
    pub contrast: f32,
    pub gamma: f32,
}
impl Default for ImageTransforms {
    fn default() -> Self {
        ImageTransforms {
            rotation: Rotation::None,
            flip_horizontal: false,
            flip_vertical: false,
            grayscale: false,
            brightness: 1.0,
            contrast: 1.0,
            gamma: 1.0,
        }
    }
}
impl ImageTransforms {
    /// Whether brightness, contrast and gamma are in their documented ranges.
    pub fn adjustments_valid(&self) -> bool {
        let non_negative = |v: f32| v.is_finite() && v >= 0.0;
        non_negative(self.brightness)
            && non_negative(self.contrast)
            && self.gamma.is_finite()
            && self.gamma > 0.0
    }
    fn adjusts(&self) -> bool {
        self.brightness != 1.0 || self.contrast != 1.0 || self.gamma != 1.0
    }
    /// The per-channel lookup table for brightness, contrast and gamma.
    fn adjustment_table(&self) -> [u8; 256] {
        let (b, c, g) = (
            f64::from(self.brightness),
            f64::from(self.contrast),
            f64::from(self.gamma),
        );
        std::array::from_fn(|i| {
            let v = (i as f64 / 255.0 * b).clamp(0.0, 1.0);
            let v = ((v - 0.5) * c + 0.5).clamp(0.0, 1.0);
            let v = v.powf(1.0 / g).clamp(0.0, 1.0);
            (v * 255.0).round() as u8
        })
    }
}
fn gray(rgb: [u8; 3]) -> u8 {
    ((77 * u32::from(rgb[0]) + 150 * u32::from(rgb[1]) + 29 * u32::from(rgb[2]) + 128) >> 8) as u8
}
pub(crate) fn prepare(
    image: &DynamicImage,
    t: ImageTransforms,
    background: [u8; 3],
) -> (DynamicImage, [u8; 3]) {
    let mut image = match t.rotation {
        Rotation::None => image.clone(),
        Rotation::Clockwise90 => image.rotate90(),
        Rotation::Clockwise180 => image.rotate180(),
        Rotation::Clockwise270 => image.rotate270(),
    };
    if t.flip_horizontal {
        image = image.fliph();
    }
    if t.flip_vertical {
        image = image.flipv();
    }
    if t.adjusts() {
        let table = t.adjustment_table();
        let mut rgba = image.to_rgba8();
        for pixel in rgba.pixels_mut() {
            for c in 0..3 {
                pixel.0[c] = table[usize::from(pixel.0[c])];
            }
        }
        image = DynamicImage::ImageRgba8(rgba);
    }
    if !t.grayscale {
        return (image, background);
    }
    let rgba = image.to_rgba8();
    let mut output = RgbImage::new(rgba.width(), rgba.height());
    for (x, y, pixel) in output.enumerate_pixels_mut() {
        let source = rgba.get_pixel(x, y).0;
        let alpha = u32::from(source[3]);
        let rgb = std::array::from_fn(|c| {
            ((u32::from(source[c]) * alpha + u32::from(background[c]) * (255 - alpha) + 127) / 255)
                as u8
        });
        pixel.0 = [gray(rgb); 3];
    }
    (DynamicImage::ImageRgb8(output), [gray(background); 3])
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, Rgba, RgbaImage};
    #[test]
    fn geometry_order_and_alpha_are_exact_and_source_is_unchanged() {
        let mut pixels = RgbaImage::new(2, 3);
        for (i, p) in pixels.pixels_mut().enumerate() {
            *p = Rgba([(i + 1) as u8, 0, 0, if i == 4 { 0 } else { 255 }]);
        }
        let source = DynamicImage::ImageRgba8(pixels.clone());
        let (result, _) = prepare(
            &source,
            ImageTransforms {
                rotation: Rotation::Clockwise90,
                flip_horizontal: true,
                flip_vertical: true,
                ..Default::default()
            },
            [0, 0, 0],
        );
        let result = result.to_rgba8();
        assert_eq!(result.dimensions(), (3, 2));
        assert_eq!(
            result.pixels().map(|p| p[0]).collect::<Vec<_>>(),
            [2, 4, 6, 1, 3, 5]
        );
        assert_eq!(result.get_pixel(2, 1)[3], 0);
        assert_eq!(source.to_rgba8(), pixels);
    }
    #[test]
    fn grayscale_composites_and_returns_gray_background() {
        let source = DynamicImage::ImageRgba8(RgbaImage::from_pixel(1, 1, Rgba([0, 255, 0, 0])));
        let (image, background) = prepare(
            &source,
            ImageTransforms {
                grayscale: true,
                ..Default::default()
            },
            [255, 0, 0],
        );
        assert_eq!(background, [77, 77, 77]);
        assert_eq!(image.to_rgb8().get_pixel(0, 0).0, [77, 77, 77]);
        let source = DynamicImage::ImageRgba8(RgbaImage::from_pixel(1, 1, Rgba([0, 255, 0, 255])));
        assert_eq!(
            prepare(
                &source,
                ImageTransforms {
                    grayscale: true,
                    ..Default::default()
                },
                [0, 0, 0]
            )
            .0
            .to_rgb8()
            .get_pixel(0, 0)
            .0,
            [149, 149, 149]
        );
    }
    #[test]
    fn adjustments_follow_the_documented_formulas() {
        let table = |b: f32, c: f32, g: f32| {
            ImageTransforms {
                brightness: b,
                contrast: c,
                gamma: g,
                ..Default::default()
            }
            .adjustment_table()
        };
        let identity = table(1.0, 1.0, 1.0);
        assert!(identity
            .iter()
            .enumerate()
            .all(|(i, &v)| usize::from(v) == i));
        assert_eq!(table(2.0, 1.0, 1.0)[100], 200);
        assert_eq!(table(2.0, 1.0, 1.0)[200], 255);
        assert_eq!(table(0.0, 1.0, 1.0)[200], 0);
        assert!(table(1.0, 0.0, 1.0).iter().all(|&v| v == 128));
        assert_eq!(table(1.0, 2.0, 1.0)[64], 0);
        assert_eq!(table(1.0, 2.0, 1.0)[160], 193);
        assert_eq!(table(1.0, 1.0, 2.0)[64], 128);
        assert_eq!(table(1.0, 1.0, 0.5)[128], 64);
        // Order: brightness, then contrast, then gamma.
        assert_eq!(table(0.5, 2.0, 1.0)[255], 128);
    }
    #[test]
    fn adjustments_keep_alpha_and_run_before_grayscale() {
        let source = DynamicImage::ImageRgba8(RgbaImage::from_pixel(1, 1, Rgba([100, 50, 0, 7])));
        let (image, _) = prepare(
            &source,
            ImageTransforms {
                brightness: 2.0,
                ..Default::default()
            },
            [0, 0, 0],
        );
        assert_eq!(image.to_rgba8().get_pixel(0, 0).0, [200, 100, 0, 7]);
        let opaque = DynamicImage::ImageRgba8(RgbaImage::from_pixel(1, 1, Rgba([100, 50, 0, 255])));
        let (gray, _) = prepare(
            &opaque,
            ImageTransforms {
                brightness: 2.0,
                grayscale: true,
                ..Default::default()
            },
            [0, 0, 0],
        );
        // gray([200, 100, 0]) = (77*200 + 150*100 + 128) >> 8 = 119
        assert_eq!(gray.to_rgb8().get_pixel(0, 0).0, [119; 3]);
    }
    #[test]
    fn adjustment_ranges_are_validated() {
        let with = |b: f32, c: f32, g: f32| ImageTransforms {
            brightness: b,
            contrast: c,
            gamma: g,
            ..Default::default()
        };
        assert!(ImageTransforms::default().adjustments_valid());
        assert!(with(0.0, 0.0, 0.1).adjustments_valid());
        for bad in [
            with(-0.1, 1.0, 1.0),
            with(1.0, -1.0, 1.0),
            with(1.0, 1.0, 0.0),
            with(f32::NAN, 1.0, 1.0),
            with(1.0, f32::INFINITY, 1.0),
            with(1.0, 1.0, f32::NAN),
        ] {
            assert!(!bad.adjustments_valid(), "{bad:?}");
        }
    }
}
