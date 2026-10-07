//! The cell screen: what the terminal shows, kept between frames.
//!
//! A [`Screen`] is a grid of [`Cell`]s, each a grapheme, its width and an
//! interned rich [`Style`] (so hyperlinks and every rich attribute survive,
//! which a ratatui cell cannot hold). Nodes write their rendered lines into
//! their own rectangle; the [`Painter`] compares only the damaged
//! rectangles with what it sent last time and writes the difference,
//! keeping colour and link state across the frame rather than resetting it
//! for every run.

use rich::{ColorSystem, Segment, Style};
use rich_ext::frame::{Frame, StyleId, StyleTable};

/// A rectangle of cells.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Rect {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

impl Rect {
    pub const fn new(x: u16, y: u16, width: u16, height: u16) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    pub fn right(&self) -> u16 {
        self.x.saturating_add(self.width)
    }

    pub fn bottom(&self) -> u16 {
        self.y.saturating_add(self.height)
    }

    pub fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }

    pub fn contains(&self, column: u16, row: u16) -> bool {
        column >= self.x && column < self.right() && row >= self.y && row < self.bottom()
    }

    /// The part of this rectangle inside `other`.
    pub fn intersection(&self, other: Rect) -> Rect {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let right = self.right().min(other.right());
        let bottom = self.bottom().min(other.bottom());
        Rect::new(x, y, right.saturating_sub(x), bottom.saturating_sub(y))
    }

    /// This rectangle without a border of `n` cells on each side.
    pub fn inner(&self, n: u16) -> Rect {
        Rect::new(
            self.x.saturating_add(n),
            self.y.saturating_add(n),
            self.width.saturating_sub(n * 2),
            self.height.saturating_sub(n * 2),
        )
    }
}

/// One cell: a grapheme and its style. A wide grapheme's second column is a
/// continuation cell (empty text, width 0).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cell {
    pub text: String,
    pub width: u8,
    pub style: StyleId,
}

impl Cell {
    fn blank() -> Cell {
        Cell {
            text: " ".into(),
            width: 1,
            style: StyleId::NONE,
        }
    }

    pub fn is_continuation(&self) -> bool {
        self.width == 0
    }

    fn set(&mut self, text: &str, width: u8, style: StyleId) {
        if self.text != text {
            self.text.clear();
            self.text.push_str(text);
        }
        self.width = width;
        self.style = style;
    }
}

/// The grid of cells, and the styles they use.
#[derive(Clone, Debug)]
pub struct Screen {
    width: u16,
    height: u16,
    cells: Vec<Cell>,
    styles: StyleTable,
}

impl Screen {
    pub fn new(width: u16, height: u16) -> Screen {
        Screen {
            width,
            height,
            cells: vec![Cell::blank(); width as usize * height as usize],
            styles: StyleTable::new(),
        }
    }

    pub fn area(&self) -> Rect {
        Rect::new(0, 0, self.width, self.height)
    }

    pub fn cell(&self, x: u16, y: u16) -> &Cell {
        &self.cells[y as usize * self.width as usize + x as usize]
    }

    fn cell_mut(&mut self, x: u16, y: u16) -> &mut Cell {
        let width = self.width as usize;
        &mut self.cells[y as usize * width + x as usize]
    }

    pub fn style(&self, id: StyleId) -> Option<&Style> {
        self.styles.get(id)
    }

    /// Intern `style` for use in this screen's cells.
    pub fn intern(&mut self, style: Option<&Style>) -> StyleId {
        self.styles.intern(style)
    }

    /// Clear `rect` to blank cells.
    pub fn clear(&mut self, rect: Rect) {
        let rect = rect.intersection(self.area());
        for y in rect.y..rect.bottom() {
            for x in rect.x..rect.right() {
                *self.cell_mut(x, y) = Cell::blank();
            }
        }
    }

    /// Write rendered `lines` into `rect`, replacing what was there: rows
    /// and columns the lines do not reach are cleared. Terminal controls in
    /// the text become visible symbols; a wide grapheme that would cross
    /// the rectangle's right edge becomes a space.
    pub fn write_lines(&mut self, rect: Rect, lines: &[Vec<Segment>]) {
        let rect = rect.intersection(self.area());
        if rect.is_empty() {
            return;
        }
        let mut segments = Vec::new();
        for line in lines.iter().take(rect.height as usize) {
            let start = segments.len();
            segments.extend(line.iter().cloned());
            rich_interact::paint::sanitize_line(&mut segments[start..]);
            segments.push(Segment::line());
        }
        let frame = Frame::from_segments(&segments);
        // The frame's style ids, translated into this screen's table once.
        let mut ids: Vec<Option<StyleId>> = vec![None; frame.styles().len() + 1];
        for row in 0..rect.height {
            let cells = if (row as usize) < frame.height() {
                frame.cells(row as usize)
            } else {
                Vec::new()
            };
            let y = rect.y + row;
            let mut x = rect.x;
            for cell in &cells {
                if x >= rect.right() {
                    break;
                }
                if cell.is_continuation() {
                    continue;
                }
                let id = *ids[cell.style.index()].get_or_insert_with(|| {
                    // A style that sets nothing is no style.
                    let style = frame.styles().get(cell.style).filter(|s| !s.is_null());
                    self.styles.intern(style)
                });
                if cell.width >= 2 && x + 1 >= rect.right() {
                    // No room for the second column.
                    self.cell_mut(x, y).set(" ", 1, id);
                    x += 1;
                    continue;
                }
                self.cell_mut(x, y).set(cell.text, cell.width.max(1), id);
                for extra in 1..cell.width {
                    self.cell_mut(x + extra as u16, y).set("", 0, id);
                }
                x += cell.width.max(1) as u16;
            }
            while x < rect.right() {
                *self.cell_mut(x, y) = Cell::blank();
                x += 1;
            }
        }
    }

    /// Lay `style` over the cell at `x`, `y` (a selection's highlight).
    pub fn restyle(&mut self, x: u16, y: u16, style: &Style) {
        if x >= self.width || y >= self.height {
            return;
        }
        let own = self.styles.get(self.cell(x, y).style).cloned();
        let combined = match own {
            Some(own) => own.combine(style),
            None => style.clone(),
        };
        let id = self.styles.intern(Some(&combined));
        self.cell_mut(x, y).style = id;
    }

    /// Copy the cells of `from` in `source` to this screen at `x`, `y`,
    /// clipped to this screen. A wide character cut by either edge of
    /// `from` becomes a space.
    pub fn blit(&mut self, source: &Screen, from: Rect, x: u16, y: u16) {
        let from = from.intersection(source.area());
        for row in 0..from.height {
            let ty = y.saturating_add(row);
            if ty >= self.height {
                break;
            }
            for column in 0..from.width {
                let tx = x.saturating_add(column);
                if tx >= self.width {
                    break;
                }
                let cell = source.cell(from.x + column, from.y + row);
                let cut = cell.is_continuation() && column == 0
                    || cell.width >= 2 && column + 1 >= from.width;
                let id = self.styles.intern(source.style(cell.style));
                if cut {
                    self.cell_mut(tx, ty).set(" ", 1, id);
                } else {
                    self.cell_mut(tx, ty).set(&cell.text, cell.width, id);
                }
            }
        }
    }

    /// Move the rows of `rect` up by `rows`, dropping the top ones; the
    /// rows that open at the bottom are cleared.
    pub fn scroll_up(&mut self, rect: Rect, rows: u16) {
        let rect = rect.intersection(self.area());
        if rect.is_empty() {
            return;
        }
        let rows = rows.min(rect.height);
        let width = self.width as usize;
        for y in rect.y..rect.bottom() - rows {
            let (from, to) = ((y + rows) as usize * width, y as usize * width);
            for x in rect.x as usize..rect.right() as usize {
                let cell = self.cells[from + x].clone();
                self.cells[to + x] = cell;
            }
        }
        self.clear(Rect::new(rect.x, rect.bottom() - rows, rect.width, rows));
    }

    /// Each row as plain text (continuation cells skipped).
    pub fn plain(&self) -> Vec<String> {
        (0..self.height)
            .map(|y| {
                (0..self.width)
                    .map(|x| self.cell(x, y))
                    .filter(|c| !c.is_continuation())
                    .map(|c| c.text.as_str())
                    .collect()
            })
            .collect()
    }
}

/// Turns screens into terminal output: the cells that changed since the
/// last paint, inside the damaged rectangles.
pub struct Painter {
    previous: Option<Screen>,
    system: Option<ColorSystem>,
    /// SGR parameters per style id of the screen being painted.
    codes: Vec<Option<String>>,
    /// Painting an inline region below the cursor, with relative moves,
    /// rather than the whole screen with absolute ones.
    inline: bool,
    /// Inline: where the terminal's cursor is, in the region; `None`
    /// before the region was made.
    cursor: Option<(u16, u16)>,
    /// Inline: the region's height.
    height: u16,
}

/// The pen's state while a frame is written.
#[derive(Default)]
struct Pen {
    at: Option<(u16, u16)>,
    style: Option<StyleId>,
    link: Option<String>,
    /// The SGR parameters in force; `None` before the first.
    codes: Option<String>,
}

impl Painter {
    pub fn new(system: Option<ColorSystem>) -> Painter {
        Painter {
            previous: None,
            system,
            codes: Vec::new(),
            inline: false,
            cursor: None,
            height: 0,
        }
    }

    /// Paint an inline region that starts on the cursor's row, moving the
    /// cursor relative to where it is, so the rows above (the shell's
    /// scrollback) are left alone.
    pub fn inline(mut self) -> Painter {
        self.inline = true;
        self
    }

    pub fn set_color_system(&mut self, system: Option<ColorSystem>) {
        if system != self.system {
            self.system = system;
            self.codes.clear();
        }
    }

    /// Move the cursor to `x`, `y` of the screen.
    pub fn move_to(&mut self, x: u16, y: u16) -> String {
        let out = self.goto(self.cursor, x, y);
        if self.inline {
            self.cursor = Some((x, y));
        }
        out
    }

    /// What to write when the app ends: inline, the cursor goes to the
    /// start of the line below the region, so what follows prints under
    /// the app's last frame.
    pub fn finish(&mut self) -> String {
        if !self.inline || self.cursor.is_none() {
            return String::new();
        }
        let out = self.move_to(0, self.height.saturating_sub(1));
        self.cursor = None;
        format!("{out}\x1b[0m\r\n")
    }

    fn goto(&self, from: Option<(u16, u16)>, x: u16, y: u16) -> String {
        if !self.inline {
            return format!("\x1b[{};{}H", y + 1, x + 1);
        }
        let (_, from_y) = from.unwrap_or((0, 0));
        let mut out = String::new();
        if y < from_y {
            out.push_str(&format!("\x1b[{}A", from_y - y));
        } else if y > from_y {
            out.push_str(&format!("\x1b[{}B", y - from_y));
        }
        // A carriage return also clears a pending wrap after the last
        // column, so the column is always counted from the left edge.
        out.push('\r');
        if x > 0 {
            out.push_str(&format!("\x1b[{x}C"));
        }
        out
    }

    /// Inline: go back to the region's first row (if it was made), and
    /// make room for `height` rows below the cursor, scrolling the
    /// terminal if the region would pass its bottom, and clear them.
    fn reserve(&mut self, height: u16) -> String {
        let mut out = String::from("\x1b[0m");
        if let Some((_, y)) = self.cursor {
            if y > 0 {
                out.push_str(&format!("\x1b[{y}A"));
            }
        }
        out.push('\r');
        if height > 1 {
            out.push_str(&"\n".repeat(height as usize - 1));
            out.push_str(&format!("\x1b[{}A", height - 1));
        }
        out.push_str("\x1b[J");
        self.cursor = Some((0, 0));
        self.height = height;
        out
    }

    /// Forget what was sent: the next paint writes everything.
    pub fn invalidate(&mut self) {
        self.previous = None;
    }

    /// What to write so the terminal shows `screen`, given that only the
    /// `damage` rectangles can have changed since the last paint (ignored
    /// on the first paint, or after a resize or [`invalidate`](Self::invalidate)).
    pub fn paint(&mut self, screen: &Screen, damage: &[Rect]) -> String {
        let mut out = String::new();
        let full = !matches!(&self.previous, Some(p) if p.width == screen.width && p.height == screen.height);
        let rects: Vec<Rect> = if full {
            if self.inline {
                out.push_str(&self.reserve(screen.height));
            } else {
                out.push_str("\x1b[0m\x1b[H\x1b[2J");
            }
            self.previous = None;
            self.codes.clear();
            vec![screen.area()]
        } else {
            damage
                .iter()
                .map(|r| r.intersection(screen.area()))
                .collect()
        };
        let mut pen = Pen {
            at: self.cursor,
            ..Pen::default()
        };
        for (y, spans) in row_spans(&rects, screen.height) {
            for (start, end) in spans {
                self.paint_span(screen, y, start, end, &mut pen, &mut out);
            }
        }
        if pen.link.is_some() {
            out.push_str("\x1b]8;;\x1b\\");
        }
        if pen.codes.as_deref().is_some_and(|c| !c.is_empty()) {
            out.push_str("\x1b[0m");
        }
        if self.inline {
            self.cursor = pen.at.or(self.cursor);
        }
        // Bring the copy of what was sent up to date.
        match &mut self.previous {
            None => self.previous = Some(screen.clone()),
            Some(previous) => {
                previous.styles = screen.styles.clone();
                for rect in &rects {
                    for y in rect.y..rect.bottom() {
                        for x in rect.x..rect.right() {
                            *previous.cell_mut(x, y) = screen.cell(x, y).clone();
                        }
                    }
                }
            }
        }
        out
    }

    fn changed(&self, screen: &Screen, x: u16, y: u16) -> bool {
        match &self.previous {
            None => true,
            Some(previous) => {
                // The previous copy shares the screen's (append-only) style
                // table, so style ids compare directly.
                previous.cell(x, y) != screen.cell(x, y)
            }
        }
    }

    fn paint_span(
        &mut self,
        screen: &Screen,
        y: u16,
        start: u16,
        end: u16,
        pen: &mut Pen,
        out: &mut String,
    ) {
        let mut x = start;
        // A span that starts on a wide grapheme's second column starts at
        // its first.
        if x > 0 && screen.cell(x, y).is_continuation() {
            x -= 1;
        }
        while x < end {
            if !self.changed(screen, x, y) {
                x += 1;
                continue;
            }
            let mut x0 = x;
            if screen.cell(x0, y).is_continuation() && x0 > 0 {
                x0 -= 1;
            }
            if pen.at != Some((x0, y)) {
                out.push_str(&self.goto(pen.at, x0, y));
            }
            let cell = screen.cell(x0, y);
            self.set_pen(screen, cell.style, pen, out);
            out.push_str(&cell.text);
            let next = x0 + cell.width.max(1) as u16;
            pen.at = Some((next, y));
            x = next;
        }
    }

    fn set_pen(&mut self, screen: &Screen, style: StyleId, pen: &mut Pen, out: &mut String) {
        if pen.style == Some(style) {
            return;
        }
        let resolved = screen.style(style);
        let link = resolved.and_then(|s| s.link()).map(str::to_string);
        if link != pen.link {
            match &link {
                Some(url) => out.push_str(&format!("\x1b]8;;{url}\x1b\\")),
                None => out.push_str("\x1b]8;;\x1b\\"),
            }
            pen.link = link;
        }
        if self.codes.len() <= style.index() {
            self.codes.resize(style.index() + 1, None);
        }
        let system = self.system;
        let codes = self.codes[style.index()]
            .get_or_insert_with(|| match (resolved, system) {
                (Some(s), Some(system)) => s.ansi_codes(system),
                _ => String::new(),
            })
            .clone();
        // Styles that differ only in their link share colours: no SGR.
        if pen.codes.as_deref() != Some(codes.as_str()) {
            out.push_str("\x1b[0");
            if !codes.is_empty() {
                out.push(';');
                out.push_str(&codes);
            }
            out.push('m');
            pen.codes = Some(codes);
        }
        pen.style = Some(style);
    }
}

/// For each row, the merged column spans the rectangles cover.
fn row_spans(rects: &[Rect], height: u16) -> Vec<(u16, Vec<(u16, u16)>)> {
    let mut rows: Vec<Vec<(u16, u16)>> = vec![Vec::new(); height as usize];
    for rect in rects {
        for y in rect.y..rect.bottom().min(height) {
            rows[y as usize].push((rect.x, rect.right()));
        }
    }
    rows.into_iter()
        .enumerate()
        .filter(|(_, spans)| !spans.is_empty())
        .map(|(y, mut spans)| {
            spans.sort_unstable();
            let mut merged: Vec<(u16, u16)> = Vec::new();
            for (a, b) in spans {
                match merged.last_mut() {
                    Some(last) if a <= last.1 => last.1 = last.1.max(b),
                    _ => merged.push((a, b)),
                }
            }
            (y as u16, merged)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(markup: &str) -> Vec<Segment> {
        let console = rich::Console::builder()
            .width(80)
            .color_system(None)
            .build();
        let text = rich::Text::from_markup(markup).unwrap();
        console
            .render_lines(&text, &console.options(), false)
            .remove(0)
    }

    #[test]
    fn lines_are_written_into_their_rectangle_only() {
        let mut screen = Screen::new(10, 3);
        screen.write_lines(
            Rect::new(0, 0, 10, 3),
            &[line("xxxxxxxxxx"), line("xxxxxxxxxx")],
        );
        screen.write_lines(Rect::new(2, 1, 4, 2), &[line("abcdefgh")]);
        assert_eq!(screen.plain(), ["xxxxxxxxxx", "xxabcdxxxx", "          "]);
    }

    #[test]
    fn wide_graphemes_take_two_cells_and_never_cross_the_edge() {
        let mut screen = Screen::new(5, 1);
        screen.write_lines(Rect::new(0, 0, 5, 1), &[line("日本語")]);
        assert_eq!(screen.plain(), ["日本 "]);
        assert!(screen.cell(1, 0).is_continuation());
        assert_eq!(screen.cell(4, 0).text, " ");
    }

    #[test]
    fn controls_in_text_become_visible() {
        let mut screen = Screen::new(6, 1);
        let evil = vec![Segment::new("a\x1b[2Jb", None)];
        screen.write_lines(Rect::new(0, 0, 6, 1), &[evil]);
        assert_eq!(screen.plain(), ["a␛[2Jb"]);
    }

    #[test]
    fn the_painter_writes_only_changed_cells_inside_the_damage() {
        let mut screen = Screen::new(20, 2);
        let mut painter = Painter::new(Some(ColorSystem::Truecolor));
        screen.write_lines(screen.area(), &[line("tick 1"), line("[bold red]alert[/]")]);
        let first = painter.paint(&screen, &[]);
        assert!(first.contains("\x1b[2J") && first.contains("alert"));
        screen.write_lines(Rect::new(0, 0, 20, 1), &[line("tick 2")]);
        let second = painter.paint(&screen, &[Rect::new(0, 0, 20, 1)]);
        // One cell changed: move there, write "2".
        assert_eq!(second, "\x1b[1;6H\x1b[0m2");
        // Nothing damaged, nothing written.
        assert_eq!(painter.paint(&screen, &[]), "");
    }

    #[test]
    fn styles_and_links_carry_across_a_run() {
        let mut screen = Screen::new(12, 1);
        let mut painter = Painter::new(Some(ColorSystem::Truecolor));
        painter.paint(&screen, &[]);
        screen.write_lines(
            screen.area(),
            &[line("[link=https://x.y]docs[/link] [b]ok[/b]")],
        );
        let out = painter.paint(&screen, &[screen.area()]);
        assert!(
            out.starts_with("\x1b[1;1H\x1b]8;;https://x.y\x1b\\"),
            "{out:?}"
        );
        // One SGR for the four link cells, not four.
        assert_eq!(out.matches("\x1b]8;;https://x.y").count(), 1, "{out:?}");
        assert!(
            out.contains("\x1b]8;;\x1b\\"),
            "the link is closed: {out:?}"
        );
        assert!(out.contains("\x1b[0;1mok"), "{out:?}");
    }

    #[test]
    fn a_wide_grapheme_replaced_by_narrow_text_clears_its_second_column() {
        let mut screen = Screen::new(4, 1);
        let mut painter = Painter::new(None);
        screen.write_lines(screen.area(), &[line("日x")]);
        painter.paint(&screen, &[]);
        screen.write_lines(screen.area(), &[line("abx")]);
        let out = painter.paint(&screen, &[screen.area()]);
        assert_eq!(out, "\x1b[1;1H\x1b[0mab");
    }
}
