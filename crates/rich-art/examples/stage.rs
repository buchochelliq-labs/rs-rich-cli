//! Several GIFs, embedded in the binary as assets, animating at once.
//!
//! ```text
//! cargo run -p rich-art --features gif --example stage
//! ```
//!
//! The GIFs are compiled in with `include_bytes!`, so the binary is
//! self-contained — nothing is read from disk at runtime. Each animation keeps
//! its own frame clock: the cat runs at 80ms/frame and the ball at 45ms, and
//! the stage redraws only when one of them actually changes.

use std::time::Duration;

use rich::color::ColorSystem;
use rich::Console;
use rich_art::{AnimatedArt, Stage, Until};

/// GIFs embedded as assets, straight from the source tree.
static CAT: &[u8] = include_bytes!("assets/cat.gif");
static BALL: &[u8] = include_bytes!("assets/ball.gif");

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let seconds: u64 = std::env::args()
        .nth(1)
        .and_then(|n| n.parse().ok())
        .unwrap_or(5);

    let stage = Stage::new()
        .with(AnimatedArt::from_bytes(CAT)?.width(34).color(true))
        .with(AnimatedArt::from_bytes(BALL)?.width(24).color(true))
        // The same cat again, in monochrome, to show assets being reused.
        .with(AnimatedArt::from_bytes(CAT)?.width(28).invert(true))
        .gap(3)
        .until(Until::Elapsed(Duration::from_secs(seconds)));

    eprintln!("playing {} animations for {seconds}s…", stage.len());

    let console = Console::builder()
        .force_terminal(true)
        .color_system(Some(ColorSystem::Truecolor))
        .no_color(false)
        .build();

    stage.play_stdout(console)?;
    Ok(())
}
