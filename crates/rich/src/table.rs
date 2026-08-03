//! Tables.
//!
//! Port of upstream `rich/table.py` (core subset). A [`Table`] lays out columns
//! and rows inside a box, sizing each column to its widest cell.
//!
//! Scope: headers, rows, box choice (with legacy/ASCII substitution), per-cell
//! padding, **`pad_edge`** + **`show_edge`** + **`collapse_padding`**, header
//! styling (incl. a per-column header-content span and a per-column header-cell
//! fill), a **table-level style** + **border style**,
//! multi-line/wrapped cells (with **ellipsis overflow**), **shrink-to-fit** +
//! **expand** column widths, per-column justify, **explicit width**, per-column
//! **`ratio`/`min_width`/`max_width`**, **per-column style**, **`no_wrap`**,
//! title, caption, and `show_lines`. Deferred (tracked in the Table issue): the
//! rare width-0 column padding edge.

use crate::cells::{cell_len, set_cell_size};
use crate::console::{Console, ConsoleOptions, Justify};
use crate::protocol::Renderable;
use crate::r#box::{Box as BoxSet, RowLevel, HEAVY_HEAD};
use crate::segment::Segment;
use crate::style::Style;
use crate::text::Text;
use crate::theme::Theme;

/// A single column definition. Mirrors the used subset of `rich.table.Column`.
struct Column {
    header: String,
    justify: Justify,
    /// An explicit content width; when set, the column doesn't shrink to fit.
    width: Option<usize>,
    /// A style applied to this column's body cells.
    style: Style,
    /// An extra style span applied to the header *content* only (over the base
    /// `header_style`), leaving the header padding as `header_style`. Mirrors
    /// upstream stylizing the heading `Text` (e.g. `markdown.table.header`).
    header_content_style: Option<Style>,
    /// A per-column header *cell* style — combined over the table-level
    /// `header_style` to fill the whole header cell (content + padding). Port of
    /// `Column.header_style` (as used by e.g. rich-cli's numeric columns).
    header_fill: Option<Style>,
    /// When set, the column flexes to this share of the free width when the table
    /// is `expand`ed (port of `Column.ratio`; makes the column "flexible").
    ratio: Option<usize>,
    /// A floor on the column's content width (port of `Column.min_width`).
    min_width: Option<usize>,
    /// A cap on the column's content width — wider cells wrap (port of
    /// `Column.max_width`).
    max_width: Option<usize>,
    /// When set, cells are never wrapped — they crop to one line (with ellipsis).
    no_wrap: bool,
}

/// A grid of cells rendered inside a box. Mirrors `rich.table.Table`.
pub struct Table {
    columns: Vec<Column>,
    rows: Vec<Vec<String>>,
    box_set: BoxSet,
    show_header: bool,
    show_lines: bool,
    show_edge: bool,
    pad_edge: bool,
    collapse_padding: bool,
    expand: bool,
    title: Option<String>,
    caption: Option<String>,
    padding: (usize, usize, usize, usize),
    header_style: Style,
    border_style: Style,
    style: Style,
}

impl Default for Table {
    fn default() -> Self {
        Table {
            columns: Vec::new(),
            rows: Vec::new(),
            box_set: HEAVY_HEAD,
            show_header: true,
            show_lines: false,
            show_edge: true,
            pad_edge: true,
            collapse_padding: false,
            expand: false,
            title: None,
            caption: None,
            padding: (0, 1, 0, 1),
            header_style: Style::parse("bold").expect("valid built-in style"),
            border_style: Style::new(),
            style: Style::new(),
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

    /// Style the box border (edges + dividers). Composed over the table-level
    /// style: `border = style + border_style`. Port of `Table(border_style=…)`.
    pub fn border_style(mut self, style: Style) -> Self {
        self.border_style = style;
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

    /// Draw a separator line between each body row.
    pub fn show_lines(mut self, show: bool) -> Self {
        self.show_lines = show;
        self
    }

    /// Draw the outer box edges (top/bottom borders + left/right glyphs). When
    /// off, only the internal dividers and content remain. Port of `show_edge`.
    pub fn show_edge(mut self, show: bool) -> Self {
        self.show_edge = show;
        self
    }

    /// Pad the outer cell edges. When off, the first column drops its left pad
    /// and the last column its right pad. Port of `pad_edge`.
    pub fn pad_edge(mut self, pad: bool) -> Self {
        self.pad_edge = pad;
        self
    }

    /// Merge adjacent cell padding: an interior column's left pad is reduced by
    /// the previous column's right pad. Port of `collapse_padding`.
    pub fn collapse_padding(mut self, collapse: bool) -> Self {
        self.collapse_padding = collapse;
        self
    }

    /// Default style for the whole table. Upstream applies it as the base of the
    /// border style (`border_style = style + border_style`); cell content keeps
    /// its own styles. Port of `Table(style=…)`.
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// The `(left, right)` padding for column `index` of `ncols`. Port of
    /// `_get_padding_width` (collapse) combined with the `pad_edge` edge drops.
    fn cell_padding(&self, index: usize, ncols: usize) -> (usize, usize) {
        let (_, pr, _, pl) = self.padding;
        // collapse_padding: interior columns shed the overlap with the previous
        // column's right pad.
        let mut left = if self.collapse_padding && index > 0 {
            pl.saturating_sub(pr)
        } else {
            pl
        };
        let mut right = pr;
        // pad_edge: the outer edges lose their padding.
        if !self.pad_edge && index == 0 {
            left = 0;
        }
        if !self.pad_edge && index + 1 == ncols {
            right = 0;
        }
        (left, right)
    }

    /// A centered title rendered above the table.
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// A centered caption rendered below the table.
    pub fn caption(mut self, caption: impl Into<String>) -> Self {
        self.caption = Some(caption.into());
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
            width: None,
            style: Style::new(),
            header_content_style: None,
            header_fill: None,
            ratio: None,
            min_width: None,
            max_width: None,
            no_wrap: false,
        });
        self
    }

    /// Pin the most-recently-added column to an explicit content width. Content
    /// wider than this wraps (with ellipsis overflow) instead of shrinking the
    /// column. Chain after `add_column`.
    pub fn column_width(&mut self, width: usize) -> &mut Self {
        if let Some(column) = self.columns.last_mut() {
            column.width = Some(width);
        }
        self
    }

    /// Give the most-recently-added column a flex `ratio`: when the table is
    /// `expand`ed, ratio columns share the free width in proportion. Chain after
    /// `add_column`. Port of `Column.ratio`.
    pub fn column_ratio(&mut self, ratio: usize) -> &mut Self {
        if let Some(column) = self.columns.last_mut() {
            column.ratio = Some(ratio);
        }
        self
    }

    /// Set a minimum content width on the most-recently-added column. Chain after
    /// `add_column`. Port of `Column.min_width`.
    pub fn column_min_width(&mut self, min_width: usize) -> &mut Self {
        if let Some(column) = self.columns.last_mut() {
            column.min_width = Some(min_width);
        }
        self
    }

    /// Set a maximum content width on the most-recently-added column — wider
    /// cells wrap. Chain after `add_column`. Port of `Column.max_width`.
    pub fn column_max_width(&mut self, max_width: usize) -> &mut Self {
        if let Some(column) = self.columns.last_mut() {
            column.max_width = Some(max_width);
        }
        self
    }

    /// Apply a style to the most-recently-added column's body cells. Chain after
    /// `add_column`.
    pub fn column_style(&mut self, style: Style) -> &mut Self {
        if let Some(column) = self.columns.last_mut() {
            column.style = style;
        }
        self
    }

    /// Style the most-recently-added column's header *content* (the visible
    /// characters), leaving its padding as the base `header_style`. Chain after
    /// `add_column`. Mirrors upstream stylizing the heading `Text`.
    pub fn column_header_style(&mut self, style: Style) -> &mut Self {
        if let Some(column) = self.columns.last_mut() {
            column.header_content_style = Some(style);
        }
        self
    }

    /// Style the most-recently-added column's whole header *cell* (content +
    /// padding), combined over the table-level `header_style`. Chain after
    /// `add_column`. Port of `Column.header_style`.
    pub fn column_header_fill(&mut self, style: Style) -> &mut Self {
        if let Some(column) = self.columns.last_mut() {
            column.header_fill = Some(style);
        }
        self
    }

    /// Mark the most-recently-added column `no_wrap`: its cells crop to a single
    /// line (with ellipsis) instead of wrapping. Chain after `add_column`.
    pub fn column_no_wrap(&mut self) -> &mut Self {
        if let Some(column) = self.columns.last_mut() {
            column.no_wrap = true;
        }
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
        let ncols = self.columns.len();
        // A fixed-width column uses its declared width; others measure content,
        // clamped to the column's [min_width, max_width]. Port of `_measure_column`.
        let content = self.max_content_widths();
        let mut widths: Vec<i64> = self
            .columns
            .iter()
            .zip(&content)
            .enumerate()
            .map(|(index, (column, &measured))| {
                let (pl, pr) = self.cell_padding(index, ncols);
                let content_width = match column.width {
                    Some(w) => w,
                    None => {
                        let mut w = measured;
                        if let Some(min) = column.min_width {
                            w = w.max(min);
                        }
                        if let Some(max) = column.max_width {
                            w = w.min(max);
                        }
                        w
                    }
                };
                (content_width + pl + pr) as i64
            })
            .collect();

        // Expand with explicit ratios: flexible (ratio) columns share the free
        // width in proportion, fixed columns keep their measured width. Port of
        // the `if self.expand: … if any(ratios)` block of `_calculate_column_widths`.
        if self.expand {
            let ratios: Vec<i64> = self
                .columns
                .iter()
                .filter(|c| c.ratio.is_some())
                .map(|c| c.ratio.unwrap() as i64)
                .collect();
            if ratios.iter().any(|&r| r > 0) {
                let fixed_widths: Vec<i64> = widths
                    .iter()
                    .zip(&self.columns)
                    .map(|(&w, c)| if c.ratio.is_some() { 0 } else { w })
                    .collect();
                let flex_minimum: Vec<i64> = self
                    .columns
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| c.ratio.is_some())
                    .map(|(index, c)| {
                        let (pl, pr) = self.cell_padding(index, ncols);
                        (c.width.unwrap_or(1) + pl + pr) as i64
                    })
                    .collect();
                let flexible_width = available as i64 - fixed_widths.iter().sum::<i64>();
                let flex_widths = ratio_distribute(flexible_width, &ratios, Some(&flex_minimum));
                let mut iter_flex = flex_widths.into_iter();
                for (index, column) in self.columns.iter().enumerate() {
                    if column.ratio.is_some() {
                        widths[index] = fixed_widths[index] + iter_flex.next().unwrap_or(0);
                    }
                }
            }
        }

        let table_width: i64 = widths.iter().sum();
        if table_width > available as i64 {
            // Only auto-width, wrapping columns may shrink; fixed and no_wrap
            // columns hold their width (no_wrap only yields via the last resort).
            let wrapable: Vec<bool> = self
                .columns
                .iter()
                .map(|c| c.width.is_none() && !c.no_wrap)
                .collect();
            widths = collapse_widths(widths, &wrapable, available as i64);
            // Last resort: if fixed columns still overflow, reduce every column
            // evenly. Port of `_calculate_column_widths`'s final `ratio_reduce`.
            let table_width: i64 = widths.iter().sum();
            if table_width > available as i64 {
                let excess = table_width - available as i64;
                let ratios = vec![1i64; widths.len()];
                widths = ratio_reduce(excess, &ratios, &widths, &widths);
            }
        }

        // Expand: distribute the leftover width proportionally. Port of the
        // `expand` tail of `_calculate_column_widths` (via `ratio_distribute`).
        let table_width: i64 = widths.iter().sum();
        if self.expand && table_width < available as i64 && table_width > 0 {
            let pad = ratio_distribute(available as i64 - table_width, &widths, None);
            for (width, extra) in widths.iter_mut().zip(pad) {
                *width += extra;
            }
        }
        widths.into_iter().map(|w| w.max(0) as usize).collect()
    }

    /// The effective style for a cell in column `index`: the header style for a
    /// header row, else that column's own style.
    fn cell_style(&self, index: usize, is_header: bool) -> Style {
        if is_header {
            // A per-column header cell style is combined over the table-level one.
            match self.columns.get(index).and_then(|c| c.header_fill.as_ref()) {
                Some(fill) => self.header_style.combine(fill),
                None => self.header_style.clone(),
            }
        } else {
            self.columns
                .get(index)
                .map(|c| c.style.clone())
                .unwrap_or_default()
        }
    }

    /// Render one table row (a list of cell strings) into visual lines.
    fn render_row(
        &self,
        theme: &Theme,
        cells: &[String],
        content_widths: &[usize],
        is_header: bool,
        edges: (char, char, char),
    ) -> Vec<Vec<Segment>> {
        // Horizontal padding is per-column (see `cell_padding`); only the
        // top/bottom vertical padding is uniform.
        let (pt, _, pb, _) = self.padding;
        let (edge_left, edge_vertical, edge_right) = edges;
        let border = Some(self.style.combine(&self.border_style));
        let ncols = self.columns.len();

        // Render each cell into padded, simplified visual lines.
        let mut cell_lines: Vec<Vec<Vec<Segment>>> = Vec::with_capacity(ncols);
        let mut height = 1;
        for (index, width) in content_widths.iter().enumerate() {
            let style = self.cell_style(index, is_header);
            let cell_fill = Some(style.clone());
            let content = cells.get(index).map(String::as_str).unwrap_or("");
            let column = self.columns.get(index);
            let justify = column.map(|c| c.justify).unwrap_or(Justify::Left);
            let no_wrap = column.map(|c| c.no_wrap).unwrap_or(false);
            // A no_wrap cell is one ellipsis-cropped line; otherwise wrap with
            // ellipsis overflow (the table default). Then justify + pad.
            let wrapped = if no_wrap {
                ellipsis_crop(content, *width)
            } else {
                wrap_cell(content, *width).join("\n")
            };
            let mut text = Text::new(wrapped).justify(justify);
            // Header content carries its own style span over `header_style`; the
            // justify/edge padding stays `header_style` (matches upstream).
            if is_header {
                if let Some(span) = column.and_then(|c| c.header_content_style.clone()) {
                    let len = text.plain().len();
                    text.stylize(span, 0, len);
                }
            }
            let mut lines = text.render_lines(theme, &style, Some(*width));
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
            let fill = Some(self.cell_style(index, is_header));
            while lines.len() < height {
                lines.push(vec![Segment::new(
                    " ".repeat(content_widths[index]),
                    fill.clone(),
                )]);
            }
        }

        let last = ncols.saturating_sub(1);
        let mut rows_out: Vec<Vec<Segment>> = Vec::with_capacity(height);
        // `r` indexes into each column's per-line vector, so a range loop is the
        // natural shape here (the columns are iterated with `enumerate`).
        #[allow(clippy::needless_range_loop)]
        for r in 0..height {
            let mut row = Vec::new();
            if self.show_edge {
                row.push(Segment::new(edge_left.to_string(), border.clone()));
            }
            for (c, column_lines) in cell_lines.iter().enumerate() {
                let fill = Some(self.cell_style(c, is_header));
                let (cpl, cpr) = self.cell_padding(c, ncols);
                if cpl > 0 {
                    row.push(Segment::new(" ".repeat(cpl), fill.clone()));
                }
                row.extend(column_lines[r].clone());
                if cpr > 0 {
                    row.push(Segment::new(" ".repeat(cpr), fill.clone()));
                }
                if c != last {
                    row.push(Segment::new(edge_vertical.to_string(), border.clone()));
                } else if self.show_edge {
                    row.push(Segment::new(edge_right.to_string(), border.clone()));
                }
            }
            rows_out.push(row);
        }
        rows_out
    }
}

impl Renderable for Table {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        if self.columns.is_empty() {
            return Vec::new();
        }
        // Fall back to a terminal-safe box on legacy Windows / non-UTF-8.
        let box_set = self.box_set.substitute(
            console.legacy_windows(),
            console.safe_box(),
            console.ascii_only(),
        );
        let ncols = self.columns.len();
        // Borders occupy: (ncols-1) dividers, plus 2 outer edges when shown.
        // Port of `_extra_width`.
        let extra_width = (if self.show_edge { 2 } else { 0 }) + ncols.saturating_sub(1);
        let available = options.max_width.saturating_sub(extra_width);

        let rendered_widths = self.column_widths(available);
        let content_widths: Vec<usize> = rendered_widths
            .iter()
            .enumerate()
            .map(|(index, w)| {
                let (pl, pr) = self.cell_padding(index, ncols);
                w.saturating_sub(pl + pr)
            })
            .collect();
        let border = Some(self.style.combine(&self.border_style));

        // Full table width (for centering title/caption): columns + borders.
        let table_width: usize = rendered_widths.iter().sum::<usize>() + extra_width;

        let mut lines: Vec<Vec<Segment>> = Vec::new();

        // Title, centered above the table.
        if let Some(title) = &self.title {
            let style = Style::parse("italic").expect("valid built-in style");
            lines.push(vec![Segment::new(center(title, table_width), Some(style))]);
        }

        let edge = self.show_edge;
        if edge {
            lines.push(vec![Segment::new(
                box_set.get_top(&rendered_widths, edge),
                border.clone(),
            )]);
        }

        let head_edges = (box_set.head_left, box_set.head_vertical, box_set.head_right);
        let body_edges = (box_set.mid_left, box_set.mid_vertical, box_set.mid_right);

        if self.show_header {
            let headers: Vec<String> = self.columns.iter().map(|c| c.header.clone()).collect();
            lines.extend(self.render_row(
                console.theme(),
                &headers,
                &content_widths,
                true,
                head_edges,
            ));
            lines.push(vec![Segment::new(
                box_set.get_row(&rendered_widths, RowLevel::Head, edge),
                border.clone(),
            )]);
        }

        let row_last = self.rows.len().saturating_sub(1);
        for (index, row) in self.rows.iter().enumerate() {
            lines.extend(self.render_row(console.theme(), row, &content_widths, false, body_edges));
            if self.show_lines && index != row_last {
                lines.push(vec![Segment::new(
                    box_set.get_row(&rendered_widths, RowLevel::Row, edge),
                    border.clone(),
                )]);
            }
        }

        if edge {
            lines.push(vec![Segment::new(
                box_set.get_bottom(&rendered_widths, edge),
                border.clone(),
            )]);
        }

        // Caption, centered below the table.
        if let Some(caption) = &self.caption {
            let style = Style::parse("dim italic").expect("valid built-in style");
            lines.push(vec![Segment::new(
                center(caption, table_width),
                Some(style),
            )]);
        }

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

/// Wrap `content` to `width` cells with **ellipsis overflow** (the table
/// default): words are broken between, and a single word wider than `width` is
/// cropped with a trailing `…`. Returns one string per visual line.
fn wrap_cell(content: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![String::new()];
    }
    // `fold = false`: over-long words stay on their own (overflowing) line,
    // which `ellipsis_crop` then trims — matching `Text(overflow="ellipsis")`.
    let breaks = crate::wrap::divide_line(content, width, false);
    let chars: Vec<char> = content.chars().collect();
    let mut lines: Vec<String> = Vec::new();
    let mut start = 0;
    for stop in breaks {
        lines.push(chars[start..stop].iter().collect());
        start = stop;
    }
    lines.push(chars[start..].iter().collect());
    // Trailing whitespace is dropped before the overflow check, so a word that
    // fills the width exactly isn't spuriously ellipsized by its trailing space.
    lines
        .iter()
        .map(|line| ellipsis_crop(line.trim_end(), width))
        .collect()
}

/// Crop `text` to `width` cells, replacing the trailing cell with `…` when it
/// doesn't fit. Port of the `overflow="ellipsis"` path of `Text.truncate`.
fn ellipsis_crop(text: &str, width: usize) -> String {
    if cell_len(text) <= width {
        return text.to_string();
    }
    if width == 0 {
        return String::new();
    }
    format!("{}\u{2026}", set_cell_size(text, width - 1))
}

/// Center `text` within `width` cells (floor-left), padding with spaces.
fn center(text: &str, width: usize) -> String {
    let excess = width.saturating_sub(cell_len(text));
    let left = excess / 2;
    let right = excess - left;
    format!("{}{}{}", " ".repeat(left), text, " ".repeat(right))
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

/// Divide `total` across slots proportionally to `ratios` (ceil each share),
/// each share floored at the matching `minimums` entry when given. Port of
/// `rich._ratio.ratio_distribute`.
fn ratio_distribute(total: i64, ratios: &[i64], minimums: Option<&[i64]>) -> Vec<i64> {
    // Upstream zeroes the ratio of any slot whose minimum is 0 (falsy).
    let ratios: Vec<i64> = match minimums {
        Some(mins) => ratios
            .iter()
            .zip(mins)
            .map(|(&r, &m)| if m != 0 { r } else { 0 })
            .collect(),
        None => ratios.to_vec(),
    };
    let mut total_ratio: i64 = ratios.iter().sum();
    let mut total_remaining = total;
    let mut result = Vec::with_capacity(ratios.len());
    for (index, &ratio) in ratios.iter().enumerate() {
        let minimum = minimums.map_or(0, |m| m[index]);
        let distributed = if total_ratio > 0 {
            // ceil(ratio * total_remaining / total_ratio) for positive values,
            // then floored at `minimum`.
            let numerator = ratio * total_remaining;
            let ceil_div = (numerator + total_ratio - 1) / total_ratio;
            minimum.max(ceil_div)
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
            .no_color(false)
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
