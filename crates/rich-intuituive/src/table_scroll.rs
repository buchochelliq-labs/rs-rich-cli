//! A wide table scrolled across (#717): with
//! [`TableOptions::scroll_x`](crate::widgets::TableOptions::scroll_x), a
//! table whose columns' natural widths do not fit keeps them and shows a
//! window onto its lines. The header is cut where the rows are, so it
//! stays over its columns; frozen columns stay at the left; `‹` and `›` in
//! the header say that columns are out of view on that side.

use std::cell::Cell;

use rich::Segment;
use rich_interact::kit::{overlay, slice};

/// What a table keeps of its scroll across between draws.
#[derive(Default)]
pub(crate) struct Across {
    /// The first column in view after the frozen ones.
    pub left: Cell<usize>,
    /// The furthest `left` goes, as of the last draw.
    pub last: Cell<usize>,
    /// Whether the table was wider than its view at the last draw.
    pub wide: Cell<bool>,
    /// The cell cursor's column the view last followed.
    followed: Cell<Option<usize>>,
}

/// One draw's window across a table's lines.
pub(crate) struct Window {
    /// The cells of the frozen columns (and the indent), the gap after
    /// them included.
    frozen: usize,
    /// Where the scrolled part starts in the full line.
    from: usize,
    /// The cells the scrolled part shows.
    shown: usize,
    width: usize,
    /// Whether columns are out of view to the left, and to the right.
    left: bool,
    right: bool,
    /// Where each column was drawn: its first column and width, `(0, 0)`
    /// when out of view.
    pub spans: Vec<(u16, u16)>,
}

impl Across {
    /// The window onto columns `widths` wide (their natural widths,
    /// `spacing` cells apart, after `indent` cells) in a view `width`
    /// wide, with `freeze` columns frozen and the cell cursor in `cursor`:
    /// `None` when they fit. A cursor that moved is scrolled into view.
    pub fn window(
        &self,
        indent: u16,
        widths: &[u16],
        width: u16,
        freeze: usize,
        cursor: Option<usize>,
        spacing: u16,
    ) -> Option<Window> {
        let count = widths.len();
        let frozen_columns = freeze.min(count);
        let mut starts = Vec::with_capacity(count);
        let mut at = indent as usize;
        for w in widths {
            starts.push(at);
            at += *w as usize + spacing as usize;
        }
        let total = at.saturating_sub(spacing as usize);
        let width = width as usize;
        if count == 0 || total <= width || frozen_columns == count {
            self.wide.set(false);
            return None;
        }
        let frozen = match frozen_columns {
            0 => indent as usize,
            n => starts[n],
        };
        let shown = width.saturating_sub(frozen);
        // Scrolled, the window starts at the gap before the first column
        // in view, where `‹` goes.
        let from = |left: usize| {
            if left > frozen_columns {
                starts[left] - 1
            } else {
                frozen
            }
        };
        let last = (frozen_columns..count)
            .find(|&left| total - from(left) <= shown)
            .unwrap_or(count - 1);
        let mut left = self.left.get().clamp(frozen_columns, last);
        if let Some(column) = cursor.filter(|&c| self.followed.get() != Some(c)) {
            self.followed.set(Some(column));
            if (frozen_columns..count).contains(&column) {
                left = left.min(column);
                let end = starts[column] + widths[column] as usize;
                while left < column && end - from(left) > shown {
                    left += 1;
                }
            }
        }
        self.left.set(left);
        self.last.set(last);
        self.wide.set(true);
        let start = from(left);
        let spans = (0..count)
            .map(|i| {
                let x = match i {
                    i if i < frozen_columns => starts[i],
                    i if i < left => return (0, 0),
                    i => frozen + starts[i] - start,
                };
                if x >= width {
                    return (0, 0);
                }
                let w = (widths[i] as usize).min(width - x);
                (x as u16, w as u16)
            })
            .collect();
        Some(Window {
            frozen,
            from: start,
            shown,
            width,
            left: left > frozen_columns,
            right: total - start > shown,
            spans,
        })
    }

    /// Scroll a column left (`-1`) or right (`1`), or to the first column
    /// (`isize::MIN`) or the last (`isize::MAX`), with `freeze` columns
    /// frozen. `false` when the table fits and so does not scroll.
    pub fn scroll(&self, by: isize, freeze: usize) -> bool {
        if !self.wide.get() {
            return false;
        }
        let (first, last) = (freeze, self.last.get().max(freeze));
        let left = self.left.get().clamp(first, last);
        self.left.set(match by {
            isize::MIN => first,
            isize::MAX => last,
            by => left.saturating_add_signed(by).clamp(first, last),
        });
        true
    }
}

impl Window {
    /// The cells of a full `line` in view.
    pub fn cut(&self, line: &[Segment]) -> Vec<Segment> {
        let mut out = slice(line, 0, self.frozen.min(self.width));
        out.extend(slice(line, self.from, self.from + self.shown));
        out
    }

    /// The header line, cut, with `‹` and `›` where columns are out of
    /// view.
    pub fn header(&self, line: &[Segment]) -> Vec<Segment> {
        let mut out = self.cut(line);
        if self.left && self.frozen < self.width {
            out = overlay(&out, self.frozen, &[Segment::new("‹", None)]);
        }
        if self.right && self.width > 0 {
            out = overlay(&out, self.width - 1, &[Segment::new("›", None)]);
        }
        out
    }
}
