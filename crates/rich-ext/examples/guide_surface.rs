//! Guide: Positioned drawing — run: cargo run -p rs-rich-ext --example guide_surface [-- --svg docs/media/guide]
//!
//! Every snippet on `docs/guide/ext/surface.md` comes from this file.
//! Without `--svg` it ticks a small dashboard inline in the terminal for a
//! few seconds; with `--svg DIR` it draws the same frames on an in-memory
//! surface and writes what the surface shows, and the bytes each frame
//! sent, as screenshots.

#[path = "guide_support/mod.rs"]
mod guide_support;

use std::io::Write;
use std::time::Duration;

use guide_support::Shots;
use rich::{Console, ConsoleOptions, Panel, Renderable, Segment, Table, Text};
use rich_ext::surface::{Drawing, Rect, Surface, Viewport};

// --8<-- [start:frame]
/// One frame of the dashboard: a title, a table of jobs and a status line,
/// each in its own rectangle.
fn dashboard(s: &mut Drawing<'_>, tick: u32) {
    let area = s.area();
    s.render(
        Rect::new(0, 0, area.width, 3),
        &Panel::new(Box::new(
            Text::from_markup("[b]deploy[/b] · production").unwrap(),
        )),
    );
    let mut jobs = Table::new();
    jobs.add_column("job")
        .add_column("state")
        .add_column("time");
    jobs.add_row(&["build", "[green]done[/]", "41s"]);
    jobs.add_row(&["test", "[green]done[/]", "2m 03s"]);
    jobs.add_row(&["rollout", "[yellow]running[/]", &format!("{tick}s")]);
    s.render(Rect::new(0, 3, area.width, 7), &jobs);
    s.render(
        Rect::new(0, area.height - 1, area.width, 1),
        &Text::from_markup(&format!("[dim]tick {tick} · updated in place[/]")).unwrap(),
    );
}
// --8<-- [end:frame]

// --8<-- [start:terminal]
fn run_in_terminal() -> std::io::Result<()> {
    // Eleven rows below the prompt; the scrollback above is left alone.
    let mut surface = Surface::stdout(Viewport::Inline(11)).synchronized(true);
    for tick in 0..5 {
        surface.draw(|s| dashboard(s, tick))?;
        std::thread::sleep(Duration::from_millis(500));
    }
    // The last frame stays on screen, with the cursor below it.
    surface.finish()
}
// --8<-- [end:terminal]

// --8<-- [start:memory]
/// The same frames on a surface that writes to memory, as a test would:
/// what each frame sent, and what the surface shows after it.
fn frames() -> (Vec<String>, Surface<Vec<u8>>) {
    let mut surface = Surface::new(Vec::new(), 46, 11);
    let mut sent = Vec::new();
    for tick in [7, 8] {
        surface.draw(|s| dashboard(s, tick)).unwrap();
        sent.push(String::from_utf8(std::mem::take(surface.writer_mut())).unwrap());
    }
    (sent, surface)
}
// --8<-- [end:memory]

/// Rendered lines as a renderable, to print what the surface shows.
struct Lines(Vec<Vec<Segment>>);

impl Renderable for Lines {
    fn rich_render(&self, _: &Console, _: &ConsoleOptions) -> Vec<Segment> {
        let mut out = Vec::new();
        for (i, line) in self.0.iter().enumerate() {
            if i > 0 {
                out.push(Segment::line());
            }
            out.extend(line.iter().cloned());
        }
        out
    }
}

/// `bytes` with escapes spelled out, as the second shot shows them.
fn visible(bytes: &str) -> String {
    bytes.replace('\x1b', "\\e")
}

fn main() -> std::io::Result<()> {
    let shots = Shots::from_args("guide_surface");
    if !shots.is_svg() {
        run_in_terminal()?;
        return std::io::stdout().flush();
    }
    let (sent, surface) = frames();
    shots.shot("frame", 46, |console| {
        console.print(&Lines(surface.screen().lines()));
    });
    shots.shot("bytes", 60, |console| {
        let mut table = Table::new();
        table
            .add_column("frame")
            .add_column("bytes")
            .add_column("sent");
        for (i, out) in sent.iter().enumerate() {
            let shown = visible(out);
            let shown = if shown.chars().count() > 40 {
                format!("{}…", shown.chars().take(40).collect::<String>())
            } else {
                shown
            };
            table.add_row(&[
                &format!("{}", i + 1),
                &out.len().to_string(),
                &rich::markup::escape(&shown),
            ]);
        }
        console.print(&table);
    });
    Ok(())
}
