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
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ImageTransforms {
    pub rotation: Rotation,
    pub flip_horizontal: bool,
    pub flip_vertical: bool,
    pub grayscale: bool,
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
                grayscale: false,
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
}
