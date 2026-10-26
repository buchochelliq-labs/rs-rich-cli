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

    /// A single character, `width` columns wide: [`Cell::new`] of it.
    #[inline]
    pub(crate) fn from_char(c: char, width: u8, style: StyleId) -> Cell {
        Cell::char(c, width, style)
    }

    /// The cell's text when it is one character, decoded from its bytes
    /// without a UTF-8 check (they were written from a `char` or a `&str`).
    #[inline]
    pub(crate) fn single_char(&self) -> Option<char> {
        let [b0, b1, b2, b3] = self.bytes.map(u32::from);
        let code = match self.len {
            1 => b0,
            2 if b0 & 0xe0 == 0xc0 => (b0 & 0x1f) << 6 | (b1 & 0x3f),
            3 if b0 & 0xf0 == 0xe0 => (b0 & 0x0f) << 12 | (b1 & 0x3f) << 6 | (b2 & 0x3f),
            4 if b0 & 0xf8 == 0xf0 => {
                (b0 & 0x07) << 18 | (b1 & 0x3f) << 12 | (b2 & 0x3f) << 6 | (b3 & 0x3f)
            }
            _ => return None,
        };
        char::from_u32(code)
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

    /// Whether the cell's text is empty (a wide character's second half).
    #[inline]
    pub(crate) fn is_empty_text(&self) -> bool {
        self.len == 0
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

/// A run of text in one style, for rs-rich-ext's fast path
/// ([`Screen::write_runs`]): the text borrowed, the style an id already
/// interned in the screen written to. rich's [`Segment`] owns its text and
/// its [`Style`]; a run owns neither, so a line of runs costs no
/// allocation a run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextRun<'a> {
    pub text: &'a str,
    pub style: StyleId,
}

impl<'a> TextRun<'a> {
    pub fn new(text: &'a str, style: StyleId) -> TextRun<'a> {
        TextRun { text, style }
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
    /// The rows [`scroll_up`](Self::scroll_up) moved since
    /// [`forget_scrolls`](Self::forget_scrolls), newest last (a few): what
    /// a painter tries first when it looks for rows that moved.
    scrolled: Vec<(Rect, u16)>,
    /// Scratch for writing lines, kept so a frame allocates none: each
    /// segment's style id, and a run's graphemes.
    scratch_ids: Vec<StyleId>,
    scratch_spans: Vec<(usize, usize, usize)>,
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
            scrolled: Vec::new(),
            scratch_ids: Vec::new(),
            scratch_spans: Vec::new(),
        }
    }

    /// The scrolls [`scroll_up`](Self::scroll_up) made since the last
    /// [`forget_scrolls`](Self::forget_scrolls): each rectangle and how
    /// many rows it moved up.
    pub fn scrolled(&self) -> &[(Rect, u16)] {
        &self.scrolled
    }

    /// Forget the scrolls made so far: after a frame is painted.
    pub fn forget_scrolls(&mut self) {
        self.scrolled.clear();
    }

    /// Row `y`'s cells.
    fn row(&self, y: u16) -> &[Cell] {
        let width = self.width as usize;
        &self.cells[y as usize * width..(y as usize + 1) * width]
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
        let mut spans = std::mem::take(&mut self.scratch_spans);
        let mut ids = std::mem::take(&mut self.scratch_ids);
        for row in 0..rect.height {
            let line = lines.get(row as usize).map_or(&[][..], Vec::as_slice);
            ids.clear();
            for segment in line {
                // A style that sets nothing is no style.
                let style = segment.style.as_ref().filter(|s| !s.is_null());
                ids.push(if segment.control {
                    StyleId::NONE
                } else {
                    self.styles.intern(style)
                });
            }
            let pieces = line
                .iter()
                .zip(&ids)
                .filter(|(segment, _)| !segment.control)
                .map(|(segment, &id)| (segment.text.as_str(), id));
            self.write_row(rect, rect.y + row, pieces, &mut spans);
        }
        self.scratch_spans = spans;
        self.scratch_ids = ids;
        true
    }

    /// The fast path's [`write_lines`](Self::write_lines): rows of
    /// [`TextRun`]s, each a borrowed text and a style already interned in
    /// this screen, written into `rect` as `write_lines` writes the same
    /// text in the same styles, without a [`Segment`] or a [`Style`] a run.
    /// Rows and columns the runs do not reach are cleared. A run with a
    /// line break falls back to `write_lines` (its text cannot be one row).
    ///
    /// ```
    /// use rich::{Segment, Style};
    /// use rich_ext::frame::StyleId;
    /// use rich_ext::surface::{Rect, Screen, TextRun};
    ///
    /// let mut fast = Screen::new(6, 1);
    /// let bold = fast.intern(Some(&Style::parse("bold").unwrap()));
    /// fast.write_runs(
    ///     Rect::new(0, 0, 6, 1),
    ///     &[vec![TextRun::new("ab", bold), TextRun::new("c", StyleId::NONE)]],
    /// );
    /// // The same cells as rich's path.
    /// let mut slow = Screen::new(6, 1);
    /// slow.write_lines(
    ///     Rect::new(0, 0, 6, 1),
    ///     &[vec![Segment::new("ab", Some(Style::parse("bold").unwrap())), Segment::new("c", None)]],
    /// );
    /// assert_eq!(fast.lines(), slow.lines());
    /// ```
    pub fn write_runs<'t, L: AsRef<[TextRun<'t>]>>(&mut self, rect: Rect, lines: &[L]) {
        let rect = rect.intersection(self.area());
        if rect.is_empty() {
            return;
        }
        let lines = &lines[..lines.len().min(rect.height as usize)];
        // What only the Frame path writes: a line break inside a run, or a
        // style with metadata (region tags).
        let mut checked = None;
        let framed = lines.iter().flat_map(AsRef::as_ref).any(|run| {
            run.text.contains('\n')
                || checked != Some(run.style) && {
                    checked = Some(run.style);
                    self.styles
                        .get(run.style)
                        .is_some_and(|s| s.meta_ref().is_some())
                }
        });
        if framed {
            let segments: Vec<Vec<Segment>> = lines
                .iter()
                .map(|line| {
                    line.as_ref()
                        .iter()
                        .map(|run| Segment::new(run.text, self.style(run.style).cloned()))
                        .collect()
                })
                .collect();
            self.write_lines(rect, &segments);
            return;
        }
        self.split_edges(rect);
        let mut spans = std::mem::take(&mut self.scratch_spans);
        let mut ids = std::mem::take(&mut self.scratch_ids);
        for row in 0..rect.height {
            let line = lines.get(row as usize).map_or(&[][..], AsRef::as_ref);
            ids.clear();
            // A style that sets nothing is no style, as for segments.
            ids.extend(line.iter().map(|run| match self.styles.get(run.style) {
                Some(style) if style.is_null() => StyleId::NONE,
                _ => run.style,
            }));
            let pieces = line.iter().zip(&ids).map(|(run, &id)| (run.text, id));
            self.write_row(rect, rect.y + row, pieces, &mut spans);
        }
        self.scratch_spans = spans;
        self.scratch_ids = ids;
    }

    /// The fast path for a grid of cells, a chart's: `cells`, `width` a
    /// row and `rows` rows, each empty (a space with no style) or a cell
    /// whose style is an index into `ids`, the screen's style for it,
    /// copied into `rect` as [`write_runs`](Self::write_runs) would write
    /// the same text. Only a grid of single characters is taken (no
    /// controls, zero-width marks or long graphemes, every wide character
    /// followed by its empty second half); for any other nothing is
    /// written and the result is false.
    pub(crate) fn write_cell_grid(
        &mut self,
        rect: Rect,
        width: usize,
        rows: usize,
        cells: &[Option<Cell>],
        ids: &[StyleId],
    ) -> bool {
        let rect = rect.intersection(self.area());
        if rect.is_empty() {
            return true;
        }
        let rows = rows.min(rect.height as usize);
        let cells = &cells[..rows * width];
        for line in cells.chunks_exact(width.max(1)) {
            let mut wide = false;
            for (at, cell) in line.iter().enumerate() {
                let Some(cell) = cell else {
                    if wide {
                        return false;
                    }
                    continue;
                };
                if cell.len == 0 {
                    // The second half of a wide character, and only that.
                    if !wide {
                        return false;
                    }
                    wide = false;
                    continue;
                }
                if wide {
                    return false;
                }
                if cell.len == 1 {
                    if !(b' '..=b'~').contains(&cell.bytes[0]) || cell.width != 1 {
                        return false;
                    }
                    continue;
                }
                let Some(c) = cell.single_char() else {
                    return false;
                };
                // Braille, blocks and box drawing: one column, no controls.
                if matches!(c, '\u{2500}'..='\u{259f}' | '\u{2800}'..='\u{28ff}') && cell.width == 1
                {
                    continue;
                }
                let w = char_width(c);
                if super::control_picture(c).is_some()
                    || !matches!(w, 1 | 2)
                    || c == '\u{200d}'
                    || c == '\u{fe0f}'
                    || usize::from(cell.width) != w
                {
                    return false;
                }
                wide = w == 2;
                if wide && at + 1 == line.len() {
                    return false;
                }
            }
            if wide {
                return false;
            }
        }
        self.split_edges(rect);
        let shown = width.min(rect.width as usize);
        for row in 0..rect.height {
            let y = rect.y + row;
            let mut x = 0usize;
            if (row as usize) < rows {
                let line = &cells[row as usize * width..row as usize * width + shown];
                while x < shown {
                    let at = y as usize * self.width as usize + rect.x as usize + x;
                    match line[x] {
                        None => {
                            self.cells[at] = Cell::blank();
                            x += 1;
                        }
                        Some(cell) => {
                            let id = ids[cell.style.index()];
                            if cell.width == 2 {
                                if x + 1 >= shown {
                                    // No room for the second column.
                                    self.cells[at] = Cell::char(' ', 1, id);
                                    x += 1;
                                    break;
                                }
                                self.cells[at] = cell.restyled(2, id);
                                self.cells[at + 1] = Cell::new("", 0, id);
                                x += 2;
                            } else {
                                self.cells[at] = cell.restyled(1, id);
                                x += 1;
                            }
                        }
                    }
                }
            }
            for gx in x..rect.width as usize {
                let at = y as usize * self.width as usize + rect.x as usize + gx;
                self.cells[at] = Cell::blank();
            }
        }
        true
    }

    /// One row of [`write_lines`](Self::write_lines) and
    /// [`write_runs`](Self::write_runs): `pieces` of text, each in a style
    /// of this screen, from the left of `rect` on row `y`; the rest of the
    /// row is cleared.
    fn write_row<'t>(
        &mut self,
        rect: Rect,
        y: u16,
        pieces: impl Iterator<Item = (&'t str, StyleId)>,
        spans: &mut Vec<(usize, usize, usize)>,
    ) {
        let mut x = rect.x;
        // What a zero-width grapheme joins: the cell last written, or,
        // before any, the row's first (`lead` gathers them); none once
        // the row is cut at the edge.
        let mut last: Option<u16> = None;
        let mut lead = String::new();
        let mut cut = false;
        for (text, id) in pieces {
            // One look at the text: ASCII printables pass at once.
            let (mut simple, mut controls) = (true, false);
            for c in text.chars() {
                if (' '..='~').contains(&c) {
                    continue;
                }
                if super::control_picture(c).is_some() {
                    controls = true;
                } else if !matches!(char_width(c), 1 | 2) || c == '\u{200d}' || c == '\u{fe0f}' {
                    simple = false;
                }
            }
            if simple && !controls {
                // A character a grapheme: straight into cells.
                let mut chars = text.chars();
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
                sanitized = text
                    .chars()
                    .map(|c| super::control_picture(c).unwrap_or(c))
                    .collect::<String>();
                sanitized.as_str()
            } else {
                text
            };
            spans.clear();
            spans.extend(rich::cells::split_graphemes(text).0);
            for &(from, to, width) in spans.iter() {
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

    /// One grapheme `width` cells wide at `x` of row `y`, for
    /// [`write_row`](Self::write_row): cut to a space at
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
        if self.scrolled.len() >= 8 {
            self.scrolled.remove(0);
        }
        self.scrolled.push((rect, rows));
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

/// The cells `c` takes, as rich measures it. rich answers the characters
/// pictures and most text are made of (ASCII, Latin letters, box drawing,
/// blocks, Braille, CJK ideographs, Hangul) without its table search, the
/// same in every Unicode version its tables cover.
#[inline]
pub(crate) fn char_width(c: char) -> usize {
    rich::cells::char_cell_width(c)
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
    fn row(&self, y: u16) -> &[Cell] {
        let width = self.width as usize;
        &self.cells[y as usize * width..(y as usize + 1) * width]
    }

    /// Move rows `top`..`bottom` by `by` (up when positive), as the
    /// terminal did: the rows it opened are blank.
    fn shift(&mut self, top: u16, bottom: u16, by: i32) {
        let width = self.width as usize;
        let rows = (bottom - top) as i32;
        let n = by.unsigned_abs() as u16;
        if by > 0 {
            for y in top..bottom - n {
                let from = (y + n) as usize * width;
                self.cells
                    .copy_within(from..from + width, y as usize * width);
            }
        } else {
            for y in (top + n..bottom).rev() {
                let from = (y - n) as usize * width;
                self.cells
                    .copy_within(from..from + width, y as usize * width);
            }
        }
        let opened = if by > 0 {
            bottom - n..bottom
        } else {
            top..top + n
        };
        debug_assert!(rows > by.abs());
        for y in opened {
            let row = y as usize * width;
            self.cells[row..row + width].fill(Cell::blank());
        }
    }
}

/// How far from its row a changed row is looked for among the rows sent,
/// to find a move.
const SCROLL_WINDOW: u16 = 32;

/// Rows `top`..`bottom` that moved together by `by` rows (up when
/// positive) since they were sent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Shift {
    top: u16,
    bottom: u16,
    by: i32,
    /// Rows the move puts right that were wrong.
    gain: u16,
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
/// assert_eq!(PaintStrategy::default(), PaintStrategy::Best);
/// assert_eq!(PaintStrategy::Scroll.name(), "scroll");
/// assert!("fastest".parse::<PaintStrategy>().is_err());
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PaintStrategy {
    /// Compare each cell inside the damage with what was sent, and write
    /// the cells that changed. A bare [`Painter`]'s strategy.
    Cells,
    /// Compare whole rows, and rewrite each
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
    /// within a bound so choosing never costs more than it saves. The
    /// default, and an intuiTUIve app's unless it chooses: within 5% of
    /// the best fixed strategy on every scene of the benchmark.
    #[default]
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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

impl Default for PaintStats {
    /// Nothing counted, under [`PaintStrategy::Cells`] (`strategy` is never
    /// `Best`).
    fn default() -> PaintStats {
        PaintStats {
            compared: 0,
            written: 0,
            elided: 0,
            move_bytes: 0,
            style_bytes: 0,
            text_bytes: 0,
            other_bytes: 0,
            strategy: PaintStrategy::Cells,
        }
    }
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
    /// True-colour pairs' parameters, per style id.
    rgb_codes: RgbCodes,
    /// The SGR written to go from one cached style's parameters to
    /// another's (by their ids): a frame goes between a few styles many
    /// times, so each change is worked out once.
    changes: SgrChanges,
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
    /// Rows may be moved with the terminal's scrolling.
    scroll: bool,
    /// Where nothing may be scrolled through (a picture the terminal does
    /// not move with its text).
    fixed: Vec<Rect>,
    /// The move [`PaintStrategy::Best`] found for this frame.
    found: Option<Shift>,
}

/// The pen's state while a frame is written.
#[derive(Default)]
struct Pen {
    at: Option<(u16, u16)>,
    style: Option<StyleId>,
    link: Option<String>,
    /// Whose SGR parameters are in force (unless `rgb` says).
    held: Held,
    /// The true-colour pair in force, when the last style set was one:
    /// then it, not `codes`, says what the pen holds.
    rgb: Option<(Option<Rgb>, Option<Rgb>)>,
}

/// Whose SGR parameters a [`Pen`] holds: a style's, cached in the
/// painter, so moving the pen copies nothing.
#[derive(Clone, Copy, Default)]
enum Held {
    /// Not known: before the first.
    #[default]
    Unknown,
    /// None: after a reset.
    Reset,
    /// The cached parameters of this style id.
    Style(StyleId),
}

/// No parameters: what a reset leaves.
static NO_SGR: Sgr = Sgr {
    full: String::new(),
    fg: String::new(),
    bg: String::new(),
    rest: String::new(),
};

/// The parameters `held` names, from `codes` (the painter's cache).
fn held_codes(codes: &[Option<Sgr>], held: Held) -> Option<&Sgr> {
    match held {
        Held::Unknown => None,
        Held::Reset => Some(&NO_SGR),
        Held::Style(id) => codes.get(id.index()).and_then(Option::as_ref),
    }
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
    /// A painter for `system`, under [`PaintStrategy::Cells`] until
    /// [`set_strategy`](Self::set_strategy): it moves no rows until the
    /// caller, who knows where pictures sit and whether tmux is in the way,
    /// asks it to. An intuiTUIve app paints with `best`.
    pub fn new(system: Option<ColorSystem>) -> Painter {
        Painter {
            previous: None,
            system,
            codes: Vec::new(),
            scratch: Sgr::default(),
            rgb_codes: RgbCodes::default(),
            changes: SgrChanges::default(),
            inline: false,
            cursor: None,
            height: 0,
            force: false,
            written: Vec::new(),
            stats: PaintStats::default(),
            // A bare painter knows nothing of pictures or tmux (an app
            // tells it, through `set_fixed` and `set_scroll`), so it moves
            // no rows unless asked.
            strategy: PaintStrategy::Cells,
            erase: true,
            // Windows consoles have been seen to scroll the wrong rows.
            scroll: !cfg!(windows),
            fixed: Vec::new(),
            found: None,
        }
    }

    /// Whether rows may be moved with the terminal's own scrolling (SU, SD
    /// and DECSTBM) under [`PaintStrategy::Scroll`] and
    /// [`PaintStrategy::Best`]: on by default but on Windows. A terminal
    /// multiplexer that redraws scroll regions itself gains little.
    pub fn set_scroll(&mut self, on: bool) {
        self.scroll = on;
    }

    /// Rectangles no scroll may move through (Sixel or iTerm2 pictures,
    /// which a terminal does not move with the rows); until the next call.
    pub fn set_fixed(&mut self, rects: &[Rect]) {
        self.fixed.clear();
        self.fixed.extend_from_slice(rects);
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
            self.changes.clear();
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
        self.rgb_codes.clear();
        self.changes.clear();
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
        let mut out = String::new();
        self.paint_into(screen, damage, &mut out);
        out
    }

    /// [`paint`](Self::paint), appending to `out`: one buffer for a whole
    /// frame.
    pub fn paint_into(&mut self, screen: &Screen, damage: &[Rect], out: &mut String) {
        self.written.clear();
        self.stats = PaintStats::default();
        let base = out.len();
        let first = !matches!(&self.previous, Some(p) if p.width == screen.width && p.height == screen.height);
        let mut rects: Vec<Rect> = if first {
            if self.inline {
                out.push_str(&self.reserve(screen.height));
            } else {
                out.push_str("\x1b[0m\x1b[H\x1b[2J");
            }
            self.previous = None;
            self.codes.clear();
            self.rgb_codes.clear();
            self.changes.clear();
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
                .filter(|r| !r.is_empty())
                .collect()
        };
        let mut pen = Pen {
            at: self.cursor,
            ..Pen::default()
        };
        let mut strategy = match self.strategy {
            _ if first || self.force => PaintStrategy::Cells,
            PaintStrategy::Best => self.choose(screen, &rects),
            other => other,
        };
        if strategy == PaintStrategy::Scroll {
            let found = match self.found.take() {
                Some(shift) => Some(shift),
                None => self.find_shift(screen, &rects),
            };
            match found {
                Some(shift) => {
                    let before = out.len();
                    self.scroll_by(shift, screen, &mut pen, out);
                    self.stats.other_bytes += out.len() - before;
                    rects.push(Rect::new(
                        0,
                        shift.top,
                        screen.width,
                        shift.bottom - shift.top,
                    ));
                }
                None => strategy = PaintStrategy::Cells,
            }
        }
        let forced = self.force;
        if strategy == PaintStrategy::Full {
            rects = vec![screen.area()];
            self.force = true;
        }
        for (y, spans) in row_spans(&rects, screen.height) {
            if strategy == PaintStrategy::Rows {
                self.paint_row(screen, y, &mut pen, out);
                continue;
            }
            for (start, end) in spans {
                // A span the same as what was sent is passed over whole: a
                // row of cells compares as one slice.
                if !self.force {
                    if let Some(previous) = &self.previous {
                        let width = screen.width as usize;
                        let row = y as usize * width;
                        let span = row + start as usize..row + end as usize;
                        if previous.cells[span.clone()] == screen.cells[span] {
                            self.stats.compared += usize::from(end - start);
                            continue;
                        }
                    }
                }
                self.paint_span(screen, y, start, end, &mut pen, out);
            }
        }
        self.force = forced;
        let before = out.len();
        if pen.link.is_some() {
            out.push_str("\x1b]8;;\x1b\\");
        }
        if pen.rgb.is_some()
            || held_codes(&self.codes, pen.held).is_some_and(|c| !c.full.is_empty())
        {
            out.push_str("\x1b[0m");
        }
        self.stats.style_bytes += out.len() - before;
        let stats = &mut self.stats;
        stats.strategy = strategy;
        stats.elided = stats.compared.saturating_sub(stats.written);
        stats.other_bytes =
            out.len() - base - stats.move_bytes - stats.style_bytes - stats.text_bytes;
        if self.inline {
            self.cursor = pen.at.or(self.cursor);
        }
        // Bring the copy of what was sent up to date.
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
                        previous.cells[span.clone()].copy_from_slice(&screen.cells[span]);
                    }
                }
            }
        }
    }

    /// [`PaintStrategy::Best`]'s pick for this frame.
    ///
    /// Choosing must cost less than it saves, so it encodes nothing twice:
    /// a frame where rows moved together scrolls them (rows compare as
    /// slices of plain cells, so finding the move is cheap); any other is
    /// painted cell by cell. The benchmark found `rows` never more than a
    /// few percent shorter once the cell encoder writes through short
    /// gaps, so it is not tried; nor is `full` for a frame where nearly
    /// every cell changed, as painting cells by cell skips the unchanged
    /// ones with fewer bytes and, on the benchmark's machines, no more
    /// time (`full` was up to 17% slower on CI's).
    fn choose(&mut self, screen: &Screen, rects: &[Rect]) -> PaintStrategy {
        self.found = self.find_shift(screen, rects);
        match self.found {
            Some(_) => PaintStrategy::Scroll,
            None => PaintStrategy::Cells,
        }
    }

    /// [`PaintStrategy::Rows`] on row `y`: when it differs from what was
    /// sent, every cell from its first change to its last.
    fn paint_row(&mut self, screen: &Screen, y: u16, pen: &mut Pen, out: &mut String) {
        let Some(previous) = &self.previous else {
            return;
        };
        let (sent, now) = (previous.row(y), screen.row(y));
        let width = screen.width as usize;
        self.stats.compared += width;
        let Some(first) = (0..width).find(|&x| sent[x] != now[x]) else {
            return;
        };
        let last = (0..width).rfind(|&x| sent[x] != now[x]).unwrap_or(first);
        // A wide grapheme's second column is written with its first.
        let end = (last + 1 + usize::from(now.get(last).is_some_and(|c| c.width >= 2))).min(width);
        let forced = self.force;
        self.force = true;
        let compared = self.stats.compared;
        self.paint_span(screen, y, first as u16, end as u16, pen, out);
        // Counted once, above.
        self.stats.compared = compared;
        self.force = forced;
    }

    /// The best run of rows that moved together since they were sent, if
    /// scrolling is allowed and one is worth it: the shift that puts the
    /// most changed rows right, looking first at the scrolls the screen
    /// was told of.
    fn find_shift(&mut self, screen: &Screen, rects: &[Rect]) -> Option<Shift> {
        if !self.scroll || self.inline {
            return None;
        }
        let previous = self.previous.as_ref()?;
        let (top, bottom) = rects
            .iter()
            .fold((u16::MAX, 0), |(t, b), r| (t.min(r.y), b.max(r.bottom())));
        if bottom <= top || bottom - top < 3 {
            return None;
        }
        let height = screen.height;
        // Rows compare as slices of plain cells: a pair that differs stops
        // at its first cell, so looking for where a row went is cheap.
        let same = |y: u16, source: u16| screen.row(y) == previous.row(source);
        // Candidate moves: those the screen was told of, then those the
        // first changed rows vote for (where each is found among the rows
        // sent near it).
        let mut votes: Vec<(i32, u16)> = Vec::new();
        let mut vote = |by: i32, n: u16| match votes.iter_mut().find(|(b, _)| *b == by) {
            Some((_, count)) => *count += n,
            None => votes.push((by, n)),
        };
        for &(_, rows) in &screen.scrolled {
            vote(i32::from(rows), height);
        }
        // Only rows inside the damage can have changed.
        let mut damaged = vec![false; height as usize];
        for rect in rects {
            for y in rect.y..rect.bottom().min(height) {
                damaged[y as usize] = true;
            }
        }
        let mut changed = 0;
        let mut found: Vec<u16> = Vec::new();
        for y in (top..bottom).filter(|&y| damaged[y as usize]) {
            if same(y, y) {
                continue;
            }
            changed += 1;
            found.clear();
            'look: for d in 1..=SCROLL_WINDOW.min(height) {
                for source in [y.checked_add(d), y.checked_sub(d)].into_iter().flatten() {
                    if source < height && same(y, source) {
                        found.push(source);
                        // A row found in many places (a blank one) says
                        // nothing of a move.
                        if found.len() > 8 {
                            break 'look;
                        }
                    }
                }
            }
            if found.len() <= 8 {
                for &source in &found {
                    vote(i32::from(source) - i32::from(y), 1);
                }
            }
            if changed >= 6 {
                break;
            }
        }
        if changed < 3 || votes.is_empty() {
            return None;
        }
        let mut best: Option<Shift> = None;
        let consider = |by: i32, best: &mut Option<Shift>| {
            // The longest run of rows y showing what row y + by showed.
            let mut run_start = None;
            let mut y = 0i32;
            while y <= height as i32 {
                let source = y + by;
                let matches = y < height as i32
                    && (0..height as i32).contains(&source)
                    && same(y as u16, source as u16);
                match (matches, run_start) {
                    (true, None) => run_start = Some(y),
                    (false, Some(start)) => {
                        run_start = None;
                        let gain = (start..y).filter(|&r| !same(r as u16, r as u16)).count() as u16;
                        let (top, bottom) = if by > 0 {
                            (start as u16, (y + by) as u16)
                        } else {
                            ((start + by) as u16, y as u16)
                        };
                        if best.is_none_or(|b| gain > b.gain) {
                            *best = Some(Shift {
                                top,
                                bottom,
                                by,
                                gain,
                            });
                        }
                    }
                    _ => {}
                }
                y += 1;
            }
        };
        votes.sort_unstable_by_key(|&(by, n)| (std::cmp::Reverse(n), by.abs()));
        for &(by, _) in votes.iter().take(3) {
            consider(by, &mut best);
        }
        let shift = best?;
        // Worth a scroll: it puts at least two rows right, and more than it
        // opens.
        if shift.gain < 2 || i32::from(shift.gain) <= shift.by.abs() {
            return None;
        }
        // No picture may sit in the rows that move.
        let region = Rect::new(0, shift.top, screen.width, shift.bottom - shift.top);
        if self.fixed.iter().any(|r| r.intersects(region)) {
            return None;
        }
        Some(shift)
    }

    /// Move rows with the terminal's scrolling: the whole screen with SU or
    /// SD, else inside DECSTBM margins (which home the cursor), the pen's
    /// colours reset first so the rows opened are blank. The copy of what
    /// was sent moves with them.
    fn scroll_by(&mut self, shift: Shift, screen: &Screen, pen: &mut Pen, out: &mut String) {
        out.push_str("\x1b[0m");
        pen.held = Held::Reset;
        pen.rgb = None;
        pen.style = None;
        let whole = shift.top == 0 && shift.bottom == screen.height;
        if !whole {
            out.push_str("\x1b[");
            push_u16(out, shift.top + 1);
            out.push(';');
            push_u16(out, shift.bottom);
            out.push('r');
        }
        let n = shift.by.unsigned_abs() as u16;
        csi(out, n, if shift.by > 0 { 'S' } else { 'T' });
        if !whole {
            out.push_str("\x1b[r");
        }
        // Where the cursor is now is the terminal's business.
        pen.at = None;
        if let Some(previous) = &mut self.previous {
            previous.shift(shift.top, shift.bottom, shift.by);
        }
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
            // Past the cells the same as what was sent, a run at a time.
            if !self.force {
                if let Some(previous) = &self.previous {
                    let row = y as usize * screen.width as usize;
                    let (from, to) = (row + x as usize, row + end as usize);
                    let same = previous.cells[from..to]
                        .iter()
                        .zip(&screen.cells[from..to])
                        .take_while(|(sent, now)| sent == now)
                        .count();
                    self.stats.compared += same;
                    x += same as u16;
                    if x >= end {
                        break;
                    }
                }
            }
            let mut x0 = x;
            if screen.cell(x0, y).is_continuation() && x0 > 0 {
                x0 -= 1;
            }
            // A run of blanks on the terminal's own background: erased
            // (EL to the end of the row, ECH within it) when that is
            // shorter than writing it.
            if self.erase && !self.inline && self.plain_blank(screen, x0, y) {
                // Blanks the pen would skip on its way here join the run,
                // so the erase starts where the pen is.
                if let Some((fx, fy)) = pen.at {
                    if fy == y && fx < x0 && (fx..x0).all(|gx| self.plain_blank(screen, gx, y)) {
                        self.stats.compared += usize::from(x0 - fx);
                        x = fx;
                        x0 = fx;
                    }
                }
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
            // An emoji with a selector or joiners is a width terminals
            // disagree on: where the cursor ended up is not known, so the
            // next write moves there.
            let several = match cell.single_char() {
                Some(c) => {
                    out.push(c);
                    self.stats.text_bytes += c.len_utf8();
                    c == '\u{200d}' || c == '\u{fe0f}'
                }
                None => {
                    let text = cell.text();
                    out.push_str(text);
                    self.stats.text_bytes += text.len();
                    cell.len == LONG || text.contains(['\u{200d}', '\u{fe0f}'])
                }
            };
            let next = x0 + cell.width.max(1) as u16;
            let columns = usize::from(next - x0);
            self.stats.compared += usize::from(next - x);
            self.stats.written += columns;
            self.note_written(x0, y, next);
            pen.at = (!several).then_some((next, y));
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
        self.goto_into(pen.at, x, y, screen.width, out);
        let jump = out.len() - before;
        // Writing the unchanged cells between again (what the terminal
        // shows already) can be shorter than the move, as Mosh does: a few
        // narrow cells, counting the style changes they need.
        if let Some((fx, fy)) = pen.at.filter(|&(fx, fy)| {
            !self.inline && fy == y && fx < x && x - fx <= 8 && jump > usize::from(x - fx)
        }) {
            let mut cost = 0;
            let mut style = pen.style;
            for gx in fx..x {
                let cell = screen.cell(gx, fy);
                if cell.width != 1 || cell.len == LONG || cell.len == 0 {
                    cost = usize::MAX;
                    break;
                }
                cost += cell.len as usize;
                if style != Some(cell.style) {
                    style = Some(cell.style);
                    // A change of style: at most a reset and its parameters.
                    cost += match screen.styles.rgb(cell.style) {
                        Some(_) => 40,
                        None if cell.style == StyleId::NONE => 4,
                        None => 4 + self.sgr(screen, cell.style).full.len(),
                    };
                }
                if cost >= jump {
                    break;
                }
            }
            if style != pen.style && cost < jump {
                // Coming back to the pen's style costs too.
                cost += 8;
            }
            if cost < jump {
                out.truncate(before);
                for gx in fx..x {
                    let cell = *screen.cell(gx, y);
                    self.set_pen(screen, cell.style, pen, out);
                    out.push_str(cell.text());
                }
            }
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
        // A colour pair (no link) in true colour while no link is open:
        // straight to the colours.
        if pen.link.is_none() {
            if let (Some(pair), Some(ColorSystem::Truecolor), Some(old)) =
                (screen.styles.rgb(style), self.system, pen.rgb)
            {
                if old != pair {
                    rgb_delta(old, pair, self.rgb_codes.get(style, pair), out);
                }
                pen.rgb = Some(pair);
                pen.style = Some(style);
                return;
            }
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
                Some(old) => rgb_delta(old, pair, self.rgb_codes.get(style, pair), out),
                None => {
                    self.scratch.set_rgb(pair.0, pair.1);
                    self.scratch
                        .write_from(held_codes(&self.codes, pen.held), out);
                }
            }
            pen.rgb = Some(pair);
            pen.style = Some(style);
            return;
        }
        if let (None, Held::Style(held)) = (pen.rgb, pen.held) {
            // From one cached style to another: worked out once.
            if let Some(change) = self.changes.get(held, style) {
                out.push_str(change);
                pen.held = Held::Style(style);
                pen.style = Some(style);
                return;
            }
            self.sgr(screen, style);
            let codes = self.codes[style.index()].as_ref().expect("cached above");
            let before = out.len();
            codes.write_from(held_codes(&self.codes, pen.held), out);
            self.changes.put(held, style, &out[before..]);
            pen.held = Held::Style(style);
            pen.style = Some(style);
            return;
        }
        self.sgr(screen, style);
        let codes = self.codes[style.index()].as_ref().expect("cached above");
        let from = match pen.rgb.take() {
            Some((fg, bg)) => {
                // What the pen holds, as parameters again.
                self.scratch.set_rgb(fg, bg);
                Some(&self.scratch)
            }
            None => held_codes(&self.codes, pen.held),
        };
        // Styles that differ only in their link share colours: no SGR
        // (`write_from` writes nothing for parameters already in force).
        codes.write_from(from, out);
        pen.held = Held::Style(style);
        pen.style = Some(style);
    }
}

/// The SGR that changes a true-colour pen from `old` to `new`: only the
/// colours that differ (a colour gone is its default, 39 or 49). `sgr` is
/// `new`'s, encoded.
fn rgb_delta(
    old: (Option<Rgb>, Option<Rgb>),
    new: (Option<Rgb>, Option<Rgb>),
    sgr: RgbSgr<'_>,
    out: &mut String,
) {
    let fg = old.0 != new.0;
    let bg = old.1 != new.1;
    if fg && bg && new.0.is_some() && new.1.is_some() {
        out.push_str(sgr.whole);
        return;
    }
    out.push_str("\x1b[");
    let mut first = true;
    for (changed, now, code, default) in [(fg, new.0, sgr.fg, "39"), (bg, new.1, sgr.bg, "49")] {
        if !changed {
            continue;
        }
        if !first {
            out.push(';');
        }
        first = false;
        out.push_str(if now.is_some() { code } else { default });
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

/// The SGR a [`Painter`] wrote to change its pen from one cached style's
/// parameters to another's, by the two ids, in one buffer. Emptied with
/// the parameters' cache, and when it holds more than [`SgrChanges::MAX`]
/// bytes.
#[derive(Default)]
struct SgrChanges {
    text: String,
    at: HashMap<(u32, u32), (u32, u32), FxBuild>,
}

impl SgrChanges {
    const MAX: usize = 1 << 16;

    fn clear(&mut self) {
        self.text.clear();
        self.at.clear();
    }

    fn get(&self, from: StyleId, to: StyleId) -> Option<&str> {
        let &(start, end) = self.at.get(&(from.index() as u32, to.index() as u32))?;
        self.text.get(start as usize..end as usize)
    }

    fn put(&mut self, from: StyleId, to: StyleId, change: &str) {
        if self.text.len() + change.len() > SgrChanges::MAX {
            self.clear();
        }
        let start = self.text.len() as u32;
        self.text.push_str(change);
        let key = (from.index() as u32, to.index() as u32);
        self.at.insert(key, (start, self.text.len() as u32));
    }
}

/// A true-colour pair's SGR, encoded once per style id: where it starts
/// in [`RgbCodes::text`], and how long the foreground's and the
/// background's parameters are.
#[derive(Clone, Copy)]
struct RgbAt {
    at: u32,
    fg: u8,
    bg: u8,
}

/// The encoded SGR of the true-colour pairs a painter has written, by
/// style id: a colour frame repeats its pairs, so each is formatted once,
/// not once a cell. Each is kept whole (`ESC [ 38;2;r;g;b ; 48;2;r;g;b m`),
/// so a change of both colours is one copy, and either colour's
/// parameters are a slice of it.
#[derive(Default)]
struct RgbCodes {
    text: String,
    at: Vec<Option<RgbAt>>,
}

/// What [`RgbCodes::get`] hands back: the whole SGR, and the foreground's
/// and the background's parameters (empty when missing).
struct RgbSgr<'a> {
    whole: &'a str,
    fg: &'a str,
    bg: &'a str,
}

impl RgbCodes {
    fn clear(&mut self) {
        self.text.clear();
        self.at.clear();
    }

    /// Style `id`'s SGR, its colours `pair`.
    fn get(&mut self, id: StyleId, pair: (Option<Rgb>, Option<Rgb>)) -> RgbSgr<'_> {
        if self.at.len() <= id.index() {
            self.at.resize(id.index() + 1, None);
        }
        let at = match self.at[id.index()] {
            Some(at) => at,
            None => {
                let start = self.text.len();
                self.text.push_str("\x1b[");
                if let Some(fg) = pair.0 {
                    rgb_code(&mut self.text, "38;2;", fg);
                }
                let middle = self.text.len();
                if pair.0.is_some() && pair.1.is_some() {
                    self.text.push(';');
                }
                let second = self.text.len();
                if let Some(bg) = pair.1 {
                    rgb_code(&mut self.text, "48;2;", bg);
                }
                let end = self.text.len();
                self.text.push('m');
                let at = RgbAt {
                    at: start as u32,
                    fg: (middle - start - 2) as u8,
                    bg: (end - second) as u8,
                };
                self.at[id.index()] = Some(at);
                at
            }
        };
        let start = at.at as usize;
        let fg = start + 2..start + 2 + at.fg as usize;
        let joint = usize::from(at.fg > 0 && at.bg > 0);
        let bg = fg.end + joint..fg.end + joint + at.bg as usize;
        RgbSgr {
            whole: &self.text[start..bg.end + 1],
            fg: &self.text[fg],
            bg: &self.text[bg],
        }
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
