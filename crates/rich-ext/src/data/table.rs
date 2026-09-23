//! Records as a table: one row per record, one column per key.

use std::borrow::Cow;
use std::collections::HashMap;

use rich::{Console, ConsoleOptions, Justify, Renderable, Segment, Table, Text};

use super::{escape_controls, scalar_text, style, truncate_chars, Node, Value};

/// How nested values are cut in a cell when no `max_string` is set.
const INLINE_LIMIT: usize = 40;

/// Overrides for a [`TableView`].
#[derive(Clone, Debug, Default)]
pub struct TableOptions {
    /// Show only these columns, in this order.
    pub columns: Option<Vec<String>>,
    /// Header text per column key (default: the key).
    pub headers: HashMap<String, String>,
    /// Justification per column key (default: right for all-number
    /// columns, left otherwise).
    pub justify: HashMap<String, Justify>,
    /// A title above the table (plain text, not markup).
    pub title: Option<String>,
    /// Show at most this many rows, then `… N more`.
    pub max_rows: Option<usize>,
    /// Cut strings (and inline nested values) to this many characters.
    pub max_string: Option<usize>,
}

impl TableOptions {
    pub fn new() -> Self {
        Self::default()
    }
    /// Select and order the columns.
    pub fn columns<S: Into<String>>(mut self, columns: impl IntoIterator<Item = S>) -> Self {
        self.columns = Some(columns.into_iter().map(Into::into).collect());
        self
    }
    /// Rename a column's header.
    pub fn header(mut self, column: impl Into<String>, header: impl Into<String>) -> Self {
        self.headers.insert(column.into(), header.into());
        self
    }
    /// Justify a column.
    pub fn justify(mut self, column: impl Into<String>, justify: Justify) -> Self {
        self.justify.insert(column.into(), justify);
        self
    }
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }
    pub fn max_rows(mut self, max_rows: usize) -> Self {
        self.max_rows = Some(max_rows);
        self
    }
    pub fn max_string(mut self, max_string: usize) -> Self {
        self.max_string = Some(max_string);
        self
    }
}

/// A sequence of records as a table.
///
/// Columns are the union of the records' keys in first-seen order; a record
/// that is not a map fills a `value` column. Missing cells stay empty,
/// nested values show as compact JSON, numbers are right-justified and nulls
/// dim. A map renders as one record; a scalar as a one-cell table.
///
/// ```
/// use rich::Console;
/// use rich_ext::data::{from_serialize, TableView};
/// use serde_json::json;
///
/// let rows = from_serialize(&json!([{"name": "a", "port": 80}, {"name": "b"}])).unwrap();
/// let out = Console::builder().width(30).build().render_export(&TableView::new(&rows));
/// assert!(out.contains("│ a    │   80 │"), "{out}");
/// ```
#[derive(Clone, Debug)]
pub struct TableView<'a> {
    node: Cow<'a, Node>,
    options: TableOptions,
}

impl<'a> TableView<'a> {
    pub fn new(node: impl Into<Cow<'a, Node>>) -> Self {
        TableView {
            node: node.into(),
            options: TableOptions::default(),
        }
    }
    /// Replace all options.
    pub fn options(mut self, options: TableOptions) -> Self {
        self.options = options;
        self
    }
    /// See [`TableOptions::columns`].
    pub fn columns<S: Into<String>>(mut self, columns: impl IntoIterator<Item = S>) -> Self {
        self.options = self.options.columns(columns);
        self
    }
    /// See [`TableOptions::header`].
    pub fn header(mut self, column: impl Into<String>, header: impl Into<String>) -> Self {
        self.options = self.options.header(column, header);
        self
    }
    /// See [`TableOptions::justify`].
    pub fn justify(mut self, column: impl Into<String>, justify: Justify) -> Self {
        self.options = self.options.justify(column, justify);
        self
    }
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.options = self.options.title(title);
        self
    }
    pub fn max_rows(mut self, max_rows: usize) -> Self {
        self.options = self.options.max_rows(max_rows);
        self
    }
    pub fn max_string(mut self, max_string: usize) -> Self {
        self.options = self.options.max_string(max_string);
        self
    }

    fn records(&self) -> Vec<&Node> {
        match &self.node.value {
            Value::Seq(items) => items.iter().collect(),
            _ => vec![&*self.node],
        }
    }

    /// The core table.
    pub fn to_table(&self, console: &Console) -> Table {
        let records = self.records();
        let columns: Vec<String> = match &self.options.columns {
            Some(columns) => columns.clone(),
            None => {
                let mut columns: Vec<String> = Vec::new();
                let mut seen = std::collections::HashSet::new();
                for record in &records {
                    match &record.value {
                        Value::Map(entries) => {
                            for (key, _) in entries {
                                if seen.insert(key.clone()) {
                                    columns.push(key.clone());
                                }
                            }
                        }
                        _ => {
                            if seen.insert("value".to_string()) {
                                columns.push("value".to_string());
                            }
                        }
                    }
                }
                columns
            }
        };
        let shown = self
            .options
            .max_rows
            .unwrap_or(usize::MAX)
            .min(records.len());

        let mut table = Table::new();
        if let Some(title) = &self.options.title {
            table = table.title(rich::markup::escape(title));
        }
        if shown < records.len() {
            table = table.caption(format!("… {} more", records.len() - shown));
        }
        for column in &columns {
            let justify = self
                .options
                .justify
                .get(column)
                .copied()
                .unwrap_or_else(|| {
                    let mut values = records
                        .iter()
                        .filter_map(|r| cell(r, column))
                        .filter(|n| n.value != Value::Null)
                        .peekable();
                    let numeric = values.peek().is_some()
                        && values.all(|n| {
                            matches!(n.value, Value::Int(_) | Value::UInt(_) | Value::Float(_))
                        });
                    if numeric {
                        Justify::Right
                    } else {
                        Justify::Left
                    }
                });
            let header = self
                .options
                .headers
                .get(column)
                .map_or(column.as_str(), String::as_str);
            table.add_column_text(Text::new(escape_controls(header)), justify);
        }
        for record in records.iter().take(shown) {
            let row = columns
                .iter()
                .map(|column| match cell(record, column) {
                    Some(node) => cell_text(console, node, self.options.max_string),
                    None => Text::new(""),
                })
                .collect();
            table.add_row_text(row);
        }
        table
    }
}

fn cell<'n>(record: &'n Node, column: &str) -> Option<&'n Node> {
    match &record.value {
        Value::Map(entries) => entries
            .iter()
            .rev()
            .find(|(k, _)| k == column)
            .map(|(_, v)| v),
        _ if column == "value" => Some(record),
        _ => None,
    }
}

/// A value in a table cell: unquoted scalars, compact JSON for containers.
pub(crate) fn cell_text(console: &Console, node: &Node, max_string: Option<usize>) -> Text {
    match &node.value {
        Value::Null => Text::styled("null", style(console, "data.null")),
        Value::Seq(_) | Value::Map(_) => {
            let json = node.to_json().to_string();
            let limit = max_string.unwrap_or(INLINE_LIMIT);
            Text::styled(
                escape_controls(&truncate_chars(&json, limit)),
                style(console, "data.summary"),
            )
        }
        Value::String(s) => {
            let shown = max_string.map_or(Cow::Borrowed(s.as_str()), |n| truncate_chars(s, n));
            Text::new(escape_controls(&shown))
        }
        other => {
            let (text, key) = scalar_text(other, false);
            Text::styled(text, style(console, key))
        }
    }
}

impl Renderable for TableView<'_> {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        self.to_table(console).rich_render(console, options)
    }
}
