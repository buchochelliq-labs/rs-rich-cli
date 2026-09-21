#![cfg(feature = "image")]
use rich::{ColorSystem, Console};
use rich_art::{ImageArt, ImageFit, ImageMode, ImageTransforms};
#[test]
fn grayscale_contain_padding_and_transparent_pixels_have_equal_channels() {
    let image = rich_art::image::DynamicImage::ImageRgba8(rich_art::image::RgbaImage::from_pixel(
        4,
        1,
        rich_art::image::Rgba([0, 255, 0, 0]),
    ));
    let art = ImageArt::new(image)
        .mode(ImageMode::Blocks)
        .width(4)
        .height(3)
        .fit(ImageFit::Contain)
        .background([255, 0, 0])
        .transforms(ImageTransforms {
            grayscale: true,
            ..Default::default()
        });
    let c = Console::builder()
        .width(4)
        .height(3)
        .force_terminal(false)
        .no_color(false)
        .color_system(Some(ColorSystem::Truecolor))
        .build();
    let segments = art.render(&c, &c.options()).unwrap();
    let mut checked = 0;
    for s in segments {
        if let Some(style) = s.style {
            for color in [style.color(), style.bgcolor()].into_iter().flatten() {
                if let Some(rgb) = color.get_truecolor() {
                    assert_eq!((rgb.red, rgb.green, rgb.blue), (77, 77, 77));
                    checked += 1;
                }
            }
        }
    }
    assert!(checked >= 12);
}
