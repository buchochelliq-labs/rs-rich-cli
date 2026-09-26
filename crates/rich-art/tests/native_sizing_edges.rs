#![cfg(feature = "image")]
//! `ImageFit::Native` sizing at its edges: the computed grid matches the
//! rendered one for extreme shapes, widths and caps (0.0.12 release test).
use rich::segment::Segment;
use rich::{color::ColorSystem, Console};
use rich_art::{
    image::{DynamicImage, Rgb, RgbImage},
    ImageArt, ImageFit, ImageMode,
};

fn console(width: usize) -> Console {
    Console::builder()
        .force_terminal(true)
        .color_system(Some(ColorSystem::Truecolor))
        .no_color(false)
        .width(width)
        .build()
}

fn solid(w: u32, h: u32) -> DynamicImage {
    DynamicImage::ImageRgb8(RgbImage::from_pixel(w, h, Rgb([200, 30, 30])))
}

fn grid(art: &ImageArt, width: usize) -> (usize, usize) {
    let c = console(width);
    let segments = art.render(&c, &c.options()).expect("renders");
    let lines: Vec<String> = Segment::split_lines(&segments)
        .into_iter()
        .map(|l| {
            l.iter()
                .filter(|s| !s.control)
                .map(|s| s.text.as_str())
                .collect()
        })
        .filter(|l: &String| !l.is_empty())
        .collect();
    let columns = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0);
    (columns, lines.len())
}

const MODES: [ImageMode; 4] = [
    ImageMode::Ascii,
    ImageMode::Blocks,
    ImageMode::Quadrants,
    ImageMode::Braille,
];

/// The computed native grid is what every text mode renders, across extreme
/// aspect ratios and caps (the cells a caller reserves must match).
#[test]
fn native_grid_matches_rendered_grid_for_extreme_shapes() {
    let sizes = [
        (1, 1),
        (1, 999),
        (999, 1),
        (3, 1000),
        (1000, 3),
        (2000, 7),
        (5, 5),
    ];
    let mut failures = Vec::new();
    for (w, h) in sizes {
        for mode in MODES {
            for (available, max_h) in [(1usize, None), (80, None), (80, Some(3usize)), (7, Some(1))]
            {
                let mut art = ImageArt::new(solid(w, h)).mode(mode).fit(ImageFit::Native);
                if let Some(rows) = max_h {
                    art = art.max_height(rows);
                }
                let expected = art.native_grid(mode, available);
                let got = grid(&art, available);
                if got != expected {
                    failures.push(format!(
                        "{w}x{h} {mode:?} avail={available} max_h={max_h:?}: grid {expected:?} rendered {got:?}"
                    ));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
