//! Guide: Animated GIFs — run: cargo run -p rs-rich-art --features gif --example guide_gifs [-- --svg docs/media/guide] [--play]
//!
//! Every snippet on the `docs/guide/art/gifs.md` page comes from this file,
//! and every screenshot on it is this file's own SVG export. The GIF is drawn
//! and encoded in code, so nothing here needs an asset. `--play` also plays
//! the animation and a two-animation stage in the terminal.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use rich::{Cell, ColorSystem, Console, Rule, Table, Text};
use rich_art::image::codecs::gif::GifEncoder;
use rich_art::image::{Delay, Frame, Rgba, RgbaImage};
use rich_art::{AnimatedArt, Repeat, Stage, Until};

// --8<-- [start:make-gif]
/// A six-frame GIF of a ball rolling across a floor, 100 ms per frame.
fn rolling_ball() -> Vec<u8> {
    let mut bytes = Vec::new();
    {
        let mut encoder = GifEncoder::new(&mut bytes);
        for step in 0..6u32 {
            let cx = 10.0 + step as f32 * 14.0;
            let image = RgbaImage::from_fn(96, 40, |x, y| {
                let (dx, dy) = (x as f32 - cx, y as f32 - 18.0);
                if dx * dx + dy * dy <= 81.0 {
                    Rgba([250, 200, 40, 255]) // the ball
                } else if y >= 30 {
                    Rgba([60, 140, 90, 255]) // the floor
                } else {
                    Rgba([20, 24, 60, 255]) // the sky
                }
            });
            let delay = Delay::from_numer_denom_ms(100, 1);
            encoder
                .encode_frame(Frame::from_parts(image, 0, 0, delay))
                .expect("encode a frame");
        }
    }
    bytes
}
// --8<-- [end:make-gif]

fn metadata(console: &Console) {
    // --8<-- [start:load]
    let art = AnimatedArt::from_bytes(&rolling_ball()) // or AnimatedArt::from_path("spin.gif")?
        .expect("a valid GIF")
        .width(32);
    console.print_str(&format!(
        "{} frames, {:?} per pass, first frame shown for {:?}",
        art.frame_count(),
        art.duration(),
        art.frame_delay(0).unwrap_or_default(),
    ));
    // An AnimatedArt printed like any other renderable shows its first frame.
    console.print(&art);
    // --8<-- [end:load]
}

/// Frames 0, 2 and 4 side by side, rendered as `render_frame` returns them.
fn frames(console: &Console, art: &AnimatedArt) {
    let mut table = Table::grid().padding(0, 2, 0, 0);
    for _ in 0..3 {
        table.add_column("");
        table.column_width(28);
    }
    let picks = [0, 2, 4];
    table.add_row_cells(
        picks
            .iter()
            .map(|i| Cell::from(Text::styled(format!("frame {i}"), "bold")))
            .collect(),
    );
    table.add_row_cells(
        picks
            .iter()
            .map(|i| Cell::Renderable(Arc::new(art.render_frame(*i).expect("frame exists"))))
            .collect(),
    );
    console.print(&table);
}

fn ascii_frames(console: &Console) {
    // --8<-- [start:ascii]
    let art = AnimatedArt::from_bytes(&rolling_ball())
        .expect("a valid GIF")
        .width(28);
    // --8<-- [end:ascii]
    frames(console, &art);
}

fn color_frames(console: &Console) {
    // --8<-- [start:color]
    // Colour each ASCII cell with its pixel.
    let ansi = AnimatedArt::from_bytes(&rolling_ball())
        .expect("a valid GIF")
        .width(28)
        .color(true);
    // --8<-- [end:color]
    frames(console, &ansi);
}

fn block_frames(console: &Console) {
    // --8<-- [start:blocks]
    // Half-block pixels on a colour terminal; ASCII everywhere else.
    let blocks = AnimatedArt::from_bytes(&rolling_ball())
        .expect("a valid GIF")
        .width(28)
        .color(true)
        .blocks(true);
    // --8<-- [end:blocks]
    frames(console, &blocks);
}

fn play() -> std::io::Result<()> {
    // --8<-- [start:play]
    let art = AnimatedArt::from_bytes(&rolling_ball())
        .expect("a valid GIF")
        .width(40)
        .color(true)
        .blocks(true)
        .max_fps(20.0) // colour frames are byte-heavy; cap the rate
        .repeat(Repeat::Times(3));
    // Draws in place through rich's Live display; returns when done.
    // Redirected output gets the first frame once instead.
    art.play_stdout(Console::builder().build())?;
    // --8<-- [end:play]

    // --8<-- [start:stage]
    let fast = AnimatedArt::from_bytes(&rolling_ball())
        .expect("a valid GIF")
        .width(30)
        .color(true)
        .repeat(Repeat::Forever);
    let slow = fast.clone().max_fps(4.0).invert(true);
    Stage::new()
        .with(fast)
        .with(slow) // each animation keeps its own clock
        .gap(4)
        .until(Until::Elapsed(Duration::from_secs(3)))
        .play_stdout(Console::builder().build())?;
    // --8<-- [end:stage]
    Ok(())
}

type Shot = (&'static str, &'static str, usize, fn(&Console));

const SHOTS: &[Shot] = &[
    ("load", "AnimatedArt", 64, metadata),
    ("ascii", "ASCII frames", 94, ascii_frames),
    ("color", "Coloured ASCII frames", 94, color_frames),
    ("blocks", "Half-block frames", 94, block_frames),
];

fn main() -> std::io::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let svg_dir = svg_dir(&args);
    let console = Console::builder().build();
    for (name, title, width, draw) in SHOTS {
        console.print(&Rule::new(*title));
        draw(&console);
        if let Some(dir) = &svg_dir {
            export(dir, name, title, *width, *draw);
        }
    }
    if args.iter().any(|a| a == "--play") {
        play()?;
    }
    Ok(())
}

/// `--svg DIR` from the command line, if given.
fn svg_dir(args: &[String]) -> Option<PathBuf> {
    let index = args.iter().position(|a| a == "--svg")?;
    Some(PathBuf::from(
        args.get(index + 1).expect("--svg needs a DIR"),
    ))
}

/// Render one shot on a fixed-width truecolor console and write it as
/// `DIR/guide_gifs-<name>.svg`.
fn export(dir: &std::path::Path, name: &str, title: &str, width: usize, draw: fn(&Console)) {
    let stem = format!("guide_gifs-{name}");
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
