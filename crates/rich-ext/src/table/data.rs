//! Plain rows with a sort, a grouping and totals, rendered as a core table.

use rich::{Console, ConsoleOptions, Renderable, Segment, Table, Text};

use super::group::{Aggregate, GroupBy};
use super::sort::{sorted_indices, SortKey};
use super::{frame_builders, headers, style, Column, Frame, Value};

/// Rows of [`Value`]s under [`Column`]s, optionally sorted, grouped and
/// totalled, rendered as one core [`Table`].
///
/// Sorting is stable and puts empty cells last ([`sort`](super::sort)); the
/// sorted columns get a `▲`/`▼` header indicator (`^`/`v` on an ASCII-only
/// console). With a [`GroupBy`], each group renders as a header row with the
/// group's key in the first column (prefixed by the grouped column's header
/// when that is not the first column), the group's rows, and — when the
/// grouping has aggregates — a summary row. [`totals`](TableData::totals) adds
/// a final summary over every row.
///
/// ```
/// use rich::{Console, Justify};
/// use rich_ext::table::{Aggregate, Column, GroupBy, SortKey, TableData, Value};
///
/// let mut data = TableData::new([
///     Column::new("team"),
///     Column::new("service"),
///     Column::new("pods").justify(Justify::Right),
/// ]);
/// data.push(["core".into(), "api".into(), Value::Int(3)]);
/// data.push(["edge".into(), "web".into(), Value::Int(5)]);
/// data.push(["core".into(), "db".into(), Value::Int(1)]);
/// let data = data
///     .sort_by([SortKey::asc(0), SortKey::asc(1)])
///     .group_by(GroupBy::new(0).aggregate(Aggregate::sum(2)));
///
/// let out = Console::builder().width(40).build().render_export(&data);
/// assert_eq!(
///     out,
///     "\
/// ┏━━━━━━━━━━┳━━━━━━━━━━━━┳━━━━━━┓
/// ┃ team ▲1  ┃ service ▲2 ┃ pods ┃
/// ┡━━━━━━━━━━╇━━━━━━━━━━━━╇━━━━━━┩
/// │ core     │            │      │
/// │ core     │ api        │    3 │
/// │ core     │ db         │    1 │
/// │ subtotal │            │    4 │
/// │ edge     │            │      │
/// │ edge     │ web        │    5 │
/// │ subtotal │            │    5 │
/// └──────────┴────────────┴──────┘
/// "
/// );
/// ```
#[derive(Clone, Debug)]
pub struct TableData {
    columns: Vec<Column>,
    rows: Vec<Vec<Value>>,
    sort: Vec<SortKey>,
    group: Option<GroupBy>,
    totals: Vec<Aggregate>,
    totals_label: String,
    pub(super) frame: Frame,
}

frame_builders!([] TableData);

impl TableData {
    /// No rows under `columns`.
    pub fn new(columns: impl IntoIterator<Item = Column>) -> Self {
        TableData {
            columns: columns.into_iter().collect(),
            rows: Vec::new(),
            sort: Vec::new(),
            group: None,
            totals: Vec::new(),
            totals_label: "total".to_string(),
            frame: Frame::default(),
        }
    }

    /// Append a row. Missing cells are `Null`; extra cells are dropped.
    pub fn push(&mut self, row: impl IntoIterator<Item = Value>) -> &mut Self {
        let row = normalize(row, self.columns.len());
        self.rows.push(row);
        self
    }

    /// Append several rows.
    pub fn extend<R: IntoIterator<Item = Value>>(
        &mut self,
        rows: impl IntoIterator<Item = R>,
    ) -> &mut Self {
        for row in rows {
            self.push(row);
        }
        self
    }

    /// Sort by `keys` when rendering (the rows keep their stored order).
    pub fn sort_by(mut self, keys: impl IntoIterator<Item = SortKey>) -> Self {
        self.set_sort(keys);
        self
    }

    /// Replace the sort keys; an empty list keeps insertion order.
    pub fn set_sort(&mut self, keys: impl IntoIterator<Item = SortKey>) {
        self.sort = keys.into_iter().collect();
    }

    /// Group the (sorted) rows.
    pub fn group_by(mut self, group: GroupBy) -> Self {
        self.group = Some(group);
        self
    }

    /// Add a final summary row over every row. `label` fills the first cell,
    /// followed by the first column's own aggregate if it has one
    /// (`total: 6`); an empty label leaves the aggregate alone.
    pub fn totals(
        mut self,
        label: impl Into<String>,
        aggregates: impl IntoIterator<Item = Aggregate>,
    ) -> Self {
        self.totals_label = label.into();
        self.totals = aggregates.into_iter().collect();
        self
    }

    /// The columns.
    pub fn columns(&self) -> &[Column] {
        &self.columns
    }

    /// The rows, in the order they were added.
    pub fn rows(&self) -> &[Vec<Value>] {
        &self.rows
    }

    /// The sort keys.
    pub fn sort_keys(&self) -> &[SortKey] {
        &self.sort
    }

    /// Row indices in display order (stable sort by the keys).
    pub fn order(&self) -> Vec<usize> {
        sorted_indices(&self.rows, &self.sort)
    }

    /// The core table: headers with sort indicators, then the sorted rows,
    /// grouped and totalled as configured.
    pub fn to_table(&self, console: &Console) -> Table {
        let headers = headers(console, &self.columns, &self.sort);
        let mut table = self.frame.table(&self.columns, &headers, true, true);
        let order = self.order();
        match &self.group {
            None => {
                for &row in &order {
                    table.add_row_text(self.cells(&self.rows[row]));
                }
            }
            Some(group) => {
                let group_style = style(console, "table.group");
                for g in group.groups(&self.rows, &order) {
                    let mut label = if g.key.is_empty() {
                        Text::new("(empty)")
                    } else {
                        self.columns
                            .get(group.column())
                            .map_or_else(|| g.key.to_text(), |c| c.cell(&g.key))
                    };
                    if group.column() != 0 {
                        if let Some(column) = self.columns.get(group.column()) {
                            label = Text::new(format!("{}: ", column.header())).append_text(&label);
                        }
                    }
                    let len = label.plain().len();
                    label.stylize(group_style.clone(), 0, len);
                    let mut header = vec![Text::new(""); self.columns.len()];
                    if let Some(first) = header.first_mut() {
                        *first = label;
                    }
                    table.add_row_text(header);
                    for &row in &g.rows {
                        table.add_row_text(self.cells(&self.rows[row]));
                    }
                    if !group.aggregates().is_empty() {
                        table.add_row_text(self.summary(
                            console,
                            group.summary_label(),
                            group.aggregates(),
                            &g.aggregates,
                        ));
                    }
                }
            }
        }
        if !self.totals.is_empty() {
            let values: Vec<Value> = self
                .totals
                .iter()
                .map(|aggregate| aggregate.over(&self.rows, &order))
                .collect();
            table.add_row_text(self.summary(console, &self.totals_label, &self.totals, &values));
        }
        table
    }

    fn cells(&self, row: &[Value]) -> Vec<Text> {
        self.columns
            .iter()
            .zip(row)
            .map(|(column, value)| column.cell(value))
            .collect()
    }

    /// A summary row: each aggregate in its column (several in one column
    /// are joined with `, `), and the label in the first cell — before that
    /// column's own aggregate, if it has one (`total: 6`).
    fn summary(
        &self,
        console: &Console,
        label: &str,
        aggregates: &[Aggregate],
        values: &[Value],
    ) -> Vec<Text> {
        let mut cells: Vec<Option<Text>> = vec![None; self.columns.len()];
        for (aggregate, value) in aggregates.iter().zip(values) {
            let Some(column) = self.columns.get(aggregate.column()) else {
                continue;
            };
            let text = if aggregate.is_count() {
                value.to_text()
            } else {
                column.cell(value)
            };
            let cell = &mut cells[aggregate.column()];
            *cell = Some(match cell.take() {
                Some(previous) => previous.append_text(&Text::new(", ")).append_text(&text),
                None => text,
            });
        }
        if let Some(first) = cells.first_mut() {
            *first = match first.take() {
                None => Some(Text::new(label)),
                Some(value) if label.is_empty() => Some(value),
                Some(value) => Some(Text::new(format!("{label}: ")).append_text(&value)),
            };
        }
        let aggregate_style = style(console, "table.aggregate");
        cells
            .into_iter()
            .map(|cell| {
                let mut text = cell.unwrap_or_else(|| Text::new(""));
                let len = text.plain().len();
                text.stylize(aggregate_style.clone(), 0, len);
                text
            })
            .collect()
    }
}

/// A row padded with `Null` (or cut) to `columns` cells.
pub(crate) fn normalize(row: impl IntoIterator<Item = Value>, columns: usize) -> Vec<Value> {
    let mut row: Vec<Value> = row.into_iter().take(columns).collect();
    row.resize(columns, Value::Null);
    row
}

impl Renderable for TableData {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        self.to_table(console).rich_render(console, options)
    }

    fn measure(&self, console: &Console, options: &ConsoleOptions) -> rich::measure::Measurement {
        self.to_table(console).measure(console, options)
    }
}

impl crate::a11y::AccessibleText for TableData {
    fn accessible_text(&self, width: usize) -> String {
        let console = Console::builder().width(width.max(1)).build();
        self.to_table(&console).accessible_text(width)
    }
}
