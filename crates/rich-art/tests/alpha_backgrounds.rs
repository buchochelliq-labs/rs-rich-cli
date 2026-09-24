#![cfg(feature = "image")]
//! Alpha backgrounds (#126): the terminal's own background, and a
//! checkerboard preview.
use rich::color::{Color, ColorSystem};
use rich::segment::Segment;
use rich::Console;
use rich_art::{
    image::{DynamicImage, Rgb, RgbImage, Rgba, RgbaImage},
    ImageArt, ImageBackground, ImageColorMode, ImageFit, ImageMode,
};

fn console() -> Console {
    Console::builder()
        .force_terminal(true)
        .color_system(Some(ColorSystem::Truecolor))
        .no_color(false)
        .width(16)
        .build()
}

fn render(art: &ImageArt) -> Vec<Segment> {
    let c = console();
    art.render(&c, &c.options()).unwrap()
}

/// One rendered cell: its text, foreground and background.
type Cell = (String, Option<Color>, Option<Color>);

/// Every cell, one row per line.
fn cells(segments: &[Segment]) -> Vec<Vec<Cell>> {
    Segment::split_lines(segments)
        .into_iter()
        .map(|line| {
            line.iter()
                .flat_map(|s| {
                    let style = s.style.as_ref();
                    s.text.chars().map(move |ch| {
                        (
                            ch.to_string(),
                            style.and_then(|st| st.color().cloned()),
                            style.and_then(|st| st.bgcolor().cloned()),
                        )
                    })
                })
                .collect()
        })
        .collect()
}

const RED: [u8; 4] = [255, 0, 0, 255];
const CLEAR: [u8; 4] = [0, 0, 0, 0];

fn rgba(w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 4]) -> DynamicImage {
    DynamicImage::ImageRgba8(RgbaImage::from_fn(w, h, |x, y| Rgba(f(x, y))))
}

#[test]
fn terminal_default_leaves_transparent_halves_unpainted() {
    // Column 0: transparent over red. Column 1: red over transparent.
    // Column 2: all transparent. Column 3: all red.
    let image = rgba(4, 2, |x, y| match (x, y) {
        (0, 1) | (1, 0) | (3, _) => RED,
        _ => CLEAR,
    });
    let art = ImageArt::new(image)
        .mode(ImageMode::Blocks)
        .width(4)
        .background_mode(ImageBackground::TerminalDefault);
    let red = Some(Color::from_rgb(255, 0, 0));
    assert_eq!(
        cells(&render(&art)),
        vec![vec![
            ("▄".into(), red.clone(), None),
            ("▀".into(), red.clone(), None),
            (" ".into(), None, None),
            ("▀".into(), red.clone(), red),
        ]]
    );
}

#[test]
fn terminal_default_keeps_opaque_images_byte_identical() {
    let image = DynamicImage::ImageRgb8(RgbImage::from_fn(9, 6, |x, y| {
        Rgb([(x * 29) as u8, (y * 41) as u8, 90])
    }));
    for mode in [
        ImageMode::Ascii,
        ImageMode::Blocks,
        ImageMode::Quadrants,
        ImageMode::Braille,
    ] {
        let make = || ImageArt::new(image.clone()).mode(mode).width(5).color(true);
        assert_eq!(
            console().render_to_string(&make()),
            console().render_to_string(&make().background_mode(ImageBackground::TerminalDefault)),
            "{mode:?}"
        );
    }
}

#[test]
fn terminal_default_blanks_ascii_and_ignores_it_for_auto_levels() {
    // A transparent left half and a mid-gray right half: with auto-levels the
    // gray would otherwise be stretched against the "dark" transparent pixels.
    let image = rgba(
        4,
        2,
        |x, _| if x < 2 { CLEAR } else { [128, 128, 128, 255] },
    );
    let art = ImageArt::new(image)
        .mode(ImageMode::Ascii)
        .width(4)
        .height(1)
        .color(true)
        .background_mode(ImageBackground::TerminalDefault);
    let row = &cells(&render(&art))[0];
    assert_eq!(row[0], (" ".into(), None, None));
    assert_eq!(row[1], (" ".into(), None, None));
    assert_ne!(row[2].0, " ");
    assert_eq!(row[2].1, Some(Color::from_rgb(128, 128, 128)));
}

#[test]
fn terminal_default_quadrants_draw_only_the_opaque_quadrants() {
    // Top-left transparent, the other three red: the complement glyph ▟ in
    // red with no background.
    let image = rgba(2, 2, |x, y| if (x, y) == (0, 0) { CLEAR } else { RED });
    let art = ImageArt::new(image)
        .mode(ImageMode::Quadrants)
        .width(1)
        .background_mode(ImageBackground::TerminalDefault);
    assert_eq!(
        cells(&render(&art)),
        vec![vec![("▟".into(), Some(Color::from_rgb(255, 0, 0)), None)]]
    );
}

#[test]
fn terminal_default_contain_padding_is_transparent() {
    // A 1×2 red image contained in 4 columns by 1 row pads left and right.
    let image = rgba(1, 2, |_, _| RED);
    let art = ImageArt::new(image)
        .mode(ImageMode::Blocks)
        .width(4)
        .height(1)
        .fit(ImageFit::Contain)
        .background_mode(ImageBackground::TerminalDefault);
    let row = &cells(&render(&art))[0];
    assert_eq!(row.len(), 4);
    assert_eq!(row[0], (" ".into(), None, None));
    assert_eq!(row[3], (" ".into(), None, None));
    assert!(row
        .iter()
        .any(|cell| cell.1 == Some(Color::from_rgb(255, 0, 0))));
}

#[test]
fn checkerboard_squares_are_two_cells_by_one_when_fitting() {
    let image = rgba(8, 4, |_, _| CLEAR);
    let art = ImageArt::new(image)
        .mode(ImageMode::Blocks)
        .width(8)
        .height(2)
        .fit(ImageFit::Stretch)
        .background_mode(ImageBackground::Checkerboard);
    let light = Some(Color::from_rgb(153, 153, 153));
    let dark = Some(Color::from_rgb(102, 102, 102));
    let shades: Vec<Vec<_>> = cells(&render(&art))
        .into_iter()
        .map(|row| row.into_iter().map(|(_, fg, bg)| (fg, bg)).collect())
        .collect();
    let l = (light.clone(), light);
    let d = (dark.clone(), dark);
    assert_eq!(
        shades,
        vec![
            vec![
                l.clone(),
                l.clone(),
                d.clone(),
                d.clone(),
                l.clone(),
                l.clone(),
                d.clone(),
                d.clone()
            ],
            vec![
                d.clone(),
                d.clone(),
                l.clone(),
                l.clone(),
                d.clone(),
                d.clone(),
                l.clone(),
                l
            ],
        ]
    );
}

#[test]
fn checkerboard_shows_through_partial_alpha_and_not_under_opaque_pixels() {
    // Without fitting, squares are a sixteenth of the longer side: 1 px here.
    let image = rgba(16, 2, |x, _| if x < 8 { RED } else { [0, 0, 255, 128] });
    let art = ImageArt::new(image)
        .mode(ImageMode::Blocks)
        .width(16)
        .background_mode(ImageBackground::Checkerboard);
    let row = &cells(&render(&art))[0];
    assert!(row[..8]
        .iter()
        .all(|cell| cell.1 == Some(Color::from_rgb(255, 0, 0))));
    // Half-transparent blue over the light square, then the dark one.
    assert_eq!(row[8].1, Some(Color::from_rgb(76, 76, 204)));
    assert_eq!(row[9].1, Some(Color::from_rgb(51, 51, 179)));
}

#[test]
fn backgrounds_combine_with_reduced_palettes() {
    let image = rgba(4, 2, |x, _| if x < 2 { CLEAR } else { RED });
    let art = ImageArt::new(image)
        .mode(ImageMode::Blocks)
        .width(4)
        .color_mode(ImageColorMode::Ansi16)
        .background_mode(ImageBackground::TerminalDefault);
    let row = &cells(&render(&art))[0];
    assert_eq!(row[0], (" ".into(), None, None));
    // Pure red is nearest ANSI 1 (170, 0, 0) in encoded RGB.
    assert_eq!(row[3].1, Some(Color::from_ansi(1)));
}

#[cfg(feature = "sixel")]
#[test]
fn sixel_keeps_transparent_pixels_transparent() {
    let image = rgba(16, 16, |x, _| if x < 8 { CLEAR } else { RED });
    let art = ImageArt::new(image)
        .mode(ImageMode::Sixel)
        .width(2)
        .background_mode(ImageBackground::TerminalDefault);
    let segments = render(&art);
    let decoded = icy_sixel::SixelImage::decode(segments[0].text.as_bytes()).unwrap();
    let alpha = |x: usize| decoded.pixels[x * 4 + 3];
    assert_eq!(alpha(0), 0);
    assert_eq!(alpha(decoded.width - 1), 255);
}

/// Left column red at alpha 130 (just over half), right column opaque red.
fn half_opaque_red() -> DynamicImage {
    rgba(2, 2, |x, _| if x == 0 { [255, 0, 0, 130] } else { RED })
}

/// Every colour a rendered row paints, foreground and background.
fn painted(row: &[Cell]) -> Vec<Color> {
    row.iter()
        .flat_map(|(_, fg, bg)| [fg.clone(), bg.clone()])
        .flatten()
        .collect()
}

#[test]
fn terminal_default_blocks_keep_half_opaque_colour_in_reduced_palettes() {
    let art = ImageArt::new(half_opaque_red())
        .mode(ImageMode::Blocks)
        .width(2)
        .color_mode(ImageColorMode::Ansi256)
        .background_mode(ImageBackground::TerminalDefault);
    let row = &cells(&render(&art))[0];
    // Pure red is ANSI 196; a premultiplied alpha-130 red would snap to 88.
    let colours = painted(row);
    assert!(!colours.is_empty());
    assert!(
        colours.iter().all(|c| *c == Color::from_ansi(196)),
        "{row:?}"
    );
}

#[test]
fn terminal_default_quadrants_keep_a_uniform_cell_whole() {
    let art = ImageArt::new(half_opaque_red())
        .mode(ImageMode::Quadrants)
        .width(1)
        .color_mode(ImageColorMode::Ansi256)
        .background_mode(ImageBackground::TerminalDefault);
    let row = &cells(&render(&art))[0];
    let colours = painted(row);
    assert!(!colours.is_empty());
    assert!(
        colours.iter().all(|c| *c == Color::from_ansi(196)),
        "{row:?}"
    );
}

#[test]
fn terminal_default_ascii_keeps_half_opaque_pixels_visible() {
    let art = ImageArt::new(half_opaque_red())
        .mode(ImageMode::Ascii)
        .width(2)
        .height(1)
        .color(true)
        .background_mode(ImageBackground::TerminalDefault);
    let row = &cells(&render(&art))[0];
    // Both pixels are the same colour once opacity is kept: same glyph too.
    assert_ne!(row[0].0, " ", "{row:?}");
    assert_eq!(row[0].0, row[1].0, "{row:?}");
    assert_eq!(row[0].1, Some(Color::from_rgb(255, 0, 0)));
}

#[test]
fn terminal_default_braille_uses_unpremultiplied_luma() {
    // Gray 150 at alpha 200: over half opacity and brighter than 128, so
    // every dot is set. Premultiplied it would be 117 and show nothing.
    let image = rgba(2, 4, |_, _| [150, 150, 150, 200]);
    let make = || {
        ImageArt::new(image.clone())
            .mode(ImageMode::Braille)
            .width(1)
    };
    let kept = make().background_mode(ImageBackground::TerminalDefault);
    assert_eq!(cells(&render(&kept))[0][0].0, "\u{28FF}");
    // Pixels under half opacity still show no dot, however bright.
    let faint = ImageArt::new(rgba(2, 4, |_, _| [255, 255, 255, 127]))
        .mode(ImageMode::Braille)
        .width(1)
        .background_mode(ImageBackground::TerminalDefault);
    assert_eq!(cells(&render(&faint))[0][0].0, "\u{2800}");
    // The default still composites onto black: unchanged.
    assert_eq!(cells(&render(&make()))[0][0].0, "\u{2800}");
}

#[test]
fn default_background_still_composites_partial_alpha_onto_black() {
    // Without TerminalDefault nothing changes: alpha 130 red is darkened.
    let art = ImageArt::new(half_opaque_red())
        .mode(ImageMode::Blocks)
        .width(2)
        .color_mode(ImageColorMode::Ansi256);
    let row = &cells(&render(&art))[0];
    assert_eq!(row[0].1, Some(Color::from_ansi(88)), "{row:?}");
    assert_eq!(row[1].1, Some(Color::from_ansi(196)), "{row:?}");
}

#[cfg(feature = "sixel")]
#[test]
fn terminal_default_sixel_keeps_half_opaque_colour_in_reduced_palettes() {
    let image = rgba(16, 16, |x, _| if x < 8 { [255, 0, 0, 130] } else { RED });
    let art = ImageArt::new(image)
        .mode(ImageMode::Sixel)
        .width(2)
        .color_mode(ImageColorMode::Ansi256)
        .background_mode(ImageBackground::TerminalDefault);
    let segments = render(&art);
    let decoded = icy_sixel::SixelImage::decode(segments[0].text.as_bytes()).unwrap();
    let pixel = |x: usize| decoded.pixels[x * 4..x * 4 + 4].to_vec();
    assert_eq!(pixel(0), vec![255, 0, 0, 255]);
    assert_eq!(pixel(decoded.width - 1), vec![255, 0, 0, 255]);
}
