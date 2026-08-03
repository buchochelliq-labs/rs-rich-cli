//! Play an animated GIF in the terminal as ANSI art.
//!
//! ```text
//! cargo run -p rich-art --features gif --example gif -- spin.gif
//! ```
//!
//! Requires the `gif` feature.

use rich::color::ColorSystem;
use rich::Console;
use rich_art::gif::{AnimatedArt, Repeat};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: gif <file.gif> [loops]");
        std::process::exit(2);
    };
    let loops: usize = std::env::args()
        .nth(2)
        .and_then(|n| n.parse().ok())
        .unwrap_or(1);

    let art = AnimatedArt::from_path(&path)?
        .color(true)
        .max_fps(20.0)
        .repeat(Repeat::Times(loops));

    eprintln!(
        "{} frames, {:.1}s per loop",
        art.frame_count(),
        art.duration().as_secs_f64()
    );

    let console = Console::builder()
        .force_terminal(true)
        .color_system(Some(ColorSystem::Truecolor))
        .no_color(false)
        .build();

    art.play_stdout(console)?;
    Ok(())
}
