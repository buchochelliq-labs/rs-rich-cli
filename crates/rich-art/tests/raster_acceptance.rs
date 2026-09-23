#![cfg(feature = "image")]
use rich::{ColorSystem, Console, Renderable};
use rich_art::image::{DynamicImage, Rgba, RgbaImage};
use rich_art::{BlockArt, BrailleArt, Dither, ImageArt, ImageColorMode, ImageMode};
fn console() -> Console {
    Console::builder()
        .width(4)
        .height(8)
        .no_color(false)
        .color_system(Some(ColorSystem::Truecolor))
        .force_terminal(false)
        .build()
}
#[test]
fn bayer_matches_fixed_palette_indices_and_rejects_unsupported_modes() {
    let image = DynamicImage::ImageRgba8(RgbaImage::from_pixel(4, 4, Rgba([128, 128, 128, 255])));
    let art = ImageArt::new(image.clone())
        .mode(ImageMode::Blocks)
        .width(4)
        .color_mode(ImageColorMode::Ansi256)
        .dither(Dither::Bayer4x4);
    let c = console();
    let segments = art.render(&c, &c.options()).unwrap();
    let cells: Vec<_> = segments.iter().filter_map(|s| s.style.as_ref()).collect();
    let upper: Vec<_> = cells
        .iter()
        .map(|s| s.color().unwrap().number.unwrap())
        .collect();
    let lower: Vec<_> = cells
        .iter()
        .map(|s| s.bgcolor().unwrap().number.unwrap())
        .collect();
    assert_eq!(upper, [242, 244, 243, 102, 243, 102, 243, 244]);
    assert_eq!(lower, [245, 243, 245, 244, 245, 244, 245, 243]);
    assert_eq!(segments, art.render(&c, &c.options()).unwrap());
    for mode in [ImageMode::Braille, ImageMode::Sixel] {
        assert!(ImageArt::new(image.clone())
            .mode(mode)
            .color_mode(ImageColorMode::Ansi256)
            .dither(Dither::Bayer4x4)
            .render(&c, &c.options())
            .is_err());
    }
    assert!(ImageArt::new(image)
        .mode(ImageMode::Blocks)
        .dither(Dither::Bayer4x4)
        .render(&c, &c.options())
        .is_err());
}
#[test]
fn every_braille_dot_has_the_correct_unicode_bit_and_alpha_is_empty() {
    for (x, y, bit) in [
        (0, 0, 0),
        (0, 1, 1),
        (0, 2, 2),
        (1, 0, 3),
        (1, 1, 4),
        (1, 2, 5),
        (0, 3, 6),
        (1, 3, 7),
    ] {
        let mut image = RgbaImage::from_pixel(2, 4, Rgba([0, 0, 0, 255]));
        image.put_pixel(x, y, Rgba([255, 255, 255, 255]));
        assert_eq!(
            BrailleArt::new(DynamicImage::ImageRgba8(image))
                .width(1)
                .to_text(1),
            char::from_u32(0x2800 + (1 << bit)).unwrap().to_string()
        );
    }
    let image = RgbaImage::from_pixel(1, 3, Rgba([255, 255, 255, 0]));
    assert!(BrailleArt::new(DynamicImage::ImageRgba8(image))
        .width(1)
        .to_text(1)
        .chars()
        .all(|c| c == '\u{2800}' || c == '\n'));
}
#[test]
fn odd_block_heights_and_transparency_keep_visible_backgrounds() {
    let image = RgbaImage::from_pixel(2, 3, Rgba([255, 0, 0, 255]));
    let c = console();
    let segments = BlockArt::new(DynamicImage::ImageRgba8(image))
        .width(2)
        .rich_render(&c, &c.options());
    assert_eq!(segments.iter().filter(|s| s.text == "▀").count(), 4);
    let mut image = RgbaImage::from_pixel(1, 2, Rgba([255, 0, 0, 255]));
    image.put_pixel(0, 0, Rgba([255, 255, 255, 0]));
    let segments = BlockArt::new(DynamicImage::ImageRgba8(image))
        .width(1)
        .rich_render(&c, &c.options());
    let style = segments[0].style.as_ref().unwrap();
    assert_eq!(
        style.color().unwrap().get_truecolor().unwrap().hex(),
        "#000000"
    );
    assert_eq!(
        style.bgcolor().unwrap().get_truecolor().unwrap().hex(),
        "#ff0000"
    );
}
