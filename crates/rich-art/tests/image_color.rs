#![cfg(feature = "image")]
use rich::{color::ColorSystem, Console};
use rich_art::{
    image::{DynamicImage, Rgb, RgbImage, Rgba, RgbaImage},
    ColorDistance, Dither, ImageArt, ImageColorMode, ImageFit, ImageMode,
};

fn console() -> Console {
    Console::builder()
        .force_terminal(true)
        .color_system(Some(ColorSystem::Truecolor))
        .no_color(false)
        .width(8)
        .build()
}
fn source() -> DynamicImage {
    DynamicImage::ImageRgb8(RgbImage::from_fn(7, 5, |x, y| {
        Rgb([(x * 37) as u8, (y * 51) as u8, 123])
    }))
}
#[test]
fn explicit_defaults_preserve_existing_bytes() {
    let c = console();
    for mode in [ImageMode::Ascii, ImageMode::Blocks, ImageMode::Braille] {
        let make = || {
            ImageArt::new(source())
                .mode(mode)
                .width(3)
                .height(2)
                .color(true)
        };
        assert_eq!(
            c.render_to_string(&make()),
            c.render_to_string(
                &make()
                    .color_mode(ImageColorMode::TrueColor)
                    .dither(Dither::None)
                    .color_distance(ColorDistance::Rgb)
            )
        );
    }
}
#[test]
fn indexed_output_is_emitted_after_final_sampling_for_both_backends() {
    let c = console();
    for mode in [ImageMode::Ascii, ImageMode::Blocks] {
        for fit in [None, Some(ImageFit::Contain), Some(ImageFit::Cover)] {
            let mut art = ImageArt::new(source())
                .mode(mode)
                .width(3)
                .height(2)
                .color(true)
                .color_mode(ImageColorMode::Ansi256)
                .dither(Dither::FloydSteinberg);
            if let Some(fit) = fit {
                art = art.fit(fit);
            }
            let out = c.render_to_string(&art);
            assert!(out.contains("38;5;"), "{out:?}");
            assert!(!out.contains("38;2;"), "{out:?}");
            assert!(!out.contains("48;2;"), "{out:?}");
            assert_eq!(out, c.render_to_string(&art));
        }
    }
}
#[test]
fn transparency_is_composited_before_palette_selection() {
    let c = console();
    for mode in [ImageMode::Ascii, ImageMode::Blocks] {
        let art = ImageArt::new(DynamicImage::ImageRgba8(RgbaImage::from_pixel(
            1,
            2,
            Rgba([255, 0, 0, 0]),
        )))
        .mode(mode)
        .width(1)
        .height(1)
        .color(true)
        .background([0, 255, 0])
        .color_mode(ImageColorMode::Ansi256);
        let segments = art.render(&c, &c.options()).unwrap();
        assert_eq!(
            segments[0].style.as_ref().unwrap().color().unwrap(),
            &rich::color::Color::from_ansi(46)
        );
    }
}
#[test]
fn unsupported_combinations_are_strict_errors() {
    let c = console();
    // Braille draws monochrome dots: there is nothing to quantize.
    for color_mode in [
        ImageColorMode::Ansi256,
        ImageColorMode::Ansi16,
        ImageColorMode::Grayscale,
    ] {
        assert_eq!(
            ImageArt::new(source())
                .mode(ImageMode::Braille)
                .color_mode(color_mode)
                .render(&c, &c.options())
                .unwrap_err(),
            rich_art::ImageArtError::UnsupportedColorOptions
        );
    }
    // Dithering and a colour distance only mean something with a palette.
    assert!(ImageArt::new(source())
        .mode(ImageMode::Ascii)
        .dither(Dither::FloydSteinberg)
        .render(&c, &c.options())
        .is_err());
    assert!(ImageArt::new(source())
        .mode(ImageMode::Blocks)
        .color_distance(ColorDistance::Oklab)
        .render(&c, &c.options())
        .is_err());
}

#[cfg(feature = "sixel")]
#[test]
fn sixel_accepts_every_reduced_palette() {
    let c = console();
    for color_mode in [
        ImageColorMode::Ansi256,
        ImageColorMode::Ansi16,
        ImageColorMode::Grayscale,
    ] {
        let segments = ImageArt::new(source())
            .mode(ImageMode::Sixel)
            .width(2)
            .color_mode(color_mode)
            .dither(Dither::Atkinson)
            .render(&c, &c.options())
            .unwrap();
        assert!(
            segments[0].text.starts_with("\x1bP9;1;0q"),
            "{color_mode:?}"
        );
    }
}

#[test]
fn atkinson_and_oklab_render_deterministically_on_every_text_backend() {
    let c = console();
    for mode in [ImageMode::Ascii, ImageMode::Blocks, ImageMode::Quadrants] {
        for distance in [ColorDistance::Rgb, ColorDistance::Oklab] {
            let make = || {
                ImageArt::new(source())
                    .mode(mode)
                    .width(4)
                    .height(2)
                    .color(true)
                    .color_mode(ImageColorMode::Ansi16)
                    .dither(Dither::Atkinson)
                    .color_distance(distance)
            };
            let out = c.render_to_string(&make());
            assert_eq!(out, c.render_to_string(&make()));
            assert!(!out.contains("38;5;") && !out.contains("38;2;"), "{out:?}");
        }
    }
}

#[test]
fn sizing_precedes_quantization_without_a_second_resample() {
    let c = console();
    // Sampling averages 0 and 26 to 13: a tie selects palette 232 (8).
    // Quantizing first would average 0 and 28 to 14, choosing 233 (18).
    for mode in [ImageMode::Ascii, ImageMode::Blocks] {
        let source = DynamicImage::ImageRgb8(RgbImage::from_fn(2, 4, |x, _| {
            Rgb([if x == 0 { 0 } else { 26 }; 3])
        }));
        let art = ImageArt::new(source)
            .mode(mode)
            .width(1)
            .height(1)
            .color(true)
            .color_mode(ImageColorMode::Ansi256);
        let segments = art.render(&c, &c.options()).unwrap();
        assert_eq!(
            segments[0].style.as_ref().unwrap().color().unwrap(),
            &rich::color::Color::from_ansi(232)
        );
        if mode == ImageMode::Blocks {
            assert_eq!(
                segments[0].style.as_ref().unwrap().bgcolor().unwrap(),
                &rich::color::Color::from_ansi(232)
            );
        }
    }
}

#[test]
fn diffusion_precedes_ascii_glyph_selection_and_half_block_pairing() {
    let c = console();
    let make = || {
        ImageArt::new(DynamicImage::ImageRgb8(RgbImage::from_pixel(
            2,
            2,
            Rgb([12; 3]),
        )))
        .width(2)
        .color_mode(ImageColorMode::Ansi256)
        .dither(Dither::FloydSteinberg)
    };
    let ascii = make().mode(ImageMode::Ascii).height(2);
    let segments = ascii.render(&c, &c.options()).unwrap();
    let text: String = segments
        .iter()
        .map(|segment| segment.text.as_str())
        .collect();
    assert_eq!(text, " M\n  ");
    let blocks = make().mode(ImageMode::Blocks).height(1);
    let segments = blocks.render(&c, &c.options()).unwrap();
    let pairs: Vec<_> = segments
        .iter()
        .map(|segment| {
            let style = segment.style.as_ref().unwrap();
            (
                style.color().unwrap().clone(),
                style.bgcolor().unwrap().clone(),
            )
        })
        .collect();
    use rich::color::Color;
    assert_eq!(
        pairs,
        vec![
            (Color::from_ansi(232), Color::from_ansi(232)),
            (Color::from_ansi(233), Color::from_ansi(232))
        ]
    );
}
