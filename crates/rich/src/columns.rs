//! Columns — arrange renderables in a grid.
//!
//! Port of upstream `rich/columns.py`. [`Columns`] packs items into as many
//! equal-gap columns as fit the available width, filling row by row.
//!
//! String (markup), `Text` and renderable items, laid out in upstream's
//! box-less `Table.grid` (collapsed gaps, no edge padding), with every
//! upstream option: `padding`, `width`, `expand`, `equal`, `column_first`,
//! `right_to_left`, `align` and `title`.

use std::sync::Arc;

use crate::align::{Align, HorizontalAlign};
use crate::console::{Console, ConsoleOptions};
use crate::measure::Measurement;
use crate::protocol::Renderable;
use crate::segment::Segment;
use crate::table::{Cell, Table};

/// Arranges items into a grid of columns. Mirrors `rich.columns.Columns`.
pub struct Columns {
    items: Vec<Cell>,
    /// `(top, right, bottom, left)`; only left/right are used for gap sizing.
    padding: (usize, usize, usize, usize),
    expand: bool,
    equal: bool,
    width: Option<usize>,
    column_first: bool,
    right_to_left: bool,
    align: Option<HorizontalAlign>,
    title: Option<String>,
}

impl Columns {
    /// Columns of string items with the default padding `(0, 1)`. Each string
    /// is console markup, converted with the console's defaults (markup, emoji
    /// and highlighting), as upstream's `console.render_str(renderable)` does.
    pub fn new(items: Vec<String>) -> Self {
        Columns::from_cells(items.into_iter().map(Cell::Markup).collect())
    }

    /// Columns of any items: markup strings, literal [`Text`](crate::text::Text)
    /// or renderables, as upstream's `Columns(renderables)` accepts.
    pub fn from_cells(items: Vec<Cell>) -> Self {
        Columns {
            items,
            padding: (0, 1, 0, 1),
            expand: false,
            equal: false,
            width: None,
            column_first: false,
            right_to_left: false,
            align: None,
            title: None,
        }
    }

    /// Add an item. Port of `Columns.add_renderable`.
    pub fn add_renderable(&mut self, item: impl Into<Cell>) -> &mut Self {
        self.items.push(item.into());
        self
    }

    /// Padding around each cell as `(top, right, bottom, left)` (upstream
    /// `padding`, default `(0, 1)`); the gap between columns is the larger of
    /// left and right.
    pub fn padding(mut self, padding: (usize, usize, usize, usize)) -> Self {
        self.padding = padding;
        self
    }

    /// A fixed width for every column (upstream `width`): as many columns as
    /// `max_width // (width + gap)`.
    pub fn width(mut self, width: usize) -> Self {
        self.width = Some(width);
        self
    }

    /// Fill columns top to bottom rather than rows left to right (upstream
    /// `column_first`).
    pub fn column_first(mut self, column_first: bool) -> Self {
        self.column_first = column_first;
        self
    }

    /// Lay each row out from the right (upstream `right_to_left`).
    pub fn right_to_left(mut self, right_to_left: bool) -> Self {
        self.right_to_left = right_to_left;
        self
    }

    /// Align every item within its column (upstream `align`).
    pub fn align(mut self, align: HorizontalAlign) -> Self {
        self.align = Some(align);
        self
    }

    /// A title (console markup) above the columns (upstream `title`).
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
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

/// The item order for `column_count` columns, `None` padding the final row
/// so it is complete. Port of `iter_renderables`.
fn iter_order(item_count: usize, column_count: usize, column_first: bool) -> Vec<Option<usize>> {
    let mut order: Vec<Option<usize>> = if column_first {
        let mut column_lengths = vec![item_count / column_count; column_count];
        for length in column_lengths.iter_mut().take(item_count % column_count) {
            *length += 1;
        }
        let row_count = item_count.div_ceil(column_count);
        let mut cells = vec![vec![None; column_count]; row_count];
        let (mut row, mut col) = (0, 0);
        for index in 0..item_count {
            cells[row][col] = Some(index);
            column_lengths[col] -= 1;
            if column_lengths[col] > 0 {
                row += 1;
            } else {
                col += 1;
                row = 0;
            }
        }
        // `if index == -1: break` at the first gap.
        cells
            .into_iter()
            .flatten()
            .map_while(|index| index.map(Some))
            .collect()
    } else {
        (0..item_count).map(Some).collect()
    };
    let remainder = item_count % column_count;
    if remainder != 0 {
        order.resize(order.len() + (column_count - remainder), None);
    }
    order
}

/// Choose the largest column count whose total width fits `max_width`.
/// Direct port of the width-fitting `while` loop in `Columns.__rich_console__`.
fn compute_column_count(
    widths: &[usize],
    max_width: usize,
    width_padding: usize,
    column_first: bool,
) -> usize {
    let mut column_count = widths.len();
    while column_count > 1 {
        let sequence = iter_order(widths.len(), column_count, column_first)
            .into_iter()
            .map(|index| index.map_or(0, |index| widths[index]));
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
        // `render_str(renderable) if isinstance(renderable, str)`: strings take
        // the console's markup, emoji and highlight defaults; a `Text` or
        // renderable is used as it is.
        let renderables: Vec<Cell> = self
            .items
            .iter()
            .map(|item| match item {
                Cell::Markup(markup) => Cell::Text(console.render_str(markup, None)),
                other => other.clone(),
            })
            .collect();

        // `Measurement.get(...).maximum` caps each width at `options.max_width`,
        // so an item wider than the console still fits in a single column.
        let mut widths: Vec<usize> = renderables
            .iter()
            .map(|cell| cell.measure_cell(console, options).maximum)
            .collect();
        if self.equal {
            let widest = widths.iter().copied().max().unwrap_or(0);
            widths = vec![widest; widths.len()];
        }

        let mut table = Table::grid()
            .padding(top, right, bottom, left)
            .collapse_padding(true)
            .pad_edge(false)
            .expand(self.expand);
        if let Some(title) = &self.title {
            table = table.title(title.clone());
        }
        let column_count = match self.width {
            Some(width) => {
                // Upstream divides by zero columns only to fail later; a
                // single column is the nearest working layout.
                let column_count = (options.max_width / (width + width_padding)).max(1);
                for _ in 0..column_count {
                    table.add_column("").column_width(width);
                }
                column_count
            }
            None => {
                let column_count = compute_column_count(
                    &widths,
                    options.max_width,
                    width_padding,
                    self.column_first,
                );
                for _ in 0..column_count {
                    table.add_column("");
                }
                column_count
            }
        };

        // Upstream yields the items in
        // `Table.grid(padding=self.padding, collapse_padding=True, pad_edge=False)`,
        // which wraps (or ellipsis-truncates) each item to its column width.
        // With `equal`, upstream wraps each item in `Constrain(renderable,
        // renderable_widths[0])`; for text items that is a no-op, since no item
        // measures wider than the constraint, so only renderables are wrapped.
        let equal_width = widths.first().copied().unwrap_or(0);
        let cells: Vec<Cell> = iter_order(renderables.len(), column_count, self.column_first)
            .into_iter()
            .map(|index| {
                // `None` pads the last row: `Table.add_row` renders an empty cell.
                let Some(index) = index else {
                    return Cell::Markup(String::new());
                };
                let mut cell = renderables[index].clone();
                if self.equal {
                    if let Cell::Renderable(renderable) = &cell {
                        cell = Cell::Renderable(Arc::new(ConstrainCell {
                            renderable: renderable.clone(),
                            width: equal_width,
                        }));
                    }
                }
                if let Some(align) = self.align {
                    let child: Arc<dyn Renderable + Send + Sync> = match cell {
                        Cell::Renderable(renderable) => renderable,
                        Cell::Text(text) => Arc::new(text),
                        Cell::Markup(markup) => Arc::new(console.render_str(&markup, None)),
                    };
                    cell = Cell::Renderable(Arc::new(AlignCell { child, align }));
                }
                cell
            })
            .collect();
        for row in cells.chunks(column_count) {
            let mut row = row.to_vec();
            if self.right_to_left {
                row.reverse();
            }
            table.add_row_cells(row);
        }
        table.rich_render(console, options)
    }
}

/// `Align(renderable, align)` around a shared cell renderable (the `align`
/// path).
struct AlignCell {
    child: Arc<dyn Renderable + Send + Sync>,
    align: HorizontalAlign,
}

impl Renderable for AlignCell {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        Align::render_child(self.child.as_ref(), self.align, console, options)
    }

    fn measure(&self, console: &Console, options: &ConsoleOptions) -> Measurement {
        Measurement::get(console, options, self.child.as_ref())
    }
}

/// `Constrain(renderable, width)` around a shared cell renderable (the
/// `equal` path). Same rules as [`Constrain`](crate::constrain::Constrain).
struct ConstrainCell {
    renderable: Arc<dyn Renderable + Send + Sync>,
    width: usize,
}

impl Renderable for ConstrainCell {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let options = options.update_width(self.width.min(options.max_width));
        if options.max_width < 1 {
            return Vec::new();
        }
        self.renderable.rich_render(console, &options)
    }

    fn measure(&self, console: &Console, options: &ConsoleOptions) -> Measurement {
        Measurement::get(
            console,
            &options.update_width(self.width),
            self.renderable.as_ref(),
        )
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
