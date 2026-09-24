//! Stable multi-column sorting of table rows.
//!
//! Rows are any `AsRef<[Value]>` (a `Vec<Value>`, an array, a slice). Each
//! [`SortKey`] names a column, a direction and a [`Compare`] mode; later keys
//! break ties of earlier ones and rows equal on every key keep their order
//! (the sort is stable). Empty cells ([`Value::is_empty`]) sort last in both
//! directions.
//!
//! [`Compare::Natural`] (the default) compares numbers numerically, strings
//! that are numbers as numbers, and other strings with digit runs by value, so
//! `file9` sorts before `file10`. Numbers sort before text.
//!
//! ```
//! use rich_ext::table::sort::{sort_rows, SortKey};
//! use rich_ext::table::Value;
//!
//! let mut rows = vec![
//!     vec![Value::from("b"), Value::from("file10")],
//!     vec![Value::from("a"), Value::from("file9")],
//!     vec![Value::from("b"), Value::from("file2")],
//!     vec![Value::from("a"), Value::Null],
//! ];
//! sort_rows(&mut rows, &[SortKey::desc(0), SortKey::asc(1)]);
//! let plain: Vec<String> = rows.iter().map(|r| r[1].plain()).collect();
//! assert_eq!(plain, ["file2", "file10", "file9", ""]);
//! ```

use std::cmp::Ordering;

use super::Value;

/// A sort direction.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Order {
    /// Smallest first.
    #[default]
    Ascending,
    /// Largest first.
    Descending,
}

/// How two non-empty cells compare.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Compare {
    /// Numbers numerically, numeric strings as numbers, other strings
    /// case-insensitively with digit runs by value (`a2 < a10`).
    #[default]
    Natural,
    /// The plain display strings, byte by byte.
    Lexical,
}

/// One sort criterion: a column index, a direction and a comparison.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SortKey {
    /// The column index.
    pub column: usize,
    /// The direction.
    pub order: Order,
    /// The comparison.
    pub compare: Compare,
}

impl SortKey {
    /// Ascending, natural comparison.
    pub fn asc(column: usize) -> Self {
        SortKey {
            column,
            order: Order::Ascending,
            compare: Compare::Natural,
        }
    }

    /// Descending, natural comparison.
    pub fn desc(column: usize) -> Self {
        SortKey {
            order: Order::Descending,
            ..SortKey::asc(column)
        }
    }

    /// Compare this key's cells as plain strings.
    pub fn lexical(mut self) -> Self {
        self.compare = Compare::Lexical;
        self
    }
}

/// The header indicator for a direction: `▲`/`▼`, or `^`/`v` when `ascii`.
pub fn indicator(order: Order, ascii: bool) -> &'static str {
    match (order, ascii) {
        (Order::Ascending, false) => "▲",
        (Order::Descending, false) => "▼",
        (Order::Ascending, true) => "^",
        (Order::Descending, true) => "v",
    }
}

/// A number parsed from a string that is only a number (`-1.5`, `42`,
/// `1e3`); words such as `inf` or `nan` are not numbers here.
fn numeric_str(s: &str) -> Option<f64> {
    let s = s.trim();
    let digits = s.trim_start_matches(['-', '+']).trim_start_matches('.');
    if !digits.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    s.parse::<f64>().ok()
}

/// The number a cell stands for under natural comparison.
fn numeric(value: &Value) -> Option<f64> {
    match value {
        Value::Int(n) => Some(*n as f64),
        Value::Float(f) => Some(*f),
        Value::Str(s) => numeric_str(s),
        Value::Text(t) => numeric_str(t.plain()),
        Value::Null => None,
    }
}

/// Compare two cells under `compare`, ascending. Empty cells sort after
/// non-empty ones; two empty cells are equal.
pub fn compare_values(a: &Value, b: &Value, compare: Compare) -> Ordering {
    match (a.is_empty(), b.is_empty()) {
        (true, true) => return Ordering::Equal,
        (true, false) => return Ordering::Greater,
        (false, true) => return Ordering::Less,
        (false, false) => {}
    }
    match compare {
        Compare::Lexical => a.plain().cmp(&b.plain()),
        Compare::Natural => {
            if let (Value::Int(x), Value::Int(y)) = (a, b) {
                return x.cmp(y);
            }
            match (numeric(a), numeric(b)) {
                (Some(x), Some(y)) => x.total_cmp(&y),
                (Some(_), None) => Ordering::Less,
                (None, Some(_)) => Ordering::Greater,
                (None, None) => natural_cmp(&a.plain(), &b.plain()),
            }
        }
    }
}

/// Natural string order: digit runs compare by value, other runs
/// case-insensitively; exact ties fall back to byte order.
///
/// ```
/// use std::cmp::Ordering;
/// use rich_ext::table::sort::natural_cmp;
///
/// assert_eq!(natural_cmp("v1.9", "v1.10"), Ordering::Less);
/// assert_eq!(natural_cmp("Beta", "alpha"), Ordering::Greater);
/// ```
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut x, mut y) = (a, b);
    loop {
        match (x.chars().next(), y.chars().next()) {
            (None, None) => return a.cmp(b),
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(c), Some(d)) if c.is_ascii_digit() && d.is_ascii_digit() => {
                let (run_x, rest_x) = split_digits(x);
                let (run_y, rest_y) = split_digits(y);
                let (value_x, value_y) =
                    (run_x.trim_start_matches('0'), run_y.trim_start_matches('0'));
                let order = value_x
                    .len()
                    .cmp(&value_y.len())
                    .then_with(|| value_x.cmp(value_y));
                if order != Ordering::Equal {
                    return order;
                }
                (x, y) = (rest_x, rest_y);
            }
            (Some(c), Some(d)) => {
                let order = c.to_lowercase().cmp(d.to_lowercase());
                if order != Ordering::Equal {
                    return order;
                }
                (x, y) = (&x[c.len_utf8()..], &y[d.len_utf8()..]);
            }
        }
    }
}

fn split_digits(s: &str) -> (&str, &str) {
    let end = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    s.split_at(end)
}

/// Compare two rows by `keys`, in priority order. Empty cells (and missing
/// ones) sort last whatever the key's direction.
pub fn compare_rows(a: &[Value], b: &[Value], keys: &[SortKey]) -> Ordering {
    const NULL: Value = Value::Null;
    for key in keys {
        let x = a.get(key.column).unwrap_or(&NULL);
        let y = b.get(key.column).unwrap_or(&NULL);
        let order = match (x.is_empty(), y.is_empty()) {
            (false, false) => {
                let order = compare_values(x, y, key.compare);
                match key.order {
                    Order::Ascending => order,
                    Order::Descending => order.reverse(),
                }
            }
            // Empties stay last in both directions.
            _ => compare_values(x, y, key.compare),
        };
        if order != Ordering::Equal {
            return order;
        }
    }
    Ordering::Equal
}

/// Sort rows in place by `keys`. Stable: rows equal on every key keep their
/// relative order.
pub fn sort_rows<R: AsRef<[Value]>>(rows: &mut [R], keys: &[SortKey]) {
    if keys.is_empty() {
        return;
    }
    rows.sort_by(|a, b| compare_rows(a.as_ref(), b.as_ref(), keys));
}

/// The indices of `rows` in sorted order, leaving the rows where they are.
/// Stable, as [`sort_rows`].
pub fn sorted_indices<R: AsRef<[Value]>>(rows: &[R], keys: &[SortKey]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..rows.len()).collect();
    if !keys.is_empty() {
        order.sort_by(|&a, &b| compare_rows(rows[a].as_ref(), rows[b].as_ref(), keys));
    }
    order
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_order_handles_digits_case_and_leading_zeros() {
        let mut words = vec!["a10", "A2", "a2", "a02", "b", "a1b", ""];
        words.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(words, ["", "a1b", "A2", "a02", "a2", "a10", "b"]);
    }

    #[test]
    fn numbers_sort_before_text_and_numeric_strings_as_numbers() {
        let mut values = [
            Value::from("x"),
            Value::from("10"),
            Value::Float(2.5),
            Value::Int(-3),
            Value::from("-4.5"),
            Value::Null,
        ];
        values.sort_by(|a, b| compare_values(a, b, Compare::Natural));
        let plain: Vec<String> = values.iter().map(Value::plain).collect();
        assert_eq!(plain, ["-4.5", "-3", "2.5", "10", "x", ""]);
        values.sort_by(|a, b| compare_values(a, b, Compare::Lexical));
        let plain: Vec<String> = values.iter().map(Value::plain).collect();
        assert_eq!(plain, ["-3", "-4.5", "10", "2.5", "x", ""]);
    }
}
