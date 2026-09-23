//! Guide: Images — run: cargo run -p rs-rich-art --features image --example guide_images [-- --svg docs/media/guide] [--sixel]
//!
//! Every snippet on the `docs/guide/art/images.md` page comes from this file,
//! and every screenshot on it is this file's own SVG export. The test picture
//! is drawn in code, so nothing here needs an image file.

use std::path::PathBuf;
use std::sync::Arc;

use rich::{Cell, ColorSystem, Console, Rule, Table, Text};
use rich_art::image::{DynamicImage, Rgba, RgbaImage};
use rich_art::{
    Dither, ImageAnchor, ImageArt, ImageArtError, ImageColorMode, ImageFit, ImageMode,
    ImageTransforms, RenderCapabilities, Rotation,
};

// --8<-- [start:test-image]
/// A 96×64 test card: a diagonal gradient, a yellow disc, a red square and a
/// white bar, so every renderer has edges, flat areas and smooth ramps.
fn test_card() -> DynamicImage {
    let (w, h) = (96u32, 64u32);
    DynamicImage::ImageRgba8(RgbaImage::from_fn(w, h, |x, y| {
        let (fx, fy) = (x as f32 / (w - 1) as f32, y as f32 / (h - 1) as f32);
        let mut rgb = [
            (30.0 + 200.0 * fx) as u8,
            (40.0 + 150.0 * fy) as u8,
            (210.0 - 150.0 * fx) as u8,
        ];
        let (dx, dy) = (x as f32 - 30.0, y as f32 - 32.0);
        if dx * dx + dy * dy <= 18.0 * 18.0 {
            rgb = [250, 205, 40]; // the disc
        }
        if (60..84).contains(&x) && (14..38).contains(&y) {
            rgb = [225, 55, 85]; // the square
        }
        if (56..90).contains(&x) && (46..52).contains(&y) {
            rgb = [245, 245, 245]; // the bar
        }
        Rgba([rgb[0], rgb[1], rgb[2], 255])
    }))
}
// --8<-- [end:test-image]

/// A disc on a fully transparent canvas, for the background examples.
fn logo() -> DynamicImage {
    DynamicImage::ImageRgba8(RgbaImage::from_fn(64, 32, |x, y| {
        let (dx, dy) = (x as f32 - 32.0, (y as f32 - 16.0) * 2.0);
        if dx * dx + dy * dy <= 26.0 * 26.0 {
            Rgba([90, 200, 250, 255])
        } else {
            Rgba([0, 0, 0, 0])
        }
    }))
}

/// A labelled grid of renderables, `per_row` to a row, each `width` columns.
fn grid(items: Vec<(&str, ImageArt)>, per_row: usize, width: usize) -> Table {
    let mut table = Table::grid().padding(0, 2, 0, 0);
    for _ in 0..per_row {
        table.add_column("");
        table.column_width(width);
    }
    let mut items = items.into_iter().peekable();
    while items.peek().is_some() {
        let row: Vec<(&str, ImageArt)> = items.by_ref().take(per_row).collect();
        let labels = row
            .iter()
            .map(|(label, _)| Cell::from(Text::styled(*label, "bold")))
            .collect();
        table.add_row_cells(labels);
        let arts = row
            .into_iter()
            .map(|(_, art)| Cell::Renderable(Arc::new(art)))
            .collect();
        table.add_row_cells(arts);
    }
    table
}

fn quickstart(console: &Console) {
    // --8<-- [start:quickstart]
    let art = ImageArt::new(test_card()) // or ImageArt::from_path("photo.png")?
        .mode(ImageMode::Blocks)
        .width(48);
    console.print(&art);
    // --8<-- [end:quickstart]
}

fn modes(console: &Console) {
    // --8<-- [start:modes]
    let card = test_card();
    let art = |mode| ImageArt::new(card.clone()).mode(mode).width(30);
    console.print(&grid(
        vec![
            ("Ascii", art(ImageMode::Ascii)),
            ("Ascii + .color(true)", art(ImageMode::Ascii).color(true)),
            ("Blocks", art(ImageMode::Blocks)),
            ("Quadrants", art(ImageMode::Quadrants)),
            ("Braille", art(ImageMode::Braille)),
        ],
        2,
        30,
    ));
    // --8<-- [end:modes]
}

fn fit(console: &Console) {
    // --8<-- [start:fit]
    let card = test_card();
    // A 20×10-cell box is taller (in pixels) than the 3:2 card, so each
    // policy has to do something different to fill it.
    let boxed = |fit| {
        ImageArt::new(card.clone())
            .mode(ImageMode::Blocks)
            .width(20)
            .height(10)
            .fit(fit)
            .background([40, 40, 40])
    };
    console.print(&grid(
        vec![
            ("Contain", boxed(ImageFit::Contain)),
            ("Cover", boxed(ImageFit::Cover)),
            (
                "Cover, TopLeft",
                boxed(ImageFit::Cover).anchor(ImageAnchor::TopLeft),
            ),
            ("Stretch", boxed(ImageFit::Stretch)),
        ],
        4,
        20,
    ));
    // --8<-- [end:fit]
}

fn background(console: &Console) {
    // --8<-- [start:background]
    let art = || ImageArt::new(logo()).mode(ImageMode::Blocks).width(28);
    console.print(&grid(
        vec![
            ("No background", art()),
            (
                "background([120, 40, 160])",
                art().background([120, 40, 160]),
            ),
        ],
        2,
        30,
    ));
    // --8<-- [end:background]
}

fn color_modes(console: &Console) {
    // --8<-- [start:color-modes]
    let card = test_card();
    let art = |mode| {
        ImageArt::new(card.clone())
            .mode(ImageMode::Blocks)
            .width(30)
            .color_mode(mode)
    };
    console.print(&grid(
        vec![
            ("TrueColor (default)", art(ImageColorMode::TrueColor)),
            ("Ansi256", art(ImageColorMode::Ansi256)),
            ("Ansi16", art(ImageColorMode::Ansi16)),
            ("Grayscale", art(ImageColorMode::Grayscale)),
        ],
        2,
        30,
    ));
    // --8<-- [end:color-modes]
}

fn dither(console: &Console) {
    // --8<-- [start:dither]
    let card = test_card();
    let art = |dither| {
        ImageArt::new(card.clone())
            .mode(ImageMode::Blocks)
            .width(30)
            .color_mode(ImageColorMode::Ansi16)
            .dither(dither)
    };
    console.print(&grid(
        vec![
            ("Ansi16, Dither::None", art(Dither::None)),
            ("Ansi16, FloydSteinberg", art(Dither::FloydSteinberg)),
            ("Ansi16, Bayer4x4", art(Dither::Bayer4x4)),
        ],
        3,
        30,
    ));
    // --8<-- [end:dither]
}

fn tone(console: &Console) {
    // --8<-- [start:tone]
    let card = test_card();
    let art = |transforms| {
        ImageArt::new(card.clone())
            .mode(ImageMode::Blocks)
            .width(22)
            .transforms(transforms)
    };
    let tone = |brightness, contrast, gamma| ImageTransforms {
        brightness,
        contrast,
        gamma,
        ..ImageTransforms::default()
    };
    console.print(&grid(
        vec![
            ("Unchanged", art(ImageTransforms::default())),
            ("brightness 0.6", art(tone(0.6, 1.0, 1.0))),
            ("contrast 1.8", art(tone(1.0, 1.8, 1.0))),
            ("gamma 2.2", art(tone(1.0, 1.0, 2.2))),
        ],
        4,
        22,
    ));
    // --8<-- [end:tone]
}

fn transforms(console: &Console) {
    // --8<-- [start:transforms]
    let card = test_card();
    let art = |transforms| {
        ImageArt::new(card.clone())
            .mode(ImageMode::Blocks)
            .width(22)
            .transforms(transforms)
    };
    console.print(&grid(
        vec![
            (
                "Clockwise90",
                art(ImageTransforms {
                    rotation: Rotation::Clockwise90,
                    ..ImageTransforms::default()
                }),
            ),
            (
                "flip_horizontal",
                art(ImageTransforms {
                    flip_horizontal: true,
                    ..ImageTransforms::default()
                }),
            ),
            (
                "flip_vertical",
                art(ImageTransforms {
                    flip_vertical: true,
                    ..ImageTransforms::default()
                }),
            ),
            (
                "grayscale",
                art(ImageTransforms {
                    grayscale: true,
                    ..ImageTransforms::default()
                }),
            ),
        ],
        4,
        22,
    ));
    // --8<-- [end:transforms]
}

fn programmatic(console: &Console) {
    // --8<-- [start:strict]
    // `console.print` cannot fail, so an impossible request quietly degrades
    // to ASCII. `render` is the strict entry point that says why.
    let art = ImageArt::new(test_card())
        .mode(ImageMode::Braille)
        .color_mode(ImageColorMode::Ansi256); // Braille has no colour
    match art.render(console, &console.options()) {
        Ok(segments) => console.print_str(&format!("{} segments", segments.len())),
        Err(ImageArtError::UnsupportedColorOptions) => {
            console.print_str("[red]rejected:[/] Braille cannot be quantized")
        }
        Err(other) => console.print_str(&format!("[red]rejected:[/] {other}")),
    }
    // --8<-- [end:strict]

    // --8<-- [start:resolve]
    // What would Auto pick? Explicit modes are returned unchanged.
    let auto = ImageArt::new(test_card());
    for (label, caps) in [
        ("no colour", RenderCapabilities::default()),
        (
            "colour",
            RenderCapabilities {
                color: true,
                sixel_supported: false,
            },
        ),
        (
            "colour + Sixel",
            RenderCapabilities {
                color: true,
                sixel_supported: true,
            },
        ),
        ("this console", RenderCapabilities::from_console(console)),
    ] {
        let mode = auto.resolve_mode(caps);
        console.print_str(&format!("Auto with {label:<14} → {mode:?}"));
    }
    // --8<-- [end:resolve]
}

/// Sixel writes raw terminal graphics, so it is never exported to SVG; run the
/// example with `--sixel` in a Sixel-capable terminal to see it.
fn sixel(console: &Console) {
    // --8<-- [start:sixel]
    // Sixel is opt-in: it needs the `sixel` feature and a terminal that
    // draws it. `render` reports why when either is missing.
    let art = ImageArt::new(test_card()).mode(ImageMode::Sixel).width(40);
    match art.render(console, &console.options()) {
        Ok(_) => console.print(&art),
        Err(ImageArtError::FeatureNotEnabled { feature, .. }) => {
            console.print_str(&format!("rebuild rs-rich-art with the {feature:?} feature"))
        }
        Err(ImageArtError::NonTerminalDestination) => {
            console.print(&art.mode(ImageMode::Blocks)) // e.g. output is redirected
        }
        Err(other) => console.print_str(&format!("no Sixel: {other}")),
    }
    // --8<-- [end:sixel]
    #[cfg(feature = "sixel")]
    sixel_direct(console);
}

#[cfg(feature = "sixel")]
fn sixel_direct(console: &Console) {
    // --8<-- [start:sixel-direct]
    use rich_art::sixel::{is_probably_supported, SixelArt};
    // A guess from environment variables (RICH_SIXEL=0/1 overrides it).
    if console.is_terminal() && is_probably_supported() {
        console.print(&SixelArt::new(test_card()).width(40).max_colors(64));
    }
    // --8<-- [end:sixel-direct]
}

type Shot = (&'static str, &'static str, usize, fn(&Console));

const SHOTS: &[Shot] = &[
    ("quickstart", "ImageArt, Blocks", 52, quickstart),
    ("modes", "Image modes", 66, modes),
    ("fit", "Fit and anchor", 90, fit),
    ("background", "Transparent background", 66, background),
    ("color-modes", "Colour modes", 66, color_modes),
    ("dither", "Dithering", 98, dither),
    ("tone", "Tone adjustments", 98, tone),
    ("transforms", "Rotation, flips, grayscale", 98, transforms),
    ("programmatic", "Strict rendering", 60, programmatic),
];

fn main() {
    let svg_dir = svg_dir();
    let console = Console::builder().build();
    for (name, title, width, draw) in SHOTS {
        console.print(&Rule::new(*title));
        draw(&console);
        if let Some(dir) = &svg_dir {
            export(dir, name, title, *width, *draw);
        }
    }
    if std::env::args().any(|a| a == "--sixel") {
        console.print(&Rule::new("Sixel"));
        sixel(&console);
    }
}

/// `--svg DIR` from the command line, if given.
fn svg_dir() -> Option<PathBuf> {
    let args: Vec<String> = std::env::args().collect();
    let index = args.iter().position(|a| a == "--svg")?;
    Some(PathBuf::from(
        args.get(index + 1).expect("--svg needs a DIR"),
    ))
}

/// Render one shot on a fixed-width truecolor console and write it as
/// `DIR/guide_images-<name>.svg`.
fn export(dir: &std::path::Path, name: &str, title: &str, width: usize, draw: fn(&Console)) {
    let stem = format!("guide_images-{name}");
    let console = Console::builder()
        .width(width)
        .force_terminal(true)
        .color_system(Some(ColorSystem::Truecolor))
        .no_color(false)
        .build();
    let svg = console.export_svg(title, &stem, draw);
    std::fs::create_dir_all(dir).expect("create the SVG directory");
    let path = dir.join(format!("{stem}.svg"));
    std::fs::write(&path, svg).expect("write the SVG");
    eprintln!("wrote {}", path.display());
}
