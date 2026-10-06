//! Conditional styles (#215): rules that style a cell, a row or a column by
//! value.
//!
//! A [`StyleRule`] names a column, a condition on that column's value and a
//! style. The condition is either a [`Comparison`] against a value
//! (`status == "failed"`, `latency > 250`) or a Rust predicate; there is no
//! expression language. The rule's [`Target`] decides what the style lands
//! on: the matching cell (the default), the whole row, or the whole column
//! when any of its cells matches.
//!
//! [`TableData::style_rules`](super::TableData::style_rules) applies a set of
//! rules while rendering; any other renderer resolves them once with
//! [`StyleRules::resolve`] and asks the result for each row's and cell's
//! style. With the `toml` feature, [`StyleRules::from_toml`] reads the same
//! rules from TOML rule tables:
//!
//! ```toml
//! [[rules]]
//! column = "status"     # a header, or a 0-based index
//! op = "eq"             # eq ne lt le gt ge contains starts_with ends_with
//! value = "failed"      #   empty not_empty (or == != < <= > >=)
//! style = "bold red"
//! target = "row"        # cell (default), row or column
//! ```
//!
//! ```
//! use rich::{Console, Style};
//! use rich_ext::table::{Column, Comparison, StyleRule, StyleRules, TableData, Target, Value};
//!
//! let rules = StyleRules::new()
//!     .rule(StyleRule::new("p99 ms", Comparison::Gt, 100, Style::parse("red").unwrap()))
//!     .rule(StyleRule::when("service", |v| v.plain() == "db", Style::parse("dim").unwrap())
//!         .target(Target::Row));
//! let mut data = TableData::new([Column::new("service"), Column::new("p99 ms")]);
//! data.push(["web".into(), Value::Int(120)]);
//! data.push(["db".into(), Value::Int(35)]);
//!
//! let resolved = rules.resolve(&["service", "p99 ms"], data.rows());
//! assert!(resolved.cell_style(&data.rows()[0], 1).is_some());
//! assert!(resolved.cell_style(&data.rows()[1], 1).is_none());
//! assert!(resolved.row_style(&data.rows()[1]).is_some());
//!
//! // Rules restyle; they never change the text.
//! let data = data.style_rules(rules);
//! let out = Console::builder().width(40).build().render_export(&data);
//! assert!(out.contains("│ web     │ 120    │"));
//! ```

use std::cmp::Ordering;
use std::fmt;
use std::sync::Arc;

use rich::Style;

use super::Value;

/// What a matching rule styles.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Target {
    /// The cell that matched.
    #[default]
    Cell,
    /// Every cell of the row the matching cell is in.
    Row,
    /// Every cell of the column, when any of its cells matches.
    Column,
}

impl Target {
    /// `cell`, `row` or `column`.
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "cell" => Some(Target::Cell),
            "row" => Some(Target::Row),
            "column" => Some(Target::Column),
            _ => None,
        }
    }
}

/// The column a rule reads: a header, or a 0-based index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ColumnRef {
    /// The first column with this header.
    Name(String),
    /// The column at this position.
    Index(usize),
}

impl From<&str> for ColumnRef {
    fn from(name: &str) -> Self {
        ColumnRef::Name(name.to_string())
    }
}

impl From<String> for ColumnRef {
    fn from(name: String) -> Self {
        ColumnRef::Name(name)
    }
}

impl From<usize> for ColumnRef {
    fn from(index: usize) -> Self {
        ColumnRef::Index(index)
    }
}

impl fmt::Display for ColumnRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ColumnRef::Name(name) => f.write_str(name),
            ColumnRef::Index(index) => write!(f, "#{index}"),
        }
    }
}

/// How a cell is compared with a rule's value.
///
/// A numeric rule value ([`Value::Int`] or [`Value::Float`]) compares
/// numerically: a cell matches when it is a number, or text that parses as
/// one, and a cell that is neither never matches an ordering. A text value
/// compares as text, byte for byte. A [`Value::Null`] value matches empty
/// cells for [`Eq`](Comparison::Eq) and the rest for
/// [`Ne`](Comparison::Ne). [`Ne`](Comparison::Ne) is always the opposite of
/// [`Eq`](Comparison::Eq).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Comparison {
    /// Equal (`eq`, `==`).
    Eq,
    /// Not equal (`ne`, `!=`).
    Ne,
    /// Less than (`lt`, `<`).
    Lt,
    /// Less than or equal (`le`, `<=`).
    Le,
    /// Greater than (`gt`, `>`).
    Gt,
    /// Greater than or equal (`ge`, `>=`).
    Ge,
    /// The cell's text contains the value's (`contains`).
    Contains,
    /// The cell's text starts with the value's (`starts_with`).
    StartsWith,
    /// The cell's text ends with the value's (`ends_with`).
    EndsWith,
    /// The cell is null or empty text (`empty`); the value is ignored.
    Empty,
    /// The cell is not empty (`not_empty`); the value is ignored.
    NotEmpty,
}

impl Comparison {
    /// The comparison named `name`: a word (`eq`, `starts_with`) or a symbol
    /// (`==`, `<=`).
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "eq" | "==" => Comparison::Eq,
            "ne" | "!=" => Comparison::Ne,
            "lt" | "<" => Comparison::Lt,
            "le" | "<=" => Comparison::Le,
            "gt" | ">" => Comparison::Gt,
            "ge" | ">=" => Comparison::Ge,
            "contains" => Comparison::Contains,
            "starts_with" => Comparison::StartsWith,
            "ends_with" => Comparison::EndsWith,
            "empty" => Comparison::Empty,
            "not_empty" => Comparison::NotEmpty,
            _ => return None,
        })
    }

    /// Whether `cell` compares to `operand` this way.
    pub fn test(self, cell: &Value, operand: &Value) -> bool {
        match self {
            Comparison::Empty => cell.is_empty(),
            Comparison::NotEmpty => !cell.is_empty(),
            Comparison::Ne => !Comparison::Eq.test(cell, operand),
            Comparison::Contains => cell.plain().contains(&operand.plain()),
            Comparison::StartsWith => cell.plain().starts_with(&operand.plain()),
            Comparison::EndsWith => cell.plain().ends_with(&operand.plain()),
            Comparison::Eq => match operand {
                Value::Null => cell.is_empty(),
                Value::Int(_) | Value::Float(_) => order(cell, operand) == Some(Ordering::Equal),
                _ => cell.plain() == operand.plain(),
            },
            Comparison::Lt | Comparison::Le | Comparison::Gt | Comparison::Ge => {
                let Some(ordering) = order(cell, operand) else {
                    return false;
                };
                match self {
                    Comparison::Lt => ordering == Ordering::Less,
                    Comparison::Le => ordering != Ordering::Greater,
                    Comparison::Gt => ordering == Ordering::Greater,
                    _ => ordering != Ordering::Less,
                }
            }
        }
    }
}

/// `cell` against `operand`: numerically when the operand is a number, as
/// text when it is text, and not at all against null.
fn order(cell: &Value, operand: &Value) -> Option<Ordering> {
    match operand {
        Value::Null => None,
        // Two integers compare exactly: as `f64` they agree past 2^53.
        Value::Int(right) => {
            let left = match cell {
                Value::Int(left) => Some(*left),
                Value::Str(_) | Value::Text(_) => cell.plain().trim().parse::<i64>().ok(),
                _ => None,
            };
            match left {
                Some(left) => Some(left.cmp(right)),
                None => order(cell, &Value::Float(*right as f64)),
            }
        }
        Value::Float(_) => {
            let left = cell
                .as_f64()
                .or_else(|| match cell {
                    Value::Str(_) | Value::Text(_) => cell.plain().trim().parse::<f64>().ok(),
                    _ => None,
                })
                .filter(|n| !n.is_nan())?;
            left.partial_cmp(&operand.as_f64()?)
        }
        _ if matches!(cell, Value::Null) => None,
        _ => Some(cell.plain().cmp(&operand.plain())),
    }
}

/// A predicate over one cell's value.
pub type Predicate = Arc<dyn Fn(&Value) -> bool + Send + Sync>;

#[derive(Clone)]
enum Condition {
    Compare(Comparison, Value),
    Predicate(Predicate),
}

impl fmt::Debug for Condition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Condition::Compare(op, value) => {
                f.debug_tuple("Compare").field(op).field(value).finish()
            }
            Condition::Predicate(_) => f.write_str("Predicate(..)"),
        }
    }
}

/// One conditional style. See the [module docs](self).
#[derive(Clone, Debug)]
pub struct StyleRule {
    column: ColumnRef,
    condition: Condition,
    style: Style,
    target: Target,
}

impl StyleRule {
    /// Style a cell of `column` when it compares to `value` by `comparison`.
    pub fn new(
        column: impl Into<ColumnRef>,
        comparison: Comparison,
        value: impl Into<Value>,
        style: Style,
    ) -> Self {
        StyleRule {
            column: column.into(),
            condition: Condition::Compare(comparison, value.into()),
            style,
            target: Target::Cell,
        }
    }

    /// Style a cell of `column` when `predicate` holds for its value.
    pub fn when(
        column: impl Into<ColumnRef>,
        predicate: impl Fn(&Value) -> bool + Send + Sync + 'static,
        style: Style,
    ) -> Self {
        StyleRule {
            column: column.into(),
            condition: Condition::Predicate(Arc::new(predicate)),
            style,
            target: Target::Cell,
        }
    }

    /// Style the row or the column instead of the cell.
    pub fn target(mut self, target: Target) -> Self {
        self.target = target;
        self
    }

    /// The column the rule reads.
    pub fn column(&self) -> &ColumnRef {
        &self.column
    }

    /// The style it applies.
    pub fn style(&self) -> &Style {
        &self.style
    }

    /// What it styles.
    pub fn target_kind(&self) -> Target {
        self.target
    }

    /// Whether `value` meets the rule's condition.
    pub fn matches(&self, value: &Value) -> bool {
        match &self.condition {
            Condition::Compare(op, operand) => op.test(value, operand),
            Condition::Predicate(predicate) => predicate(value),
        }
    }
}

/// An ordered set of [`StyleRule`]s. Where several match, their styles
/// combine in order, a later rule winning wherever both set something.
#[derive(Clone, Debug, Default)]
pub struct StyleRules {
    rules: Vec<StyleRule>,
}

impl StyleRules {
    /// No rules.
    pub fn new() -> Self {
        StyleRules::default()
    }

    /// Add a rule (builder form).
    pub fn rule(mut self, rule: StyleRule) -> Self {
        self.rules.push(rule);
        self
    }

    /// Add a rule.
    pub fn push(&mut self, rule: StyleRule) -> &mut Self {
        self.rules.push(rule);
        self
    }

    /// The rules, in order.
    pub fn rules(&self) -> &[StyleRule] {
        &self.rules
    }

    /// The number of rules.
    pub fn len(&self) -> usize {
        self.rules.len()
    }

    /// Whether there are no rules.
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// Resolve the rules against a table's `headers` and `rows`: each rule's
    /// column, and the styles of column rules (which need every row, so the
    /// rows are only read when a column rule exists).
    pub fn resolve<'a, R>(
        &'a self,
        headers: &[&str],
        rows: impl IntoIterator<Item = R>,
    ) -> ResolvedRules<'a>
    where
        R: AsRef<[Value]>,
    {
        let columns: Vec<Option<usize>> = self
            .rules
            .iter()
            .map(|rule| match &rule.column {
                ColumnRef::Name(name) => headers.iter().position(|h| h == name),
                ColumnRef::Index(index) => (*index < headers.len()).then_some(*index),
            })
            .collect();
        let mut column_styles: Vec<Option<Style>> = vec![None; headers.len()];
        let column_rules: Vec<usize> = (0..self.rules.len())
            .filter(|&i| self.rules[i].target == Target::Column && columns[i].is_some())
            .collect();
        if !column_rules.is_empty() {
            let mut hit = vec![false; self.rules.len()];
            for row in rows {
                let row = row.as_ref();
                for &i in &column_rules {
                    if !hit[i] {
                        let column = columns[i].expect("resolved above");
                        hit[i] = row.get(column).is_some_and(|v| self.rules[i].matches(v));
                    }
                }
                if column_rules.iter().all(|&i| hit[i]) {
                    break;
                }
            }
            for &i in column_rules.iter().filter(|&&i| hit[i]) {
                let column = columns[i].expect("resolved above");
                combine(&mut column_styles[column], &self.rules[i].style);
            }
        }
        ResolvedRules {
            rules: self,
            columns,
            column_styles,
        }
    }
}

fn combine(slot: &mut Option<Style>, style: &Style) {
    *slot = Some(match slot.take() {
        Some(previous) => previous.combine(style),
        None => style.clone(),
    });
}

/// [`StyleRules`] resolved against one table, from
/// [`StyleRules::resolve`].
#[derive(Clone, Debug)]
pub struct ResolvedRules<'a> {
    rules: &'a StyleRules,
    columns: Vec<Option<usize>>,
    column_styles: Vec<Option<Style>>,
}

impl ResolvedRules<'_> {
    /// The style for the whole of `row`: its row rules that match, combined.
    pub fn row_style(&self, row: &[Value]) -> Option<Style> {
        let mut style = None;
        for (rule, column) in self.rules.rules.iter().zip(&self.columns) {
            if rule.target != Target::Row {
                continue;
            }
            if let Some(value) = column.and_then(|c| row.get(c)) {
                if rule.matches(value) {
                    combine(&mut style, &rule.style);
                }
            }
        }
        style
    }

    /// The style for `row`'s cell in `column`: the column's style from
    /// column rules, then its cell rules that match, combined.
    pub fn cell_style(&self, row: &[Value], column: usize) -> Option<Style> {
        let mut style = self.column_styles.get(column).cloned().flatten();
        let Some(value) = row.get(column) else {
            return style;
        };
        for (rule, resolved) in self.rules.rules.iter().zip(&self.columns) {
            if rule.target == Target::Cell && *resolved == Some(column) && rule.matches(value) {
                combine(&mut style, &rule.style);
            }
        }
        style
    }

    /// The style column rules give `column`, if any matched.
    pub fn column_style(&self, column: usize) -> Option<&Style> {
        self.column_styles.get(column).and_then(Option::as_ref)
    }

    /// The rules whose column is not in the table (a header that is not
    /// there, an index past the end). They never match.
    pub fn unresolved(&self) -> Vec<&StyleRule> {
        self.rules
            .rules
            .iter()
            .zip(&self.columns)
            .filter(|(_, column)| column.is_none())
            .map(|(rule, _)| rule)
            .collect()
    }
}

/// Why rule tables could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuleError(String);

impl fmt::Display for RuleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for RuleError {}

#[cfg(feature = "toml")]
impl StyleRules {
    /// Read the `[[rules]]` tables of a TOML document. See the
    /// [module docs](self) for the keys.
    pub fn from_toml(text: &str) -> Result<Self, RuleError> {
        let table: toml::Table = text
            .parse()
            .map_err(|e: toml::de::Error| RuleError(format!("not TOML: {}", e.message())))?;
        match table.get("rules") {
            Some(rules) => Self::from_toml_value(rules),
            None => Ok(StyleRules::new()),
        }
    }

    /// Read rule tables from an array of tables, wherever a configuration
    /// keeps them (a profile's `rules` key, say).
    pub fn from_toml_value(value: &toml::Value) -> Result<Self, RuleError> {
        let toml::Value::Array(items) = value else {
            return Err(RuleError("rules: expected an array of tables".into()));
        };
        let mut rules = StyleRules::new();
        for (index, item) in items.iter().enumerate() {
            let at = |message: String| RuleError(format!("rules[{index}]: {message}"));
            let toml::Value::Table(table) = item else {
                return Err(at("expected a table".into()));
            };
            if let Some(key) = table
                .keys()
                .find(|k| !matches!(k.as_str(), "column" | "op" | "value" | "style" | "target"))
            {
                return Err(at(format!("unknown key `{key}`")));
            }
            let column = match table.get("column") {
                Some(toml::Value::String(name)) => ColumnRef::Name(name.clone()),
                Some(toml::Value::Integer(n)) => usize::try_from(*n)
                    .map(ColumnRef::Index)
                    .map_err(|_| at(format!("column index {n} is negative")))?,
                Some(_) => return Err(at("`column` is a header or an index".into())),
                None => return Err(at("missing `column`".into())),
            };
            let op = match table.get("op") {
                None => Comparison::Eq,
                Some(toml::Value::String(name)) => {
                    Comparison::parse(name).ok_or_else(|| at(format!("unknown op `{name}`")))?
                }
                Some(_) => return Err(at("`op` is a string".into())),
            };
            let operand = match table.get("value") {
                None if matches!(op, Comparison::Empty | Comparison::NotEmpty) => Value::Null,
                None => return Err(at("missing `value`".into())),
                Some(toml::Value::String(s)) => Value::Str(s.clone()),
                Some(toml::Value::Integer(n)) => Value::Int(*n),
                Some(toml::Value::Float(f)) => Value::Float(*f),
                Some(toml::Value::Boolean(b)) => Value::Str(b.to_string()),
                Some(_) => return Err(at("`value` is a string, number or boolean".into())),
            };
            let style = match table.get("style") {
                Some(toml::Value::String(spec)) => {
                    Style::parse(spec).map_err(|e| at(format!("style `{spec}`: {e}")))?
                }
                Some(_) => return Err(at("`style` is a string".into())),
                None => return Err(at("missing `style`".into())),
            };
            let target = match table.get("target") {
                None => Target::Cell,
                Some(toml::Value::String(name)) => {
                    Target::parse(name).ok_or_else(|| at(format!("unknown target `{name}`")))?
                }
                Some(_) => return Err(at("`target` is a string".into())),
            };
            rules.push(StyleRule::new(column, op, operand, style).target(target));
        }
        Ok(rules)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn red() -> Style {
        Style::parse("red").unwrap()
    }

    #[test]
    fn comparisons_read_numbers_from_text() {
        let cell = Value::Str("12.5".into());
        assert!(Comparison::Gt.test(&cell, &Value::Int(10)));
        assert!(Comparison::Le.test(&cell, &Value::Float(12.5)));
        assert!(Comparison::Eq.test(&Value::Int(3), &Value::Float(3.0)));
        assert!(!Comparison::Lt.test(&Value::Str("n/a".into()), &Value::Int(1)));
        assert!(!Comparison::Gt.test(&Value::Null, &Value::Int(-1)));
        assert!(Comparison::Ne.test(&Value::Str("n/a".into()), &Value::Int(1)));
        // Integers past 2^53 are told apart, as cells and as text.
        let big = 9_007_199_254_740_993_i64;
        assert!(!Comparison::Eq.test(&Value::Int(big), &Value::Int(big - 1)));
        assert!(Comparison::Gt.test(&Value::Int(big), &Value::Int(big - 1)));
        assert!(Comparison::Gt.test(&Value::Str(big.to_string()), &Value::Int(big - 1)));
        assert!(Comparison::Eq.test(&Value::Str(" 7 ".into()), &Value::Int(7)));
        assert!(Comparison::Lt.test(&Value::Float(6.5), &Value::Int(7)));
        assert!(Comparison::Gt.test(&Value::Str("7.5".into()), &Value::Int(7)));
    }

    #[test]
    fn comparisons_on_text_and_null() {
        let cell = Value::Str("failed: timeout".into());
        assert!(Comparison::StartsWith.test(&cell, &"failed".into()));
        assert!(Comparison::EndsWith.test(&cell, &"timeout".into()));
        assert!(Comparison::Contains.test(&cell, &": ".into()));
        assert!(!Comparison::Eq.test(&cell, &"failed".into()));
        assert!(Comparison::Lt.test(&Value::Str("a".into()), &"b".into()));
        assert!(Comparison::Eq.test(&Value::Null, &Value::Null));
        assert!(Comparison::Eq.test(&Value::Str(String::new()), &Value::Null));
        assert!(Comparison::Ne.test(&Value::Int(0), &Value::Null));
        assert!(Comparison::Empty.test(&Value::Null, &Value::Null));
        assert!(Comparison::NotEmpty.test(&Value::Int(0), &Value::Null));
        for name in ["==", "!=", "<", "<=", ">", ">=", "starts_with", "not_empty"] {
            assert!(Comparison::parse(name).is_some(), "{name}");
        }
        assert_eq!(Comparison::parse("like"), None);
    }

    #[test]
    fn resolution_targets_cells_rows_and_columns() {
        let rows = vec![
            vec![Value::from("a"), Value::Int(1)],
            vec![Value::from("b"), Value::Int(-1)],
        ];
        let rules = StyleRules::new()
            .rule(StyleRule::new(1, Comparison::Lt, 0, red()))
            .rule(StyleRule::new("name", Comparison::Eq, "a", red()).target(Target::Row))
            .rule(
                StyleRule::when(
                    "n",
                    |v| v.as_f64() == Some(-1.0),
                    Style::parse("bold").unwrap(),
                )
                .target(Target::Column),
            )
            .rule(StyleRule::new(
                "missing",
                Comparison::NotEmpty,
                Value::Null,
                red(),
            ));
        let resolved = rules.resolve(&["name", "n"], &rows);
        assert_eq!(resolved.cell_style(&rows[0], 0), None);
        // Column rule only.
        assert_eq!(
            resolved.cell_style(&rows[0], 1),
            Some(Style::parse("bold").unwrap())
        );
        // Column rule, then the cell rule on top.
        assert_eq!(
            resolved.cell_style(&rows[1], 1),
            Some(Style::parse("bold red").unwrap())
        );
        assert_eq!(resolved.row_style(&rows[0]), Some(red()));
        assert_eq!(resolved.row_style(&rows[1]), None);
        assert_eq!(resolved.unresolved().len(), 1);
        assert_eq!(resolved.unresolved()[0].column().to_string(), "missing");
    }

    #[cfg(feature = "toml")]
    #[test]
    fn rule_tables_from_toml() {
        let rules = StyleRules::from_toml(
            "[[rules]]\ncolumn = 'status'\nvalue = 'failed'\nstyle = 'bold red'\ntarget = 'row'\n\
             [[rules]]\ncolumn = 2\nop = '>='\nvalue = 0.5\nstyle = 'yellow'\n\
             [[rules]]\ncolumn = 'note'\nop = 'empty'\nstyle = 'dim'\n",
        )
        .unwrap();
        assert_eq!(rules.len(), 3);
        assert_eq!(rules.rules()[0].target_kind(), Target::Row);
        assert_eq!(rules.rules()[1].column(), &ColumnRef::Index(2));
        assert!(rules.rules()[1].matches(&Value::Float(0.75)));
        assert!(rules.rules()[2].matches(&Value::Null));
        assert!(StyleRules::from_toml("").unwrap().is_empty());

        let error = |text: &str| StyleRules::from_toml(text).unwrap_err().to_string();
        assert_eq!(
            error("[[rules]]\ncolumn='a'\nvalue=1\nstyle='red'\nwhen='x'"),
            "rules[0]: unknown key `when`"
        );
        assert_eq!(
            error("[[rules]]\ncolumn='a'\nop='like'\nvalue=1\nstyle='red'"),
            "rules[0]: unknown op `like`"
        );
        assert_eq!(
            error("[[rules]]\ncolumn='a'\nstyle='red'"),
            "rules[0]: missing `value`"
        );
        assert_eq!(error("rules = 3"), "rules: expected an array of tables");
        assert!(
            error("[[rules]]\ncolumn='a'\nvalue=1\nstyle='nonsense colour'")
                .starts_with("rules[0]: style `nonsense colour`")
        );
    }
}
