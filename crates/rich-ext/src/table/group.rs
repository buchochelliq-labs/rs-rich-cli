//! Grouping rows by a column, with per-group aggregates.
//!
//! [`GroupBy`] names the grouped column and the [`Aggregate`]s to compute for
//! each group. Groups appear in the order their first row appears, so sort by
//! the grouped column first to get them in key order. [`TableData`] renders
//! each group as a header row (styled `table.group`), its rows, and a summary
//! row (styled `table.aggregate`).
//!
//! ```
//! use rich_ext::table::{Aggregate, GroupBy, Value};
//!
//! let rows = vec![
//!     vec![Value::from("api"), Value::Int(3)],
//!     vec![Value::from("web"), Value::Int(5)],
//!     vec![Value::from("api"), Value::Int(4)],
//! ];
//! let by = GroupBy::new(0).aggregate(Aggregate::sum(1));
//! let groups = by.groups(&rows, &[0, 1, 2]);
//! assert_eq!(groups[0].key.plain(), "api");
//! assert_eq!(groups[0].rows, [0, 2]);
//! assert_eq!(groups[0].aggregates[0], Value::Int(7));
//! ```
//!
//! [`TableData`]: super::TableData

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use super::sort::{compare_values, Compare};
use super::Value;

/// A custom aggregate: the group's cells of one column in, a value out.
pub type AggregateFn = Arc<dyn Fn(&[&Value]) -> Value + Send + Sync>;

#[derive(Clone)]
enum Kind {
    Count,
    Sum,
    Min,
    Max,
    Mean,
    Custom(AggregateFn),
}

/// A summary computed over one column of a group of rows.
#[derive(Clone)]
pub struct Aggregate {
    column: usize,
    kind: Kind,
}

impl fmt::Debug for Aggregate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = match self.kind {
            Kind::Count => "count",
            Kind::Sum => "sum",
            Kind::Min => "min",
            Kind::Max => "max",
            Kind::Mean => "mean",
            Kind::Custom(_) => "custom",
        };
        f.debug_struct("Aggregate")
            .field("column", &self.column)
            .field("kind", &kind)
            .finish()
    }
}

impl Aggregate {
    fn new(column: usize, kind: Kind) -> Self {
        Aggregate { column, kind }
    }

    /// The number of non-empty cells. Shown as a plain integer, not through
    /// the column's formatter.
    pub fn count(column: usize) -> Self {
        Self::new(column, Kind::Count)
    }

    /// The sum of the numeric cells: an `Int` when every one is an integer
    /// (and the sum fits), else a `Float`; `Null` when there are none.
    pub fn sum(column: usize) -> Self {
        Self::new(column, Kind::Sum)
    }

    /// The smallest non-empty cell, in natural order.
    pub fn min(column: usize) -> Self {
        Self::new(column, Kind::Min)
    }

    /// The largest non-empty cell, in natural order.
    pub fn max(column: usize) -> Self {
        Self::new(column, Kind::Max)
    }

    /// The mean of the numeric cells as a `Float`; `Null` when there are none.
    pub fn mean(column: usize) -> Self {
        Self::new(column, Kind::Mean)
    }

    /// Any summary: `f` receives the group's cells of `column`, in row order.
    pub fn custom(column: usize, f: impl Fn(&[&Value]) -> Value + Send + Sync + 'static) -> Self {
        Self::new(column, Kind::Custom(Arc::new(f)))
    }

    /// The column this aggregate summarises.
    pub fn column(&self) -> usize {
        self.column
    }

    /// Whether this is a [`count`](Aggregate::count), which is displayed
    /// without the column's formatter.
    pub fn is_count(&self) -> bool {
        matches!(self.kind, Kind::Count)
    }

    /// Compute the aggregate over one column's cells.
    pub fn compute(&self, values: &[&Value]) -> Value {
        let non_empty = || values.iter().copied().filter(|v| !v.is_empty());
        match &self.kind {
            Kind::Count => Value::from(non_empty().count()),
            Kind::Sum => {
                let numbers: Vec<&Value> = values
                    .iter()
                    .copied()
                    .filter(|v| v.as_f64().is_some())
                    .collect();
                if numbers.is_empty() {
                    return Value::Null;
                }
                let ints: Option<i64> = numbers.iter().try_fold(0i64, |sum, v| match v {
                    Value::Int(n) => sum.checked_add(*n),
                    _ => None,
                });
                ints.map_or_else(
                    || Value::Float(numbers.iter().filter_map(|v| v.as_f64()).sum()),
                    Value::Int,
                )
            }
            Kind::Min => non_empty()
                .min_by(|a, b| compare_values(a, b, Compare::Natural))
                .cloned()
                .unwrap_or_default(),
            Kind::Max => non_empty()
                // `max_by` keeps the last of equal maxima; the first is wanted.
                .reduce(|best, v| {
                    if compare_values(v, best, Compare::Natural).is_gt() {
                        v
                    } else {
                        best
                    }
                })
                .cloned()
                .unwrap_or_default(),
            Kind::Mean => {
                let numbers: Vec<f64> = values.iter().filter_map(|v| v.as_f64()).collect();
                if numbers.is_empty() {
                    Value::Null
                } else {
                    Value::Float(numbers.iter().sum::<f64>() / numbers.len() as f64)
                }
            }
            Kind::Custom(f) => f(values),
        }
    }

    /// Compute the aggregate over `rows` (indices into `data`).
    pub fn over<R: AsRef<[Value]>>(&self, data: &[R], rows: &[usize]) -> Value {
        const NULL: Value = Value::Null;
        let cells: Vec<&Value> = rows
            .iter()
            .map(|&i| data[i].as_ref().get(self.column).unwrap_or(&NULL))
            .collect();
        self.compute(&cells)
    }
}

/// Group rows by one column, with aggregates per group.
#[derive(Clone, Debug)]
pub struct GroupBy {
    column: usize,
    aggregates: Vec<Aggregate>,
    label: String,
}

impl GroupBy {
    /// Group by `column`, with no aggregates and the summary label `subtotal`.
    pub fn new(column: usize) -> Self {
        GroupBy {
            column,
            aggregates: Vec::new(),
            label: "subtotal".to_string(),
        }
    }

    /// Add an aggregate to each group's summary row.
    pub fn aggregate(mut self, aggregate: Aggregate) -> Self {
        self.aggregates.push(aggregate);
        self
    }

    /// The text in the summary row's first cell, followed by that column's
    /// own aggregate if it has one (`subtotal: 2`). Empty for none.
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// The grouped column.
    pub fn column(&self) -> usize {
        self.column
    }

    /// The aggregates, in the order they were added.
    pub fn aggregates(&self) -> &[Aggregate] {
        &self.aggregates
    }

    /// The summary row's label.
    pub fn summary_label(&self) -> &str {
        &self.label
    }

    /// Group `rows`, visiting them in `order` (indices, e.g. from
    /// [`sorted_indices`](super::sort::sorted_indices)). Groups are keyed by
    /// the cell's plain display string (so `Int(1)` and `"1"` share a group)
    /// and appear in order of their first row; empty cells form one group.
    pub fn groups<R: AsRef<[Value]>>(&self, rows: &[R], order: &[usize]) -> Vec<Group> {
        let mut groups: Vec<Group> = Vec::new();
        let mut by_key: HashMap<String, usize> = HashMap::new();
        for &row in order {
            let key = rows[row]
                .as_ref()
                .get(self.column)
                .cloned()
                .unwrap_or_default();
            let slot = *by_key.entry(key.plain()).or_insert_with(|| {
                groups.push(Group {
                    key,
                    rows: Vec::new(),
                    aggregates: Vec::new(),
                });
                groups.len() - 1
            });
            groups[slot].rows.push(row);
        }
        for group in &mut groups {
            group.aggregates = self
                .aggregates
                .iter()
                .map(|aggregate| aggregate.over(rows, &group.rows))
                .collect();
        }
        groups
    }
}

/// One group of rows.
#[derive(Clone, Debug)]
pub struct Group {
    /// The grouped column's value (from the group's first row).
    pub key: Value,
    /// The group's rows, as indices in visiting order.
    pub rows: Vec<usize>,
    /// One value per [`GroupBy::aggregate`], in the same order.
    pub aggregates: Vec<Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aggregates_over_mixed_cells() {
        let cells = [
            Value::Int(3),
            Value::Null,
            Value::Float(1.5),
            Value::from("x"),
            Value::Int(-2),
        ];
        let refs: Vec<&Value> = cells.iter().collect();
        assert_eq!(Aggregate::count(0).compute(&refs), Value::Int(4));
        assert_eq!(Aggregate::sum(0).compute(&refs), Value::Float(2.5));
        assert_eq!(Aggregate::min(0).compute(&refs), Value::Int(-2));
        assert_eq!(Aggregate::max(0).compute(&refs), Value::from("x"));
        assert_eq!(Aggregate::mean(0).compute(&refs), Value::Float(2.5 / 3.0));
        assert_eq!(Aggregate::sum(0).compute(&[]), Value::Null);
        let ints = [Value::Int(i64::MAX), Value::Int(1)];
        let refs: Vec<&Value> = ints.iter().collect();
        assert_eq!(
            Aggregate::sum(0).compute(&refs),
            Value::Float(i64::MAX as f64 + 1.0)
        );
    }
}
