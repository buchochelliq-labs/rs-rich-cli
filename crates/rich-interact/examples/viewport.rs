//! Page through a Markdown file in a viewport:
//! `cargo run -p rs-rich-interact --example viewport -- README.md`.
//!
//! Arrows, PageUp/PageDown, Space, Home/End and the mouse wheel scroll;
//! Enter or `q` leaves. Piped or under CI, it prints the document instead.

use rich_interact::policy::Policy;
use rich_interact::{run, Context, Outcome, RunOptions, SessionOptions, Viewport};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "README.md".into());
    let source = std::fs::read_to_string(&path)?;
    let markdown = rich::markdown::Markdown::new(&source);
    let (columns, rows) = crossterm::terminal::size().unwrap_or((80, 24));
    let console = rich::Console::builder()
        .width(columns as usize)
        .height(rows as usize)
        .force_terminal(true)
        .build();
    let context = Context {
        console: &console,
        width: columns as usize,
        height: rows as usize,
    };
    // Without a terminal there is nothing to page: print it.
    if Policy::default().detect().is_err() {
        rich::Console::builder().build().print(&markdown);
        return Ok(());
    }
    let viewport = Viewport::new(context.lines(&markdown));
    let options = RunOptions {
        session: SessionOptions {
            alternate_screen: true,
            mouse: true,
            ..SessionOptions::default()
        },
        ..RunOptions::default()
    };
    match run(viewport, &options)? {
        Outcome::Done(line) => eprintln!("left at line {}", line + 1),
        Outcome::Cancelled | Outcome::Interrupted => {}
    }
    Ok(())
}
