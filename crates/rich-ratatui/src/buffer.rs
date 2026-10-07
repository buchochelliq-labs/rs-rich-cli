//! Converting between rich lines and ratatui buffers.
//!
//! - [`lines_to_buffer`] writes rich lines (`Vec<Vec<Segment>>`, what
//!   `Console::render_lines` returns) into a rectangle of a ratatui
//!   [`Buffer`]. [`RichWidget`](crate::RichWidget) is built on it.
//! - [`buffer_to_lines`] and [`buffer_area_to_lines`] read a buffer (or a
//!   rectangle of one) back into rich lines.
//! - [`cells`] walks a rectangle of a buffer cell by cell, without
//!   allocating, for a consumer with its own cell screen: each
//!   [`BufferCell`] is a leading cell with its column, row, symbol, width and
//!   [`CellStyle`], and a [`StyleCache`] turns those styles into rich
//!   [`Style`]s once each.

use std::collections::HashMap;

use ratatui_core::buffer::{Buffer, Cell, CellDiffOption, CellWidth};
use ratatui_core::layout::Rect;
use ratatui_core::style::{Color as RColor, Modifier, Style as RStyle};
use rich::{Segment, Style};

use crate::style::{to_ratatui_style, to_rich_style};

// ---------------------------------------------------------------------------
// rich lines → ratatui buffer
// ---------------------------------------------------------------------------

/// Write rich lines into `area` of `buffer`: line *i* on row `area.y + i`,
/// at most `area.height` lines, each cropped to `area.width` columns. The
/// area is first clipped to the buffer's own.
///
/// Each segment is placed at the column rich measured for it
/// ([`rich::cells::cell_len`]), not where ratatui's own width table would
/// put it, so the two disagreeing about a character (some emoji) shifts
/// nothing after that segment. A wide character that would straddle the
/// right edge is replaced by a space, as rich's own cropping does, and cells
/// left over in a segment are filled with spaces in its style.
///
/// Styles *patch* the cells (ratatui's convention, as its `Paragraph` does):
/// an unstyled segment keeps the background a `Block` painted beneath it.
/// Cells right of a short line, and rows below the last line, are left
/// untouched. Control segments are skipped. Hyperlinks and the attributes
/// ratatui lacks are dropped (see [`to_ratatui_style`]).
pub fn lines_to_buffer(lines: &[Vec<Segment>], area: Rect, buffer: &mut Buffer) {
    let area = area.intersection(buffer.area);
    let right = area.right();
    for (line, y) in lines.iter().zip(area.top()..area.bottom()) {
        let mut x = area.x;
        for segment in line {
            if x >= right {
                break;
            }
            if segment.control || segment.text.is_empty() {
                continue;
            }
            let style = segment
                .style
                .as_ref()
                .map(to_ratatui_style)
                .unwrap_or_default();
            let width = rich::cells::cell_len(&segment.text);
            let width = u16::try_from(width).unwrap_or(u16::MAX);
            let end = right.min(x.saturating_add(width));
            let (written, _) = buffer.set_stringn(x, y, &segment.text, usize::from(end - x), style);
            // Pad what ratatui did not fill: a wide character cut at the
            // edge, or a character ratatui measures narrower than rich does.
            for pad in written..end {
                buffer[(pad, y)].set_symbol(" ").set_style(style);
            }
            x = end;
        }
    }
}

// ---------------------------------------------------------------------------
// Cell-level reading
// ---------------------------------------------------------------------------

/// A buffer cell's style, as [`cells`] reports it: a small `Copy` key, so a
/// consumer can compare and hash it per cell and convert it to a rich
/// [`Style`] only when it changes (or once, through a [`StyleCache`]).
///
/// The underline colour is not part of it (rich has none).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CellStyle {
    /// The foreground colour; [`Reset`](RColor::Reset) when the cell sets none.
    pub fg: RColor,
    /// The background colour; [`Reset`](RColor::Reset) when the cell sets none.
    pub bg: RColor,
    /// The attributes that are on.
    pub modifier: Modifier,
}

impl CellStyle {
    /// The style of `cell`.
    pub fn of(cell: &Cell) -> CellStyle {
        CellStyle {
            fg: cell.fg,
            bg: cell.bg,
            modifier: cell.modifier,
        }
    }

    /// Whether the cell is unstyled: both colours `Reset`, no attributes.
    pub fn is_plain(self) -> bool {
        self.fg == RColor::Reset && self.bg == RColor::Reset && self.modifier.is_empty()
    }

    /// The rich style of a cell with this style, or `None` for a plain cell.
    ///
    /// **Lossy:** [`Reset`](RColor::Reset) reads as *unset* here, not as
    /// rich's `default`. Every buffer cell has a colour, and a fresh one has
    /// `Reset`, so reading it as `default` would style every blank cell; an
    /// explicit `default` drawn into a buffer therefore comes back unset.
    pub fn to_rich_style(self) -> Option<Style> {
        let set = |c: RColor| (c != RColor::Reset).then_some(c);
        to_rich_style(RStyle {
            fg: set(self.fg),
            bg: set(self.bg),
            add_modifier: self.modifier,
            ..RStyle::default()
        })
    }
}

/// Converts [`CellStyle`]s into rich [`Style`]s, each distinct one once.
///
/// Building a rich `Style` allocates (colour names are strings), and a
/// screen has few distinct styles and many cells, so a consumer converting
/// cells keeps one cache for the whole buffer, or for its lifetime.
#[derive(Debug, Default)]
pub struct StyleCache {
    styles: HashMap<CellStyle, Option<Style>>,
}

impl StyleCache {
    /// An empty cache.
    pub fn new() -> StyleCache {
        StyleCache::default()
    }

    /// The rich style for `style` ([`CellStyle::to_rich_style`]), converted
    /// on first use.
    pub fn get(&mut self, style: CellStyle) -> Option<&Style> {
        self.styles
            .entry(style)
            .or_insert_with(|| style.to_rich_style())
            .as_ref()
    }

    /// How many distinct styles the cache holds.
    pub fn len(&self) -> usize {
        self.styles.len()
    }

    /// Whether the cache holds no styles.
    pub fn is_empty(&self) -> bool {
        self.styles.is_empty()
    }
}

/// One cell of a ratatui buffer, as [`cells`] yields it.
#[derive(Clone, Copy, Debug)]
pub struct BufferCell<'a> {
    /// The column, in the buffer's coordinates.
    pub x: u16,
    /// The row, in the buffer's coordinates.
    pub y: u16,
    /// What to draw: the cell's symbol, or `" "` where [`cells`] substitutes
    /// a space (see there).
    pub symbol: &'a str,
    /// How many columns the symbol covers, at least 1. The columns it
    /// covers after the first are not yielded.
    pub width: u16,
    /// The cell's colours and attributes.
    pub style: CellStyle,
    /// The ratatui cell itself, for what the other fields leave out: the
    /// underline colour, and [`Cell::diff_option`] (a
    /// [`ForcedWidth`](CellDiffOption::ForcedWidth) symbol may carry escape
    /// sequences, such as a hyperlink, that only the terminal understands).
    pub cell: &'a Cell,
}

impl BufferCell<'_> {
    /// Whether the cell's width was forced
    /// ([`CellDiffOption::ForcedWidth`]): its symbol is not measured, and
    /// may hold escape sequences rather than plain text.
    pub fn is_forced_width(&self) -> bool {
        matches!(self.cell.diff_option, CellDiffOption::ForcedWidth(_))
    }
}

/// The cells of `area` in `buffer`, row by row, left to right.
///
/// `area` is clipped to the buffer's own. Every column of every row is
/// covered exactly once: each yielded cell starts at `x` and covers `width`
/// columns, and the next one starts at `x + width`, so a row's widths sum
/// to the area's width. In particular:
///
/// - a wide symbol's trailing cells are skipped. ratatui does not mark them
///   (in 0.30, `set_stringn` resets them to blank cells), so they are found
///   as ratatui's own diff finds them, by the leading symbol's width
///   (a [`ForcedWidth`](CellDiffOption::ForcedWidth) counts);
/// - a wide symbol that would run past the area's right edge (because the
///   area cuts it, or because it was set by hand in the last column) is
///   yielded as spaces of width 1, in its style, up to the edge;
/// - a zero-width symbol (an empty string) is yielded as one space.
///
/// Nothing is allocated: symbols borrow from the buffer, and styles are
/// [`CellStyle`] keys. To convert a rectangle into another cell screen:
///
/// ```
/// use ratatui_core::buffer::Buffer;
/// use ratatui_core::layout::Rect;
/// use ratatui_core::style::{Color, Style};
/// use rich_ratatui::buffer::{cells, StyleCache};
///
/// let mut buffer = Buffer::empty(Rect::new(0, 0, 10, 2));
/// buffer.set_string(0, 0, "日本 ok", Style::default().fg(Color::Red));
///
/// let mut styles = StyleCache::new();
/// let mut row = String::new();
/// for cell in cells(&buffer, Rect::new(0, 0, 6, 1)) {
///     // Hand cell.x, cell.y, cell.symbol, cell.width and the style to
///     // your own screen; here, just collect the text.
///     let _style: Option<&rich::Style> = styles.get(cell.style);
///     row.push_str(cell.symbol);
/// }
/// assert_eq!(row, "日本 o");
/// assert_eq!(styles.len(), 1);
/// ```
pub fn cells(buffer: &Buffer, area: Rect) -> Cells<'_> {
    let area = area.intersection(buffer.area);
    Cells {
        buffer,
        area,
        x: area.x,
        y: area.y,
        overflow: None,
    }
}

/// The iterator [`cells`] returns.
#[derive(Debug, Clone)]
pub struct Cells<'a> {
    buffer: &'a Buffer,
    area: Rect,
    x: u16,
    y: u16,
    /// While padding the rest of a row after a wide symbol that ran past the
    /// edge: the style to pad in and the cell it came from.
    overflow: Option<(CellStyle, &'a Cell)>,
}

impl<'a> Iterator for Cells<'a> {
    type Item = BufferCell<'a>;

    fn next(&mut self) -> Option<BufferCell<'a>> {
        let right = self.area.right();
        if self.area.is_empty() {
            return None;
        }
        if self.x >= right {
            self.x = self.area.x;
            self.y += 1;
            self.overflow = None;
        }
        if self.y >= self.area.bottom() {
            return None;
        }
        let (x, y) = (self.x, self.y);
        if let Some((style, cell)) = self.overflow {
            self.x += 1;
            return Some(space(x, y, style, cell));
        }
        let cell = &self.buffer[(x, y)];
        let style = CellStyle::of(cell);
        let width = cell.cell_width();
        if width == 0 {
            self.x += 1;
            return Some(space(x, y, style, cell));
        }
        if x.saturating_add(width) > right {
            self.overflow = Some((style, cell));
            self.x += 1;
            return Some(space(x, y, style, cell));
        }
        self.x += width;
        Some(BufferCell {
            x,
            y,
            symbol: cell.symbol(),
            width,
            style,
            cell,
        })
    }
}

fn space<'a>(x: u16, y: u16, style: CellStyle, cell: &'a Cell) -> BufferCell<'a> {
    BufferCell {
        x,
        y,
        symbol: " ",
        width: 1,
        style,
        cell,
    }
}

// ---------------------------------------------------------------------------
// ratatui buffer → rich lines
// ---------------------------------------------------------------------------

/// Convert every row of `buffer` into a rich line, full width.
///
/// The same as [`buffer_area_to_lines`] over the buffer's whole area.
pub fn buffer_to_lines(buffer: &Buffer) -> Vec<Vec<Segment>> {
    buffer_area_to_lines(buffer, buffer.area)
}

/// Convert the rows of `area` in `buffer` into rich lines, one per row, each
/// exactly the area's width in cells (nothing is trimmed, so a view keeps a
/// stable layout). `area` is clipped to the buffer's own.
///
/// Runs of cells with the same style become one [`Segment`]. Cells are read
/// as [`cells`] reads them (wide symbols' trailing cells skipped, a wide
/// symbol cut by the edge as spaces), and styles as
/// [`CellStyle::to_rich_style`] converts them (`Reset` is unset).
///
/// **Lossy:** the underline colour is dropped, and a
/// [`ForcedWidth`](CellDiffOption::ForcedWidth) cell becomes spaces of its
/// width, since its symbol may hold escape sequences rich cannot measure.
pub fn buffer_area_to_lines(buffer: &Buffer, area: Rect) -> Vec<Vec<Segment>> {
    let area = area.intersection(buffer.area);
    let mut lines = vec![Vec::new(); usize::from(area.height)];
    let mut styles = StyleCache::new();
    let mut text = String::with_capacity(usize::from(area.width));
    let mut current: Option<(u16, CellStyle)> = None;
    let mut flush =
        |lines: &mut Vec<Vec<Segment>>, row: u16, style: CellStyle, text: &mut String| {
            let segment = Segment::new(std::mem::take(text), styles.get(style).cloned());
            lines[usize::from(row - area.y)].push(segment);
        };
    for cell in cells(buffer, area) {
        let key = (cell.y, cell.style);
        if current != Some(key) {
            if let Some((row, style)) = current {
                flush(&mut lines, row, style, &mut text);
            }
            current = Some(key);
        }
        if cell.is_forced_width() {
            text.extend(std::iter::repeat_n(' ', usize::from(cell.width)));
        } else {
            text.push_str(cell.symbol);
        }
    }
    if let Some((row, style)) = current {
        flush(&mut lines, row, style, &mut text);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui_core::style::Color as RColor;

    pub(crate) fn plain(lines: &[Vec<Segment>]) -> Vec<String> {
        lines
            .iter()
            .map(|line| line.iter().map(|s| s.text.as_str()).collect())
            .collect()
    }

    fn buffer_rows(buffer: &Buffer) -> Vec<String> {
        plain(&buffer_to_lines(buffer))
    }

    #[test]
    fn wide_character_at_the_last_column_does_not_overflow() {
        // Through lines_to_buffer: rich's line is wider than the area.
        let area = Rect::new(0, 0, 4, 1);
        let mut buffer = Buffer::empty(Rect::new(0, 0, 6, 1));
        buffer.set_string(4, 0, "xy", RStyle::default());
        let line = vec![Segment::new("abc日", Some(Style::parse("bold").unwrap()))];
        lines_to_buffer(&[line], area, &mut buffer);
        assert_eq!(buffer_rows(&buffer), ["abc xy"]);
        assert!(buffer[(3, 0)].modifier.contains(Modifier::BOLD));

        // Through buffer_to_lines: a wide symbol set by hand in the last
        // column becomes a space, so the line stays 3 cells.
        let mut buffer = Buffer::empty(Rect::new(0, 0, 3, 1));
        buffer[(2, 0)].set_symbol("日");
        assert_eq!(buffer_rows(&buffer), ["   "]);
    }

    #[test]
    fn buffer_groups_cells_by_style() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 8, 1));
        buffer.set_string(0, 0, "ab", RStyle::default().fg(RColor::Red));
        buffer.set_string(2, 0, "cd", RStyle::default().fg(RColor::Red));
        let lines = buffer_to_lines(&buffer);
        assert_eq!(lines[0].len(), 2);
        assert_eq!(lines[0][0].text, "abcd");
        assert_eq!(lines[0][1].text, "    ");
        assert_eq!(lines[0][1].style, None);
    }

    #[test]
    fn cells_cover_every_column_of_a_region_once() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 8, 3));
        buffer.set_string(1, 1, "日本x", RStyle::default().fg(RColor::Indexed(9)));
        // A region that cuts 本 in half, inside a larger buffer.
        let area = Rect::new(1, 1, 3, 2);
        let got: Vec<(u16, u16, &str, u16)> = cells(&buffer, area)
            .map(|c| (c.x, c.y, c.symbol, c.width))
            .collect();
        assert_eq!(
            got,
            [
                (1, 1, "日", 2),
                (3, 1, " ", 1),
                (1, 2, " ", 1),
                (2, 2, " ", 1),
                (3, 2, " ", 1),
            ]
        );
        // The cut half keeps 本's style; blank cells are plain.
        let styles: Vec<bool> = cells(&buffer, area).map(|c| c.style.is_plain()).collect();
        assert_eq!(styles, [false, false, true, true, true]);
        // The same region as lines: two rows of three cells.
        let lines = buffer_area_to_lines(&buffer, area);
        assert_eq!(plain(&lines), ["日 ", "   "]);
        // Outside the buffer: nothing.
        assert_eq!(cells(&buffer, Rect::new(20, 20, 4, 4)).count(), 0);
        assert_eq!(
            buffer_area_to_lines(&buffer, Rect::new(20, 20, 4, 4)).len(),
            0
        );
    }

    #[test]
    fn forced_width_cells_become_spaces() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 4, 1));
        buffer[(0, 0)]
            .set_symbol("\x1b]8;;https://example.com\x1b\\ab\x1b]8;;\x1b\\")
            .diff_option = CellDiffOption::ForcedWidth(std::num::NonZeroU16::new(2).unwrap());
        let first = cells(&buffer, buffer.area).next().unwrap();
        assert!(first.is_forced_width());
        assert_eq!(first.width, 2);
        assert_eq!(buffer_rows(&buffer), ["    "]);
    }
}
