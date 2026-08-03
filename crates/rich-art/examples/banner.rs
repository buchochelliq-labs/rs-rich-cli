//! Print a FIGlet banner through a `rich` console.
//!
//! ```text
//! cargo run -p rich-art --example banner -- "Hello"
//! ```

use rich::color::ColorSystem;
use rich::style::Style;
use rich::Console;
use rich_art::{Figlet, Justify};

fn main() {
    let text = std::env::args().nth(1).unwrap_or_else(|| "rich art".into());

    let console = Console::builder()
        .force_terminal(true)
        .color_system(Some(ColorSystem::Truecolor))
        .no_color(false)
        .build();

    console.print(&Figlet::new(&text).style(Style::parse("bold cyan").unwrap()));
    console.print(&Figlet::new("centered").justify(Justify::Center));
}
