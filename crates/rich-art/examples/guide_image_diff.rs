//! Guide: Image diff — run: cargo run -p rs-rich-art --features image --example guide_image_diff [-- --svg docs/media/guide]
//!
//! Every snippet on the `docs/guide/art/image-diff.md` page comes from this
//! file, and every screenshot on it is this file's own SVG export. Both images
//! are drawn in code, so nothing here needs an image file.

use std::path::PathBuf;
use std::sync::Arc;

use rich::{Cell, ColorSystem, Console, Justify, Rule, Table, Text};
use rich_art::image::{DynamicImage, Rgb, RgbImage};
use rich_art::imagediff::{diff, DiffError, DiffSettings};
use rich_art::{ImageArt, ImageMode};

// --8<-- [start:pair]
/// A 160×100 "screenshot": a dark gradient, a disc, and optionally a badge in
/// the given colour. Adding or recolouring the badge is the change to find.
fn screenshot(badge: Option<[u8; 3]>) -> DynamicImage {
    DynamicImage::ImageRgb8(RgbImage::from_fn(160, 100, |x, y| {
        let (dx, dy) = (x as f32 - 45.0, y as f32 - 50.0);
        if dx * dx + dy * dy <= 28.0 * 28.0 {
            return Rgb([250, 205, 40]); // the disc: identical in both
        }
        if let Some(colour) = badge {
            if (100..140).contains(&x) && (30..70).contains(&y) {
                return Rgb(colour); // the badge
            }
        }
        Rgb([
            (20 + x * 60 / 159) as u8,
            (30 + y * 50 / 99) as u8,
            (90 - x * 40 / 159) as u8,
        ])
    }))
}
// --8<-- [end:pair]

fn report(console: &Console) {
    // --8<-- [start:diff]
    let before = screenshot(None);
    let after = screenshot(Some([245, 245, 245]));
    let report = diff(&before, &after, &DiffSettings::default()).expect("same size");

    console.print_str(&format!(
        "{:.1}% changed perceptibly (a plain pixel diff would say {:.1}%)",
        report.changed_fraction * 100.0,
        report.naive_changed_fraction * 100.0,
    ));
    let mut table = Table::new();
    table.add_column("Region");
    table.add_column_justify("Share", Justify::Right);
    table.add_column_justify("Mean ΔE", Justify::Right);
    for region in &report.regions {
        table.add_row(&[
            &format!(
                "{}×{} at ({}, {})",
                region.width, region.height, region.x, region.y
            ),
            &format!("{:.0}%", region.share_of_change * 100.0),
            &format!("{:.1}", region.mean_delta_e),
        ]);
    }
    console.print(&table);
    // --8<-- [end:diff]
}

fn pictures(console: &Console) {
    // --8<-- [start:pictures]
    let (before, after) = (screenshot(None), screenshot(Some([245, 245, 245])));
    let report = diff(&before, &after, &DiffSettings::default()).expect("same size");
    let view = |image: DynamicImage| ImageArt::new(image).mode(ImageMode::Blocks).width(30);

    let mut grid = Table::grid().padding(0, 2, 0, 0);
    for _ in 0..3 {
        grid.add_column("");
        grid.column_width(30);
    }
    grid.add_row_cells(
        ["after", "report.heatmap()", "report.highlight(&after)"]
            .into_iter()
            .map(|label| Cell::from(Text::styled(label, "bold")))
            .collect(),
    );
    grid.add_row_cells(vec![
        Cell::Renderable(Arc::new(view(after.clone()))),
        Cell::Renderable(Arc::new(view(report.heatmap()))),
        Cell::Renderable(Arc::new(view(report.highlight(&after)))),
    ]);
    console.print(&grid);
    // --8<-- [end:pictures]
}

fn gate(console: &Console) {
    // --8<-- [start:gate]
    // A CI gate: fail when more than 2% of the canvas changed perceptibly.
    let before = screenshot(None);
    let after = screenshot(Some([245, 245, 245]));
    let report = diff(&before, &after, &DiffSettings::default()).expect("same size");
    let limit = 2.0;
    let changed = report.changed_fraction * 100.0;
    if changed > limit {
        console.print_str(&format!(
            "[red]FAIL[/] {changed:.1}% changed, limit {limit:.1}%"
        ));
    } else {
        console.print_str(&format!("[green]OK[/] {changed:.1}% changed"));
    }
    // --8<-- [end:gate]

    // --8<-- [start:settings]
    // Screenshots are far cleaner than regenerated artwork, so a lower ΔE
    // threshold and a smaller minimum region suit them.
    let strict = DiffSettings {
        threshold: 10.0,
        min_region: 50,
        top: 5,
        ..DiffSettings::default()
    };
    // A subtle recolour: light grey to white.
    let (grey, white) = (
        screenshot(Some([200, 200, 200])),
        screenshot(Some([245, 245, 245])),
    );
    for (label, settings) in [("default", DiffSettings::default()), ("strict", strict)] {
        let report = diff(&grey, &white, &settings).expect("same size");
        console.print_str(&format!(
            "{label:>7}: {:.1}% changed, {} region(s)",
            report.changed_fraction * 100.0,
            report.regions.len()
        ));
    }
    // --8<-- [end:settings]

    // --8<-- [start:mismatch]
    let small = DynamicImage::ImageRgb8(RgbImage::new(80, 50));
    match diff(&screenshot(None), &small, &DiffSettings::default()) {
        Err(DiffError::SizeMismatch { before, after }) => {
            console.print_str(&format!("cannot compare {before:?} with {after:?}"))
        }
        Ok(_) => unreachable!("sizes differ"),
    }
    // --8<-- [end:mismatch]
}

type Shot = (&'static str, &'static str, usize, fn(&Console));

const SHOTS: &[Shot] = &[
    ("report", "DiffReport", 72, report),
    ("pictures", "Heatmap and highlight", 96, pictures),
    ("gate", "Gates and settings", 72, gate),
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
/// `DIR/guide_image_diff-<name>.svg`.
fn export(dir: &std::path::Path, name: &str, title: &str, width: usize, draw: fn(&Console)) {
    let stem = format!("guide_image_diff-{name}");
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
