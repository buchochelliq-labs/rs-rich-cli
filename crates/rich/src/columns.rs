//! Columns — arrange renderables in a grid.
//!
//! Port of upstream `rich/columns.py`. [`Columns`] packs items into as many
//! equal-gap columns as fit the available width, filling row by row.
//!
//! Slice scope: string items with the default padding `(0, 1)` (which upstream
//! renders as a box-less grid with collapsed single-space gaps and no edge
//! padding). `equal`/`expand`/`column_first`/`right_to_left`/`align` and
//! non-string renderables are deferred with the rest of `columns.py`.

use crate::cells::cell_len;
use crate::console::{Console, ConsoleOptions};
use crate::protocol::Renderable;
use crate::segment::Segment;
use crate::style::Style;
use crate::text::Text;

/// Arranges items into a grid of columns. Mirrors `rich.columns.Columns`.
pub struct Columns {
    items: Vec<String>,
    /// `(top, right, bottom, left)`; only left/right are used for gap sizing.
    padding: (usize, usize, usize, usize),
}

impl Columns {
    /// Columns of string items with the default padding `(0, 1)`.
    pub fn new(items: Vec<String>) -> Self {
        Columns {
            items,
            padding: (0, 1, 0, 1),
        }
    }
}

/// The width sequence for `column_count`: item widths, then zero-padded so the
/// final row is complete. Port of the non-`column_first` branch of
/// `iter_renderables`.
fn iter_widths(widths: &[usize], column_count: usize) -> Vec<usize> {
    let mut sequence = widths.to_vec();
    let remainder = widths.len() % column_count;
    if remainder != 0 {
        sequence.resize(widths.len() + (column_count - remainder), 0);
    }
    sequence
}

/// Choose the largest column count whose total width fits `max_width`.
/// Direct port of the width-fitting `while` loop in `Columns.__rich_console__`.
fn compute_column_count(widths: &[usize], max_width: usize, width_padding: usize) -> usize {
    let mut column_count = widths.len();
    while column_count > 1 {
        let sequence = iter_widths(widths, column_count);
        let mut columns: Vec<usize> = Vec::new();
        let mut column_no = 0usize;
        let mut broke = false;
        for width in sequence {
            if column_no == columns.len() {
                columns.push(width);
            } else {
                columns[column_no] = columns[column_no].max(width);
            }
            let total: usize =
                columns.iter().sum::<usize>() + width_padding * columns.len().saturating_sub(1);
            if total > max_width {
                column_count = columns.len().saturating_sub(1);
                broke = true;
                break;
            }
            column_no = (column_no + 1) % column_count;
        }
        if !broke {
            break;
        }
    }
    column_count.max(1)
}

/// Per-column widths for a fixed `column_count` (max over the round-robin items).
fn column_widths(widths: &[usize], column_count: usize) -> Vec<usize> {
    let mut result = vec![0usize; column_count];
    for (index, &width) in widths.iter().enumerate() {
        let column = index % column_count;
        result[column] = result[column].max(width);
    }
    result
}

impl Renderable for Columns {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        if self.items.is_empty() {
            return Vec::new();
        }
        let (_, right, _, left) = self.padding;
        let width_padding = left.max(right);
        let widths: Vec<usize> = self.items.iter().map(|item| cell_len(item)).collect();

        let column_count = compute_column_count(&widths, options.max_width, width_padding);
        let col_widths = column_widths(&widths, column_count);

        let style = Some(Style::new());
        let gap = " ".repeat(width_padding);

        let mut lines: Vec<Vec<Segment>> = Vec::new();
        let mut start = 0;
        while start < self.items.len() {
            let mut row: Vec<Segment> = Vec::new();
            for (column, &col_width) in col_widths.iter().enumerate() {
                if column > 0 {
                    row.push(Segment::new(gap.clone(), style.clone()));
                }
                let index = start + column;
                let content = self.items.get(index).map(String::as_str).unwrap_or("");
                let rendered = Text::new(content).render_lines(
                    console.theme(),
                    &Style::new(),
                    Some(col_width),
                );
                let cell = rendered.into_iter().next().unwrap_or_default();
                let padded = Segment::adjust_line_length(&cell, col_width, style.clone());
                row.extend(Segment::simplify(&padded));
            }
            lines.push(row);
            start += column_count;
        }

        let mut segments = Vec::new();
        let last = lines.len().saturating_sub(1);
        for (index, line) in lines.into_iter().enumerate() {
            segments.extend(line);
            if index != last {
                segments.push(Segment::line());
            }
        }
        segments
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::ColorSystem;

    fn console(width: usize) -> Console {
        Console::builder()
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .width(width)
            .build()
    }

    fn columns(items: &[&str]) -> Columns {
        Columns::new(items.iter().map(|s| s.to_string()).collect())
    }

    #[test]
    fn packs_into_two_rows() {
        let out =
            console(20).render_export(&columns(&["one", "two", "three", "four", "five", "six"]));
        assert_eq!(out, "one  two three four\nfive six           \n");
    }

    #[test]
    fn single_row_when_it_fits() {
        let out = console(30).render_export(&columns(&["alpha", "beta", "gamma", "delta"]));
        assert_eq!(out, "alpha beta gamma delta\n");
    }
}
