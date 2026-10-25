//! Positioned drawing: render any rich renderable into a rectangle of the
//! terminal, and write only the cells that changed.
//!
//! A [`Surface`] keeps a [`Screen`] of cells between frames. Each
//! [`draw`](Surface::draw) hands a [`Drawing`] to a closure that renders
//! into rectangles of it; the surface then writes what changed inside those
//! rectangles and nothing else, so a clock ticking in one corner sends a few
//! bytes rather than the whole screen. It is the drawing half of
//! `rs-rich-intuituive`, which builds on the same [`Screen`] and [`Painter`],
//! without the widget tree or the event loop: input stays yours.
//!
//! ```
//! use rich::Text;
//! use rich_ext::surface::{Rect, Surface};
//!
//! let mut surface = Surface::new(Vec::new(), 20, 2);
//! surface
//!     .draw(|s| {
//!         s.render(Rect::new(0, 0, 20, 1), &Text::new("tick 1"));
//!     })
//!     .unwrap();
//! surface.writer_mut().clear();
//! surface
//!     .draw(|s| {
//!         s.render(Rect::new(0, 0, 20, 1), &Text::new("tick 2"));
//!     })
//!     .unwrap();
//! // One cell changed: move there and write it.
//! let out = String::from_utf8(surface.writer().clone()).unwrap();
//! assert_eq!(out, "\x1b[1;6H\x1b[0m2");
//! ```
//!
//! The surface fills the terminal on the alternate screen
//! ([`Viewport::Fullscreen`], the default), or takes a fixed number of rows
//! below the cursor and leaves the scrollback alone ([`Viewport::Inline`]).
//! A [`resize`](Surface::resize) keeps what was drawn and writes everything
//! once. Graphics (micro assets drawn with Kitty, iTerm2 or Sixel) come
//! through a [`PlacementSource`], as they do for [`frame`](crate::frame)s.

mod screen;

use std::io::{self, Write};
use std::sync::Arc;

use rich::{Console, Renderable, Segment};

use crate::frame::{plan_graphics, Change, Frame, Placement, PlacementSource};

pub use screen::{Cell, PaintStats, PaintStrategy, Painter, Rect, Screen};

/// Begins a synchronized update (DEC private mode 2026): the terminal holds
/// the screen until [`END_SYNCHRONIZED_UPDATE`].
pub const BEGIN_SYNCHRONIZED_UPDATE: &str = "\x1b[?2026h";
/// Ends a synchronized update: the terminal shows what was written since
/// [`BEGIN_SYNCHRONIZED_UPDATE`] in one go.
pub const END_SYNCHRONIZED_UPDATE: &str = "\x1b[?2026l";

/// The visible stand-in for a terminal control, or `None` for any other
/// character. One character for one, in the style of
/// [`sanitize_terminal_controls`](crate::sanitize_terminal_controls): C0
/// controls as their control pictures (`␛`, `␇`), DEL as `␡`, a tab as a
/// space (it would move the cursor), and C1 controls (8-bit CSI and OSC
/// among them) as `�`.
pub fn control_picture(c: char) -> Option<char> {
    match c {
        '\t' => Some(' '),
        '\u{1b}' => Some('␛'),
        '\u{7f}' => Some('␡'),
        '\0'..='\u{1f}' => char::from_u32(0x2400 + c as u32),
        '\u{80}'..='\u{9f}' => Some('\u{fffd}'),
        _ => None,
    }
}

/// Make the terminal controls in `line`'s text visible, so nothing drawn
/// can drive the terminal: styles are already [`Style`](rich::Style)s, and
/// whatever is left in the text would otherwise reach it raw. Each control
/// becomes one character (see [`control_picture`]), so character offsets
/// into the text still hold. Control segments are left alone; frames skip
/// them.
pub fn sanitize_line(line: &mut [Segment]) {
    for segment in line.iter_mut().filter(|segment| !segment.control) {
        if segment.text.chars().any(|c| control_picture(c).is_some()) {
            segment.text = segment
                .text
                .chars()
                .map(|c| control_picture(c).unwrap_or(c))
                .collect();
        }
    }
}

/// Where a [`Surface`] draws.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Viewport {
    /// The whole terminal, on the alternate screen: what was there before
    /// comes back when the surface finishes.
    #[default]
    Fullscreen,
    /// This many rows from the cursor's row down, scrolling the terminal if
    /// they pass its bottom. The rows above (the shell's scrollback) are
    /// left alone, and the last frame stays on screen when the surface
    /// finishes, with the cursor below it.
    Inline(u16),
}

/// A drawing handle on the terminal: a screen of cells kept between frames,
/// the painter that writes the difference, and where it writes. See the
/// [module docs](self).
pub struct Surface<W: Write> {
    writer: W,
    console: Console,
    screen: Screen,
    painter: Painter,
    viewport: Viewport,
    synchronized: bool,
    started: bool,
    finished: bool,
    cursor_shown: bool,
    /// The next draw writes every cell.
    full: bool,
    graphics: Option<Arc<dyn PlacementSource>>,
    placements: Vec<Placement>,
}

impl Surface<io::Stdout> {
    /// A surface on standard output, as large as the terminal (inline, as
    /// wide as it), with the colours `Console::new()` detects.
    pub fn stdout(viewport: Viewport) -> Surface<io::Stdout> {
        let console = Console::new();
        let size = console.size();
        let width = clamp(size.width);
        let height = match viewport {
            Viewport::Fullscreen => clamp(size.height),
            // No taller than the terminal: relative moves stop at its edges.
            Viewport::Inline(rows) => rows.min(clamp(size.height)),
        };
        Surface::new(io::stdout(), width, height)
            .viewport(viewport)
            .console(console)
    }
}

impl<W: Write> Surface<W> {
    /// A full-screen surface of `width` by `height` cells writing to
    /// `writer`, in truecolor. [`console`](Self::console) sets the colours
    /// and how renderables are rendered.
    pub fn new(writer: W, width: u16, height: u16) -> Surface<W> {
        let console = Console::builder()
            .width(width.max(1) as usize)
            .height(height.max(1) as usize)
            .force_terminal(true)
            .color_system(Some(rich::ColorSystem::Truecolor))
            .build();
        let system = console.color_system();
        Surface {
            writer,
            console,
            screen: Screen::new(width, height),
            painter: Painter::new(system),
            viewport: Viewport::Fullscreen,
            synchronized: false,
            started: false,
            finished: false,
            cursor_shown: true,
            full: true,
            graphics: None,
            placements: Vec::new(),
        }
    }

    /// Draw inline or full screen. An inline viewport's row count replaces
    /// the surface's height, up to that height (the terminal's).
    #[must_use]
    pub fn viewport(mut self, viewport: Viewport) -> Surface<W> {
        self.viewport = viewport;
        if let Viewport::Inline(rows) = viewport {
            // The surface's height is the terminal's: a region taller than
            // that would lose rows at its edges.
            let area = self.screen.area();
            self.screen = Screen::new(area.width, rows.min(area.height.max(1)));
        }
        let system = self.console.color_system();
        self.painter = match viewport {
            Viewport::Fullscreen => Painter::new(system),
            Viewport::Inline(_) => Painter::new(system).inline(),
        };
        self.full = true;
        self
    }

    /// Render with `console`: its colour system is the one written (`None`
    /// writes no colour at all), and its theme and highlighting apply.
    #[must_use]
    pub fn console(mut self, console: Console) -> Surface<W> {
        self.painter.set_color_system(console.color_system());
        self.console = console;
        self
    }

    /// Write each frame as one synchronized update (DEC private mode 2026),
    /// which a terminal that has it shows whole and one that does not
    /// ignores (off by default).
    #[must_use]
    pub fn synchronized(mut self, on: bool) -> Surface<W> {
        self.synchronized = on;
        self
    }

    /// Draw the graphics `source` places on the screen's cells, after the
    /// cells, and redraw them when the cells under them are repainted.
    #[must_use]
    pub fn graphics(mut self, source: Option<Arc<dyn PlacementSource>>) -> Surface<W> {
        self.graphics = source;
        self
    }

    /// The whole surface, at `(0, 0)`.
    pub fn area(&self) -> Rect {
        self.screen.area()
    }

    /// What is on the surface now.
    pub fn screen(&self) -> &Screen {
        &self.screen
    }

    pub fn writer(&self) -> &W {
        &self.writer
    }

    pub fn writer_mut(&mut self) -> &mut W {
        &mut self.writer
    }

    /// The terminal changed size: keep what was drawn (cut to the new
    /// size), and write everything on the next [`draw`](Self::draw). An
    /// inline viewport keeps its row count, up to the terminal's height,
    /// and takes the new width.
    pub fn resize(&mut self, width: u16, height: u16) {
        let height = match self.viewport {
            Viewport::Fullscreen => height,
            Viewport::Inline(rows) => rows.min(height.max(1)),
        };
        if (width, height) == (self.screen.area().width, self.screen.area().height) {
            return;
        }
        let mut screen = Screen::new(width, height);
        let old = self.screen.area();
        screen.blit(&self.screen, old, 0, 0);
        self.screen = screen;
        self.full = true;
    }

    /// Blank the surface; the next [`draw`](Self::draw) writes everything.
    pub fn clear(&mut self) {
        let area = self.screen.area();
        self.screen.clear(area);
        self.full = true;
    }

    /// Draw a frame: `f` renders into rectangles of the [`Drawing`], then
    /// what changed inside them is written and flushed. Cells nothing drew
    /// keep what they showed. The cursor is hidden unless `f` placed it
    /// with [`Drawing::set_cursor`].
    pub fn draw(&mut self, f: impl FnOnce(&mut Drawing<'_>)) -> io::Result<()> {
        let mut drawing = Drawing {
            screen: &mut self.screen,
            console: &self.console,
            graphics: self.graphics.as_deref(),
            damage: Vec::new(),
            cursor: None,
        };
        f(&mut drawing);
        let Drawing { damage, cursor, .. } = drawing;
        let out = self.frame(damage, cursor);
        self.writer.write_all(out.as_bytes())?;
        self.writer.flush()
    }

    /// The bytes for one frame.
    fn frame(&mut self, damage: Vec<Rect>, cursor: Option<(u16, u16)>) -> String {
        let mut out = String::new();
        if !self.started {
            if self.viewport == Viewport::Fullscreen {
                out.push_str("\x1b[?1049h");
            }
            self.started = true;
        }
        let full = std::mem::take(&mut self.full);
        if full {
            self.painter.invalidate();
        }
        let damage = if full {
            vec![self.screen.area()]
        } else {
            damage
        };
        let painted = self.painter.paint(&self.screen, &damage);
        let written = self.painter.written().to_vec();
        if self.cursor_shown && !painted.is_empty() {
            // Hidden while the frame is written, so it does not flicker
            // across the screen.
            out.push_str("\x1b[?25l");
            self.cursor_shown = false;
        }
        out.push_str(&painted);
        if self.graphics.is_some() {
            out.push_str(&self.draw_graphics(&written));
        }
        let area = self.screen.area();
        match cursor.filter(|&(x, y)| x < area.width && y < area.height) {
            Some((x, y)) => {
                out.push_str(&self.painter.move_to(x, y));
                if !self.cursor_shown {
                    out.push_str("\x1b[?25h");
                    self.cursor_shown = true;
                }
            }
            None => {
                if self.cursor_shown {
                    out.push_str("\x1b[?25l");
                    self.cursor_shown = false;
                }
            }
        }
        // Styles no cell uses any more go, once they pile up.
        self.painter.compact(&mut self.screen);
        if self.synchronized && !out.is_empty() {
            out = format!("{BEGIN_SYNCHRONIZED_UPDATE}{out}{END_SYNCHRONIZED_UPDATE}");
        }
        out
    }

    /// After the cells: the graphics that moved, went away or had their
    /// cells repainted under them. `written` is what the painter actually
    /// wrote: a damaged cell that did not change was not written, so a
    /// graphic over it was not wiped.
    fn draw_graphics(&mut self, written: &[Rect]) -> String {
        let Some(source) = self.graphics.clone() else {
            return String::new();
        };
        let frame = screen_frame(&self.screen);
        let height = frame.height();
        let next: Vec<Placement> = source
            .placements(&frame)
            .into_iter()
            .filter(|p| p.row + p.rows <= height)
            .collect();
        let changes: Vec<Change> = written
            .iter()
            .flat_map(|rect| {
                (rect.y..rect.bottom()).map(move |row| Change {
                    row: row as usize,
                    columns: rect.x as usize..rect.right() as usize,
                })
            })
            .collect();
        let plan = plan_graphics(&self.placements, &next, &changes);
        let mut out = String::new();
        let repaint: Vec<Rect> = plan
            .repaint
            .iter()
            .map(|(row, columns)| {
                Rect::new(
                    columns.start as u16,
                    *row as u16,
                    (columns.end - columns.start) as u16,
                    1,
                )
            })
            .collect();
        if !repaint.is_empty() {
            out.push_str(&self.painter.repaint(&self.screen, &repaint));
        }
        for placement in &plan.draw {
            out.push_str(
                &self
                    .painter
                    .move_to(placement.column as u16, placement.row as u16),
            );
            out.push_str(&placement.graphic.draw(placement.frame));
        }
        self.placements = next;
        out
    }

    /// Give the terminal back: the alternate screen is left (full screen),
    /// or the cursor goes below the last frame (inline); styles are reset
    /// and the cursor shown. Called by `Drop` if not before.
    pub fn finish(&mut self) -> io::Result<()> {
        if self.finished || !self.started {
            self.finished = true;
            return Ok(());
        }
        let mut out = String::new();
        if let Some(source) = &self.graphics {
            let retained = match self.viewport {
                Viewport::Inline(_) => self.placements.clone(),
                Viewport::Fullscreen => Vec::new(),
            };
            out.push_str(&source.release(&retained));
        }
        out.push_str(&self.painter.finish());
        out.push_str("\x1b[0m\x1b[?25h");
        if self.viewport == Viewport::Fullscreen {
            out.push_str("\x1b[?1049l");
        }
        self.writer.write_all(out.as_bytes())?;
        self.writer.flush()?;
        // Only now: a failed write leaves `finish` (and `Drop`) to try again.
        self.finished = true;
        Ok(())
    }
}

impl<W: Write> Drop for Surface<W> {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}

/// One frame being drawn: render into rectangles of the surface. See
/// [`Surface::draw`].
pub struct Drawing<'a> {
    screen: &'a mut Screen,
    console: &'a Console,
    graphics: Option<&'a dyn PlacementSource>,
    damage: Vec<Rect>,
    cursor: Option<(u16, u16)>,
}

impl Drawing<'_> {
    /// The whole surface, at `(0, 0)`.
    pub fn area(&self) -> Rect {
        self.screen.area()
    }

    /// The console renderables are rendered with.
    pub fn console(&self) -> &Console {
        self.console
    }

    /// Render `renderable` into `rect`, `rect.width` cells wide and at most
    /// `rect.height` rows tall, replacing what was there: the cells it does
    /// not reach are cleared.
    pub fn render(&mut self, rect: Rect, renderable: &dyn Renderable) {
        let options = self
            .console
            .options()
            .update_width(rect.width.max(1) as usize)
            .update_height(rect.height.max(1) as usize);
        let lines = self.console.render_lines(renderable, &options, false);
        self.write_lines(rect, lines);
    }

    /// Write rendered `lines` into `rect`, replacing what was there (see
    /// [`Screen::write_lines`]).
    pub fn write_lines(&mut self, rect: Rect, lines: Vec<Vec<Segment>>) {
        let lines: Vec<Vec<Segment>> = match self.graphics {
            Some(source) => lines.into_iter().map(|l| source.prepare(l)).collect(),
            None => lines,
        };
        self.screen.write_lines(rect, &lines);
        self.damage.push(rect);
    }

    /// Clear `rect` to blank cells.
    pub fn clear(&mut self, rect: Rect) {
        self.screen.clear(rect);
        self.damage.push(rect);
    }

    /// Show the cursor at `x`, `y` once the frame is written.
    pub fn set_cursor(&mut self, x: u16, y: u16) {
        self.cursor = Some((x, y));
    }

    /// The screen itself, for what rendering does not cover (laying a
    /// style over cells with [`Screen::restyle`], copying with
    /// [`Screen::blit`]). Everything is compared with what was sent.
    pub fn screen_mut(&mut self) -> &mut Screen {
        let area = self.screen.area();
        self.damage.push(area);
        self.screen
    }
}

/// The screen as a frame, for finding the graphics placed on its cells.
fn screen_frame(screen: &Screen) -> Frame {
    let mut segments = Vec::new();
    for line in screen.lines() {
        segments.extend(line);
        segments.push(Segment::line());
    }
    Frame::from_segments(&segments)
}

fn clamp(cells: usize) -> u16 {
    cells.clamp(1, u16::MAX as usize) as u16
}

#[cfg(test)]
mod tests {
    use super::*;
    use rich::Text;

    fn output(surface: &mut Surface<Vec<u8>>) -> String {
        String::from_utf8(std::mem::take(surface.writer_mut())).unwrap()
    }

    #[test]
    fn the_first_draw_enters_the_alternate_screen_and_writes_everything() {
        let mut surface = Surface::new(Vec::new(), 10, 2);
        surface
            .draw(|s| s.render(Rect::new(0, 0, 10, 1), &Text::new("hello")))
            .unwrap();
        let out = output(&mut surface);
        assert!(out.starts_with("\x1b[?1049h\x1b[?25l"), "{out:?}");
        assert!(out.contains("\x1b[2J") && out.contains("hello"), "{out:?}");
        surface.finish().unwrap();
        let out = output(&mut surface);
        assert!(out.ends_with("\x1b[0m\x1b[?25h\x1b[?1049l"), "{out:?}");
    }

    #[test]
    fn an_unchanged_frame_writes_nothing() {
        let mut surface = Surface::new(Vec::new(), 10, 2);
        let draw = |s: &mut Drawing<'_>| s.render(Rect::new(0, 0, 10, 1), &Text::new("same"));
        surface.draw(draw).unwrap();
        output(&mut surface);
        surface.draw(draw).unwrap();
        assert_eq!(output(&mut surface), "");
    }

    #[test]
    fn a_resize_keeps_the_cells_and_writes_everything_once() {
        let mut surface = Surface::new(Vec::new(), 10, 2);
        surface
            .draw(|s| s.render(Rect::new(0, 0, 10, 1), &Text::new("kept")))
            .unwrap();
        output(&mut surface);
        surface.resize(6, 3);
        assert_eq!(surface.screen().plain()[0], "kept  ");
        surface.draw(|_| {}).unwrap();
        let out = output(&mut surface);
        assert!(out.contains("\x1b[2J") && out.contains("kept"), "{out:?}");
        surface.draw(|_| {}).unwrap();
        assert_eq!(output(&mut surface), "");
    }

    #[test]
    fn inline_draws_below_the_cursor_and_leaves_the_frame_on_finish() {
        let mut surface = Surface::new(Vec::new(), 8, 24).viewport(Viewport::Inline(2));
        assert_eq!(surface.area(), Rect::new(0, 0, 8, 2));
        surface
            .draw(|s| s.render(s.area(), &Text::new("one\ntwo")))
            .unwrap();
        let out = output(&mut surface);
        assert!(!out.contains("\x1b[?1049h"), "{out:?}");
        assert!(!out.contains("\x1b[2J"), "{out:?}");
        assert!(out.contains("one") && out.contains("two"), "{out:?}");
        surface.finish().unwrap();
        let out = output(&mut surface);
        assert!(out.ends_with("\r\n\x1b[0m\x1b[?25h"), "{out:?}");
    }

    #[test]
    fn the_cursor_shows_only_where_a_frame_puts_it() {
        let mut surface = Surface::new(Vec::new(), 10, 2);
        surface
            .draw(|s| {
                s.render(Rect::new(0, 0, 10, 1), &Text::new("> "));
                s.set_cursor(2, 0);
            })
            .unwrap();
        let out = output(&mut surface);
        assert!(out.ends_with("\x1b[1;3H\x1b[?25h"), "{out:?}");
        surface.draw(|_| {}).unwrap();
        assert_eq!(output(&mut surface), "\x1b[?25l");
    }

    #[test]
    fn a_synchronized_surface_wraps_each_frame() {
        let mut surface = Surface::new(Vec::new(), 10, 1).synchronized(true);
        surface
            .draw(|s| s.render(s.area(), &Text::new("x")))
            .unwrap();
        let out = output(&mut surface);
        assert!(out.starts_with(BEGIN_SYNCHRONIZED_UPDATE), "{out:?}");
        assert!(out.ends_with(END_SYNCHRONIZED_UPDATE), "{out:?}");
        surface.draw(|_| {}).unwrap();
        assert_eq!(output(&mut surface), "", "nothing to write, no wrapper");
    }

    #[test]
    fn controls_in_rendered_text_become_visible() {
        let mut surface = Surface::new(Vec::new(), 10, 1);
        surface
            .draw(|s| s.render(s.area(), &Text::new("a\x1b[2Jb")))
            .unwrap();
        assert_eq!(surface.screen().plain()[0], "a␛[2Jb    ");
    }

    /// A writer whose next `fails` writes fail.
    struct Flaky {
        fails: usize,
        out: Vec<u8>,
    }

    impl Write for Flaky {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            if self.fails > 0 {
                self.fails -= 1;
                return Err(io::Error::other("full"));
            }
            self.out.write(buf)
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn a_failed_finish_can_be_retried() {
        let mut surface = Surface::new(
            Flaky {
                fails: 0,
                out: Vec::new(),
            },
            4,
            1,
        );
        surface
            .draw(|s| s.render(s.area(), &Text::new("x")))
            .unwrap();
        surface.writer_mut().fails = 1;
        assert!(surface.finish().is_err());
        surface.finish().unwrap();
        let out = String::from_utf8(surface.writer().out.clone()).unwrap();
        assert!(out.ends_with("\x1b[?1049l"), "{out:?}");
    }

    /// A one-cell graphic on every `*`.
    #[derive(Debug)]
    struct Star;

    impl crate::frame::Graphic for Star {
        fn key(&self) -> u64 {
            1
        }
        fn draw(&self, _frame: usize) -> String {
            "<star>".into()
        }
    }

    /// One star at a column it is told, whatever the cells say.
    struct Moving(std::sync::Mutex<usize>);

    impl PlacementSource for Moving {
        fn placements(&self, _frame: &Frame) -> Vec<Placement> {
            vec![Placement {
                row: 0,
                column: *self.0.lock().unwrap(),
                cols: 1,
                rows: 1,
                graphic: Arc::new(Star),
                frame: 0,
            }]
        }
    }

    #[test]
    fn a_graphic_that_moves_off_unchanged_cells_leaves_no_ghost() {
        let source = Arc::new(Moving(std::sync::Mutex::new(1)));
        let mut surface = Surface::new(Vec::new(), 6, 1)
            .graphics(Some(source.clone() as Arc<dyn PlacementSource>));
        let draw = |s: &mut Drawing<'_>| s.render(s.area(), &Text::new("abcdef"));
        surface.draw(draw).unwrap();
        output(&mut surface);
        *source.0.lock().unwrap() = 3;
        // The row is drawn again with the same cells: none is written by the
        // diff, so the cell under the old star must be repainted on purpose.
        surface.draw(draw).unwrap();
        let out = output(&mut surface);
        assert_eq!(out, "\x1b[1;2H\x1b[0mb\x1b[1;4H<star>", "{out:?}");
    }

    #[test]
    fn dropping_a_started_surface_gives_the_terminal_back() {
        let mut sink = Vec::new();
        {
            let mut surface = Surface::new(&mut sink, 4, 1);
            surface
                .draw(|s| s.render(s.area(), &Text::new("x")))
                .unwrap();
        }
        let out = String::from_utf8(sink).unwrap();
        assert!(out.ends_with("\x1b[?1049l"), "{out:?}");
    }
}
