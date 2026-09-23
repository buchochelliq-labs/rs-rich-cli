//! Columns — arrange renderables in a grid.
//!
//! Port of upstream `rich/columns.py`. [`Columns`] packs items into as many
//! equal-gap columns as fit the available width, filling row by row.
//!
//! Slice scope: string items with the default padding `(0, 1)`, laid out in
//! upstream's box-less `Table.grid` (collapsed single-space gaps, no edge
//! padding), plus `equal` and `expand`. `width`/`column_first`/
//! `right_to_left`/`align`/`title` and non-string renderables are deferred
//! with the rest of `columns.py`.

use crate::console::{Console, ConsoleOptions};
use crate::measure::Measurement;
use crate::protocol::Renderable;
use crate::segment::Segment;
use crate::table::Table;
use crate::text::Text;

/// Arranges items into a grid of columns. Mirrors `rich.columns.Columns`.
pub struct Columns {
    items: Vec<String>,
    /// `(top, right, bottom, left)`; only left/right are used for gap sizing.
    padding: (usize, usize, usize, usize),
    expand: bool,
    equal: bool,
}

impl Columns {
    /// Columns of string items with the default padding `(0, 1)`.
    pub fn new(items: Vec<String>) -> Self {
        Columns {
            items,
            padding: (0, 1, 0, 1),
            expand: false,
            equal: false,
        }
    }

    /// Expand columns to the full width (upstream `expand`).
    pub fn expand(mut self, expand: bool) -> Self {
        self.expand = expand;
        self
    }

    /// Arrange into equal sized columns (upstream `equal`).
    pub fn equal(mut self, equal: bool) -> Self {
        self.equal = equal;
        self
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

impl Renderable for Columns {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        if self.items.is_empty() {
            return Vec::new();
        }
        let (top, right, bottom, left) = self.padding;
        let width_padding = left.max(right);
        let renderables: Vec<Text> = self.items.iter().map(Text::new).collect();

        // `Measurement.get(...).maximum` caps each width at `options.max_width`,
        // so an item wider than the console still fits in a single column.
        let mut widths: Vec<usize> = renderables
            .iter()
            .map(|text| Measurement::get(console, options, text).maximum)
            .collect();
        if self.equal {
            let widest = widths.iter().copied().max().unwrap_or(0);
            widths = vec![widest; widths.len()];
        }

        let column_count = compute_column_count(&widths, options.max_width, width_padding);

        // Upstream yields the items in
        // `Table.grid(padding=self.padding, collapse_padding=True, pad_edge=False)`,
        // which wraps (or ellipsis-truncates) each item to its column width.
        // With `equal`, upstream wraps each item in `Constrain(renderable,
        // renderable_widths[0])`; for text items that is a no-op, since no item
        // measures wider than the constraint.
        let mut table = Table::grid()
            .padding(top, right, bottom, left)
            .collapse_padding(true)
            .pad_edge(false)
            .expand(self.expand);
        for _ in 0..column_count {
            table.add_column("");
        }
        let mut cells = renderables;
        let remainder = cells.len() % column_count;
        if remainder != 0 {
            // `iter_renderables` pads the last row with `None`, which
            // `Table.add_row` renders as an empty cell.
            cells.resize(cells.len() + (column_count - remainder), Text::new(""));
        }
        for row in cells.chunks(column_count) {
            table.add_row_text(row.to_vec());
        }
        table.rich_render(console, options)
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

    #[test]
    fn truncates_an_item_wider_than_the_width() {
        let out = console(8).render_export(&columns(&["supercalifragilistic"]));
        assert_eq!(out, "superca…\n");
    }

    #[test]
    fn wraps_an_item_wider_than_the_width() {
        let out = console(13).render_export(&columns(&["name name name"]));
        assert_eq!(out, "name name    \nname         \n");
    }
}
