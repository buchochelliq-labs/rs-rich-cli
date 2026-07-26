//! Tables.
//!
//! Port of upstream `rich/table.py` (core subset). A [`Table`] lays out columns
//! and rows inside a box, sizing each column to its widest cell.
//!
//! Slice scope: headers, rows, box choice, per-cell padding, header styling,
//! multi-line/wrapped cells, and **shrink-to-fit** column widths (the widest
//! columns collapse + wrap when the table overflows the width). Deferred
//! (tracked in the Table issue): `expand`, explicit `ratio`/`width`/min/max,
//! `no_wrap`, per-column justify/style, `show_lines`, titles, and footers.

use crate::cells::cell_len;
use crate::console::{Console, ConsoleOptions, Justify};
use crate::protocol::Renderable;
use crate::r#box::{Box as BoxSet, RowLevel, HEAVY_HEAD};
use crate::segment::Segment;
use crate::style::Style;
use crate::text::Text;

/// A single column definition. Mirrors the used subset of `rich.table.Column`.
struct Column {
    header: String,
    justify: Justify,
}

/// A grid of cells rendered inside a box. Mirrors `rich.table.Table`.
pub struct Table {
    columns: Vec<Column>,
    rows: Vec<Vec<String>>,
    box_set: BoxSet,
    show_header: bool,
    expand: bool,
    padding: (usize, usize, usize, usize),
    header_style: Style,
    border_style: Style,
}

impl Default for Table {
    fn default() -> Self {
        Table {
            columns: Vec::new(),
            rows: Vec::new(),
            box_set: HEAVY_HEAD,
            show_header: true,
            expand: false,
            padding: (0, 1, 0, 1),
            header_style: Style::parse("bold").expect("valid built-in style"),
            border_style: Style::new(),
        }
    }
}

impl Table {
    pub fn new() -> Self {
        Table::default()
    }

    /// Choose the box-drawing set.
    pub fn box_set(mut self, box_set: BoxSet) -> Self {
        self.box_set = box_set;
        self
    }

    /// Whether to render the header row.
    pub fn show_header(mut self, show: bool) -> Self {
        self.show_header = show;
        self
    }

    /// Expand the table to fill the available width.
    pub fn expand(mut self, expand: bool) -> Self {
        self.expand = expand;
        self
    }

    /// Add a left-justified column with the given header.
    pub fn add_column(&mut self, header: impl Into<String>) -> &mut Self {
        self.add_column_justify(header, Justify::Left)
    }

    /// Add a column with an explicit justification.
    pub fn add_column_justify(&mut self, header: impl Into<String>, justify: Justify) -> &mut Self {
        self.columns.push(Column {
            header: header.into(),
            justify,
        });
        self
    }

    /// Add a row of cells (extra cells are ignored; missing cells render empty).
    pub fn add_row(&mut self, cells: &[&str]) -> &mut Self {
        self.rows
            .push(cells.iter().map(|s| s.to_string()).collect());
        self
    }

    /// The measured content width of each column (widest cell, header included).
    fn max_content_widths(&self) -> Vec<usize> {
        let mut widths = vec![0usize; self.columns.len()];
        for (index, column) in self.columns.iter().enumerate() {
            if self.show_header {
                widths[index] = cell_len(&column.header);
            }
        }
        for row in &self.rows {
            for (index, cell) in row.iter().enumerate() {
                if index < widths.len() {
                    widths[index] = widths[index].max(cell_len(cell));
                }
            }
        }
        widths
    }

    /// The rendered width (content + padding) of each column, shrinking the
    /// widest columns to fit `available` when necessary. Port of the non-flexible
    /// path of `Table._calculate_column_widths` + `_collapse_widths`.
    fn column_widths(&self, available: usize) -> Vec<usize> {
        let (_, pr, _, pl) = self.padding;
        let padding = pl + pr;
        let mut widths: Vec<i64> = self
            .max_content_widths()
            .into_iter()
            .map(|w| (w + padding) as i64)
            .collect();

        let table_width: i64 = widths.iter().sum();
        if table_width > available as i64 {
            let wrapable = vec![true; widths.len()];
            widths = collapse_widths(widths, &wrapable, available as i64);
        }

        // Expand: distribute the leftover width proportionally. Port of the
        // `expand` tail of `_calculate_column_widths` (via `ratio_distribute`).
        let table_width: i64 = widths.iter().sum();
        if self.expand && table_width < available as i64 && table_width > 0 {
            let pad = ratio_distribute(available as i64 - table_width, &widths);
            for (width, extra) in widths.iter_mut().zip(pad) {
                *width += extra;
            }
        }
        widths.into_iter().map(|w| w.max(0) as usize).collect()
    }

    /// Render one table row (a list of cell strings) into visual lines.
    fn render_row(
        &self,
        cells: &[String],
        content_widths: &[usize],
        cell_style: &Style,
        edges: (char, char, char),
    ) -> Vec<Vec<Segment>> {
        let (pt, pr, pb, pl) = self.padding;
        let (edge_left, edge_vertical, edge_right) = edges;
        let border = Some(self.border_style.clone());
        let cell_fill = Some(cell_style.clone());
        let ncols = self.columns.len();

        // Render each cell into padded, simplified visual lines.
        let mut cell_lines: Vec<Vec<Vec<Segment>>> = Vec::with_capacity(ncols);
        let mut height = 1;
        for (index, width) in content_widths.iter().enumerate() {
            let content = cells.get(index).map(String::as_str).unwrap_or("");
            let justify = self
                .columns
                .get(index)
                .map(|column| column.justify)
                .unwrap_or(Justify::Left);
            let text = Text::new(content).justify(justify);
            let mut lines = text.render_lines(cell_style, Some(*width));
            if lines.is_empty() {
                lines.push(Vec::new());
            }
            // Vertical padding (blank content lines top/bottom).
            let blank = || Segment::new(" ".repeat(*width), cell_fill.clone());
            let mut padded_lines: Vec<Vec<Segment>> = Vec::new();
            for _ in 0..pt {
                padded_lines.push(vec![blank()]);
            }
            for line in &lines {
                let padded = Segment::adjust_line_length(line, *width, cell_fill.clone());
                padded_lines.push(Segment::simplify(&padded));
            }
            for _ in 0..pb {
                padded_lines.push(vec![blank()]);
            }
            height = height.max(padded_lines.len());
            cell_lines.push(padded_lines);
        }

        // Pad every column to the row height with blank lines.
        for (index, lines) in cell_lines.iter_mut().enumerate() {
            while lines.len() < height {
                lines.push(vec![Segment::new(
                    " ".repeat(content_widths[index]),
                    cell_fill.clone(),
                )]);
            }
        }

        let last = ncols.saturating_sub(1);
        let mut rows_out: Vec<Vec<Segment>> = Vec::with_capacity(height);
        // `r` indexes into each column's per-line vector, so a range loop is the
        // natural shape here (the columns are iterated with `enumerate`).
        #[allow(clippy::needless_range_loop)]
        for r in 0..height {
            let mut row = vec![Segment::new(edge_left.to_string(), border.clone())];
            for (c, column_lines) in cell_lines.iter().enumerate() {
                if pl > 0 {
                    row.push(Segment::new(" ".repeat(pl), cell_fill.clone()));
                }
                row.extend(column_lines[r].clone());
                if pr > 0 {
                    row.push(Segment::new(" ".repeat(pr), cell_fill.clone()));
                }
                let edge = if c == last { edge_right } else { edge_vertical };
                row.push(Segment::new(edge.to_string(), border.clone()));
            }
            rows_out.push(row);
        }
        rows_out
    }
}

impl Renderable for Table {
    fn rich_render(&self, _console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        if self.columns.is_empty() {
            return Vec::new();
        }
        let (_, pr, _, pl) = self.padding;
        // Borders occupy: 2 edges + (ncols - 1) dividers. Port of `_extra_width`.
        let extra_width = 2 + self.columns.len().saturating_sub(1);
        let available = options.max_width.saturating_sub(extra_width);

        let rendered_widths = self.column_widths(available);
        let content_widths: Vec<usize> = rendered_widths
            .iter()
            .map(|w| w.saturating_sub(pl + pr))
            .collect();
        let border = Some(self.border_style.clone());

        let mut lines: Vec<Vec<Segment>> = Vec::new();
        lines.push(vec![Segment::new(
            self.box_set.get_top(&rendered_widths),
            border.clone(),
        )]);

        let head_edges = (
            self.box_set.head_left,
            self.box_set.head_vertical,
            self.box_set.head_right,
        );
        let body_edges = (
            self.box_set.mid_left,
            self.box_set.mid_vertical,
            self.box_set.mid_right,
        );

        if self.show_header {
            let headers: Vec<String> = self.columns.iter().map(|c| c.header.clone()).collect();
            lines.extend(self.render_row(
                &headers,
                &content_widths,
                &self.header_style,
                head_edges,
            ));
            lines.push(vec![Segment::new(
                self.box_set.get_row(&rendered_widths, RowLevel::Head),
                border.clone(),
            )]);
        }

        for row in &self.rows {
            lines.extend(self.render_row(row, &content_widths, &Style::new(), body_edges));
        }

        lines.push(vec![Segment::new(
            self.box_set.get_bottom(&rendered_widths),
            border.clone(),
        )]);

        // Join visual lines with newline segments (no trailing newline).
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

/// Round half to even (banker's rounding), matching Python's `round`.
fn round_half_even(value: f64) -> i64 {
    let floor = value.floor();
    let diff = value - floor;
    if (diff - 0.5).abs() < 1e-9 {
        let f = floor as i64;
        if f % 2 == 0 {
            f
        } else {
            f + 1
        }
    } else {
        value.round() as i64
    }
}

/// Reduce `values` by `total`, distributed across slots by `ratios` (capped by
/// `maximums`). Direct port of `rich._ratio.ratio_reduce`.
fn ratio_reduce(total: i64, ratios: &[i64], maximums: &[i64], values: &[i64]) -> Vec<i64> {
    let ratios: Vec<i64> = ratios
        .iter()
        .zip(maximums)
        .map(|(&r, &m)| if m != 0 { r } else { 0 })
        .collect();
    let mut total_ratio: i64 = ratios.iter().sum();
    if total_ratio == 0 {
        return values.to_vec();
    }
    let mut total_remaining = total;
    let mut result = Vec::with_capacity(values.len());
    for ((&ratio, &maximum), &value) in ratios.iter().zip(maximums).zip(values) {
        if ratio != 0 && total_ratio > 0 {
            let distributed = maximum.min(round_half_even(
                ratio as f64 * total_remaining as f64 / total_ratio as f64,
            ));
            result.push(value - distributed);
            total_remaining -= distributed;
            total_ratio -= ratio;
        } else {
            result.push(value);
        }
    }
    result
}

/// Divide `total` across slots proportionally to `ratios` (ceil each share).
/// Port of `rich._ratio.ratio_distribute` (no minimums).
fn ratio_distribute(total: i64, ratios: &[i64]) -> Vec<i64> {
    let mut total_ratio: i64 = ratios.iter().sum();
    let mut total_remaining = total;
    let mut result = Vec::with_capacity(ratios.len());
    for &ratio in ratios {
        let distributed = if total_ratio > 0 {
            // ceil(ratio * total_remaining / total_ratio) for positive values.
            let numerator = ratio * total_remaining;
            (numerator + total_ratio - 1) / total_ratio
        } else {
            total_remaining
        };
        result.push(distributed);
        total_ratio -= ratio;
        total_remaining -= distributed;
    }
    result
}

/// Reduce `widths` so their total is under `max_width`, shrinking the widest
/// wrapable columns first. Direct port of `Table._collapse_widths`.
fn collapse_widths(mut widths: Vec<i64>, wrapable: &[bool], max_width: i64) -> Vec<i64> {
    let mut total_width: i64 = widths.iter().sum();
    let mut excess_width = total_width - max_width;
    if wrapable.iter().any(|&w| w) {
        while total_width != 0 && excess_width > 0 {
            let max_column = widths
                .iter()
                .zip(wrapable)
                .filter(|(_, &w)| w)
                .map(|(&x, _)| x)
                .max()
                .unwrap_or(0);
            let second_max_column = widths
                .iter()
                .zip(wrapable)
                .map(|(&x, &w)| if w && x != max_column { x } else { 0 })
                .max()
                .unwrap_or(0);
            let column_difference = max_column - second_max_column;
            let ratios: Vec<i64> = widths
                .iter()
                .zip(wrapable)
                .map(|(&x, &w)| i64::from(x == max_column && w))
                .collect();
            if !ratios.iter().any(|&r| r != 0) || column_difference == 0 {
                break;
            }
            let max_reduce = vec![excess_width.min(column_difference); widths.len()];
            widths = ratio_reduce(excess_width, &ratios, &max_reduce, &widths);
            total_width = widths.iter().sum();
            excess_width = total_width - max_width;
        }
    }
    widths
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::ColorSystem;
    use crate::r#box::SQUARE;

    fn console() -> Console {
        Console::builder()
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .width(40)
            .build()
    }

    #[test]
    fn simple_square_table() {
        let mut table = Table::new().box_set(SQUARE);
        table.add_column("Name");
        table.add_column("Age");
        table.add_row(&["Alice", "30"]);
        table.add_row(&["Bob", "7"]);
        let out = console().render_export(&table);
        let expected = concat!(
            "┌───────┬─────┐\n",
            "│\x1b[1m \x1b[0m\x1b[1mName \x1b[0m\x1b[1m \x1b[0m│\x1b[1m \x1b[0m\x1b[1mAge\x1b[0m\x1b[1m \x1b[0m│\n",
            "├───────┼─────┤\n",
            "│ Alice │ 30  │\n",
            "│ Bob   │ 7   │\n",
            "└───────┴─────┘\n",
        );
        assert_eq!(out, expected);
    }
}
