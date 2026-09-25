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
//! title, caption, and `show_lines`; the table-level `width` / `min_width`,
//! footers (`show_footer`, column `footer` / `footer_style`), `leading`,
//! alternating `row_styles`, per-row `style` and sections (`add_section`,
//! `end_section`), `header_style` / `footer_style` / `title_style` /
//! `caption_style`, `title_justify` / `caption_justify` and `safe_box`.
//! A row with more cells than columns adds columns, as upstream's `add_row`
//! does. Headers and cells may be styled [`Text`]
//! (`add_column_text`, `add_row_text`), as upstream accepts renderables; plain
//! strings (`add_column`, `add_row`) are console markup, as upstream's `str`
//! cells are (see [`Cell::Markup`]).
//! Deferred (tracked in the Table issue): the rare width-0 column padding edge.

use std::sync::Arc;

use crate::console::{Console, ConsoleOptions, Justify, Overflow};
use crate::measure::Measurement;
use crate::protocol::{LineRenderable, Renderable};
use crate::r#box::{Box as BoxSet, RowLevel, HEAVY_HEAD};
use crate::segment::Segment;
use crate::style::{Style, StyleType};
use crate::text::Text;

/// A single column definition. Mirrors the used subset of `rich.table.Column`.
struct Column {
    header: Cell,
    /// The footer cell (port of `Column.footer`), shown with `show_footer`.
    footer: Cell,
    /// A per-column footer *cell* style, combined over the table-level
    /// `footer_style` (port of `Column.footer_style`).
    footer_fill: Option<Style>,
    /// Vertical alignment of body cells (port of `Column.vertical`).
    vertical: crate::align::VerticalAlign,
    /// Highlight `str` cells (port of `Column.highlight`); `None` takes the
    /// table's `highlight`, as `add_column(highlight=None)` does.
    highlight: Option<bool>,
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
    /// How over-long cell text is handled (upstream `Column.overflow`,
    /// default `"ellipsis"`). A cell `Text`'s own overflow wins.
    overflow: Overflow,
}

impl Column {
    /// A column with every upstream default and the given header / justify.
    fn new(header: Cell, justify: Justify) -> Self {
        Column {
            header,
            footer: Cell::Markup(String::new()),
            footer_fill: None,
            highlight: None,
            vertical: crate::align::VerticalAlign::Top,
            justify,
            width: None,
            style: Style::new(),
            header_content_style: None,
            header_fill: None,
            ratio: None,
            min_width: None,
            max_width: None,
            no_wrap: false,
            overflow: Overflow::Ellipsis,
        }
    }
}

/// A row's cells and the row's own options. Port of `rich.table.Row` (which
/// upstream keeps apart from the cells, stored per column).
struct Row {
    cells: Vec<Cell>,
    /// A style for the whole row (`add_row(..., style=…)`).
    style: Option<StyleType>,
    /// Draw a line beneath the row (`end_section`).
    end_section: bool,
}

/// Which kind of row is being rendered: the header, a body row (with its
/// resolved row style) or the footer.
#[derive(Clone)]
enum RowKind {
    Header,
    Body(Style),
    Footer,
}

/// A table cell: a markup string, styled text, or any renderable (upstream
/// accepts all three).
#[derive(Clone)]
pub enum Cell {
    /// A text cell, rendered literally (a `Text` is never re-parsed); its own
    /// `justify`, `overflow` and `no_wrap` override the column's.
    Text(Text),
    /// A plain string, which upstream renders through `Console.render_str`:
    /// console markup and emoji codes are applied, and the column's
    /// `highlight` decides whether it is highlighted. Pass [`Cell::Text`] for
    /// data that must stay literal.
    Markup(String),
    /// A renderable cell, measured with `__rich_measure__` and rendered at the
    /// column width, as upstream's `Padding(renderable)` cell is.
    Renderable(Arc<dyn Renderable + Send + Sync>),
}

impl From<Text> for Cell {
    fn from(text: Text) -> Self {
        Cell::Text(text)
    }
}

/// A string cell is console markup, as upstream's `str` renderables are.
impl From<&str> for Cell {
    fn from(text: &str) -> Self {
        Cell::Markup(text.to_string())
    }
}

impl From<String> for Cell {
    fn from(text: String) -> Self {
        Cell::Markup(text)
    }
}

impl From<&String> for Cell {
    fn from(text: &String) -> Self {
        Cell::Markup(text.clone())
    }
}

impl Cell {
    /// The cell as `Text`, or `None` for a renderable. A markup string goes
    /// through [`Console::render_str`] with `highlight`.
    pub(crate) fn to_text(&self, console: &Console, highlight: Option<bool>) -> Option<Text> {
        match self {
            Cell::Text(text) => Some(text.clone()),
            Cell::Markup(markup) => Some(console.render_str(markup, highlight)),
            Cell::Renderable(_) => None,
        }
    }

    /// `Measurement.get(console, options, cell)`. A string is measured as
    /// upstream measures a `str`: `render_str(..., highlight=False)`, then the
    /// resulting `Text`'s `__rich_measure__`.
    pub(crate) fn measure_cell(&self, console: &Console, options: &ConsoleOptions) -> Measurement {
        match self {
            Cell::Text(text) => Measurement::get(console, options, text),
            // A string with no `[` or `:` is its own plain text (markup and
            // emoji leave it alone); measure it without building a `Text`.
            Cell::Markup(markup) if !markup.contains(['[', ':']) => {
                if options.max_width < 1 {
                    return Measurement::new(0, 0);
                }
                let (minimum, maximum) = crate::text::measure_plain(markup);
                let width = Measurement::new(minimum, maximum)
                    .normalize()
                    .with_maximum(options.max_width);
                if width.maximum < 1 {
                    Measurement::new(0, 0)
                } else {
                    width.normalize()
                }
            }
            Cell::Markup(markup) => {
                Measurement::get(console, options, &console.render_str(markup, Some(false)))
            }
            Cell::Renderable(renderable) => Measurement::get(console, options, renderable.as_ref()),
        }
    }
}

impl Default for Cell {
    fn default() -> Self {
        Cell::Text(Text::default())
    }
}

/// Options for a column, as upstream's `Column(...)` takes them. Pass to
/// [`Table::add_column_with`].
#[derive(Clone, Debug)]
pub struct ColumnOptions {
    /// Content justification (`justify`, default left).
    pub justify: Justify,
    /// A fixed content width (`width`).
    pub width: Option<usize>,
    /// A floor on the content width (`min_width`).
    pub min_width: Option<usize>,
    /// A cap on the content width (`max_width`).
    pub max_width: Option<usize>,
    /// A share of the free width when the table expands (`ratio`).
    pub ratio: Option<usize>,
    /// Never wrap cells (`no_wrap`).
    pub no_wrap: bool,
    /// How over-long text is handled (`overflow`, default ellipsis).
    pub overflow: Overflow,
    /// The style of the column's body cells (`style`).
    pub style: Style,
}

impl Default for ColumnOptions {
    fn default() -> Self {
        ColumnOptions {
            justify: Justify::Left,
            width: None,
            min_width: None,
            max_width: None,
            ratio: None,
            no_wrap: false,
            overflow: Overflow::Ellipsis,
            style: Style::new(),
        }
    }
}

/// A grid of cells rendered inside a box. Mirrors `rich.table.Table`.
pub struct Table {
    columns: Vec<Column>,
    rows: Vec<Row>,
    box_set: BoxSet,
    /// `box=None`: no borders and no column dividers (see [`Table::grid`]).
    no_box: bool,
    show_header: bool,
    show_lines: bool,
    show_edge: bool,
    pad_edge: bool,
    collapse_padding: bool,
    expand: bool,
    /// The title: a markup string or a [`Text`] (upstream `TextType`).
    title: Option<Cell>,
    caption: Option<Cell>,
    padding: (usize, usize, usize, usize),
    /// The header row's style (upstream default `"table.header"`).
    header_style: StyleType,
    /// The footer row's style (upstream default `"table.footer"`).
    footer_style: StyleType,
    border_style: Style,
    style: Style,
    /// Highlight `str` cells (port of `Table.highlight`, default `False`).
    highlight: bool,
    /// The table's width, borders included; setting it implies `expand`.
    width: Option<usize>,
    /// The table's minimum width, borders included.
    min_width: Option<usize>,
    show_footer: bool,
    /// Blank lines between rows (drawn with the box's `mid` row).
    leading: usize,
    /// Styles that body rows cycle through.
    row_styles: Vec<StyleType>,
    /// `None` (the default) takes `table.title` / `table.caption`.
    title_style: Option<StyleType>,
    caption_style: Option<StyleType>,
    title_justify: Justify,
    caption_justify: Justify,
    /// Substitute boxes a legacy Windows console cannot draw; `None` takes
    /// the console's `safe_box`.
    safe_box: Option<bool>,
}

impl Default for Table {
    fn default() -> Self {
        Table {
            columns: Vec::new(),
            rows: Vec::new(),
            box_set: HEAVY_HEAD,
            no_box: false,
            show_header: true,
            show_lines: false,
            show_edge: true,
            pad_edge: true,
            collapse_padding: false,
            expand: false,
            title: None,
            caption: None,
            padding: (0, 1, 0, 1),
            header_style: StyleType::Name("table.header".to_string()),
            footer_style: StyleType::Name("table.footer".to_string()),
            border_style: Style::new(),
            style: Style::new(),
            highlight: false,
            width: None,
            min_width: None,
            show_footer: false,
            leading: 0,
            row_styles: Vec::new(),
            title_style: None,
            caption_style: None,
            title_justify: Justify::Center,
            caption_justify: Justify::Center,
            safe_box: None,
        }
    }
}

impl Table {
    pub fn new() -> Self {
        Table::default()
    }

    /// A table with no borders, for laying out columns. Port of `Table.grid`:
    /// `box=None`, no header or edge, `padding=0`, `collapse_padding=True` and
    /// `pad_edge=False`.
    pub fn grid() -> Self {
        Table {
            no_box: true,
            show_header: false,
            show_edge: false,
            pad_edge: false,
            collapse_padding: true,
            padding: (0, 0, 0, 0),
            ..Table::default()
        }
    }

    /// Draw no borders and no column dividers (upstream `box=None`).
    pub fn without_box(mut self) -> Self {
        self.no_box = true;
        self
    }

    /// Cell padding as `(top, right, bottom, left)` (upstream `padding`).
    pub fn padding(mut self, top: usize, right: usize, bottom: usize, left: usize) -> Self {
        self.padding = (top, right, bottom, left);
        self
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

    /// Highlight string cells with the console's highlighter (upstream
    /// `Table(highlight=…)`, default off). Columns added without their own
    /// setting use it.
    pub fn highlight(mut self, highlight: bool) -> Self {
        self.highlight = highlight;
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

    /// A title rendered above the table (console markup, centered by
    /// default).
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(Cell::Markup(title.into()));
        self
    }

    /// A caption rendered below the table (console markup).
    pub fn caption(mut self, caption: impl Into<String>) -> Self {
        self.caption = Some(Cell::Markup(caption.into()));
        self
    }

    /// A [`Text`] title (upstream `title=Text(...)`): rendered as it is, in
    /// its own style and justify, with no `title_style`.
    pub fn title_text(mut self, title: Text) -> Self {
        self.title = Some(Cell::Text(title));
        self
    }

    /// A [`Text`] caption (upstream `caption=Text(...)`).
    pub fn caption_text(mut self, caption: Text) -> Self {
        self.caption = Some(Cell::Text(caption));
        self
    }

    /// The style of a markup title (upstream `title_style`; default
    /// `table.title`).
    pub fn title_style(mut self, style: impl Into<StyleType>) -> Self {
        self.title_style = Some(style.into());
        self
    }

    /// The style of a markup caption (upstream `caption_style`; default
    /// `table.caption`).
    pub fn caption_style(mut self, style: impl Into<StyleType>) -> Self {
        self.caption_style = Some(style.into());
        self
    }

    /// How the title is justified (upstream `title_justify`, default center).
    pub fn title_justify(mut self, justify: Justify) -> Self {
        self.title_justify = justify;
        self
    }

    /// How the caption is justified (upstream `caption_justify`).
    pub fn caption_justify(mut self, justify: Justify) -> Self {
        self.caption_justify = justify;
        self
    }

    /// The header row's style (upstream `header_style`, default
    /// `table.header`). A column's own header style combines over it.
    pub fn header_style(mut self, style: impl Into<StyleType>) -> Self {
        self.header_style = style.into();
        self
    }

    /// The footer row's style (upstream `footer_style`, default
    /// `table.footer`).
    pub fn footer_style(mut self, style: impl Into<StyleType>) -> Self {
        self.footer_style = style.into();
        self
    }

    /// The table's width, borders included (upstream `width`). Setting it
    /// expands the table to exactly that width.
    pub fn width(mut self, width: Option<usize>) -> Self {
        self.width = width;
        self
    }

    /// The table's minimum width, borders included (upstream `min_width`).
    pub fn min_width(mut self, min_width: Option<usize>) -> Self {
        self.min_width = min_width;
        self
    }

    /// Render a footer row from each column's `footer` (upstream
    /// `show_footer`).
    pub fn show_footer(mut self, show: bool) -> Self {
        self.show_footer = show;
        self
    }

    /// Blank lines between rows (upstream `leading`); takes precedence over
    /// `show_lines`.
    pub fn leading(mut self, leading: usize) -> Self {
        self.leading = leading;
        self
    }

    /// Styles body rows cycle through (upstream `row_styles`).
    pub fn row_styles(mut self, styles: Vec<StyleType>) -> Self {
        self.row_styles = styles;
        self
    }

    /// Whether to substitute boxes a legacy Windows console cannot draw
    /// (upstream `safe_box`; `None` takes the console's).
    pub fn safe_box(mut self, safe_box: Option<bool>) -> Self {
        self.safe_box = safe_box;
        self
    }

    /// Whether the table expands: `expand`, or any explicit `width`. Port
    /// of the `Table.expand` property.
    pub fn is_expand(&self) -> bool {
        self.expand || self.width.is_some()
    }

    /// The number of rows (upstream `row_count`).
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// The number of columns.
    pub fn column_count(&self) -> usize {
        self.columns.len()
    }

    /// The style of body row `index`: its place in `row_styles`, then its
    /// own style. Port of `Table.get_row_style`.
    pub fn get_row_style(&self, console: &Console, index: usize) -> Style {
        let theme = console.theme();
        let mut style = Style::new();
        if !self.row_styles.is_empty() {
            style = style.combine(
                &theme.get_style_or_null(&self.row_styles[index % self.row_styles.len()]),
            );
        }
        if let Some(row_style) = self.rows.get(index).and_then(|row| row.style.as_ref()) {
            style = style.combine(&theme.get_style_or_null(row_style));
        }
        style
    }

    /// End the current section: draw a line beneath the last row. Port of
    /// `Table.add_section`.
    pub fn add_section(&mut self) -> &mut Self {
        if let Some(row) = self.rows.last_mut() {
            row.end_section = true;
        }
        self
    }

    /// Add a left-justified column with the given header, which is console
    /// markup as upstream's `add_column("[b]Name")` is.
    pub fn add_column(&mut self, header: impl Into<String>) -> &mut Self {
        self.add_column_justify(header, Justify::Left)
    }

    /// Add a column with an explicit justification. The header is console
    /// markup; use [`add_column_text`](Self::add_column_text) for a literal one.
    pub fn add_column_justify(&mut self, header: impl Into<String>, justify: Justify) -> &mut Self {
        self.add_column_text(Text::default(), justify);
        if let Some(column) = self.columns.last_mut() {
            column.header = Cell::Markup(header.into());
        }
        self
    }

    /// Add a column whose header is a styled [`Text`], as upstream's
    /// `add_column(header=Text(...))` does. The text's spans survive into the
    /// header cell; its own `justify`, `overflow` and `no_wrap` override the
    /// column's, as `Text.__rich_console__` prefers them over the options.
    pub fn add_column_text(&mut self, header: Text, justify: Justify) -> &mut Self {
        self.columns.push(Column::new(Cell::Text(header), justify));
        self
    }

    /// Add a column whose header is any [`Cell`] (upstream's `header` is any
    /// renderable).
    pub fn add_column_cell(&mut self, header: Cell, justify: Justify) -> &mut Self {
        self.columns.push(Column::new(header, justify));
        self
    }

    /// Set the most-recently-added column's footer (upstream
    /// `Column.footer`), shown with [`show_footer`](Self::show_footer).
    pub fn column_footer(&mut self, footer: impl Into<Cell>) -> &mut Self {
        if let Some(column) = self.columns.last_mut() {
            column.footer = footer.into();
        }
        self
    }

    /// Style the most-recently-added column's whole footer cell, combined
    /// over the table-level `footer_style` (upstream `Column.footer_style`).
    pub fn column_footer_fill(&mut self, style: Style) -> &mut Self {
        if let Some(column) = self.columns.last_mut() {
            column.footer_fill = Some(style);
        }
        self
    }

    /// Add a column with every [`ColumnOptions`] set, as upstream's
    /// `add_column(header, justify=…, width=…, ratio=…, …)` does.
    pub fn add_column_with(&mut self, header: Text, options: ColumnOptions) -> &mut Self {
        self.columns.push(Column {
            width: options.width,
            style: options.style,
            ratio: options.ratio,
            min_width: options.min_width,
            max_width: options.max_width,
            no_wrap: options.no_wrap,
            overflow: options.overflow,
            ..Column::new(Cell::Text(header), options.justify)
        });
        self
    }

    /// Set the vertical alignment of the most-recently-added column's body
    /// cells (upstream `Column.vertical`, default top). A cell whose
    /// renderable has its own [`vertical`](crate::protocol::Renderable::vertical)
    /// uses that instead. Chain after `add_column`.
    pub fn column_vertical(&mut self, vertical: crate::align::VerticalAlign) -> &mut Self {
        if let Some(column) = self.columns.last_mut() {
            column.vertical = vertical;
        }
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

    /// Set how the most-recently-added column handles over-long text (upstream
    /// `Column.overflow`, default ellipsis). Chain after `add_column`.
    pub fn column_overflow(&mut self, overflow: Overflow) -> &mut Self {
        if let Some(column) = self.columns.last_mut() {
            column.overflow = overflow;
        }
        self
    }

    /// Set whether the most-recently-added column highlights its string cells
    /// (upstream `Column.highlight`). Chain after `add_column`.
    pub fn column_highlight(&mut self, highlight: bool) -> &mut Self {
        if let Some(column) = self.columns.last_mut() {
            column.highlight = Some(highlight);
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

    /// Add a row of string cells (extra cells are ignored; missing cells render
    /// empty). Each string is console markup, as upstream's `add_row("[b]x")`
    /// is; use [`add_row_text`](Self::add_row_text) for literal data.
    pub fn add_row(&mut self, cells: &[&str]) -> &mut Self {
        self.push_row(
            cells
                .iter()
                .map(|s| Cell::Markup((*s).to_string()))
                .collect(),
            None,
            false,
        )
    }

    /// Add a row with upstream's keyword options: a `style` for the whole
    /// row and `end_section` to draw a line beneath it. Port of
    /// `add_row(*renderables, style=…, end_section=…)`.
    pub fn add_row_with(
        &mut self,
        cells: Vec<Cell>,
        style: Option<StyleType>,
        end_section: bool,
    ) -> &mut Self {
        self.push_row(cells, style, end_section)
    }

    /// Port of `add_row`'s body: a row longer than the table adds a column
    /// (with every default and the table's `highlight`) for each extra cell.
    fn push_row(
        &mut self,
        cells: Vec<Cell>,
        style: Option<StyleType>,
        end_section: bool,
    ) -> &mut Self {
        while self.columns.len() < cells.len() {
            let mut column = Column::new(Cell::Markup(String::new()), Justify::Left);
            column.highlight = Some(self.highlight);
            self.columns.push(column);
        }
        self.rows.push(Row {
            cells,
            style,
            end_section,
        });
        self
    }

    /// Add a row of styled [`Text`] cells, as upstream's `add_row(Text(...))`.
    /// Each cell keeps its spans, and its own `justify`, `overflow` and
    /// `no_wrap` override the column's.
    pub fn add_row_text(&mut self, cells: Vec<Text>) -> &mut Self {
        self.push_row(cells.into_iter().map(Cell::Text).collect(), None, false)
    }

    /// Add a row of [`Cell`]s, which may be any renderable.
    pub fn add_row_cells(&mut self, cells: Vec<Cell>) -> &mut Self {
        self.push_row(cells, None, false)
    }

    /// The width of the borders: `ncols - 1` dividers, plus the two outer
    /// edges when shown; no box, no border. Port of `_extra_width`.
    fn extra_width(&self) -> usize {
        if self.no_box {
            0
        } else {
            (if self.show_edge { 2 } else { 0 }) + self.columns.len().saturating_sub(1)
        }
    }

    /// The column's padding width. Port of `_get_padding_width`, which (unlike
    /// the per-cell padding of `_get_cells`) drops the left pad entirely under
    /// `collapse_padding`.
    fn padding_width(&self, index: usize) -> usize {
        let (_, mut pad_right, _, mut pad_left) = self.padding;
        if self.collapse_padding {
            pad_left = 0;
        }
        if !self.pad_edge {
            if index == 0 {
                pad_left = 0;
            }
            if index + 1 == self.columns.len() {
                pad_right = 0;
            }
        }
        pad_left + pad_right
    }

    /// `Measurement.get` of one of `_get_cells`' cells: the cell wrapped in
    /// `Padding(renderable, (0, right, 0, left))` when the table has any
    /// padding. Port of `Padding.__rich_measure__` over the cell.
    fn measure_padded_cell(
        &self,
        console: &Console,
        options: &ConsoleOptions,
        cell: &Cell,
        (left, right): (usize, usize),
    ) -> Measurement {
        let max_width = options.max_width;
        if max_width < 1 {
            return Measurement::new(0, 0);
        }
        let (top, pr, bottom, pl) = self.padding;
        if top == 0 && pr == 0 && bottom == 0 && pl == 0 {
            return cell.measure_cell(console, options);
        }
        let extra_width = left + right;
        let width = if max_width < extra_width + 1 {
            Measurement::new(max_width, max_width)
        } else {
            let inner = cell.measure_cell(console, options);
            Measurement::new(inner.minimum + extra_width, inner.maximum + extra_width)
                .with_maximum(max_width)
        };
        // `Measurement.get` around the `Padding`.
        let width = width.normalize().with_maximum(max_width);
        if width.maximum < 1 {
            Measurement::new(0, 0)
        } else {
            width.normalize()
        }
    }

    /// The minimum and maximum width of column `index` (content + padding).
    /// Port of `Table._measure_column`: every cell, header included, is
    /// measured with `Measurement.get`, so a nested renderable (a `Table`,
    /// `Panel`, …) sizes its column by its own `__rich_measure__`.
    fn measure_column(
        &self,
        console: &Console,
        options: &ConsoleOptions,
        index: usize,
    ) -> Measurement {
        let max_width = options.max_width;
        if max_width < 1 {
            return Measurement::new(0, 0);
        }
        let column = &self.columns[index];
        let padding_width = self.padding_width(index);
        if let Some(width) = column.width {
            // Fixed width column.
            return Measurement::new(width + padding_width, width + padding_width)
                .with_maximum(max_width);
        }
        // Every cell of a column shares its left/right padding; only the
        // vertical padding depends on the row.
        let padding = self.cell_padding(index, self.columns.len());
        let empty = Cell::Markup(String::new());
        let header = self.show_header.then_some(&column.header);
        let body = self
            .rows
            .iter()
            .map(|row| row.cells.get(index).unwrap_or(&empty));
        let footer = self.show_footer.then_some(&column.footer);
        let mut measured = false;
        let (mut minimum, mut maximum) = (0, 0);
        for cell in header.into_iter().chain(body).chain(footer) {
            let width = self.measure_padded_cell(console, options, cell, padding);
            minimum = minimum.max(width.minimum);
            maximum = maximum.max(width.maximum);
            measured = true;
        }
        let measurement = if measured {
            Measurement::new(minimum, maximum)
        } else {
            Measurement::new(1, max_width)
        }
        .with_maximum(max_width);
        measurement.clamp(
            column.min_width.map(|width| width + padding_width),
            column.max_width.map(|width| width + padding_width),
        )
    }

    /// The rendered width (content + padding) of each column, shrinking the
    /// widest columns to fit `available` when necessary. Port of
    /// `Table._calculate_column_widths` + `_collapse_widths`.
    fn column_widths(
        &self,
        console: &Console,
        options: &ConsoleOptions,
        available: usize,
    ) -> Vec<usize> {
        let options = &options.update_width(available);
        let max_width = available as i64;
        let extra_width = self.extra_width() as i64;
        // A fixed-width column uses its declared width; others measure content,
        // clamped to the column's [min_width, max_width]. Port of `_measure_column`.
        let maximums: Vec<i64> = (0..self.columns.len())
            .map(|index| self.measure_column(console, options, index).maximum as i64)
            .collect();
        let mut widths: Vec<i64> = maximums.iter().map(|&width| width.max(1)).collect();

        // Expand with explicit ratios: flexible (ratio) columns share the free
        // width in proportion, fixed columns keep their measured width. Port of
        // the `if self.expand: … if any(ratios)` block of `_calculate_column_widths`.
        if self.is_expand() {
            let ratios: Vec<i64> = self
                .columns
                .iter()
                .filter(|c| c.ratio.is_some())
                .map(|c| i64::try_from(c.ratio.unwrap()).unwrap_or(i64::MAX))
                .collect();
            if ratios.iter().any(|&r| r > 0) {
                let fixed_widths: Vec<i64> = maximums
                    .iter()
                    .zip(&self.columns)
                    .map(|(&w, c)| if c.ratio.is_some() { 0 } else { w })
                    .collect();
                let flex_minimum: Vec<i64> = self
                    .columns
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| c.ratio.is_some())
                    .map(|(index, c)| (c.width.unwrap_or(1) + self.padding_width(index)) as i64)
                    .collect();
                let flexible_width = max_width - fixed_widths.iter().sum::<i64>();
                let flex_widths = ratio_distribute(flexible_width, &ratios, Some(&flex_minimum));
                let mut iter_flex = flex_widths.into_iter();
                for (index, column) in self.columns.iter().enumerate() {
                    if column.ratio.is_some() {
                        widths[index] = fixed_widths[index] + iter_flex.next().unwrap_or(0);
                    }
                }
            }
        }

        let mut table_width: i64 = widths.iter().sum();
        if table_width > max_width {
            // Only auto-width, wrapping columns may shrink; fixed and no_wrap
            // columns hold their width (no_wrap only yields via the last resort).
            let wrapable: Vec<bool> = self
                .columns
                .iter()
                .map(|c| c.width.is_none() && !c.no_wrap)
                .collect();
            widths = collapse_widths(widths, &wrapable, max_width);
            table_width = widths.iter().sum();
            // Last resort: if fixed columns still overflow, reduce every column
            // evenly. Port of `_calculate_column_widths`'s final `ratio_reduce`.
            if table_width > max_width {
                let excess = table_width - max_width;
                let ratios = vec![1i64; widths.len()];
                widths = ratio_reduce(excess, &ratios, &widths, &widths);
                table_width = widths.iter().sum();
            }
            // Upstream measures every column again at its reduced width, so a
            // `min_width` re-inflates its column and the table overflows (the
            // console crop then cuts it). `table_width` keeps the reduced sum,
            // as upstream's does.
            widths = widths
                .iter()
                .enumerate()
                .map(|(index, &width)| {
                    self.measure_column(
                        console,
                        &options.update_width(width.max(0) as usize),
                        index,
                    )
                    .maximum as i64
                })
                .collect();
        }

        // Expand, or grow to the table's `min_width`: distribute the leftover
        // width proportionally (`ratio_distribute`, which asserts a positive
        // total ratio — a table of zero-width columns is left as it is).
        let min_width = self.min_width.map(|width| width as i64);
        if (table_width < max_width && self.is_expand())
            || min_width.is_some_and(|min_width| table_width < min_width - extra_width)
        {
            let target = match min_width {
                None => max_width,
                Some(min_width) => (min_width - extra_width).min(max_width),
            };
            if widths.iter().sum::<i64>() > 0 {
                let pad = ratio_distribute(target - table_width, &widths, None);
                for (width, extra) in widths.iter_mut().zip(pad) {
                    *width += extra;
                }
            }
        }
        widths.into_iter().map(|w| w.max(0) as usize).collect()
    }

    /// `cell_padding` shrunk so that padding alone can never exceed the width
    /// the column was actually allotted.
    ///
    /// When many columns compete for a narrow terminal a column can be squeezed
    /// below its own padding. The cell then still emitted a full left and right
    /// pad, so every such column spent two cells where its border spent one and
    /// the content row grew wider than the table — at 29 columns in an 80-cell
    /// terminal the row overflowed by 15 cells and was cropped, taking the
    /// right-hand border with it while the border rows kept theirs.
    fn cell_padding_fitted(&self, index: usize, ncols: usize, rendered: usize) -> (usize, usize) {
        let (mut pl, mut pr) = self.cell_padding(index, ncols);
        while pl + pr > rendered {
            if pr > pl {
                pr -= 1;
            } else if pl > 0 {
                pl -= 1;
            } else {
                break;
            }
        }
        (pl, pr)
    }

    /// The effective style for a cell in column `index`: `_get_cells`' cell
    /// style plus the row style. The header row takes `header_style` and the
    /// column's own header style, the footer likewise, and a body cell the
    /// column's style and then its row's.
    fn cell_style(&self, console: &Console, index: usize, kind: &RowKind) -> Style {
        let column = self.columns.get(index);
        match kind {
            RowKind::Header => {
                let base = console.theme().get_style_or_null(&self.header_style);
                match column.and_then(|c| c.header_fill.as_ref()) {
                    Some(fill) => base.combine(fill),
                    None => base,
                }
            }
            RowKind::Footer => {
                let base = console.theme().get_style_or_null(&self.footer_style);
                match column.and_then(|c| c.footer_fill.as_ref()) {
                    Some(fill) => base.combine(fill),
                    None => base,
                }
            }
            RowKind::Body(row_style) => column
                .map(|c| c.style.clone())
                .unwrap_or_default()
                .combine(row_style),
        }
    }

    /// Pad a cell's rendered lines to its width, with the vertical padding
    /// above and below: upstream's `Padding` around the cell. Blank rows are
    /// one run across the whole cell, as `Padding`'s blank lines are.
    /// The `(top, bottom)` padding of a cell in the first and/or last of the
    /// rendered rows (header included). Port of `_get_cells`' `get_padding`:
    /// with `collapse_padding` every row but the last keeps only
    /// `max(0, top - bottom)` below it, and without `pad_edge` the first row
    /// loses its top and the last row its bottom.
    fn vertical_padding(&self, first_row: bool, last_row: bool) -> (usize, usize) {
        let (mut top, _, mut bottom, _) = self.padding;
        if self.collapse_padding && !last_row {
            bottom = top.saturating_sub(bottom);
        }
        if !self.pad_edge {
            if first_row {
                top = 0;
            }
            if last_row {
                bottom = 0;
            }
        }
        (top, bottom)
    }

    fn pad_cell_lines(
        &self,
        lines: Vec<Vec<Segment>>,
        width: usize,
        (cpl, cpr): (usize, usize),
        (pt, pb): (usize, usize),
        style: &Style,
    ) -> Vec<Vec<Segment>> {
        let cell_fill = Some(style.clone());
        let cell_width = cpl + width + cpr;
        // The cell's `Padding` renders at the whole cell width, and
        // `Console.render` yields nothing at all below width 1: no content and
        // no vertical padding either (the row keeps its minimum height of 1).
        if cell_width == 0 {
            return Vec::new();
        }
        let blank = || vec![Segment::new(" ".repeat(cell_width), cell_fill.clone())];
        let mut padded_lines: Vec<Vec<Segment>> = Vec::new();
        for _ in 0..pt {
            padded_lines.push(blank());
        }
        for line in &lines {
            let mut row = Vec::new();
            if cpl > 0 {
                row.push(Segment::new(" ".repeat(cpl), cell_fill.clone()));
            }
            // The cell's segments pass through, as `Padding` yields them.
            row.extend(Segment::adjust_line_length(line, width, cell_fill.clone()));
            if cpr > 0 {
                row.push(Segment::new(" ".repeat(cpr), cell_fill.clone()));
            }
            padded_lines.push(row);
        }
        for _ in 0..pb {
            padded_lines.push(blank());
        }
        padded_lines
    }

    /// Render one table row (a list of cell strings) into visual lines.
    #[allow(clippy::too_many_arguments)]
    fn render_row(
        &self,
        console: &Console,
        options: &ConsoleOptions,
        cells: &[Cell],
        rendered_widths: &[usize],
        kind: &RowKind,
        (first_row, last_row): (bool, bool),
        edges: Option<(Segment, Segment, Segment)>,
    ) -> Vec<Vec<Segment>> {
        let is_header = matches!(kind, RowKind::Header);
        // Horizontal padding is per-column (see `cell_padding`); vertical
        // padding depends on the row's place (see `vertical_padding`).
        let vertical = self.vertical_padding(first_row, last_row);
        let ncols = self.columns.len();
        // Derived here rather than by the caller so the padding used to lay the
        // row out is the same padding the content width was reduced by.
        let paddings: Vec<(usize, usize)> = (0..ncols)
            .map(|index| {
                let rendered = rendered_widths.get(index).copied().unwrap_or(0);
                self.cell_padding_fitted(index, ncols, rendered)
            })
            .collect();
        let content_widths: Vec<usize> = rendered_widths
            .iter()
            .zip(&paddings)
            .map(|(w, (pl, pr))| w.saturating_sub(pl + pr))
            .collect();

        // Render each cell into padded, simplified visual lines.
        let mut cell_lines: Vec<Vec<Vec<Segment>>> = Vec::with_capacity(ncols);
        let mut height = 1;
        for (index, width) in content_widths.iter().enumerate() {
            let style = self.cell_style(console, index, kind);
            let column = self.columns.get(index);
            let mut text = match cells.get(index) {
                Some(Cell::Text(text)) => text.clone(),
                // `render_options.update(highlight=column.highlight)`, then
                // `Console.render` of a `str` calls `render_str`.
                Some(Cell::Markup(markup)) => console.render_str(
                    markup,
                    Some(column.and_then(|c| c.highlight).unwrap_or(self.highlight)),
                ),
                None => Text::default(),
                Some(Cell::Renderable(renderable)) => {
                    // `console.render_lines(renderable, render_options, style)`
                    // at the content width, with the column's justify,
                    // no_wrap and overflow as options.
                    let mut cell_options = options.update_width(*width);
                    cell_options.highlight =
                        Some(column.and_then(|c| c.highlight).unwrap_or(self.highlight));
                    cell_options.justify = column.map_or(Justify::Left, |c| c.justify);
                    cell_options.no_wrap = Some(column.is_some_and(|c| c.no_wrap));
                    cell_options.overflow = Some(column.map_or(Overflow::Ellipsis, |c| c.overflow));
                    let lines = if *width == 0 {
                        Vec::new()
                    } else {
                        console.render_lines_styled(
                            renderable.as_ref(),
                            &cell_options,
                            Some(&style),
                            true,
                        )
                    };
                    cell_lines.push(self.pad_cell_lines(
                        lines,
                        *width,
                        paddings[index],
                        vertical,
                        &style,
                    ));
                    height = height.max(cell_lines.last().map_or(0, Vec::len));
                    continue;
                }
            };
            // Upstream renders the cell `Text` with the column's `justify`,
            // `no_wrap` and `overflow="ellipsis"` as options, which the text's own
            // settings override: wrap, then justify (which strips a right- or
            // center-justified line before measuring it), then truncate.
            let justify = match text.get_justify() {
                Justify::Default => column.map(|c| c.justify).unwrap_or(Justify::Left),
                own => own,
            };
            let overflow = text
                .get_overflow()
                .unwrap_or_else(|| column.map_or(Overflow::Ellipsis, |c| c.overflow));
            let no_wrap = text
                .get_no_wrap()
                .unwrap_or_else(|| column.map(|c| c.no_wrap).unwrap_or(false));
            // Header content carries its own style span over `header_style`; the
            // justify/edge padding stays `header_style` (matches upstream).
            if is_header {
                if let Some(span) = column.and_then(|c| c.header_content_style.clone()) {
                    let len = text.plain().len();
                    text.stylize(span, 0, len);
                }
            }
            // Upstream renders the cell as `Padding(renderable, …)` through
            // `render_lines`: a zero-width content area renders no lines
            // (`Console.render` returns nothing below width 1), while empty
            // text still renders one blank line.
            //
            // The text renders on its own and the cell style is applied to the
            // result (`render_lines(..., style=...)`), so a span keeps its own
            // segment even where it matches the cell style: `[b]Name` under a
            // bold header is `Name` + padding, as upstream prints it. The
            // justify padding joins the text's last run only where no span
            // ends there, as `Text.pad_right` on the plain string does.
            let tab_size = text.console_tab_size(console);
            let mut lines: Vec<Vec<Segment>> = if *width == 0 {
                Vec::new()
            } else {
                text.render_lines_wrapped_tabs(
                    console.theme(),
                    &Style::new(),
                    Some(*width),
                    justify,
                    overflow,
                    no_wrap,
                    tab_size,
                )
                .iter()
                .map(|line| Segment::apply_style(line, &style))
                .collect()
            };
            if lines.is_empty() && *width > 0 {
                lines.push(Vec::new());
            }
            let padded_lines =
                self.pad_cell_lines(lines, *width, paddings[index], vertical, &style);
            height = height.max(padded_lines.len());
            cell_lines.push(padded_lines);
        }

        // Shape every cell to the row height (#445). Upstream aligns each cell
        // to `row_height` (the tallest cell, possibly 0) with the cell style:
        // header cells to the bottom, body cells (vertical "top") to the top.
        // `Segment.set_shape` then pads to `max_height` (at least 1) with an
        // unstyled blank.
        let row_height = cell_lines.iter().map(Vec::len).max().unwrap_or(0);
        for (index, lines) in cell_lines.iter_mut().enumerate() {
            // `getattr(renderable, "vertical", None) or column.vertical`; a
            // header row aligns to the bottom and a footer to the top,
            // whatever the cell says.
            let vertical = if is_header {
                crate::align::VerticalAlign::Bottom
            } else if matches!(kind, RowKind::Footer) {
                crate::align::VerticalAlign::Top
            } else {
                match cells.get(index) {
                    Some(Cell::Renderable(renderable)) => renderable.vertical(),
                    _ => None,
                }
                .unwrap_or_else(|| {
                    self.columns
                        .get(index)
                        .map_or(crate::align::VerticalAlign::Top, |c| c.vertical)
                })
            };
            let (cpl, cpr) = paddings[index];
            let blank = " ".repeat(cpl + content_widths[index] + cpr);
            let filler = vec![Segment::new(
                blank.clone(),
                Some(self.cell_style(console, index, kind)),
            )];
            let missing = row_height.saturating_sub(lines.len());
            let top = match vertical {
                crate::align::VerticalAlign::Top => 0,
                crate::align::VerticalAlign::Middle => missing / 2,
                crate::align::VerticalAlign::Bottom => missing,
            };
            lines.splice(0..0, std::iter::repeat_n(filler.clone(), top));
            lines.extend(std::iter::repeat_n(filler, missing - top));
            while lines.len() < height {
                lines.push(vec![Segment::new(blank.clone(), None)]);
            }
        }

        let last = ncols.saturating_sub(1);
        let mut rows_out: Vec<Vec<Segment>> = Vec::with_capacity(height);
        // `r` indexes into each column's per-line vector, so a range loop is the
        // natural shape here (the columns are iterated with `enumerate`).
        #[allow(clippy::needless_range_loop)]
        for r in 0..height {
            let mut row = Vec::new();
            if let (Some((edge_left, _, _)), true) = (&edges, self.show_edge) {
                row.push(edge_left.clone());
            }
            for (c, column_lines) in cell_lines.iter().enumerate() {
                row.extend(column_lines[r].clone());
                let Some((_, divider, edge_right)) = &edges else {
                    continue;
                };
                if c != last {
                    row.push(divider.clone());
                } else if self.show_edge {
                    row.push(edge_right.clone());
                }
            }
            rows_out.push(row);
        }
        rows_out
    }
}

impl LineRenderable for Table {
    /// Render visual lines in order without retaining the full rendered table.
    ///
    /// Like upstream's `Table.__rich_console__` / `_render` generators, this
    /// measures all columns first, then renders only one row block at a time.
    /// Lines contain styled segments without a trailing newline. The callback
    /// may write each line immediately; its first error stops rendering.
    /// The table still owns its source rows for column-width measurement.
    fn try_for_each_line<E>(
        &self,
        console: &Console,
        options: &ConsoleOptions,
        mut emit: impl FnMut(Vec<Segment>) -> Result<(), E>,
    ) -> Result<(), E> {
        if self.columns.is_empty() {
            return emit(vec![Segment::new("", None)]);
        }
        // Fall back to a terminal-safe box on legacy Windows / non-UTF-8, and
        // to a plain-headed box when there is no header to set apart.
        let box_set = self.box_set.substitute(
            console.legacy_windows(),
            self.safe_box.unwrap_or_else(|| console.safe_box()),
            console.ascii_only(),
        );
        let box_set = if self.show_header {
            box_set
        } else {
            box_set.get_plain_headed_box()
        };
        // `max_width = self.width` when the table has one.
        let max_width = self.width.unwrap_or(options.max_width);
        let extra_width = self.extra_width();
        let available = max_width.saturating_sub(extra_width);

        let rendered_widths = self.column_widths(console, options, available);
        let border_style = self.style.combine(&self.border_style);
        let border = Some(border_style.clone());

        // Full table width (for the title and caption): columns + borders.
        let table_width: usize = rendered_widths.iter().sum::<usize>() + extra_width;

        if let Some(title) = &self.title {
            let style = self
                .title_style
                .clone()
                .unwrap_or_else(|| StyleType::Name("table.title".to_string()));
            for line in render_annotation(
                console,
                options,
                title,
                &style,
                self.title_justify,
                table_width,
            ) {
                emit(line)?;
            }
        }

        let edge = self.show_edge;
        let boxed = !self.no_box;
        if boxed && edge {
            emit(vec![Segment::new(
                box_set.get_top(&rendered_widths, edge),
                border.clone(),
            )])?;
        }

        // `box_segments[0 if first else (2 if last else 1)]`: the first row
        // drawn (header or not) takes the head glyphs and the last the foot's.
        let segments = |(left, vertical, right): (char, char, char)| {
            (
                Segment::new(left.to_string(), border.clone()),
                Segment::new(vertical.to_string(), border.clone()),
                Segment::new(right.to_string(), border.clone()),
            )
        };
        let head_edges = segments((box_set.head_left, box_set.head_vertical, box_set.head_right));
        let mid_edges = segments((box_set.mid_left, box_set.mid_vertical, box_set.mid_right));
        let foot_edges = segments((box_set.foot_left, box_set.foot_vertical, box_set.foot_right));

        // Every row drawn, as upstream's `row_cells` holds them.
        let mut row_list: Vec<(RowKind, &[Cell], Option<&Row>)> = Vec::new();
        let header_cells: Vec<Cell> = self.columns.iter().map(|c| c.header.clone()).collect();
        let footer_cells: Vec<Cell> = self.columns.iter().map(|c| c.footer.clone()).collect();
        if self.show_header {
            row_list.push((RowKind::Header, &header_cells, None));
        }
        for (index, row) in self.rows.iter().enumerate() {
            row_list.push((
                RowKind::Body(self.get_row_style(console, index)),
                &row.cells,
                Some(row),
            ));
        }
        if self.show_footer {
            row_list.push((RowKind::Footer, &footer_cells, None));
        }
        let total = row_list.len();
        for (index, (kind, cells, row)) in row_list.iter().enumerate() {
            let first = index == 0;
            let last = index + 1 == total;
            let header_row = first && self.show_header;
            if boxed && last && self.show_footer {
                emit(vec![Segment::new(
                    box_set.get_row(&rendered_widths, RowLevel::Foot, edge),
                    border.clone(),
                )])?;
            }
            let edges = boxed.then(|| {
                let (left, divider, right) = if first {
                    head_edges.clone()
                } else if last {
                    foot_edges.clone()
                } else {
                    mid_edges.clone()
                };
                // A whitespace divider takes the row's background too.
                let divider = if divider.text.trim().is_empty() {
                    let background = match kind {
                        RowKind::Body(row_style) => {
                            Style::from_color(None, row_style.bgcolor().cloned())
                        }
                        _ => Style::new(),
                    };
                    Segment::new(divider.text.clone(), Some(background.combine(&border_style)))
                } else {
                    divider
                };
                (left, divider, right)
            });
            for line in self.render_row(
                console,
                options,
                cells,
                &rendered_widths,
                kind,
                (first, last),
                edges,
            ) {
                emit(line)?;
            }
            if boxed && header_row {
                emit(vec![Segment::new(
                    box_set.get_row(&rendered_widths, RowLevel::Head, edge),
                    border.clone(),
                )])?;
            }
            let end_section = row.is_some_and(|row| row.end_section);
            if boxed
                && (self.show_lines || self.leading > 0 || end_section)
                && !last
                && !(self.show_footer && index + 2 >= total)
                && !header_row
            {
                let line = if self.leading > 0 {
                    // Upstream repeats the row on one line, as it is here.
                    box_set
                        .get_row(&rendered_widths, RowLevel::Mid, edge)
                        .repeat(self.leading)
                } else {
                    box_set.get_row(&rendered_widths, RowLevel::Row, edge)
                };
                emit(vec![Segment::new(line, border.clone())])?;
            }
        }

        if boxed && edge {
            emit(vec![Segment::new(
                box_set.get_bottom(&rendered_widths, edge),
                border.clone(),
            )])?;
        }

        if let Some(caption) = &self.caption {
            let style = self
                .caption_style
                .clone()
                .unwrap_or_else(|| StyleType::Name("table.caption".to_string()));
            for line in render_annotation(
                console,
                options,
                caption,
                &style,
                self.caption_justify,
                table_width,
            ) {
                emit(line)?;
            }
        }

        Ok(())
    }
}

impl crate::protocol::OwnedTableRows for Table {
    fn extend_owned_rows(&mut self, rows: Vec<Vec<String>>) -> &mut Self {
        for row in rows {
            self.push_row(row.into_iter().map(Cell::Markup).collect(), None, false);
        }
        self
    }
}

impl Renderable for Table {
    /// Port of `Table.__rich_measure__`: the column widths the table would
    /// render at, then each column measured within their total; an explicit
    /// `width` is the maximum, and `min_width` a floor.
    fn measure(&self, console: &Console, options: &ConsoleOptions) -> Measurement {
        let max_width = self.width.unwrap_or(options.max_width);
        if self.columns.is_empty() {
            // `_extra_width` counts `len(columns) - 1` dividers, so an empty
            // boxed table measures `2 - 1` with edges and `-1` (normalized to
            // 0) without.
            let width = usize::from(!self.no_box && self.show_edge);
            return Measurement::new(width, self.width.unwrap_or(width))
                .clamp(self.min_width, None);
        }
        let extra_width = self.extra_width();
        let columns_width: usize = self
            .column_widths(console, options, max_width.saturating_sub(extra_width))
            .iter()
            .sum();
        let options = options.update_width(columns_width);
        let (minimum, maximum) = (0..self.columns.len())
            .map(|index| self.measure_column(console, &options, index))
            .fold((0, 0), |(minimum, maximum), width| {
                (minimum + width.minimum, maximum + width.maximum)
            });
        let maximum = self.width.unwrap_or(maximum + extra_width);
        Measurement::new(minimum + extra_width, maximum).clamp(self.min_width, None)
    }

    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let mut segments = Vec::new();
        let mut first = true;
        let result: Result<(), std::convert::Infallible> =
            self.try_for_each_line(console, options, |line| {
                if !first {
                    segments.push(Segment::line());
                }
                first = false;
                segments.extend(line);
                Ok(())
            });
        match result {
            Ok(()) => segments,
            Err(never) => match never {},
        }
    }
}

/// Port of `Table.__rich_console__.render_annotation`: a string title is
/// `console.render_str(text, style=style, highlight=False)` (markup and emoji
/// on, no highlighting); a `Text` renders as it is. Either renders at the
/// table's width with `justify` unless the text has its own.
fn render_annotation(
    console: &Console,
    options: &ConsoleOptions,
    annotation: &Cell,
    style: &StyleType,
    justify: Justify,
    width: usize,
) -> Vec<Vec<Segment>> {
    let text = match annotation {
        Cell::Markup(markup) => {
            if markup.is_empty() {
                return Vec::new();
            }
            let mut text = console.render_str(markup, Some(false));
            text.set_base_style(style.clone());
            text
        }
        Cell::Text(text) => {
            if text.plain().is_empty() {
                return Vec::new();
            }
            text.clone()
        }
        Cell::Renderable(_) => return Vec::new(),
    };
    if width == 0 {
        return Vec::new();
    }
    let justify = match text.get_justify() {
        Justify::Default => justify,
        own => own,
    };
    let overflow = text
        .get_overflow()
        .or(options.overflow)
        .unwrap_or(Overflow::Fold);
    let no_wrap = text.get_no_wrap().or(options.no_wrap).unwrap_or(false);
    let tab_size = text.console_tab_size(console);
    text.render_lines_wrapped_tabs(
        console.theme(),
        console.base_style(),
        Some(width),
        justify,
        overflow,
        no_wrap,
        tab_size,
    )
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
    let mut total_ratio: i128 = ratios.iter().map(|&r| i128::from(r)).sum();
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
            total_ratio -= i128::from(ratio);
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
    // Python ints never overflow; `ratio * total_remaining` can exceed i64
    // for a huge ratio, so the arithmetic runs in i128.
    let mut total_ratio: i128 = ratios.iter().map(|&r| i128::from(r)).sum();
    let mut total_remaining = i128::from(total);
    let mut result = Vec::with_capacity(ratios.len());
    for (index, &ratio) in ratios.iter().enumerate() {
        let ratio = i128::from(ratio);
        let minimum = i128::from(minimums.map_or(0, |m| m[index]));
        let distributed = if total_ratio > 0 {
            // ceil(ratio * total_remaining / total_ratio) for positive values,
            // then floored at `minimum`.
            let numerator = ratio * total_remaining;
            let ceil_div = (numerator + total_ratio - 1) / total_ratio;
            minimum.max(ceil_div)
        } else {
            total_remaining
        };
        result.push(i64::try_from(distributed).unwrap_or(if distributed < 0 {
            i64::MIN
        } else {
            i64::MAX
        }));
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

    #[test]
    fn a_huge_column_ratio_does_not_overflow() {
        // Python ints never overflow. Expected output captured from rich 15.0.0
        // (`ratio=2**64 - 1`; the collapsed widths come out narrow there too).
        let console = crate::Console::builder()
            .width(40)
            .color_system(None)
            .build();
        let mut table = Table::new().expand(true);
        table.add_column("a").column_ratio(usize::MAX);
        table.add_column("b").column_ratio(1);
        table.add_row(&["x", "y"]);
        assert_eq!(
            console.render_to_string(&table) + "\n",
            "┏━━━┳━━━┓\n┃ a ┃ b ┃\n┡━━━╇━━━┩\n│ x │ y │\n└───┴───┘\n"
        );
    }

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
    fn owned_rows_preserve_measurement_styles_and_missing_cells() {
        use crate::protocol::OwnedTableRows;
        for width in [1, 12, 40, 80] {
            let console = Console::builder().width(width).force_terminal(true).build();
            let build = || {
                let mut table = Table::new()
                    .title("Rows")
                    .caption("owned or borrowed")
                    .show_lines(true);
                table.add_column("Name");
                table.add_column_justify("Value", Justify::Right);
                table
            };
            let mut borrowed = build();
            let mut owned = build();
            for row in [
                vec!["漢字\n🙂", "123"],
                vec!["short"],
                vec!["extra", "4", "ignored"],
            ] {
                borrowed.add_row(&row);
                owned.extend_owned_rows(vec![row.into_iter().map(str::to_owned).collect()]);
            }
            assert_eq!(
                console.render_to_string(&borrowed),
                console.render_to_string(&owned)
            );
        }
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

    #[test]
    fn streamed_lines_match_styled_table_output() {
        let mut table = Table::new().box_set(SQUARE);
        table.add_column("Name");
        table.add_column("Age");
        table.add_row(&["Alice", "30"]);
        table.add_row(&["Bob", "7"]);
        let console = console();
        let mut streamed = String::new();
        table
            .try_for_each_line(&console, &console.options(), |line| {
                assert!(line.iter().all(|segment| !segment.text.contains('\n')));
                streamed.push_str(&console.segments_to_string(&line));
                streamed.push('\n');
                Ok::<_, std::convert::Infallible>(())
            })
            .unwrap();
        // `simple_square_table` above fixes these bytes independently of the
        // collection path, including distinct header-style segments.
        assert_eq!(streamed, console.render_export(&table));
        assert_eq!(streamed.lines().count(), 6);
    }

    #[test]
    fn streamed_lines_stop_at_the_first_writer_error() {
        let mut table = Table::new()
            .box_set(SQUARE)
            .title("People")
            .caption("End")
            .show_lines(true);
        table.add_column("Name");
        table.add_row(&["Alice\nBob"]);
        table.add_row(&["Carol"]);
        let console = console();
        let mut visits = 0;
        let result = table.try_for_each_line(&console, &console.options(), |_| {
            visits += 1;
            if visits == 5 {
                Err("writer failed")
            } else {
                Ok(())
            }
        });
        assert_eq!(result, Err("writer failed"));
        assert_eq!(visits, 5);
    }

    /// A column squeezed below its own padding still emitted a full left and
    /// right pad, so each such column spent two cells where its border spent
    /// one. The content row then overflowed the table and was cropped, losing
    /// its right-hand border while the border rows kept theirs.
    #[test]
    fn a_column_narrower_than_its_padding_stays_inside_the_border() {
        for ncols in [20usize, 29, 40] {
            let mut table = Table::new().box_set(SQUARE);
            for i in 0..ncols {
                table.add_column(format!("c{i}"));
            }
            let row: Vec<String> = (0..ncols).map(|i| i.to_string()).collect();
            table.add_row(&row.iter().map(String::as_str).collect::<Vec<_>>());
            let console = Console::builder().width(80).color_system(None).build();
            let out = console.render_to_string(&table);
            let rows: Vec<&str> = out.lines().filter(|l| !l.trim().is_empty()).collect();
            let widths: Vec<usize> = rows.iter().map(|r| r.chars().count()).collect();
            assert!(
                widths.iter().all(|w| *w == widths[0]),
                "{ncols} columns produced ragged rows: {widths:?}"
            );
            for (index, row) in rows.iter().enumerate() {
                let last = row.chars().last().expect("non-empty row");
                assert!(
                    !last.is_whitespace(),
                    "{ncols} columns: row {index} lost its right border: {row:?}"
                );
            }
        }
    }

    /// A cell spanning several lines occupies its WIDEST line. Measuring the raw
    /// string made it as wide as all its lines summed — `\n` measures zero, so
    /// nothing capped it — and a quoted CSV cell holding two sentences blew its
    /// column out to 31 cells where upstream gives 23.
    #[test]
    fn a_multi_line_cell_is_measured_by_its_widest_line() {
        let mut table = Table::new().box_set(SQUARE);
        table.add_column("name");
        table.add_column("bio");
        table.add_row(&["Alice", "line one\nline two is much longer"]);
        table.add_row(&["Bob", "short"]);
        let console = Console::builder().width(60).color_system(None).build();
        let out = console.render_to_string(&table);
        let top = out.lines().next().expect("a top border");
        let width = top.chars().count();
        // "line two is much longer" is 23 cells; summing both lines would be 31.
        assert!(
            width < 40,
            "the multi-line cell was measured as the sum of its lines: {width} wide"
        );
        assert!(
            out.contains("line two is much longer"),
            "content lost: {out:?}"
        );
    }
}
