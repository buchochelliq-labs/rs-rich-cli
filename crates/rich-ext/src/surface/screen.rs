//! The cell screen and its painter, which [`Surface`](super::Surface) draws
//! with. Moved here from `rs-rich-intuituive`, which re-exports them.
//!
//! A [`Screen`] is a grid of [`Cell`]s, each a grapheme, its width and an
//! interned rich [`Style`] (so hyperlinks and every rich attribute survive,
//! which a ratatui cell cannot hold). Callers write rendered lines into a
//! rectangle; the [`Painter`] compares only the damaged rectangles with
//! what it sent last time and writes the difference, keeping colour and
//! link state across the frame rather than resetting it for every run.

use std::collections::HashMap;

use rich::{ColorSystem, Segment, Style};

use crate::frame::{Frame, FxBuild, Rgb, StyleId, StyleRemap, StyleTable};

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
    #[must_use]
    pub fn intersection(&self, other: Rect) -> Rect {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let right = self.right().min(other.right());
        let bottom = self.bottom().min(other.bottom());
        Rect::new(x, y, right.saturating_sub(x), bottom.saturating_sub(y))
    }

    /// This rectangle without a border of `n` cells on each side.
    #[must_use]
    pub fn inner(&self, n: u16) -> Rect {
        Rect::new(
            self.x.saturating_add(n),
            self.y.saturating_add(n),
            self.width.saturating_sub(n * 2),
            self.height.saturating_sub(n * 2),
        )
    }

    /// This rectangle without `top`, `right`, `bottom` and `left` cells on
    /// those sides (CSS's order), never less than empty.
    ///
    /// ```
    /// use rich_ext::surface::Rect;
    ///
    /// assert_eq!(Rect::new(0, 0, 10, 5).inset(1, 2, 0, 3), Rect::new(3, 1, 5, 4));
    /// assert!(Rect::new(0, 0, 4, 4).inset(0, 3, 0, 3).is_empty());
    /// ```
    #[must_use]
    pub fn inset(&self, top: u16, right: u16, bottom: u16, left: u16) -> Rect {
        let width = self.width.saturating_sub(left.saturating_add(right));
        let height = self.height.saturating_sub(top.saturating_add(bottom));
        Rect::new(
            self.x.saturating_add(left.min(self.width)),
            self.y.saturating_add(top.min(self.height)),
            width,
            height,
        )
    }

    /// A rectangle `width` x `height` in the middle of this one, cut to it
    /// when it is larger (an odd cell left over goes after it): where a
    /// pop-up or a dialog goes.
    ///
    /// ```
    /// use rich_ext::surface::Rect;
    ///
    /// assert_eq!(Rect::new(0, 0, 20, 10).centered(8, 3), Rect::new(6, 3, 8, 3));
    /// assert_eq!(Rect::new(2, 2, 4, 4).centered(10, 2), Rect::new(2, 3, 4, 2));
    /// ```
    #[must_use]
    pub fn centered(&self, width: u16, height: u16) -> Rect {
        let (width, height) = (width.min(self.width), height.min(self.height));
        Rect::new(
            self.x + (self.width - width) / 2,
            self.y + (self.height - height) / 2,
            width,
            height,
        )
    }

    /// This rectangle moved `dx` columns right and `dy` rows down (left and
    /// up when negative), its size kept and its corner kept on the grid:
    /// at least 0, and no further than its size allows.
    ///
    /// ```
    /// use rich_ext::surface::Rect;
    ///
    /// assert_eq!(Rect::new(5, 5, 2, 2).offset(3, -2), Rect::new(8, 3, 2, 2));
    /// assert_eq!(Rect::new(1, 1, 2, 2).offset(-4, 0), Rect::new(0, 1, 2, 2));
    /// ```
    #[must_use]
    pub fn offset(&self, dx: i32, dy: i32) -> Rect {
        let moved = |at: u16, by: i32, size: u16| {
            (i32::from(at) + by).clamp(0, i32::from(u16::MAX - size)) as u16
        };
        Rect::new(
            moved(self.x, dx, self.width),
            moved(self.y, dy, self.height),
            self.width,
            self.height,
        )
    }

    /// The smallest rectangle holding both. An empty rectangle holds
    /// nothing, so it adds nothing.
    ///
    /// ```
    /// use rich_ext::surface::Rect;
    ///
    /// let a = Rect::new(0, 0, 2, 2);
    /// assert_eq!(a.union(Rect::new(5, 1, 1, 4)), Rect::new(0, 0, 6, 5));
    /// assert_eq!(a.union(Rect::default()), a);
    /// ```
    #[must_use]
    pub fn union(&self, other: Rect) -> Rect {
        if other.is_empty() {
            return *self;
        }
        if self.is_empty() {
            return other;
        }
        let (x, y) = (self.x.min(other.x), self.y.min(other.y));
        let right = self.right().max(other.right());
        let bottom = self.bottom().max(other.bottom());
        Rect::new(x, y, right - x, bottom - y)
    }

    /// Whether the two share a cell.
    ///
    /// ```
    /// use rich_ext::surface::Rect;
    ///
    /// let a = Rect::new(0, 0, 4, 4);
    /// assert!(a.intersects(Rect::new(3, 3, 2, 2)));
    /// assert!(!a.intersects(Rect::new(4, 0, 2, 2)), "touching is not sharing");
    /// ```
    pub fn intersects(&self, other: Rect) -> bool {
        !self.intersection(other).is_empty()
    }

    /// Each of its rows, top to bottom, as a rectangle one cell tall.
    ///
    /// ```
    /// use rich_ext::surface::Rect;
    ///
    /// let rows: Vec<Rect> = Rect::new(1, 2, 3, 2).rows().collect();
    /// assert_eq!(rows, [Rect::new(1, 2, 3, 1), Rect::new(1, 3, 3, 1)]);
    /// ```
    pub fn rows(self) -> impl DoubleEndedIterator<Item = Rect> + ExactSizeIterator {
        (self.y..self.bottom()).map(move |y| Rect::new(self.x, y, self.width, 1))
    }

    /// Each of its columns, left to right, as a rectangle one cell wide.
    ///
    /// ```
    /// use rich_ext::surface::Rect;
    ///
    /// let columns: Vec<Rect> = Rect::new(1, 2, 2, 3).columns().collect();
    /// assert_eq!(columns, [Rect::new(1, 2, 1, 3), Rect::new(2, 2, 1, 3)]);
    /// ```
    pub fn columns(self) -> impl DoubleEndedIterator<Item = Rect> + ExactSizeIterator {
        (self.x..self.right()).map(move |x| Rect::new(x, self.y, 1, self.height))
    }
}

/// One cell: a grapheme and its style. A wide grapheme's second column is a
/// continuation cell (empty text, width 0).
///
/// A cell is small and `Copy`, with no heap string: a grapheme of up to
/// four bytes (every single character, and short clusters such as `e` and
/// a combining accent) is kept inline, and a longer one (an emoji joined
/// with ZWJs) as an index into a table of every such grapheme seen, which
/// is shared by all screens and only grows. Comparing, copying and
/// scrolling cells moves plain data, as a game keeps its hot data flat.
///
/// ```
/// use rich_ext::frame::StyleId;
/// use rich_ext::surface::Cell;
///
/// let a = Cell::new("é", 1, StyleId::NONE);
/// let family = Cell::new("👨\u{200d}👩\u{200d}👧", 2, StyleId::NONE);
/// assert_eq!((a.text(), family.text()), ("é", "👨\u{200d}👩\u{200d}👧"));
/// assert_eq!(family, Cell::new("👨\u{200d}👩\u{200d}👧", 2, StyleId::NONE));
/// assert_eq!(std::mem::size_of::<Cell>(), 12);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Cell {
    /// The grapheme's UTF-8, or a long grapheme's index (little-endian).
    bytes: [u8; 4],
    /// How many of `bytes` are the text, or [`LONG`] for an index.
    len: u8,
    pub width: u8,
    pub style: StyleId,
}

/// [`Cell::len`] for a grapheme kept in the shared table.
const LONG: u8 = u8::MAX;

/// The graphemes longer than four bytes, each once, for every screen: what
/// a cell's index names. Leaked, so a cell's text is borrowed for as long
/// as the program runs.
struct Graphemes {
    texts: Vec<&'static str>,
    index: HashMap<&'static str, u32>,
}

fn graphemes() -> &'static std::sync::RwLock<Graphemes> {
    static TABLE: std::sync::OnceLock<std::sync::RwLock<Graphemes>> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| {
        std::sync::RwLock::new(Graphemes {
            texts: Vec::new(),
            index: HashMap::new(),
        })
    })
}

impl Cell {
    /// A cell showing `text` (one grapheme), `width` columns wide (0 for a
    /// continuation), in `style`.
    pub fn new(text: &str, width: u8, style: StyleId) -> Cell {
        let mut bytes = [0u8; 4];
        let len = if text.len() <= 4 {
            bytes[..text.len()].copy_from_slice(text.as_bytes());
            text.len() as u8
        } else {
            let known = graphemes()
                .read()
                .unwrap_or_else(|e| e.into_inner())
                .index
                .get(text)
                .copied();
            let at = known.unwrap_or_else(|| {
                let mut table = graphemes().write().unwrap_or_else(|e| e.into_inner());
                if let Some(&at) = table.index.get(text) {
                    return at;
                }
                let at = table.texts.len() as u32;
                let text: &'static str = Box::leak(text.to_string().into_boxed_str());
                table.texts.push(text);
                table.index.insert(text, at);
                at
            });
            bytes = at.to_le_bytes();
            LONG
        };
        Cell {
            bytes,
            len,
            width,
            style,
        }
    }

    /// A single character, `width` columns wide.
    #[inline]
    fn char(c: char, width: u8, style: StyleId) -> Cell {
        let mut bytes = [0u8; 4];
        let len = c.encode_utf8(&mut bytes).len() as u8;
        Cell {
            bytes,
            len,
            width,
            style,
        }
    }

    fn blank() -> Cell {
        Cell::char(' ', 1, StyleId::NONE)
    }

    /// The grapheme the cell shows; empty for a continuation.
    pub fn text(&self) -> &str {
        if self.len == LONG {
            let at = u32::from_le_bytes(self.bytes) as usize;
            let table = graphemes().read().unwrap_or_else(|e| e.into_inner());
            return table.texts[at];
        }
        // Written from a `&str` or a `char`, so the bytes are UTF-8.
        std::str::from_utf8(&self.bytes[..self.len as usize]).unwrap_or(" ")
    }

    pub fn is_continuation(&self) -> bool {
        self.width == 0
    }

    #[inline]
    fn set(&mut self, text: &str, width: u8, style: StyleId) {
        *self = Cell::new(text, width, style);
    }

    /// The same grapheme, `width` wide, in `style`.
    #[inline]
    fn restyled(self, width: u8, style: StyleId) -> Cell {
        Cell {
            width,
            style,
            ..self
        }
    }
}

/// The grid of cells, and the styles they use.
///
/// The style table only grows as cells are written; a screen kept between
/// frames drops the styles no cell uses any more with
/// [`compact_styles`](Self::compact_styles) (a [`Painter`] does it for the
/// screen it paints with [`Painter::compact`]), so a picture whose colours
/// change every frame does not pile them up.
#[derive(Clone, Debug)]
pub struct Screen {
    width: u16,
    height: u16,
    cells: Vec<Cell>,
    styles: StyleTable,
    /// The style last laid under or over cells, and its id.
    layer: Option<(Style, StyleId)>,
    /// What laying a style under or over a cell's made: by (layer, own,
    /// over), the combined style's id.
    layered: HashMap<(u32, u32, bool), StyleId, FxBuild>,
}

impl Screen {
    pub fn new(width: u16, height: u16) -> Screen {
        Screen {
            width,
            height,
            cells: vec![Cell::blank(); width as usize * height as usize],
            styles: StyleTable::new(),
            layer: None,
            layered: HashMap::default(),
        }
    }

    pub fn area(&self) -> Rect {
        Rect::new(0, 0, self.width, self.height)
    }

    pub fn cell(&self, x: u16, y: u16) -> &Cell {
        &self.cells[y as usize * self.width as usize + x as usize]
    }

    /// The cell at `x`, `y`, if it is on the screen; its style is
    /// [`style`](Self::style) of its id.
    ///
    /// ```
    /// use rich::{Segment, Style};
    /// use rich_ext::surface::{Rect, Screen};
    ///
    /// let mut screen = Screen::new(4, 1);
    /// let bold = Style::parse("bold").unwrap();
    /// screen.write_lines(Rect::new(0, 0, 4, 1), &[vec![Segment::new("ab", Some(bold.clone()))]]);
    /// let cell = screen.get(1, 0).unwrap();
    /// assert_eq!((cell.text(), screen.style(cell.style)), ("b", Some(&bold)));
    /// assert!(screen.get(4, 0).is_none());
    /// ```
    pub fn get(&self, x: u16, y: u16) -> Option<&Cell> {
        (x < self.width && y < self.height).then(|| self.cell(x, y))
    }

    fn cell_mut(&mut self, x: u16, y: u16) -> &mut Cell {
        let width = self.width as usize;
        &mut self.cells[y as usize * width + x as usize]
    }

    pub fn style(&self, id: StyleId) -> Option<&Style> {
        self.styles.get(id)
    }

    /// How many styles the screen's table holds, counting "no style":
    /// what its cells use, and what they used before the last
    /// [`compact_styles`](Self::compact_styles).
    pub fn style_count(&self) -> usize {
        self.styles.len()
    }

    /// Intern `style` for use in this screen's cells.
    pub fn intern(&mut self, style: Option<&Style>) -> StyleId {
        self.styles.intern(style)
    }

    /// Whether the style table has grown well past what the cells used
    /// when it was last compacted: time for
    /// [`compact_styles`](Self::compact_styles).
    pub fn styles_piled_up(&self) -> bool {
        self.styles.piled_up()
    }

    /// Drop the styles no cell uses, and renumber the rest. The result maps
    /// the old ids to the new, for whatever kept an id of this screen's
    /// between frames: a [`Painter`] that painted it is moved over with
    /// [`Painter::remap_styles`] (or compacts it itself with
    /// [`Painter::compact`]). The cells keep their styles; only the ids
    /// change.
    ///
    /// ```
    /// use rich::{Color, Style};
    /// use rich_ext::surface::Screen;
    ///
    /// let mut screen = Screen::new(2, 1);
    /// for n in 0..=255u8 {
    ///     screen.put_rgb(0, 0, '▀', Some([n, 0, 0]), Some([0, 0, n]));
    /// }
    /// assert_eq!(screen.style_count(), 257);
    /// screen.compact_styles();
    /// // "No style" (the blank cell) and the last colour pair.
    /// assert_eq!(screen.style_count(), 2);
    /// let cell = screen.cell(0, 0);
    /// let last = Style::from_color(Some(Color::from_rgb(255, 0, 0)), Some(Color::from_rgb(0, 0, 255)));
    /// assert_eq!(screen.style(cell.style), Some(&last));
    /// ```
    pub fn compact_styles(&mut self) -> StyleRemap {
        let mut keep = vec![false; self.styles.len()];
        for cell in &self.cells {
            if let Some(used) = keep.get_mut(cell.style.index()) {
                *used = true;
            }
        }
        let remap = self.styles.retain(&keep);
        for cell in &mut self.cells {
            cell.style = remap.get_or_gone(cell.style);
        }
        self.layer = None;
        self.layered.clear();
        remap
    }

    /// Set the cell at `x`, `y` to `ch` with foreground `fg` and background
    /// `bg` in true colour (either missing), without building a
    /// [`Segment`] or, unless the pair is new, a [`Style`]: the fast way to
    /// draw a picture a cell at a time (half blocks, quadrants, Braille).
    /// The cell takes the style [`Style::from_color`] of the two
    /// [`Color::from_rgb`](rich::Color::from_rgb)s would give. Off the
    /// screen, nothing is written.
    ///
    /// `ch` takes one cell: a wide or zero-width character becomes a space,
    /// and a terminal control its visible stand-in (see
    /// [`control_picture`](super::control_picture)). A wide grapheme the
    /// cell was half of becomes a space.
    ///
    /// ```
    /// use rich::{Color, Style};
    /// use rich_ext::surface::Screen;
    ///
    /// let mut screen = Screen::new(2, 1);
    /// screen.put_rgb(1, 0, '▀', Some([255, 0, 0]), Some([0, 0, 255]));
    /// let cell = screen.cell(1, 0);
    /// let style = Style::from_color(
    ///     Some(Color::from_rgb(255, 0, 0)),
    ///     Some(Color::from_rgb(0, 0, 255)),
    /// );
    /// assert_eq!((cell.text(), screen.style(cell.style)), ("▀", Some(&style)));
    /// ```
    pub fn put_rgb(&mut self, x: u16, y: u16, ch: char, fg: Option<Rgb>, bg: Option<Rgb>) {
        if x >= self.width || y >= self.height {
            return;
        }
        let id = self.styles.intern_rgb(fg, bg);
        self.put_cell(x, y, ch, id);
    }

    /// Set the cell at `x`, `y` to `ch` in the style `style` (an id from
    /// [`intern`](Self::intern) on this screen), as
    /// [`put_rgb`](Self::put_rgb) does: for many cells in one style,
    /// interned once.
    ///
    /// ```
    /// use rich::Style;
    /// use rich_ext::surface::Screen;
    ///
    /// let mut screen = Screen::new(3, 1);
    /// let bold = screen.intern(Some(&Style::parse("bold").unwrap()));
    /// for x in 0..3 {
    ///     screen.put(x, 0, '#', bold);
    /// }
    /// assert_eq!(screen.plain(), ["###"]);
    /// ```
    pub fn put(&mut self, x: u16, y: u16, ch: char, style: StyleId) {
        if x >= self.width
            || y >= self.height
            || self.styles.get(style).is_none() && style != StyleId::NONE
        {
            return;
        }
        self.put_cell(x, y, ch, style);
    }

    #[inline]
    fn put_cell(&mut self, x: u16, y: u16, ch: char, style: StyleId) {
        let ch = narrow(ch);
        let at = y as usize * self.width as usize + x as usize;
        let width = self.cells[at].width;
        if width == 0 && x > 0 {
            // The second column of a wide grapheme: its first is erased too.
            let lead = &mut self.cells[at - 1];
            *lead = Cell::char(' ', 1, lead.style);
        } else if width >= 2 && x + 1 < self.width {
            let next = &mut self.cells[at + 1];
            *next = Cell::char(' ', 1, next.style);
        }
        self.cells[at] = Cell::char(ch, 1, style);
    }

    /// A wide grapheme that `rect` cuts in half, on either side, becomes a
    /// space: the terminal erases a wide grapheme when either column is
    /// written, so the half outside `rect` must not stay in the model.
    fn split_edges(&mut self, rect: Rect) {
        let rect = rect.intersection(self.area());
        if rect.is_empty() {
            return;
        }
        for y in rect.y..rect.bottom() {
            if rect.x > 0 && self.cell(rect.x, y).is_continuation() {
                let style = self.cell(rect.x - 1, y).style;
                self.cell_mut(rect.x - 1, y).set(" ", 1, style);
            }
            if rect.right() < self.width && self.cell(rect.right(), y).is_continuation() {
                let style = self.cell(rect.right(), y).style;
                self.cell_mut(rect.right(), y).set(" ", 1, style);
            }
        }
    }

    /// Clear `rect` to blank cells.
    pub fn clear(&mut self, rect: Rect) {
        let rect = rect.intersection(self.area());
        self.split_edges(rect);
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
        self.split_edges(rect);
        if !self.write_simple_lines(rect, lines) {
            self.write_frame_lines(rect, lines);
        }
    }

    /// [`write_lines`](Self::write_lines) through a [`Frame`], for any
    /// text: graphemes of several characters, controls, region tags.
    fn write_frame_lines(&mut self, rect: Rect, lines: &[Vec<Segment>]) {
        let mut segments = Vec::new();
        for line in lines.iter().take(rect.height as usize) {
            let start = segments.len();
            segments.extend(line.iter().cloned());
            super::sanitize_line(&mut segments[start..]);
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

    /// [`write_lines`](Self::write_lines) without building a [`Frame`]:
    /// segments go straight into cells, a character a cell where every
    /// character is its own grapheme (most text, every picture), and
    /// through rich's grapheme splitting where it is not (combining marks,
    /// joined emoji). It keeps the Frame path's rules: a zero-width
    /// grapheme joins the cell before it, even across segments, or the
    /// row's first cell when nothing precedes it. Lines with a line break
    /// inside a segment, or a region tag, are left to the Frame path:
    /// whether it wrote them; when it did not, nothing was changed.
    fn write_simple_lines(&mut self, rect: Rect, lines: &[Vec<Segment>]) -> bool {
        let lines = &lines[..lines.len().min(rect.height as usize)];
        let direct = lines.iter().flatten().all(|segment| {
            segment.control
                || segment
                    .style
                    .as_ref()
                    .is_none_or(|s| s.meta_ref().is_none())
                    && !segment.text.contains('\n')
        });
        if !direct {
            return false;
        }
        let mut spans: Vec<(usize, usize, usize)> = Vec::new();
        for row in 0..rect.height {
            let y = rect.y + row;
            let mut x = rect.x;
            // What a zero-width grapheme joins: the cell last written, or,
            // before any, the row's first (`lead` gathers them); none once
            // the row is cut at the edge.
            let mut last: Option<u16> = None;
            let mut lead = String::new();
            let mut cut = false;
            for segment in lines.get(row as usize).into_iter().flatten() {
                if segment.control {
                    continue;
                }
                // A style that sets nothing is no style.
                let style = segment.style.as_ref().filter(|s| !s.is_null());
                let id = self.styles.intern(style);
                // One look at the text: ASCII printables pass at once.
                let (mut simple, mut controls) = (true, false);
                for c in segment.text.chars() {
                    if (' '..='~').contains(&c) {
                        continue;
                    }
                    if super::control_picture(c).is_some() {
                        controls = true;
                    } else if !matches!(char_width(c), 1 | 2) || c == '\u{200d}' || c == '\u{fe0f}'
                    {
                        simple = false;
                    }
                }
                if simple && !controls {
                    // A character a grapheme: straight into cells.
                    let mut chars = segment.text.chars();
                    if !lead.is_empty() && !cut {
                        if let Some(c) = chars.next() {
                            lead.push(c);
                            let piece = std::mem::take(&mut lead);
                            self.put_grapheme(
                                rect,
                                &mut x,
                                y,
                                &piece,
                                char_width(c),
                                id,
                                &mut last,
                                &mut cut,
                            );
                        }
                    }
                    for c in chars {
                        if cut {
                            break;
                        }
                        if x >= rect.right() {
                            cut = true;
                            break;
                        }
                        let width = char_width(c) as u16;
                        if width == 2 && x + 1 >= rect.right() {
                            self.cell_mut(x, y).set(" ", 1, id);
                            x += 1;
                            cut = true;
                            continue;
                        }
                        *self.cell_mut(x, y) = Cell::char(c, width as u8, id);
                        if width == 2 {
                            self.cell_mut(x + 1, y).set("", 0, id);
                        }
                        last = Some(x);
                        x += width;
                    }
                    continue;
                }
                let sanitized;
                let text = if controls {
                    sanitized = segment
                        .text
                        .chars()
                        .map(|c| super::control_picture(c).unwrap_or(c))
                        .collect::<String>();
                    sanitized.as_str()
                } else {
                    segment.text.as_str()
                };
                spans.clear();
                spans.extend(rich::cells::split_graphemes(text).0);
                for &(from, to, width) in &spans {
                    let piece = &text[from..to];
                    if width == 0 {
                        if cut {
                            continue;
                        }
                        match last {
                            Some(at) => {
                                let cell = *self.cell(at, y);
                                let joined = format!("{}{piece}", cell.text());
                                *self.cell_mut(at, y) = Cell::new(&joined, cell.width, cell.style);
                            }
                            None => lead.push_str(piece),
                        }
                        continue;
                    }
                    if !lead.is_empty() && !cut {
                        let piece = std::mem::take(&mut lead) + piece;
                        self.put_grapheme(rect, &mut x, y, &piece, width, id, &mut last, &mut cut);
                    } else {
                        self.put_grapheme(rect, &mut x, y, piece, width, id, &mut last, &mut cut);
                    }
                }
            }
            while x < rect.right() {
                *self.cell_mut(x, y) = Cell::blank();
                x += 1;
            }
        }
        true
    }

    /// One grapheme `width` cells wide at `x` of row `y`, for
    /// [`write_simple_lines`](Self::write_simple_lines): cut to a space at
    /// the rectangle's edge, nothing past it.
    #[allow(clippy::too_many_arguments)]
    fn put_grapheme(
        &mut self,
        rect: Rect,
        x: &mut u16,
        y: u16,
        piece: &str,
        width: usize,
        id: StyleId,
        last: &mut Option<u16>,
        cut: &mut bool,
    ) {
        if *cut {
            return;
        }
        if *x >= rect.right() {
            *cut = true;
            return;
        }
        let width = width as u16;
        if width >= 2 && *x + 1 >= rect.right() {
            // No room for the second column.
            self.cell_mut(*x, y).set(" ", 1, id);
            *x += 1;
            *cut = true;
            return;
        }
        *self.cell_mut(*x, y) = Cell::new(piece, width as u8, id);
        for extra in 1..width {
            self.cell_mut(*x + extra, y).set("", 0, id);
        }
        *last = Some(*x);
        *x += width;
    }

    /// Lay `style` under the cell at `x`, `y`: it takes what the cell's own
    /// style leaves unset (a stylesheet's colours, as CSS inherits them).
    pub fn underlay(&mut self, x: u16, y: u16, style: &Style) {
        self.lay(x, y, style, false);
    }

    /// Lay `style` over the cell at `x`, `y` (a selection's highlight).
    pub fn restyle(&mut self, x: u16, y: u16, style: &Style) {
        self.lay(x, y, style, true);
    }

    /// Combine `style` with the cell's own, under it or over it. A style is
    /// usually laid over many cells of a few styles, so each combination is
    /// made and interned once.
    fn lay(&mut self, x: u16, y: u16, style: &Style, over: bool) {
        if x >= self.width || y >= self.height {
            return;
        }
        let layer = match &self.layer {
            Some((last, id)) if last == style => *id,
            _ => {
                let id = self.styles.intern(Some(style));
                self.layer = Some((style.clone(), id));
                id
            }
        };
        let own = self.cell(x, y).style;
        let key = (layer.index() as u32, own.index() as u32, over);
        let id = match self.layered.get(&key) {
            Some(&id) => id,
            None => {
                let combined = match self.styles.get(own) {
                    Some(own) if over => own.combine(style),
                    Some(own) => style.combine(own),
                    None => style.clone(),
                };
                let id = self.styles.intern(Some(&combined));
                self.layered.insert(key, id);
                id
            }
        };
        self.cell_mut(x, y).style = id;
    }

    /// Copy the cells of `from` in `source` to this screen at `x`, `y`,
    /// clipped to this screen. A wide character cut by either edge of
    /// `from`, or by this screen's right edge, becomes a space.
    pub fn blit(&mut self, source: &Screen, from: Rect, x: u16, y: u16) {
        let from = from.intersection(source.area());
        self.split_edges(Rect::new(x, y, from.width, from.height));
        // The source's style ids, translated into this screen's table once.
        let mut ids: Vec<Option<StyleId>> = vec![None; source.styles.len()];
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
                let cell = *source.cell(from.x + column, from.y + row);
                let cut = cell.is_continuation() && column == 0
                    || cell.width >= 2 && (column + 1 >= from.width || tx + 1 >= self.width);
                let styles = &mut self.styles;
                let id = match ids.get_mut(cell.style.index()) {
                    Some(slot) => {
                        *slot.get_or_insert_with(|| styles.intern(source.style(cell.style)))
                    }
                    None => styles.intern(source.style(cell.style)),
                };
                if cut {
                    self.cell_mut(tx, ty).set(" ", 1, id);
                } else {
                    *self.cell_mut(tx, ty) = cell.restyled(cell.width, id);
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
        self.split_edges(rect);
        let width = self.width as usize;
        for y in rect.y..rect.bottom() - rows {
            let (from, to) = ((y + rows) as usize * width, y as usize * width);
            let (a, b) = (from + rect.x as usize, from + rect.right() as usize);
            self.cells.copy_within(a..b, to + rect.x as usize);
        }
        self.clear(Rect::new(rect.x, rect.bottom() - rows, rect.width, rows));
    }

    /// Each row as rendered segments, a run of cells in one style per
    /// segment: what any rich renderer takes, so a frame can be drawn
    /// somewhere else (into a ratatui buffer with `rs-rich-ratatui`'s
    /// `lines_to_buffer`, for one).
    pub fn lines(&self) -> Vec<Vec<Segment>> {
        (0..self.height)
            .map(|y| {
                let mut line: Vec<Segment> = Vec::new();
                let mut run = String::new();
                let mut style = None;
                for x in 0..self.width {
                    let cell = self.cell(x, y);
                    if cell.is_continuation() {
                        continue;
                    }
                    if style != Some(cell.style) && !run.is_empty() {
                        let id = style.expect("a run has a style");
                        line.push(Segment::new(
                            std::mem::take(&mut run),
                            self.style(id).cloned(),
                        ));
                    }
                    style = Some(cell.style);
                    run.push_str(cell.text());
                }
                if let Some(id) = style.filter(|_| !run.is_empty()) {
                    line.push(Segment::new(run, self.style(id).cloned()));
                }
                line
            })
            .collect()
    }

    /// Each row as plain text (continuation cells skipped).
    pub fn plain(&self) -> Vec<String> {
        (0..self.height)
            .map(|y| {
                (0..self.width)
                    .map(|x| self.cell(x, y))
                    .filter(|c| !c.is_continuation())
                    .map(|c| c.text())
                    .collect()
            })
            .collect()
    }
}

/// The cells `c` takes, as rich measures it, with the characters pictures
/// and most text are made of answered without a table lookup.
#[inline]
fn char_width(c: char) -> usize {
    if (' '..='~').contains(&c)
        || ('\u{2500}'..='\u{259f}').contains(&c)
        || ('\u{2800}'..='\u{28ff}').contains(&c)
    {
        1
    } else {
        rich::cells::char_cell_width(c)
    }
}

/// One cell wide: `ch`, or a space for a wide or zero-width character, or
/// a control's visible stand-in.
#[inline]
fn narrow(ch: char) -> char {
    // ASCII, box drawing and block elements, Braille: the pictures' cells.
    if (' '..='~').contains(&ch)
        || ('\u{2500}'..='\u{259f}').contains(&ch)
        || ('\u{2800}'..='\u{28ff}').contains(&ch)
    {
        return ch;
    }
    if let Some(picture) = super::control_picture(ch) {
        return picture;
    }
    let mut buffer = [0u8; 4];
    if rich::cells::cell_len(ch.encode_utf8(&mut buffer)) == 1 {
        ch
    } else {
        ' '
    }
}

/// What a [`Painter`] last sent: the cells, by the style ids of the screen
/// it painted (whose table it does not copy).
struct Sent {
    width: u16,
    height: u16,
    cells: Vec<Cell>,
}

impl Sent {
    fn cell(&self, x: u16, y: u16) -> &Cell {
        &self.cells[y as usize * self.width as usize + x as usize]
    }
}

/// How a [`Painter`] turns a frame's changes into bytes.
///
/// Every strategy draws the same screen; they differ in the bytes they
/// send and the time they take to find them. The benchmark in
/// rs-rich-intuituive's `tests/paint_bench.rs` measures each on a set of
/// scenes, and `docs/design/render-performance.md` keeps its table.
///
/// ```
/// use rich_ext::surface::PaintStrategy;
///
/// assert_eq!("best".parse::<PaintStrategy>(), Ok(PaintStrategy::Best));
/// assert_eq!(PaintStrategy::default(), PaintStrategy::Cells);
/// assert_eq!(PaintStrategy::Scroll.name(), "scroll");
/// assert!("fastest".parse::<PaintStrategy>().is_err());
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PaintStrategy {
    /// Compare each cell inside the damage with what was sent, and write
    /// the cells that changed. The default.
    #[default]
    Cells,
    /// Compare whole rows (by hash, then by their cells), and rewrite each
    /// changed row from its first change to its last.
    Rows,
    /// Write everything: each row in turn, the rest of the screen erased.
    Full,
    /// Find runs of rows that moved together (a log's new line, a list
    /// scrolled), move them with the terminal's own scrolling, then write
    /// the cells that still differ. Off where scrolling is not safe (a
    /// Windows console, an inline region, an image in the moved rows).
    Scroll,
    /// Each frame, encode the strategies that apply and send the shortest,
    /// within a bound so choosing never costs more than it saves.
    Best,
}

impl PaintStrategy {
    /// Every strategy, in the order of their names.
    pub const ALL: [PaintStrategy; 5] = [
        PaintStrategy::Cells,
        PaintStrategy::Rows,
        PaintStrategy::Full,
        PaintStrategy::Scroll,
        PaintStrategy::Best,
    ];

    /// The strategy's name: `cells`, `rows`, `full`, `scroll` or `best`.
    pub fn name(self) -> &'static str {
        match self {
            PaintStrategy::Cells => "cells",
            PaintStrategy::Rows => "rows",
            PaintStrategy::Full => "full",
            PaintStrategy::Scroll => "scroll",
            PaintStrategy::Best => "best",
        }
    }

    /// The strategy `RICH_PAINT_STRATEGY` names, if it is set to one.
    pub fn from_env() -> Option<PaintStrategy> {
        std::env::var("RICH_PAINT_STRATEGY")
            .ok()?
            .trim()
            .parse()
            .ok()
    }
}

impl std::fmt::Display for PaintStrategy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

impl std::str::FromStr for PaintStrategy {
    type Err = String;

    /// A strategy by its [name](PaintStrategy::name), in any case.
    fn from_str(name: &str) -> Result<PaintStrategy, String> {
        PaintStrategy::ALL
            .into_iter()
            .find(|s| s.name().eq_ignore_ascii_case(name))
            .ok_or_else(|| format!("no paint strategy {name:?}: cells, rows, full, scroll or best"))
    }
}

/// What the last [`Painter::paint`] did, counted as notcurses counts a
/// render: the cells it looked at and wrote, and its bytes by kind. The
/// bytes add up to the length of what it returned.
///
/// ```
/// use rich::{ColorSystem, Segment};
/// use rich_ext::surface::{Painter, Rect, Screen};
///
/// let mut screen = Screen::new(10, 1);
/// let mut painter = Painter::new(Some(ColorSystem::Truecolor));
/// painter.paint(&screen, &[]);
/// screen.write_lines(screen.area(), &[vec![Segment::new("hi", None)]]);
/// let out = painter.paint(&screen, &[Rect::new(0, 0, 4, 1)]);
/// let stats = painter.stats();
/// // The damage, and a column to its right: two cells changed.
/// assert_eq!((stats.compared, stats.written, stats.elided), (5, 2, 3));
/// assert_eq!(stats.text_bytes, 2);
/// assert_eq!(stats.bytes(), out.len());
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PaintStats {
    /// Cells compared with what was sent (every cell, on a full repaint).
    pub compared: usize,
    /// Cells written.
    pub written: usize,
    /// Cells compared and found unchanged, so not written.
    pub elided: usize,
    /// Bytes of cursor moves.
    pub move_bytes: usize,
    /// Bytes of SGR styles and hyperlinks.
    pub style_bytes: usize,
    /// Bytes of the cells' text.
    pub text_bytes: usize,
    /// Everything else: clearing the screen, erasing, scrolling.
    pub other_bytes: usize,
    /// The strategy the paint used (never [`PaintStrategy::Best`], which
    /// picks one of the others).
    pub strategy: PaintStrategy,
}

impl PaintStats {
    /// All the bytes the paint wrote.
    pub fn bytes(&self) -> usize {
        self.move_bytes + self.style_bytes + self.text_bytes + self.other_bytes
    }

    /// Add `other`'s counts to these.
    pub fn add(&mut self, other: &PaintStats) {
        self.compared += other.compared;
        self.written += other.written;
        self.elided += other.elided;
        self.move_bytes += other.move_bytes;
        self.style_bytes += other.style_bytes;
        self.text_bytes += other.text_bytes;
        self.other_bytes += other.other_bytes;
        self.strategy = other.strategy;
    }
}

/// Turns screens into terminal output: the cells that changed since the
/// last paint, inside the damaged rectangles.
///
/// It keeps a copy of the cells it sent, not of the style table: cells
/// compare by style id, so a screen's ids must not change under it, except
/// through [`compact`](Self::compact) or [`remap_styles`](Self::remap_styles).
pub struct Painter {
    previous: Option<Sent>,
    system: Option<ColorSystem>,
    /// SGR parameters per style id of the screen being painted.
    codes: Vec<Option<Sgr>>,
    /// The SGR parameters of the style being set, when they are not cached.
    scratch: Sgr,
    /// Painting an inline region below the cursor, with relative moves,
    /// rather than the whole screen with absolute ones.
    inline: bool,
    /// Inline: where the terminal's cursor is, in the region; `None`
    /// before the region was made.
    cursor: Option<(u16, u16)>,
    /// Inline: the region's height.
    height: u16,
    /// Write every cell inside the damage, changed or not.
    force: bool,
    /// The cells the last paint wrote, one rectangle per run of a row.
    written: Vec<Rect>,
    /// The last paint's counts.
    stats: PaintStats,
    /// How frames become bytes.
    strategy: PaintStrategy,
    /// Runs of blank cells may be erased (ECH, EL) rather than written.
    erase: bool,
}

/// The pen's state while a frame is written.
#[derive(Default)]
struct Pen {
    at: Option<(u16, u16)>,
    style: Option<StyleId>,
    link: Option<String>,
    /// The SGR parameters in force; `None` before the first.
    codes: Option<Sgr>,
    /// The true-colour pair in force, when the last style set was one:
    /// then it, not `codes`, says what the pen holds.
    rgb: Option<(Option<Rgb>, Option<Rgb>)>,
}

/// A style's SGR parameters, split so a change of colour alone can be sent
/// as that change: what a game calls a state change, kept small.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Sgr {
    /// All of them, in the order the style writes them.
    full: String,
    /// The foreground colour's parameters (`31`, `38;2;r;g;b`), if any.
    fg: String,
    /// The background colour's.
    bg: String,
    /// Everything else (attributes, underline colour), in order.
    rest: String,
}

impl Sgr {
    fn parse(codes: &str) -> Sgr {
        let mut sgr = Sgr {
            full: codes.to_string(),
            ..Sgr::default()
        };
        let tokens: Vec<&str> = codes.split(';').filter(|t| !t.is_empty()).collect();
        let mut i = 0;
        while i < tokens.len() {
            let token = tokens[i];
            // An extended colour takes its sub-parameters with it.
            let take = match (token, tokens.get(i + 1).copied()) {
                ("38" | "48" | "58", Some("5")) => 3,
                ("38" | "48" | "58", Some("2")) => 5,
                _ => 1,
            };
            let end = (i + take).min(tokens.len());
            let group = tokens[i..end].join(";");
            let n: u16 = token.parse().unwrap_or(0);
            let (slot, colour) = match n {
                30..=39 | 90..=97 => (&mut sgr.fg, true),
                40..=49 | 100..=107 => (&mut sgr.bg, true),
                _ => (&mut sgr.rest, false),
            };
            if colour {
                // A colour set twice: the last wins, as on a terminal.
                slot.clear();
            }
            if !slot.is_empty() {
                slot.push(';');
            }
            slot.push_str(&group);
            i = end;
        }
        sgr
    }

    /// Set from a true-colour pair, as `Style::ansi_codes` writes it.
    fn set_rgb(&mut self, fg: Option<Rgb>, bg: Option<Rgb>) {
        self.fg.clear();
        self.bg.clear();
        self.rest.clear();
        self.full.clear();
        if let Some(fg) = fg {
            rgb_code(&mut self.fg, "38;2;", fg);
        }
        if let Some(bg) = bg {
            rgb_code(&mut self.bg, "48;2;", bg);
        }
        self.full.push_str(&self.fg);
        if !self.fg.is_empty() && !self.bg.is_empty() {
            self.full.push(';');
        }
        self.full.push_str(&self.bg);
    }

    /// Write what changes the terminal's pen from `from` (unknown when
    /// `None`) to this: nothing, the colours that changed, or a reset and
    /// everything, whichever is shortest.
    fn write_from(&self, from: Option<&Sgr>, out: &mut String) {
        let full_len = 4 + if self.full.is_empty() {
            0
        } else {
            1 + self.full.len()
        };
        if let Some(from) = from {
            if from.full == self.full {
                return;
            }
            if from.rest == self.rest {
                let part = |now: &str, was: &str, default: usize| match (now == was, now.is_empty())
                {
                    (true, _) => 0,
                    (false, true) => default,
                    (false, false) => now.len(),
                };
                let fg = part(&self.fg, &from.fg, 2);
                let bg = part(&self.bg, &from.bg, 2);
                let delta = 3 + fg + bg + usize::from(fg > 0 && bg > 0);
                if delta < full_len {
                    out.push_str("\x1b[");
                    if fg > 0 {
                        out.push_str(if self.fg.is_empty() { "39" } else { &self.fg });
                    }
                    if fg > 0 && bg > 0 {
                        out.push(';');
                    }
                    if bg > 0 {
                        out.push_str(if self.bg.is_empty() { "49" } else { &self.bg });
                    }
                    out.push('m');
                    return;
                }
            }
        }
        out.push_str("\x1b[0");
        if !self.full.is_empty() {
            out.push(';');
            out.push_str(&self.full);
        }
        out.push('m');
    }
}

impl Painter {
    pub fn new(system: Option<ColorSystem>) -> Painter {
        Painter {
            previous: None,
            system,
            codes: Vec::new(),
            scratch: Sgr::default(),
            inline: false,
            cursor: None,
            height: 0,
            force: false,
            written: Vec::new(),
            stats: PaintStats::default(),
            strategy: PaintStrategy::default(),
            erase: true,
        }
    }

    /// Whether runs of blank cells may be erased with ECH or EL rather
    /// than written as spaces (on by default; only blanks with the
    /// terminal's own background are, so a terminal without
    /// background-colour erase shows the same).
    pub fn set_erase(&mut self, on: bool) {
        self.erase = on;
    }

    /// Paint with `strategy` from now on.
    pub fn set_strategy(&mut self, strategy: PaintStrategy) {
        self.strategy = strategy;
    }

    /// The strategy this painter paints with.
    pub fn strategy(&self) -> PaintStrategy {
        self.strategy
    }

    /// What the last [`paint`](Self::paint) or [`repaint`](Self::repaint)
    /// did: cells compared and written, and bytes by kind.
    pub fn stats(&self) -> PaintStats {
        self.stats
    }

    /// Paint an inline region that starts on the cursor's row, moving the
    /// cursor relative to where it is, so the rows above (the shell's
    /// scrollback) are left alone.
    #[must_use]
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
        let mut out = String::new();
        self.goto_into(from, x, y, u16::MAX, &mut out);
        out
    }

    /// Move the cursor from `from` (unknown when `None`) to `x`, `y`, the
    /// cheapest way: an absolute move (CUP), or a relative one (up, down,
    /// left, right, a carriage return, `\r\n`) when it is shorter.
    /// `width` is the screen's: a cursor past its last column is waiting
    /// to wrap, so only a carriage return fixes its column.
    fn goto_into(&self, from: Option<(u16, u16)>, x: u16, y: u16, width: u16, out: &mut String) {
        if self.inline {
            let (_, from_y) = from.unwrap_or((0, 0));
            if y < from_y {
                csi(out, from_y - y, 'A');
            } else if y > from_y {
                csi(out, y - from_y, 'B');
            }
            // A carriage return also clears a pending wrap after the last
            // column, so the column is always counted from the left edge.
            out.push('\r');
            if x > 0 {
                csi(out, x, 'C');
            }
            return;
        }
        let cup = if x == 0 && y == 0 {
            3
        } else {
            3 + digits(y + 1) + if x > 0 { 1 + digits(x + 1) } else { 0 }
        };
        let Some((fx, fy)) = from else {
            return push_cup(out, x, y);
        };
        let step = |n: u16| if n == 1 { 3 } else { 3 + digits(n) };
        let vertical = match y.cmp(&fy) {
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Greater => step(y - fy),
            std::cmp::Ordering::Less => step(fy - y),
        };
        let wrapping = fx >= width;
        // Along the row, from where the cursor is (not while it waits to
        // wrap), or from the left edge after a carriage return.
        let along = if wrapping || x == fx {
            usize::MAX
        } else if x > fx {
            step(x - fx)
        } else {
            step(fx - x)
        };
        let along = if x == fx && !wrapping { 0 } else { along };
        let from_edge = 1 + if x > 0 { step(x) } else { 0 };
        let crlf = x == 0 && y == fy + 1;
        let relative = vertical.saturating_add(along.min(from_edge));
        if crlf && 2 < cup && 2 <= relative {
            out.push_str("\r\n");
            return;
        }
        if relative >= cup {
            return push_cup(out, x, y);
        }
        if along > from_edge {
            out.push('\r');
        }
        match y.cmp(&fy) {
            std::cmp::Ordering::Greater => csi(out, y - fy, 'B'),
            std::cmp::Ordering::Less => csi(out, fy - y, 'A'),
            std::cmp::Ordering::Equal => {}
        }
        if along <= from_edge {
            if x > fx {
                csi(out, x - fx, 'C');
            } else if x < fx {
                csi(out, fx - x, 'D');
            }
        } else if x > 0 {
            csi(out, x, 'C');
        }
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

    /// The screen this painter paints had its styles renumbered
    /// ([`Screen::compact_styles`]): follow the new ids, so what was sent
    /// still compares with it. A cell sent in a style that was dropped
    /// reads as changed.
    pub fn remap_styles(&mut self, remap: &StyleRemap) {
        if let Some(previous) = &mut self.previous {
            for cell in &mut previous.cells {
                cell.style = remap.get_or_gone(cell.style);
            }
        }
        let mut codes = Vec::with_capacity(self.codes.len());
        for (old, code) in self.codes.drain(..).enumerate() {
            let Some(code) = code else { continue };
            if let Some(new) = remap.get(StyleId::from_index(old)) {
                if codes.len() <= new.index() {
                    codes.resize(new.index() + 1, None);
                }
                codes[new.index()] = Some(code);
            }
        }
        self.codes = codes;
    }

    /// Drop the styles `screen`'s cells no longer use, when its table has
    /// piled up ([`Screen::styles_piled_up`]), keeping this painter in
    /// step: call it after each [`paint`](Self::paint), and a screen whose
    /// cells change colour every frame keeps only the styles of a frame or
    /// two. Whether it compacted. What is painted next is the same either
    /// way.
    ///
    /// ```
    /// use rich::ColorSystem;
    /// use rich_ext::surface::{Painter, Screen};
    ///
    /// let mut screen = Screen::new(16, 4);
    /// let mut painter = Painter::new(Some(ColorSystem::Truecolor));
    /// for frame in 0..100u8 {
    ///     for y in 0..4 {
    ///         for x in 0..16 {
    ///             let c = [frame, x as u8 * 16, y as u8 * 64];
    ///             screen.put_rgb(x, y, '▀', Some(c), Some([0, 0, frame]));
    ///         }
    ///     }
    ///     painter.paint(&screen, &[screen.area()]);
    ///     painter.compact(&mut screen);
    /// }
    /// // A frame uses 64 styles; a hundred frames made 6,400.
    /// assert!(screen.style_count() < 2_000);
    /// ```
    pub fn compact(&mut self, screen: &mut Screen) -> bool {
        if !screen.styles_piled_up() {
            return false;
        }
        let remap = screen.compact_styles();
        self.remap_styles(&remap);
        true
    }

    /// What to write so the terminal shows `screen`, given that only the
    /// `damage` rectangles can have changed since the last paint (ignored
    /// on the first paint, or after a resize or [`invalidate`](Self::invalidate)).
    pub fn paint(&mut self, screen: &Screen, damage: &[Rect]) -> String {
        self.written.clear();
        self.stats = PaintStats::default();
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
            // A column either side too: writing a rectangle can turn the
            // half of a wide grapheme it cut, just outside it, into a space.
            // (Not for a forced repaint, which writes every cell it is given.)
            let wide = u16::from(!self.force);
            damage
                .iter()
                .map(|r| {
                    let x = r.x.saturating_sub(wide);
                    let right = r.right().saturating_add(wide);
                    Rect::new(x, r.y, right - x, r.height).intersection(screen.area())
                })
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
        let before = out.len();
        if pen.link.is_some() {
            out.push_str("\x1b]8;;\x1b\\");
        }
        if pen.rgb.is_some() || pen.codes.as_ref().is_some_and(|c| !c.full.is_empty()) {
            out.push_str("\x1b[0m");
        }
        self.stats.style_bytes += out.len() - before;
        let stats = &mut self.stats;
        stats.elided = stats.compared.saturating_sub(stats.written);
        stats.other_bytes = out.len() - stats.move_bytes - stats.style_bytes - stats.text_bytes;
        if self.inline {
            self.cursor = pen.at.or(self.cursor);
        }
        // Bring the copy of what was sent up to date, reusing its cells'
        // text buffers.
        match &mut self.previous {
            None => {
                self.previous = Some(Sent {
                    width: screen.width,
                    height: screen.height,
                    cells: screen.cells.clone(),
                })
            }
            Some(previous) => {
                let width = screen.width as usize;
                for rect in &rects {
                    for y in rect.y..rect.bottom() {
                        let row = y as usize * width;
                        let span = row + rect.x as usize..row + rect.right() as usize;
                        for (sent, cell) in previous.cells[span.clone()]
                            .iter_mut()
                            .zip(&screen.cells[span])
                        {
                            *sent = *cell;
                        }
                    }
                }
            }
        }
        out
    }

    /// The cells the last [`paint`](Self::paint) or [`repaint`](Self::repaint)
    /// wrote, one rectangle per run of a row: what a graphic drawn over them
    /// lost.
    pub fn written(&self) -> &[Rect] {
        &self.written
    }

    /// [`paint`](Self::paint), writing every cell of `rects` whether it
    /// changed or not: for cells something else drew over (a graphic that
    /// went away).
    pub fn repaint(&mut self, screen: &Screen, rects: &[Rect]) -> String {
        self.force = true;
        let out = self.paint(screen, rects);
        self.force = false;
        out
    }

    fn changed(&self, screen: &Screen, x: u16, y: u16) -> bool {
        if self.force {
            return true;
        }
        match &self.previous {
            None => true,
            Some(previous) => {
                // The copy holds the screen's style ids (moved over when the
                // screen compacts), so they compare directly.
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
                self.stats.compared += 1;
                x += 1;
                continue;
            }
            let mut x0 = x;
            if screen.cell(x0, y).is_continuation() && x0 > 0 {
                x0 -= 1;
            }
            // A run of blanks on the terminal's own background: erased
            // (EL to the end of the row, ECH within it) when that is
            // shorter than writing it.
            if self.erase && !self.inline && self.plain_blank(screen, x0, y) {
                let mut run = 1;
                while x0 + run < end && self.plain_blank(screen, x0 + run, y) {
                    run += 1;
                }
                let to_edge = x0 + run == screen.width;
                let erase = if to_edge { 3 } else { 3 + digits(run) };
                if usize::from(run) > erase {
                    self.move_pen(screen, x0, y, pen, out);
                    let before = out.len();
                    self.set_pen(screen, StyleId::NONE, pen, out);
                    self.stats.style_bytes += out.len() - before;
                    if to_edge {
                        out.push_str("\x1b[K");
                    } else {
                        csi(out, run, 'X');
                    }
                    self.stats.compared += usize::from(x0 + run - x);
                    self.stats.written += usize::from(run);
                    self.note_written(x0, y, x0 + run);
                    x = x0 + run;
                    continue;
                }
            }
            self.move_pen(screen, x0, y, pen, out);
            let cell = screen.cell(x0, y);
            let before = out.len();
            self.set_pen(screen, cell.style, pen, out);
            self.stats.style_bytes += out.len() - before;
            let text = cell.text();
            out.push_str(text);
            self.stats.text_bytes += text.len();
            let next = x0 + cell.width.max(1) as u16;
            let columns = usize::from(next - x0);
            self.stats.compared += usize::from(next - x);
            self.stats.written += columns;
            self.note_written(x0, y, next);
            pen.at = Some((next, y));
            x = next;
        }
    }

    /// Note cells `x`..`next` of row `y` as written this paint.
    fn note_written(&mut self, x: u16, y: u16, next: u16) {
        match self.written.last_mut() {
            Some(run) if run.y == y && run.right() == x => run.width = next - run.x,
            _ => self.written.push(Rect::new(x, y, next - x, 1)),
        }
    }

    /// Move the pen to `x`, `y`: through the cells between, when they are
    /// a few unchanged ASCII characters in the pen's style (shorter than
    /// any move), else the cheapest move.
    fn move_pen(&mut self, screen: &Screen, x: u16, y: u16, pen: &mut Pen, out: &mut String) {
        if pen.at == Some((x, y)) {
            return;
        }
        let before = out.len();
        let through = match pen.at {
            Some((fx, fy)) if !self.inline && fy == y && fx < x && x - fx <= 3 => {
                (fx..x).all(|gx| {
                    let cell = screen.cell(gx, y);
                    cell.width == 1 && cell.len == 1 && Some(cell.style) == pen.style
                })
            }
            _ => false,
        };
        if let (true, Some((fx, _))) = (through, pen.at) {
            for gx in fx..x {
                out.push_str(screen.cell(gx, y).text());
            }
        } else {
            self.goto_into(pen.at, x, y, screen.width, out);
        }
        self.stats.move_bytes += out.len() - before;
        pen.at = Some((x, y));
    }

    /// Whether the cell at `x`, `y` is a space with no style the terminal
    /// shows on a blank: what erasing leaves.
    fn plain_blank(&mut self, screen: &Screen, x: u16, y: u16) -> bool {
        let cell = screen.cell(x, y);
        if cell.len != 1 || cell.bytes[0] != b' ' || cell.width != 1 {
            return false;
        }
        if cell.style == StyleId::NONE {
            return true;
        }
        let Some(style) = screen.style(cell.style) else {
            return true;
        };
        // Colours and attributes would show on the blank; a link would not
        // be opened by an erase.
        style.link().is_none() && self.sgr(screen, cell.style).full.is_empty()
    }

    /// The SGR parameters of `style`, cached by id.
    fn sgr(&mut self, screen: &Screen, style: StyleId) -> &Sgr {
        if self.codes.len() <= style.index() {
            self.codes.resize(style.index() + 1, None);
        }
        let system = self.system;
        self.codes[style.index()].get_or_insert_with(|| match (screen.style(style), system) {
            (Some(s), Some(system)) => Sgr::parse(&s.ansi_codes(system)),
            _ => Sgr::default(),
        })
    }

    fn set_pen(&mut self, screen: &Screen, style: StyleId, pen: &mut Pen, out: &mut String) {
        if pen.style == Some(style) {
            return;
        }
        let resolved = screen.style(style);
        // A URL holding a control would end the OSC early and drive the
        // terminal with the rest: such a link is dropped.
        let link = resolved
            .and_then(|s| s.link())
            .filter(|url| !url.chars().any(|c| super::control_picture(c).is_some()));
        if link != pen.link.as_deref() {
            match link {
                Some(url) => {
                    out.push_str("\x1b]8;;");
                    out.push_str(url);
                    out.push_str("\x1b\\");
                }
                None => out.push_str("\x1b]8;;\x1b\\"),
            }
            pen.link = link.map(str::to_string);
        }
        if let (Some(pair), Some(ColorSystem::Truecolor)) = (screen.styles.rgb(style), self.system)
        {
            // A colour pair in true colour: written straight from the
            // colours, as `Style::ansi_codes` would, with nothing cached,
            // and from the last pair only the colour that changed.
            match pen.rgb {
                Some(old) if old == pair => {}
                Some(old) => rgb_delta(old, pair, out),
                None => {
                    self.scratch.set_rgb(pair.0, pair.1);
                    if pen.codes.as_ref() != Some(&self.scratch) {
                        self.scratch.write_from(pen.codes.as_ref(), out);
                    }
                }
            }
            pen.rgb = Some(pair);
            pen.style = Some(style);
            return;
        }
        if let Some((fg, bg)) = pen.rgb.take() {
            // What the pen holds, as parameters again.
            self.scratch.set_rgb(fg, bg);
            pen.codes = Some(self.scratch.clone());
        }
        self.sgr(screen, style);
        let codes = self.codes[style.index()].as_ref().expect("cached above");
        // Styles that differ only in their link share colours: no SGR.
        if pen.codes.as_ref() != Some(codes) {
            codes.write_from(pen.codes.as_ref(), out);
            match &mut pen.codes {
                Some(current) => current.clone_from(codes),
                None => pen.codes = Some(codes.clone()),
            }
        }
        pen.style = Some(style);
    }
}

/// The SGR that changes a true-colour pen from `old` to `new`: only the
/// colours that differ (a colour gone is its default, 39 or 49).
fn rgb_delta(old: (Option<Rgb>, Option<Rgb>), new: (Option<Rgb>, Option<Rgb>), out: &mut String) {
    out.push_str("\x1b[");
    let mut first = true;
    for (was, now, lead, default) in [(old.0, new.0, "38;2;", "39"), (old.1, new.1, "48;2;", "49")]
    {
        if was == now {
            continue;
        }
        if !first {
            out.push(';');
        }
        first = false;
        match now {
            Some(rgb) => rgb_code(out, lead, rgb),
            None => out.push_str(default),
        }
    }
    out.push('m');
}

/// `CSI n c`, with `n` left out when it is 1.
fn csi(out: &mut String, n: u16, c: char) {
    out.push_str("\x1b[");
    if n != 1 {
        push_u16(out, n);
    }
    out.push(c);
}

/// An absolute move to `x`, `y` (CUP), its column left out when it is the
/// first.
fn push_cup(out: &mut String, x: u16, y: u16) {
    out.push_str("\x1b[");
    if x > 0 || y > 0 {
        push_u16(out, y + 1);
        if x > 0 {
            out.push(';');
            push_u16(out, x + 1);
        }
    }
    out.push('H');
}

/// The decimal digits of `n`.
fn digits(n: u16) -> usize {
    match n {
        0..=9 => 1,
        10..=99 => 2,
        100..=999 => 3,
        1000..=9999 => 4,
        _ => 5,
    }
}

/// `n` in decimal.
fn push_u16(out: &mut String, n: u16) {
    let mut buffer = [0u8; 5];
    let mut at = buffer.len();
    let mut n = n;
    loop {
        at -= 1;
        buffer[at] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    for &b in &buffer[at..] {
        out.push(char::from(b));
    }
}

/// `lead` and a true colour's parameters (`38;2;r;g;b`).
fn rgb_code(out: &mut String, lead: &str, rgb: Rgb) {
    out.push_str(lead);
    for (i, part) in rgb.into_iter().enumerate() {
        if i > 0 {
            out.push(';');
        }
        push_u8(out, part);
    }
}

/// `n` in decimal.
fn push_u8(out: &mut String, n: u8) {
    if n >= 100 {
        out.push(char::from(b'0' + n / 100));
    }
    if n >= 10 {
        out.push(char::from(b'0' + n / 10 % 10));
    }
    out.push(char::from(b'0' + n % 10));
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
        assert_eq!(screen.cell(4, 0).text(), " ");
    }

    #[test]
    fn simple_lines_write_as_a_frame_would() {
        let pieces = [
            "ab",
            "日本",
            "●x",
            "⣿⠁",
            " ",
            "é",
            "a\u{301}",
            "\x1b",
            "─┐",
            "\u{301}x",
            "👨\u{200d}👩\u{200d}👧",
            "❤\u{fe0f}",
            "\u{200d}",
            "e\u{301}\u{302}",
        ];
        let styles = [
            None,
            Style::parse("bold red").ok(),
            Style::parse("on blue").ok(),
        ];
        let mut n = 7usize;
        for case in 0..1500 {
            let lines: Vec<Vec<Segment>> = (0..3)
                .map(|_| {
                    (0..4)
                        .map(|_| {
                            n = n.wrapping_mul(1103515245).wrapping_add(12345) >> 3;
                            let text = pieces[n % pieces.len()];
                            Segment::new(text, styles[n / 9 % 3].clone())
                        })
                        .collect()
                })
                .collect();
            let rect = Rect::new((case % 3) as u16, 1, 5 + (case % 4) as u16, 3);
            let mut fast = Screen::new(12, 5);
            fast.write_lines(Rect::new(0, 0, 12, 5), &[line("日本語日本語")]);
            let mut slow = fast.clone();
            fast.write_lines(rect, &lines);
            slow.split_edges(rect.intersection(slow.area()));
            slow.write_frame_lines(rect.intersection(slow.area()), &lines);
            assert_eq!(fast.lines(), slow.lines(), "{lines:?} in {rect:?}");
            let widths = |s: &Screen| (0..12).map(|x| s.cell(x, 1).width).collect::<Vec<_>>();
            assert_eq!(widths(&fast), widths(&slow), "{lines:?}");
        }
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
            out.starts_with("\x1b[H\x1b]8;;https://x.y\x1b\\"),
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
    fn a_wide_grapheme_cut_by_the_destination_edge_becomes_a_space() {
        let mut source = Screen::new(4, 1);
        source.write_lines(source.area(), &[line("ab日")]);
        let mut narrow = Screen::new(3, 1);
        narrow.blit(&source, source.area(), 0, 0);
        assert_eq!(narrow.plain(), ["ab "]);
        assert_eq!(narrow.cell(2, 0).width, 1);
    }

    #[test]
    fn the_painter_reports_the_runs_it_wrote() {
        let mut screen = Screen::new(10, 2);
        let mut painter = Painter::new(None);
        screen.write_lines(screen.area(), &[line("abc"), line("xyz")]);
        painter.paint(&screen, &[]);
        screen.write_lines(Rect::new(0, 1, 10, 1), &[line("xYz")]);
        painter.paint(&screen, &[Rect::new(0, 0, 10, 2)]);
        assert_eq!(painter.written(), [Rect::new(1, 1, 1, 1)]);
    }

    #[test]
    fn a_wide_grapheme_replaced_by_narrow_text_clears_its_second_column() {
        let mut screen = Screen::new(4, 1);
        let mut painter = Painter::new(None);
        screen.write_lines(screen.area(), &[line("日x")]);
        painter.paint(&screen, &[]);
        screen.write_lines(screen.area(), &[line("abx")]);
        let out = painter.paint(&screen, &[screen.area()]);
        assert_eq!(out, "\x1b[H\x1b[0mab");
    }

    /// A small, fast generator: the same noise on every run.
    struct Noise(u64);

    impl Noise {
        fn rgb(&mut self) -> [u8; 3] {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let [r, g, b, _] = ((self.0 >> 33) as u32).to_le_bytes();
            [r, g, b]
        }
    }

    /// A frame of `screen` in random colour pairs, as `put_rgb` cells, or
    /// as rendered segments when `segments`.
    fn noise_frame(screen: &mut Screen, noise: &mut Noise, segments: bool) {
        let area = screen.area();
        if segments {
            let lines: Vec<Vec<Segment>> = (0..area.height)
                .map(|_| {
                    (0..area.width)
                        .map(|_| {
                            let [r, g, b] = noise.rgb();
                            let [r2, g2, b2] = noise.rgb();
                            let style = Style::from_color(
                                Some(rich::Color::from_rgb(r, g, b)),
                                Some(rich::Color::from_rgb(r2, g2, b2)),
                            );
                            Segment::new("▀", Some(style))
                        })
                        .collect()
                })
                .collect();
            screen.write_lines(area, &lines);
        } else {
            for y in 0..area.height {
                for x in 0..area.width {
                    let (fg, bg) = (noise.rgb(), noise.rgb());
                    screen.put_rgb(x, y, '▀', Some(fg), Some(bg));
                }
            }
        }
    }

    #[test]
    fn the_style_table_stays_bounded_over_a_thousand_random_frames() {
        for segments in [false, true] {
            let mut screen = Screen::new(40, 12);
            let mut painter = Painter::new(Some(ColorSystem::Truecolor));
            let mut noise = Noise(1);
            let mut most = 0;
            for _ in 0..1000 {
                noise_frame(&mut screen, &mut noise, segments);
                painter.paint(&screen, &[screen.area()]);
                painter.compact(&mut screen);
                most = most.max(screen.style_count());
            }
            // 480 cells, each a new pair a frame: about a frame's worth is
            // kept, never a thousand frames'.
            assert!(most <= 2 * 1024 + 1, "{most} styles ({segments})");
            assert!(painter.codes.len() <= most, "{}", painter.codes.len());
        }
    }

    #[test]
    fn compacting_changes_no_byte_painted() {
        // Two screens drawn alike, one compacted after every paint and one
        // never: the painters write the same bytes, frame after frame.
        let mut kept = (
            Screen::new(30, 6),
            Painter::new(Some(ColorSystem::Truecolor)),
        );
        let mut compacted = (
            Screen::new(30, 6),
            Painter::new(Some(ColorSystem::Truecolor)),
        );
        let mut noise = Noise(9);
        for frame in 0..60 {
            let mut lines = vec![
                line("[bold]title[/] [link=https://x.y]docs[/link]"),
                line(&format!("[red]{frame}[/] [on blue]  [/]")),
            ];
            if frame % 3 == 0 {
                lines.push(line("[italic green]every third[/]"));
            }
            let mut cells = Vec::new();
            for x in 0..30 {
                cells.push((x, noise.rgb(), noise.rgb()));
            }
            let underlay = Style::parse("on #202020").unwrap();
            let mut bytes = Vec::new();
            for (screen, painter) in [&mut kept, &mut compacted] {
                screen.write_lines(Rect::new(0, 0, 30, 3), &lines);
                // Only some cells change colour, so most are not repainted.
                for &(x, fg, bg) in &cells[..(frame % 30)] {
                    screen.put_rgb(x, 4, '▀', Some(fg), Some(bg));
                }
                for x in 0..30 {
                    screen.underlay(x, 1, &underlay);
                }
                bytes.push(painter.paint(screen, &[Rect::new(0, 0, 30, 5)]));
            }
            let remap = compacted.0.compact_styles();
            compacted.1.remap_styles(&remap);
            assert_eq!(bytes[0], bytes[1], "frame {frame}");
            assert_eq!(kept.0.plain(), compacted.0.plain());
        }
        assert!(compacted.0.style_count() < kept.0.style_count());
    }

    #[test]
    fn a_cell_sent_in_a_dropped_style_reads_as_changed() {
        let mut screen = Screen::new(2, 1);
        let mut painter = Painter::new(Some(ColorSystem::Truecolor));
        screen.put_rgb(0, 0, 'a', Some([1, 2, 3]), None);
        painter.paint(&screen, &[]);
        // Drawn over without a paint, then compacted: the style sent went.
        screen.put_rgb(0, 0, 'a', Some([4, 5, 6]), None);
        let remap = screen.compact_styles();
        painter.remap_styles(&remap);
        let out = painter.paint(&screen, &[screen.area()]);
        assert_eq!(out, "\x1b[H\x1b[0;38;2;4;5;6ma\x1b[0m");
    }

    #[test]
    fn pixel_cells_write_what_their_style_would() {
        let mut noise = Noise(3);
        for system in [
            ColorSystem::Truecolor,
            ColorSystem::EightBit,
            ColorSystem::Standard,
        ] {
            for case in 0..200 {
                let fg = (case % 3 != 0).then(|| noise.rgb());
                let bg = (case % 5 != 0).then(|| noise.rgb());
                let mut fast = Screen::new(1, 1);
                fast.put_rgb(0, 0, '▄', fg, bg);
                let mut slow = Screen::new(1, 1);
                let style = Style::from_color(
                    fg.map(|[r, g, b]| rich::Color::from_rgb(r, g, b)),
                    bg.map(|[r, g, b]| rich::Color::from_rgb(r, g, b)),
                );
                slow.write_lines(slow.area(), &[vec![Segment::new("▄", Some(style))]]);
                let paint = |screen: &Screen| Painter::new(Some(system)).paint(screen, &[]);
                assert_eq!(paint(&fast), paint(&slow), "{fg:?} {bg:?} {system:?}");
            }
        }
    }

    #[test]
    fn a_pixel_cell_over_half_a_wide_grapheme_clears_the_other_half() {
        let mut screen = Screen::new(5, 1);
        screen.write_lines(screen.area(), &[line("日本x")]);
        screen.put_rgb(1, 0, '▀', Some([255, 0, 0]), None);
        screen.put_rgb(2, 0, '▀', None, Some([0, 0, 255]));
        assert_eq!(screen.plain(), [" ▀▀ x"]);
        assert!((0..5).all(|x| screen.cell(x, 0).width == 1));
        // Wide, zero-width and control characters take one cell.
        screen.put_rgb(0, 0, '日', None, None);
        screen.put_rgb(1, 0, '\u{301}', None, None);
        screen.put_rgb(2, 0, '\x1b', None, None);
        assert_eq!(screen.plain(), ["  ␛ x"]);
        // Off the screen, nothing.
        screen.put_rgb(5, 0, 'z', None, None);
        screen.put(0, 1, 'z', StyleId::NONE);
        assert_eq!(screen.plain(), ["  ␛ x"]);
    }

    #[test]
    fn the_pixel_glyphs_are_one_cell_wide() {
        let ranges = [
            (' ', '~'),
            ('\u{2500}', '\u{259f}'),
            ('\u{2800}', '\u{28ff}'),
        ];
        for (first, last) in ranges {
            for c in first..=last {
                assert_eq!(rich::cells::cell_len(&c.to_string()), 1, "{c:?}");
            }
        }
    }

    #[test]
    fn layering_a_style_over_many_cells_combines_each_pair_once() {
        let mut screen = Screen::new(8, 1);
        screen.write_lines(screen.area(), &[line("[red]abcd[/][blue]efgh[/]")]);
        let before = screen.style_count();
        let under = Style::parse("on black").unwrap();
        for x in 0..8 {
            screen.underlay(x, 0, &under);
        }
        // The layer, and red and blue on black.
        assert_eq!(screen.style_count(), before + 3);
        let style_of = |screen: &Screen, x| screen.style(screen.cell(x, 0).style).cloned();
        let parse = |s| Style::parse(s).ok();
        assert_eq!(
            (style_of(&screen, 0), style_of(&screen, 7)),
            (parse("red on black"), parse("blue on black"))
        );
        let over = Style::parse("bold").unwrap();
        screen.restyle(0, 0, &over);
        assert_eq!(style_of(&screen, 0), parse("bold red on black"));
        screen.compact_styles();
        screen.underlay(1, 0, &under);
        assert_eq!(style_of(&screen, 1), parse("red on black"));
    }

    #[test]
    fn a_blit_translates_each_style_once() {
        let mut source = Screen::new(6, 1);
        source.write_lines(source.area(), &[line("[bold]ab[/][red]cd[/]ef")]);
        let mut target = Screen::new(6, 1);
        target.blit(&source, source.area(), 0, 0);
        assert_eq!(target.lines(), source.lines());
        assert_eq!(target.style_count(), source.style_count());
    }
}
