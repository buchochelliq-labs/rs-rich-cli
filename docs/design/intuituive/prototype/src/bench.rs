//! The dashboard benchmark: time and bytes per frame for ratatui, the
//! prototype with caching off (re-render everything each frame, today's
//! `rich-interact` model), and the prototype with signals and retention.
//!
//! Both rich variants paint with `rich-interact`'s `Painter` (a cell diff
//! against the last frame, the bytes a terminal would receive). ratatui
//! draws through its own `CrosstermBackend` into a byte buffer (its buffer
//! diff, encoded the way it encodes for a terminal).

use std::io;
use std::time::Instant;

use ratatui::backend::{Backend, ClearType, CrosstermBackend, WindowSize};
use ratatui::buffer::Cell;
use ratatui::layout::{Position, Rect, Size};
use ratatui::{Terminal, TerminalOptions, Viewport};
use rich::ColorSystem;
use rich_interact::paint::Painter;
use rich_interact::View;

use crate::dashboard::{author_lines, log_line, prototype, RatatuiDash, SERVICES};
use crate::paint::RowPainter;

/// Bytes written, shared with the benchmark that counts them.
#[derive(Clone, Default)]
struct Sink(std::rc::Rc<std::cell::RefCell<Vec<u8>>>);

impl io::Write for Sink {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.borrow_mut().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// A crossterm backend writing to memory, with a fixed size, so the bytes
/// ratatui would send a terminal can be counted.
struct Counting {
    inner: CrosstermBackend<Sink>,
    size: Size,
}

impl Backend for Counting {
    type Error = io::Error;

    fn draw<'a, I: Iterator<Item = (u16, u16, &'a Cell)>>(&mut self, content: I) -> io::Result<()> {
        self.inner.draw(content)
    }
    fn hide_cursor(&mut self) -> io::Result<()> {
        self.inner.hide_cursor()
    }
    fn show_cursor(&mut self) -> io::Result<()> {
        self.inner.show_cursor()
    }
    fn get_cursor_position(&mut self) -> io::Result<Position> {
        Ok(Position::ORIGIN)
    }
    fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> io::Result<()> {
        self.inner.set_cursor_position(position)
    }
    fn clear(&mut self) -> io::Result<()> {
        self.inner.clear()
    }
    fn clear_region(&mut self, clear_type: ClearType) -> io::Result<()> {
        self.inner.clear_region(clear_type)
    }
    fn size(&self) -> io::Result<Size> {
        Ok(self.size)
    }
    fn window_size(&mut self) -> io::Result<WindowSize> {
        Ok(WindowSize {
            columns_rows: self.size,
            pixels: Size::new(0, 0),
        })
    }
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// What happens between frames.
#[derive(Clone, Copy)]
pub enum Scenario {
    /// The footer's counter changes; nothing else.
    Tick,
    /// The selection moves down one row (and wraps).
    Select,
    /// A log line arrives and the counter changes.
    Log,
}

impl Scenario {
    fn name(self) -> &'static str {
        match self {
            Scenario::Tick => "tick",
            Scenario::Select => "select",
            Scenario::Log => "log",
        }
    }
}

#[derive(Debug, Default)]
pub struct Result {
    pub micros_per_frame: f64,
    pub bytes_per_frame: f64,
    pub first_frame_bytes: usize,
    /// Nodes rendered per frame (rich variants only).
    pub nodes_per_frame: f64,
}

fn run_ratatui(scenario: Scenario, width: u16, height: u16, frames: usize) -> Result {
    let sink = Sink::default();
    let backend = Counting {
        inner: CrosstermBackend::new(sink.clone()),
        size: Size::new(width, height),
    };
    let options = TerminalOptions {
        viewport: Viewport::Fixed(Rect::new(0, 0, width, height)),
    };
    let mut terminal = Terminal::with_options(backend, options).unwrap();
    let mut dash = RatatuiDash {
        selected: 0,
        tick: 0,
        log: (0..50).map(log_line).collect(),
    };
    terminal.draw(|f| dash.draw(f)).unwrap();
    let first = sink.0.borrow().len();
    let start = Instant::now();
    for n in 0..frames {
        match scenario {
            Scenario::Tick => dash.tick += 1,
            Scenario::Select => dash.selected = (dash.selected + 1) % SERVICES,
            Scenario::Log => {
                dash.log.push(log_line(50 + n));
                dash.tick += 1;
            }
        }
        terminal.draw(|f| dash.draw(f)).unwrap();
    }
    let elapsed = start.elapsed();
    let total = sink.0.borrow().len() - first;
    Result {
        micros_per_frame: elapsed.as_secs_f64() * 1e6 / frames as f64,
        bytes_per_frame: total as f64 / frames as f64,
        first_frame_bytes: first,
        nodes_per_frame: 0.0,
    }
}

fn run_rich(
    scenario: Scenario,
    immediate: bool,
    rows: bool,
    width: usize,
    height: usize,
    frames: usize,
) -> Result {
    let console = rich::Console::builder()
        .width(width)
        .color_system(Some(ColorSystem::Truecolor))
        .force_terminal(true)
        .build();
    let (app, s) = prototype();
    let app = app.immediate(immediate);
    s.log.update(|l| l.extend((0..50).map(log_line)));
    let system = Some(ColorSystem::Truecolor);
    let mut painter = Painter::new(system, false);
    let mut row_painter = RowPainter::default();
    // The real painter takes a `View` (a copy of the lines); the row
    // painter reads the tree's lines in place.
    let mut paint = |lines: &crate::tree::Lines| {
        if rows {
            row_painter.paint(lines, system).len()
        } else {
            painter.paint(&View::new(lines.to_vec()), height).len()
        }
    };
    let first = paint(&app.frame(&console, width, height));
    let mut total = 0;
    let mut nodes = 0;
    let start = Instant::now();
    for n in 0..frames {
        match scenario {
            Scenario::Tick => s.tick.update(|t| *t += 1),
            Scenario::Select => s.select((s.selected.get_untracked() + 1) % SERVICES),
            Scenario::Log => {
                s.log.update(|l| l.push(log_line(50 + n)));
                s.tick.update(|t| *t += 1);
            }
        }
        let lines = app.frame(&console, width, height);
        nodes += app.stats().rendered;
        total += paint(&lines);
    }
    let elapsed = start.elapsed();
    Result {
        micros_per_frame: elapsed.as_secs_f64() * 1e6 / frames as f64,
        bytes_per_frame: total as f64 / frames as f64,
        first_frame_bytes: first,
        nodes_per_frame: nodes as f64 / frames as f64,
    }
}

/// The cells that differ from `previous`, looked for only inside the damaged
/// rectangles (ratatui's `Buffer::diff` compares the whole screen). A wide
/// character's trailing cell is skipped, as ratatui's diff skips it; a wide
/// character straddling a rectangle's edge is the case this does not handle
/// (a full implementation would widen the rectangle to the character).
fn damaged_diff<'a>(
    previous: &ratatui::buffer::Buffer,
    screen: &'a ratatui::buffer::Buffer,
    rects: &[Rect],
) -> Vec<(u16, u16, &'a Cell)> {
    use ratatui::buffer::CellWidth;
    let mut updates = Vec::new();
    for rect in rects {
        for y in rect.top()..rect.bottom() {
            let mut x = rect.left();
            while x < rect.right() {
                let cell = &screen[(x, y)];
                if *cell != previous[(x, y)] {
                    updates.push((x, y, cell));
                }
                x += cell.cell_width().max(1);
            }
        }
    }
    updates
}

/// The prototype drawing into a retained ratatui `Buffer`: rich renders the
/// nodes that changed, writes them into their rectangles, and ratatui's own
/// diff and crossterm encoder (the same as the ratatui variant's) send them.
fn run_buffer(scenario: Scenario, width: u16, height: u16, frames: usize) -> Result {
    let console = rich::Console::builder()
        .width(width as usize)
        .color_system(Some(ColorSystem::Truecolor))
        .force_terminal(true)
        .build();
    let (app, s) = prototype();
    s.log.update(|l| l.extend((0..50).map(log_line)));
    let area = Rect::new(0, 0, width, height);
    let mut screen = ratatui::buffer::Buffer::empty(area);
    let mut previous = ratatui::buffer::Buffer::empty(area);
    let sink = Sink::default();
    let mut backend = CrosstermBackend::new(sink.clone());
    let mut flush = |screen: &ratatui::buffer::Buffer,
                     previous: &mut ratatui::buffer::Buffer,
                     damage: crate::tree::Damage| {
        let updates = damaged_diff(previous, screen, &damage.rects);
        backend.draw(updates.into_iter()).unwrap();
        Backend::flush(&mut backend).unwrap();
        // Bring the previous screen up to date where it was written.
        for rect in damage.rects {
            for y in rect.top()..rect.bottom() {
                for x in rect.left()..rect.right() {
                    previous[(x, y)] = screen[(x, y)].clone();
                }
            }
        }
    };
    let damage = app.frame_into(&console, &mut screen);
    flush(&screen, &mut previous, damage);
    let first = sink.0.borrow().len();
    let mut nodes = 0;
    let start = Instant::now();
    for n in 0..frames {
        match scenario {
            Scenario::Tick => s.tick.update(|t| *t += 1),
            Scenario::Select => s.select((s.selected.get_untracked() + 1) % SERVICES),
            Scenario::Log => {
                s.log.update(|l| l.push(log_line(50 + n)));
                s.tick.update(|t| *t += 1);
            }
        }
        let damage = app.frame_into(&console, &mut screen);
        nodes += app.stats().rendered;
        flush(&screen, &mut previous, damage);
    }
    let elapsed = start.elapsed();
    let total = sink.0.borrow().len() - first;
    Result {
        micros_per_frame: elapsed.as_secs_f64() * 1e6 / frames as f64,
        bytes_per_frame: total as f64 / frames as f64,
        first_frame_bytes: first,
        nodes_per_frame: nodes as f64 / frames as f64,
    }
}

/// Run every scenario at two sizes and print a Markdown table.
pub fn run() {
    let frames = 500;
    println!(
        "Author code: ratatui {} lines, prototype {} lines\n",
        author_lines("ratatui"),
        author_lines("prototype")
    );
    println!("| Size | Scenario | Variant | µs/frame | bytes/frame | first frame (bytes) | nodes rendered/frame |");
    println!("|---|---|---|---:|---:|---:|---:|");
    for (w, h) in [(80, 24), (200, 60)] {
        for scenario in [Scenario::Tick, Scenario::Select, Scenario::Log] {
            let best = |f: &dyn Fn() -> Result| {
                (0..3)
                    .map(|_| f())
                    .min_by(|a, b| a.micros_per_frame.total_cmp(&b.micros_per_frame))
                    .unwrap()
            };
            let rows = [
                (
                    "ratatui",
                    best(&|| run_ratatui(scenario, w as u16, h as u16, frames)),
                ),
                (
                    "rich, re-render all",
                    best(&|| run_rich(scenario, true, false, w, h, frames)),
                ),
                (
                    "rich, signals + retained",
                    best(&|| run_rich(scenario, false, false, w, h, frames)),
                ),
                (
                    "rich, retained + row painter",
                    best(&|| run_rich(scenario, false, true, w, h, frames)),
                ),
                (
                    "rich, retained + cell buffer",
                    best(&|| run_buffer(scenario, w as u16, h as u16, frames)),
                ),
            ];
            for (variant, r) in rows {
                let nodes = if variant == "ratatui" {
                    "—".to_string()
                } else {
                    format!("{:.1}", r.nodes_per_frame)
                };
                println!(
                    "| {w}x{h} | {} | {variant} | {:.1} | {:.0} | {} | {nodes} |",
                    scenario.name(),
                    r.micros_per_frame,
                    r.bytes_per_frame,
                    r.first_frame_bytes,
                );
            }
        }
    }
}

/// Where a retained frame's time goes, per scenario: the tree (leaves that
/// render plus the containers that recompose) and the row painter.
pub fn breakdown(width: usize, height: usize, frames: usize) {
    let console = rich::Console::builder()
        .width(width)
        .color_system(Some(ColorSystem::Truecolor))
        .force_terminal(true)
        .build();
    for scenario in [Scenario::Tick, Scenario::Select, Scenario::Log] {
        let (app, s) = prototype();
        s.log.update(|l| l.extend((0..50).map(log_line)));
        let mut painter = RowPainter::default();
        let system = Some(ColorSystem::Truecolor);
        painter.paint(&app.frame(&console, width, height), system);
        let (mut tree, mut paint) = (0.0, 0.0);
        for n in 0..frames {
            match scenario {
                Scenario::Tick => s.tick.update(|t| *t += 1),
                Scenario::Select => s.select((s.selected.get_untracked() + 1) % SERVICES),
                Scenario::Log => {
                    s.log.update(|l| l.push(log_line(50 + n)));
                    s.tick.update(|t| *t += 1);
                }
            }
            let t0 = Instant::now();
            let lines = app.frame(&console, width, height);
            let t1 = Instant::now();
            painter.paint(&lines, system);
            tree += (t1 - t0).as_secs_f64();
            paint += (t1.elapsed()).as_secs_f64();
        }
        let us = |x: f64| x * 1e6 / frames as f64;
        println!(
            "{width}x{height} {}: tree {:.1} µs, row painter {:.1} µs",
            scenario.name(),
            us(tree),
            us(paint)
        );
    }
}
