//! An intuiTUIve app inside a ratatui program: ratatui owns the terminal
//! and the loop, and draws the left half; the app runs on a [`Driver`] in
//! the right half, and its frame is copied into ratatui's buffer.
//!
//! The same [`Driver`] lets any loop host an app: your own crossterm or
//! termion loop, a game loop, a socket. `App::run` is that loop written
//! for you.
//!
//! ```sh
//! cargo run -p rs-rich-intuituive --example in_ratatui
//! ```
//!
//! `+` counts, Tab moves between the buttons, `q` quits.
//!
//! [`Driver`]: intuituive::Driver

use std::io;
use std::time::Instant;

use intuituive::interact::event::from_crossterm;
use intuituive::interact::Event;
use intuituive::prelude::*;
use ratatui::crossterm::event;
use ratatui::layout::{Constraint, Layout};
use ratatui::widgets::{Block, Paragraph};

/// The intuiTUIve half: reactive state, focus and key bindings, as in any
/// app.
fn counter() -> App {
    App::new(|| {
        let count = signal(0);
        column([
            text!("count [b]{count}[/]").fixed(1),
            label("[green]add[/]")
                .focus_style("reverse")
                .on_key("enter", move |_| count.update(|n| *n += 1))
                .fixed(1),
            label("[red]reset[/]")
                .focus_style("reverse")
                .on_key("enter", move |_| count.set(0))
                .fixed(1),
            label("[dim]+ counts · tab · q quits[/]"),
        ])
        .padding(0, 1)
        .panel("intuiTUIve")
        .on_key("+", move |_| count.update(|n| *n += 1))
        .on_key("q", |cx| cx.quit())
    })
}

/// The right half of the terminal.
fn pane(width: u16) -> u16 {
    width / 2
}

fn main() -> io::Result<()> {
    let mut terminal = ratatui::init();
    let size = terminal.size()?;
    let mut driver = counter().driver(pane(size.width), size.height);
    let start = Instant::now();
    let mut turns = 0u64;
    let result = loop {
        driver.update(start.elapsed());
        if driver.is_done() {
            break Ok(());
        }
        // The app draws into its own screen; ratatui sends the bytes, so
        // the driver's are not needed.
        let _ = driver.render();
        turns += 1;
        let drawn = terminal.draw(|frame| {
            let [left, right] =
                Layout::horizontal([Constraint::Fill(1), Constraint::Percentage(50)])
                    .areas(frame.area());
            let text = format!("ratatui owns the terminal\nand the loop.\n\n{turns} turns so far.");
            frame.render_widget(
                Paragraph::new(text).block(Block::bordered().title("ratatui")),
                left,
            );
            rich_ratatui::lines_to_buffer(&driver.screen().lines(), right, frame.buffer_mut());
        });
        if let Err(error) = drawn {
            break Err(error);
        }
        match event::poll(driver.timeout(start.elapsed())) {
            Ok(true) => match event::read().map(from_crossterm) {
                // The app gets its pane's size, not the terminal's.
                Ok(Some(Event::Resize { columns, rows })) => driver.resize(pane(columns), rows),
                Ok(Some(other)) => driver.event(other),
                Ok(None) => {}
                Err(error) => break Err(error),
            },
            Ok(false) => {}
            Err(error) => break Err(error),
        }
    };
    let _ = driver.finish();
    ratatui::restore();
    result
}
