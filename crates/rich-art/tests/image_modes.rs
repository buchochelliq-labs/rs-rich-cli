#![cfg(feature = "image")]
//! 0.0.10 image options: ANSI16/grayscale (#125), stretch, caps and tone
//! adjustments (#126), and quadrant blocks (#199).
use rich::segment::Segment;
use rich::{color::ColorSystem, Console};
use rich_art::{
    image::{DynamicImage, Rgb, RgbImage, Rgba, RgbaImage},
    Dither, ImageArt, ImageArtError, ImageColorMode, ImageFit, ImageMode, ImageTransforms,
};

fn console(width: usize) -> Console {
    Console::builder()
        .force_terminal(true)
        .color_system(Some(ColorSystem::Truecolor))
        .no_color(false)
        .width(width)
        .build()
}

fn gradient(w: u32, h: u32) -> DynamicImage {
    DynamicImage::ImageRgb8(RgbImage::from_fn(w, h, |x, y| {
        Rgb([(x * 37 % 256) as u8, (y * 51 % 256) as u8, 123])
    }))
}

fn rows(segments: &[Segment]) -> Vec<String> {
    Segment::split_lines(segments)
        .into_iter()
        .map(|l| l.iter().map(|s| s.text.as_str()).collect())
        .collect()
}

fn render(art: &ImageArt, width: usize) -> Result<Vec<Segment>, ImageArtError> {
    let c = console(width);
    art.render(&c, &c.options())
}

#[test]
fn ansi16_and_grayscale_emit_only_their_palettes_with_every_dither() {
    let c = console(8);
    for mode in [ImageMode::Ascii, ImageMode::Blocks, ImageMode::Quadrants] {
        for dither in [Dither::None, Dither::FloydSteinberg, Dither::Bayer4x4] {
            let make = |color| {
                ImageArt::new(gradient(9, 7))
                    .mode(mode)
                    .width(4)
                    .color(true)
                    .color_mode(color)
                    .dither(dither)
            };
            let ansi16 = c.render_to_string(&make(ImageColorMode::Ansi16));
            assert!(
                !ansi16.contains("38;2;") && !ansi16.contains("38;5;"),
                "{ansi16:?}"
            );
            assert!(
                !ansi16.contains("48;2;") && !ansi16.contains("48;5;"),
                "{ansi16:?}"
            );
            let gray = c.render_to_string(&make(ImageColorMode::Grayscale));
            assert!(gray.contains("38;5;"), "{gray:?}");
            assert!(
                !gray.contains("38;2;") && !gray.contains("48;2;"),
                "{gray:?}"
            );
            for (at, _) in gray.match_indices("8;5;") {
                let index: u8 = gray[at + 4..]
                    .split(|c: char| !c.is_ascii_digit())
                    .next()
                    .unwrap()
                    .parse()
                    .unwrap();
                assert!(index == 16 || index >= 231, "{mode:?} {dither:?}: {index}");
            }
        }
    }
}

#[test]
fn quantized_modes_still_reject_unsupported_backends_and_truecolor_dither() {
    for color in [ImageColorMode::Ansi16, ImageColorMode::Grayscale] {
        let art = ImageArt::new(gradient(4, 4))
            .mode(ImageMode::Braille)
            .color_mode(color);
        assert_eq!(render(&art, 8), Err(ImageArtError::UnsupportedColorOptions));
    }
    let art = ImageArt::new(gradient(4, 4))
        .mode(ImageMode::Quadrants)
        .dither(Dither::Bayer4x4);
    assert_eq!(render(&art, 8), Err(ImageArtError::UnsupportedColorOptions));
}

#[test]
fn unset_adjustments_and_caps_leave_output_byte_identical() {
    let c = console(12);
    for mode in [
        ImageMode::Ascii,
        ImageMode::Blocks,
        ImageMode::Braille,
        ImageMode::Quadrants,
    ] {
        let base = || {
            ImageArt::new(gradient(9, 7))
                .mode(mode)
                .width(6)
                .color(true)
        };
        let expected = c.render_to_string(&base());
        let explicit = base()
            .transforms(ImageTransforms::default())
            .max_width(99)
            .max_height(99);
        assert_eq!(c.render_to_string(&explicit), expected, "{mode:?}");
    }
}

#[test]
fn stretch_fills_the_rectangle_ignoring_aspect() {
    // A 4×1 strip: contain letterboxes it, stretch fills all three rows.
    let strip = DynamicImage::ImageRgb8(RgbImage::from_fn(4, 1, |x, _| {
        Rgb(if x < 2 { [255, 0, 0] } else { [0, 0, 255] })
    }));
    let make = |fit| {
        ImageArt::new(strip.clone())
            .mode(ImageMode::Blocks)
            .width(4)
            .height(3)
            .fit(fit)
            .background([0, 255, 0])
    };
    let c = console(8);
    let stretched = c.render_to_string(&make(ImageFit::Stretch));
    assert!(!stretched.contains("0;255;0"), "{stretched:?}");
    let contained = c.render_to_string(&make(ImageFit::Contain));
    assert!(contained.contains("0;255;0"), "{contained:?}");
    let segments = render(&make(ImageFit::Stretch), 8).unwrap();
    assert_eq!(rows(&segments), ["▀▀▀▀", "▀▀▀▀", "▀▀▀▀"]);
}

#[test]
fn max_width_and_height_cap_every_backend() {
    for mode in [
        ImageMode::Ascii,
        ImageMode::Blocks,
        ImageMode::Braille,
        ImageMode::Quadrants,
    ] {
        let wide = ImageArt::new(gradient(40, 10))
            .mode(mode)
            .width(30)
            .max_width(10);
        let out = rows(&render(&wide, 40).unwrap());
        assert!(
            out.iter().all(|r| r.chars().count() == 10),
            "{mode:?} {out:?}"
        );

        // A tall image capped at 4 rows narrows rather than squashing.
        let tall = ImageArt::new(gradient(10, 80))
            .mode(mode)
            .width(20)
            .max_height(4);
        let out = rows(&render(&tall, 40).unwrap());
        assert!(out.len() <= 4, "{mode:?} {out:?}");
        assert!(out[0].chars().count() < 20, "{mode:?} {out:?}");
    }
    // With fitting, the caps clamp the rectangle.
    let fitted = ImageArt::new(gradient(8, 8))
        .mode(ImageMode::Blocks)
        .width(12)
        .height(6)
        .fit(ImageFit::Stretch)
        .max_width(5)
        .max_height(2);
    let out = rows(&render(&fitted, 40).unwrap());
    assert_eq!(out, ["▀▀▀▀▀", "▀▀▀▀▀"]);
}

#[test]
fn tone_adjustments_change_output_and_invalid_values_are_errors() {
    let c = console(8);
    let make = |t| {
        ImageArt::new(gradient(6, 4))
            .mode(ImageMode::Blocks)
            .width(3)
            .transforms(t)
    };
    let base = c.render_to_string(&make(ImageTransforms::default()));
    let dark = c.render_to_string(&make(ImageTransforms {
        brightness: 0.0,
        ..Default::default()
    }));
    assert_ne!(base, dark);
    assert!(
        dark.contains("38;2;0;0;0") && !dark.contains("38;2;0;0;123"),
        "{dark:?}"
    );
    for t in [
        ImageTransforms {
            gamma: 0.0,
            ..Default::default()
        },
        ImageTransforms {
            contrast: -1.0,
            ..Default::default()
        },
    ] {
        assert_eq!(render(&make(t), 8), Err(ImageArtError::InvalidAdjustment));
    }
}

#[test]
fn quadrants_split_cells_and_handle_odd_sizes_and_transparency() {
    // 2×2 checkerboard in one cell: the diagonal glyph with both colours.
    let checker = DynamicImage::ImageRgb8(RgbImage::from_fn(2, 2, |x, y| {
        Rgb(if (x + y) % 2 == 0 {
            [255, 0, 0]
        } else {
            [0, 0, 255]
        })
    }));
    let art = ImageArt::new(checker).mode(ImageMode::Quadrants).width(1);
    let segments = render(&art, 4).unwrap();
    assert_eq!(rows(&segments), ["▚"]);
    let out = console(4).render_to_string(&art);
    assert!(
        out.contains("38;2;255;0;0") && out.contains("48;2;0;0;255"),
        "{out:?}"
    );

    // Left red, right blue at one cell per pixel pair: two full blocks.
    let halves = DynamicImage::ImageRgb8(RgbImage::from_fn(4, 2, |x, _| {
        Rgb(if x < 2 { [255, 0, 0] } else { [0, 0, 255] })
    }));
    let art = ImageArt::new(halves).mode(ImageMode::Quadrants).width(2);
    assert_eq!(rows(&render(&art, 4).unwrap()), ["██"]);

    // Odd source sizes still give a full, rectangular grid.
    for (w, h, cols) in [(3, 3, 3), (5, 7, 5), (1, 9, 1), (7, 1, 7)] {
        let art = ImageArt::new(gradient(w, h))
            .mode(ImageMode::Quadrants)
            .width(cols);
        let out = rows(&render(&art, 20).unwrap());
        assert!(!out.is_empty());
        assert!(
            out.iter().all(|r| r.chars().count() == cols),
            "{w}x{h}: {out:?}"
        );
    }

    // Transparency composites onto black, like half blocks.
    let clear = DynamicImage::ImageRgba8(RgbaImage::from_pixel(4, 4, Rgba([255, 255, 255, 0])));
    let art = ImageArt::new(clear).mode(ImageMode::Quadrants).width(2);
    let out = console(4).render_to_string(&art);
    assert!(
        out.contains("38;2;0;0;0") && !out.contains("255;255;255"),
        "{out:?}"
    );

    // A fitted quadrant raster keeps the cell grid of the other backends.
    let fitted = ImageArt::new(gradient(9, 5))
        .mode(ImageMode::Quadrants)
        .width(5)
        .height(3)
        .fit(ImageFit::Contain);
    let out = rows(&render(&fitted, 20).unwrap());
    assert_eq!(out.len(), 3);
    assert!(out.iter().all(|r| r.chars().count() == 5), "{out:?}");
}
