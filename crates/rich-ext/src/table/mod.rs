//! Table data operations layered on the core [`rich::Table`].
//!
//! The core `Table` is a faithful port of upstream and renders whatever rows
//! it is given. This module adds the parts upstream leaves to the caller,
//! without forking the table renderer — everything here *builds* core tables:
//!
//! * [`Value`] and [`Column`]: typed cells and column definitions (header,
//!   core [`ColumnOptions`], an optional formatter), shared by the views below.
//! * [`sort`]: stable multi-column sorting with natural/numeric comparison,
//!   empty cells last, and `▲`/`▼` header indicators.
//! * [`group`]: grouping by a column with group header rows and per-group
//!   aggregates (count, sum, min, max, mean, custom) as summary rows.
//! * [`TableData`]: plain rows plus a sort, a grouping and totals, rendered as
//!   one core `Table`.
//! * [`stream`]: [`StreamingTable`], keyed rows for append/update workloads
//!   under a live display, re-rendering only the rows that changed.
//!
//! ```
//! use rich::{Console, Justify};
//! use rich_ext::table::{Column, SortKey, TableData, Value};
//!
//! let mut data = TableData::new([
//!     Column::new("service"),
//!     Column::new("p99 ms").justify(Justify::Right),
//! ]);
//! data.push(["web".into(), Value::Int(120)]);
//! data.push(["api".into(), Value::Int(35)]);
//! data.push(["db".into(), Value::Null]);
//! let data = data.sort_by([SortKey::asc(1)]);
//!
//! let console = Console::builder().width(40).build();
//! let out = console.render_export(&data);
//! let lines: Vec<&str> = out.lines().collect();
//! assert_eq!(lines[1], "┃ service ┃ p99 ms ▲ ┃");
//! assert_eq!(lines[3], "│ api     │       35 │");
//! assert_eq!(lines[5], "│ db      │          │"); // empty cells sort last
//! ```
//!
//! Theme keys are listed in [`STYLES`]; [`extended_theme`] includes them and
//! the renderers fall back to them when a theme lacks a key. Every renderer
//! reads as plain text without colour.
//!
//! [`extended_theme`]: crate::theme::extended_theme

pub mod data;
pub mod group;
pub mod sort;
pub mod stream;

use std::fmt;
use std::sync::Arc;

use rich::r#box::Box as BoxSet;
use rich::{ColumnOptions, Console, Justify, Style, Table, Text};

pub use data::TableData;
pub use group::{Aggregate, Group, GroupBy};
pub use sort::{Compare, Order, SortKey};
pub use stream::{RenderStats, StreamingTable, Window};

/// Theme keys used by this module, with their fallback styles.
///
/// The names avoid upstream's own `table.header`, `table.footer`,
/// `table.cell`, `table.title` and `table.caption`.
pub const STYLES: &[(&str, &str)] = &[
    ("table.group", "bold"),
    ("table.aggregate", "italic"),
    ("table.sort_indicator", "cyan"),
    ("table.more", "dim"),
];

/// A theme style for `key`, falling back to [`STYLES`].
pub(crate) fn style(console: &Console, key: &str) -> Style {
    let fallback = STYLES
        .iter()
        .find(|(name, _)| *name == key)
        .map_or("", |(_, spec)| *spec);
    crate::event::theme_style(console, key, fallback)
}

/// A typed table cell.
///
/// The type drives sorting ([`sort`]) and aggregation ([`group`]); the
/// column's formatter (or [`Value::to_text`]) drives display.
#[derive(Clone, Debug, Default)]
pub enum Value {
    /// No value. Renders empty and sorts last.
    #[default]
    Null,
    /// An integer.
    Int(i64),
    /// A floating-point number.
    Float(f64),
    /// Literal text (never parsed as markup).
    Str(String),
    /// Styled text; sorts and groups by its plain string.
    Text(Text),
}

impl Value {
    /// Whether the value is [`Null`](Value::Null) or has an empty string.
    pub fn is_empty(&self) -> bool {
        match self {
            Value::Null => true,
            Value::Str(s) => s.is_empty(),
            Value::Text(t) => t.plain().is_empty(),
            Value::Int(_) | Value::Float(_) => false,
        }
    }

    /// The value as a number, for `Int` and `Float` only.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Int(n) => Some(*n as f64),
            Value::Float(f) => Some(*f),
            _ => None,
        }
    }

    /// The plain display string: empty for `Null`, `Display` for numbers.
    pub fn plain(&self) -> String {
        match self {
            Value::Null => String::new(),
            Value::Int(n) => n.to_string(),
            Value::Float(f) => f.to_string(),
            Value::Str(s) => s.clone(),
            Value::Text(t) => t.plain().to_string(),
        }
    }

    /// The default display: [`plain`](Value::plain) as literal text, or the
    /// `Text` itself.
    pub fn to_text(&self) -> Text {
        match self {
            Value::Text(t) => t.clone(),
            other => Text::new(other.plain()),
        }
    }
}

/// Values compare by variant and content (floats bit for bit). A
/// [`Value::Text`] never equals anything, itself included, because its base
/// style cannot be compared: [`StreamingTable`] treats writing one as a change.
/// Prefer [`Value::Str`] for data.
impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Null, Value::Null) => true,
            (Value::Int(a), Value::Int(b)) => a == b,
            (Value::Float(a), Value::Float(b)) => a.to_bits() == b.to_bits(),
            (Value::Str(a), Value::Str(b)) => a == b,
            _ => false,
        }
    }
}

impl From<&str> for Value {
    fn from(s: &str) -> Self {
        Value::Str(s.to_string())
    }
}

impl From<String> for Value {
    fn from(s: String) -> Self {
        Value::Str(s)
    }
}

impl From<Text> for Value {
    fn from(t: Text) -> Self {
        Value::Text(t)
    }
}

impl From<f64> for Value {
    fn from(f: f64) -> Self {
        Value::Float(f)
    }
}

macro_rules! int_value {
    ($($t:ty),*) => {$(
        impl From<$t> for Value {
            fn from(n: $t) -> Self {
                i64::try_from(n).map_or(Value::Float(n as f64), Value::Int)
            }
        }
    )*};
}
int_value!(i8, i16, i32, i64, u8, u16, u32, u64, usize, isize);

impl<T: Into<Value>> From<Option<T>> for Value {
    fn from(v: Option<T>) -> Self {
        v.map_or(Value::Null, Into::into)
    }
}

/// Formats a [`Value`] for display in a column.
pub type Formatter = Arc<dyn Fn(&Value) -> Text + Send + Sync>;

/// A column definition: a header, core [`ColumnOptions`] and an optional
/// formatter.
///
/// ```
/// use rich::{Justify, Text};
/// use rich_ext::table::{Column, Value};
///
/// let size = Column::new("size")
///     .justify(Justify::Right)
///     .format(|v| match v.as_f64() {
///         Some(n) => Text::new(rich_ext::format::bytes(n as u64)),
///         None => Text::new(""),
///     });
/// assert_eq!(size.cell(&Value::Int(2048)).plain(), "2.0 kB");
/// ```
#[derive(Clone)]
pub struct Column {
    header: String,
    options: ColumnOptions,
    formatter: Option<Formatter>,
}

impl fmt::Debug for Column {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Column")
            .field("header", &self.header)
            .field("options", &self.options)
            .field("formatter", &self.formatter.is_some())
            .finish()
    }
}

impl Column {
    /// A left-justified column. The header is literal text, not markup.
    pub fn new(header: impl Into<String>) -> Self {
        Column {
            header: header.into(),
            options: ColumnOptions::default(),
            formatter: None,
        }
    }

    /// Justify the column's cells.
    pub fn justify(mut self, justify: Justify) -> Self {
        self.options.justify = justify;
        self
    }

    /// Replace every core column option (width, ratio, no_wrap, style, …).
    pub fn options(mut self, options: ColumnOptions) -> Self {
        self.options = options;
        self
    }

    /// Display values through `formatter` instead of [`Value::to_text`].
    /// Aggregates other than counts are formatted the same way.
    pub fn format(mut self, formatter: impl Fn(&Value) -> Text + Send + Sync + 'static) -> Self {
        self.formatter = Some(Arc::new(formatter));
        self
    }

    /// The header text.
    pub fn header(&self) -> &str {
        &self.header
    }

    /// The core column options.
    pub fn column_options(&self) -> &ColumnOptions {
        &self.options
    }

    /// A value as this column displays it.
    pub fn cell(&self, value: &Value) -> Text {
        match &self.formatter {
            Some(format) => format(value),
            None => value.to_text(),
        }
    }
}

/// The header text of every column, with a sort indicator on sorted columns.
/// With more than one key each indicator carries its priority (`▲1`, `▼2`).
pub(crate) fn headers(console: &Console, columns: &[Column], keys: &[SortKey]) -> Vec<Text> {
    let ascii = console.ascii_only();
    columns
        .iter()
        .enumerate()
        .map(|(index, column)| {
            let mut text = Text::new(column.header.clone());
            if let Some(priority) = keys.iter().position(|k| k.column == index) {
                let mut mark = sort::indicator(keys[priority].order, ascii).to_string();
                if keys.len() > 1 {
                    mark.push_str(&(priority + 1).to_string());
                }
                text.append(" ", None);
                text.append(&mark, Some(style(console, "table.sort_indicator").into()));
            }
            text
        })
        .collect()
}

/// Presentation shared by the views: what a core `Table` is built with.
#[derive(Clone, Debug)]
pub(crate) struct Frame {
    pub title: Option<String>,
    pub caption: Option<String>,
    /// `None` draws no box (`Table::without_box`).
    pub box_set: Option<BoxSet>,
    pub show_edge: bool,
    pub expand: bool,
    pub border_style: Style,
}

impl Default for Frame {
    fn default() -> Self {
        Frame {
            title: None,
            caption: None,
            box_set: Some(rich::r#box::HEAVY_HEAD),
            show_edge: true,
            expand: false,
            border_style: Style::new(),
        }
    }
}

impl Frame {
    /// An empty core table with this frame and the given columns.
    pub fn table(
        &self,
        columns: &[Column],
        headers: &[Text],
        show_header: bool,
        annotations: bool,
    ) -> Table {
        let mut table = Table::new()
            .show_header(show_header)
            .show_edge(self.show_edge)
            .expand(self.expand)
            .border_style(self.border_style.clone());
        table = match self.box_set {
            Some(box_set) => table.box_set(box_set),
            None => table.without_box(),
        };
        if annotations {
            if let Some(title) = &self.title {
                table = table.title(title.clone());
            }
            if let Some(caption) = &self.caption {
                table = table.caption(caption.clone());
            }
        }
        for (column, header) in columns.iter().zip(headers) {
            table.add_column_with(header.clone(), column.options.clone());
        }
        table
    }

    /// Lines drawn above and below the body by the box edges.
    pub fn edge_lines(&self) -> usize {
        usize::from(self.box_set.is_some() && self.show_edge)
    }
}

/// Builder methods for the shared [`Frame`], generated for each view.
macro_rules! frame_builders {
    ([$($g:ident)?] $ty:ty) => {
        impl$(<$g>)? $ty {
            /// A centered title above the table (console markup, as the core
            /// `Table::title`).
            pub fn title(mut self, title: impl Into<String>) -> Self {
                self.frame.title = Some(title.into());
                self
            }
            /// A centered caption below the table (console markup).
            pub fn caption(mut self, caption: impl Into<String>) -> Self {
                self.frame.caption = Some(caption.into());
                self
            }
            /// The box-drawing set (default `HEAVY_HEAD`, as the core table).
            pub fn box_set(mut self, box_set: rich::r#box::Box) -> Self {
                self.frame.box_set = Some(box_set);
                self
            }
            /// Draw no borders and no column dividers.
            pub fn without_box(mut self) -> Self {
                self.frame.box_set = None;
                self
            }
            /// Draw the outer edges (default on).
            pub fn show_edge(mut self, show: bool) -> Self {
                self.frame.show_edge = show;
                self
            }
            /// Expand to the full available width.
            pub fn expand(mut self, expand: bool) -> Self {
                self.frame.expand = expand;
                self
            }
            /// Style the box border.
            pub fn border_style(mut self, style: rich::Style) -> Self {
                self.frame.border_style = style;
                self
            }
        }
    };
}
pub(crate) use frame_builders;
